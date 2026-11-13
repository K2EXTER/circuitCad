//! The left side panel, the selection panel and the context hint.

use super::*;

/// A heading that splits the left panel into its two halves: the tools drawn by hand and the steps to follow.
/// The numbered steps of the "手順" tab (one open at a time).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Place,
    Route,
    Check,
    Order,
}

/// One step's fold, open when it is `open` (the renewed `gen` makes a new fold, so the others close). True when the learner has just
/// opened it.
#[allow(clippy::too_many_arguments)]
fn step_header(ui: &mut egui::Ui, step: Step, open: Step, round: u32, done: bool, title: &str, hint: &str, body: impl FnOnce(&mut egui::Ui)) -> bool {
    let id = egui::Id::new(("board_step", step as u8, round));
    let was = egui::collapsing_header::CollapsingState::load(ui.ctx(), id).map_or(open == step, |s| s.is_open());
    crate::widgets::section_header_marked(ui, id, open == step, title, hint, None, done, body);
    !was && egui::collapsing_header::CollapsingState::load(ui.ctx(), id).is_some_and(|s| s.is_open())
}

fn group_heading(ui: &mut egui::Ui, text: &str, tip: &str) {
    ui.add_space(2.0);
    ui.label(egui::RichText::new(text).strong().color(crate::theme::INFO)).on_hover_text(tip);
    ui.separator();
}

impl BoardEditor {
    /// How many heat pads lack thermal vias (counted again only when the copper changed).
    fn thermal_lacking(&self) -> usize {
        let key = pcb_core::zonefill::geometry_key(&self.board);
        match self.thermal_cache.get() {
            Some((k, n)) if k == key => n,
            // while parts are dragged the count is not made again (it fills every pour): the last one stands until the drag ends
            Some((_, n)) if self.slow_results_kept() => n,
            _ => {
                let n = pcb_core::drc::thermal_via_checks(&self.board).len();
                self.thermal_cache.set(Some((key, n)));
                n
            }
        }
    }

    /// A tip for what to do next on the board.
    /// The step the board is at: placing until there are parts and an outline, routing while connections are open, checking until
    /// the DRC has passed since the last edit, then ordering.
    pub(super) fn current_step(&self) -> Step {
        if self.board.parts.is_empty() || self.board.outline.len() < 3 {
            Step::Place
        } else if !self.cached_airwires().is_empty() {
            Step::Route
        } else if !(self.drc_ran && self.violations.is_empty()) {
            Step::Check
        } else {
            Step::Order
        }
    }

    pub fn hint(&self) -> String {
        if self.board.parts.is_empty() {
            return "まず回路図タブで回路を作り、ここで「回路図から取り込み」を押すと、部品が基板に読み込まれます".into();
        }
        if self.board.outline.len() >= 3 && self.board.tracks.is_empty() && self.board.parts.iter().any(|p| self.board.pads.iter().any(|d| d.refdes == p.refdes && !inside_polygon(&self.board.outline, d.pos))) {
            return "部品が基板の外にあります。左の「部品を並べて外形を決める」で自動的に並べるか、部品をドラッグして基板の中に置きましょう".into();
        }
        if self.board.bounds().is_none() || self.board.outline.is_empty() {
            return "基板の外形を決めましょう。左の「部品を並べて外形を決める」で部品に合わせて作るか、「変える…」で大きさを数字で決めます。発注にはこの外形が必要です".into();
        }
        let unrouted = self.cached_airwires().len();
        if unrouted > 0 {
            return format!("黄色の線 (未配線 {unrouted} 本) が残っています。「配線」ツールで手でつなぐか、左の「自動配線」に任せましょう。広い GND は「銅箔ゾーン」にすると簡単です");
        }
        if self.violations.is_empty() {
            return "配線が終わりました。「DRC を実行」でルール違反がないか確認し、問題なければ「発注の準備」で最終チェックをして、発注用データ (ガーバー) を作れます".into();
        }
        "DRC の指摘は、クリックするとその場所に飛びます。直したらもう一度 DRC を実行しましょう。0 件になれば発注データを書き出せます".into()
    }

    pub fn side_panel(&mut self, ui: &mut egui::Ui, sch: &SchEditor) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            // icon-only buttons are packed side by side; with labels they stay one per line
            let wrap = crate::icons::icon_only();
            // the width panel lives under the wire tool's settings: show it when something asks for it (simulated currents, --ir)
            // a simulated current or an IR-drop result opens step ③, where the currents and the drop are shown
            if self.widths_open.take().is_some() {
                self.step_open = Some(Step::Check);
                self.step_gen += 1;
            }
            crate::widgets::tab_bar(
                ui,
                &mut self.panel_tab,
                &[
                    (PanelTab::Steps, "手順", "回路図の取り込みから、配置・配線・確認と発注まで、上から順に進めます"),
                    (PanelTab::Draw, "手で描く", "ツールで基板に自分で描き込みます。作業する層・グリッド・層構成もここです"),
                ],
            );
            self.selection_panel(ui);
            if self.panel_tab == PanelTab::Steps {
            // the first thing to press, so it is not inside a fold
            if crate::widgets::primary_button_wide(ui, true, "回路図から取り込み").on_hover_text(i18n::tr("回路図の部品とつながり (ネット) を基板に読み込みます。部品が並び、つなぐべき所に黄色の線 (未配線) が出ます。回路図を直したらもう一度押します")).clicked() {
                self.import_from_schematic(sch);
            }
                let diff = self.schematic_diff(sch).clone();
                if !diff.is_empty() {
                    ui.colored_label(crate::theme::WARN, i18n::trf!("回路図と違います: {}", diff.summary()))
                        .on_hover_text(i18n::tr("取り込んだあとに、回路図が変わったか、基板が変わりました。「回路図から取り込み」を押すと、基板の部品の位置と配線は残したまま、値・形・つながりが回路図に合わせて更新されます"));
                }
                for n in &self.import_notes {
                    ui.colored_label(crate::theme::WARN, n);
                }
            ui.add_space(4.0);
            group_heading(ui, i18n::tr("手順 (上から順に)"), i18n::tr("基板づくりの流れです。①から順に進めると、発注用のデータまで作れます。いつでも途中に戻れます"));
            // one step open at a time: at first the step the board is at (`current_step`), then the one the learner opens
            let open_step = self.step_open.unwrap_or_else(|| self.current_step());
            let round = self.step_gen;
            let placed = !self.board.parts.is_empty() && self.board.outline.len() >= 3;
            let routed = placed && self.cached_airwires().is_empty();
            let checked = routed && self.drc_ran && self.violations.is_empty();
            let mut opened = None;
            if step_header(ui, Step::Place, open_step, round, placed, "① 部品を並べる", "部品を基板の上に並べて、基板の外形 (大きさ・形) を決めます (部品の取り込みは、パネルの一番上のボタン)。ここまでが「何をどこに置くか」です", |ui| {
                self.sheet_legend(ui);
                ui.label(i18n::trf!("未配線: {} 本 (黄色の線)", self.cached_airwires().len())).on_hover_text(i18n::tr("まだ銅の線でつながっていない部分です。黄色の線 (ラッツネスト) が、つなぐべき相手を結んで示します。配線すると減って、0 になれば全部つながっています"));
                if crate::widgets::primary_button_wide(ui, true, "部品を並べて外形を決める").on_hover_text(i18n::tr("すべての部品を、つながりの強いものが近くなるように並べ、基板の外形をそれに合わせます。最初の叩き台を作る機能なので、並べたあとに手で動かして整えてください。コネクタなど置き場所の決まった部品は、先に置いて「固定する」と、動かさずにそのまわりへ残りを並べます。先に「銅箔ゾーン」で GND のベタを塗っておくと、ベタでつながる GND は配線が要らないので、それを除いて部品を近づけ、小さくまとまります (ベタも新しい外形に合わせます)。「GND ベタ」を置いたあとでも使えます (そのビアと短い線は、並べたあとに置き直します)。ほかの配線がある基板では使えません (部品を動かすと配線が合わなくなるため)")).clicked() {
                    self.autoplace();
                }
                if self.sheet_colors().len() >= 2 {
                    ui.checkbox(&mut self.place_by_sheet, i18n::tr("シートごとに塊にして並べる")).on_hover_text(i18n::tr("回路図が複数のシートのとき、シートごとの部品を1つの塊(長方形)にして並べ、塊どうしをつながりの強い順に並べます。塊の中は、足の多い部品から、つながりの強い順です。どの部品がどのシートのものか分かりやすく、手で直しやすくなります。そのぶん、基板が大きくなり、配線が長くなります (マイクロオームメーターの回路 (6 シート・191 部品) で、面積 +37%、自動配線の長さ +13%、コンデンサが電源ピンから遠くなります)。そのため、初めはオフです。オフだと、シートに関係なく並べます"));
                }
                ui.separator();
                // the outline in one line: "部品を並べて外形を決める" above makes it; the numbers, circles and rounded corners
                // are the outline tool's settings (the "手で描く" tab), where the drawn outlines are too
                ui.horizontal_wrapped(|ui| {
                    match self.board.bounds().filter(|_| self.board.outline.len() >= 3) {
                        Some((lo, hi)) => {
                            ui.label(i18n::trf!("外形 {:.1} × {:.1} mm", hi.x - lo.x, hi.y - lo.y)).on_hover_text(i18n::tr("基板の大きさと形 (黄色の枠) です。発注するときの基板サイズになります"));
                        }
                        None => {
                            ui.colored_label(crate::theme::WARN, i18n::tr("外形がまだありません")).on_hover_text(i18n::tr("発注には基板の外形が要ります。上の「部品を並べて外形を決める」で作るか、「変える…」で大きさを数字で決めるか、描きます"));
                        }
                    }
                    if ui.small_button(i18n::tr("変える…")).on_hover_text(i18n::tr("「手で描く」タブの外形ツールを選びます。幅・高さ・角R を数字で入れて矩形や円・長円にしたり、角を丸めたり、ドラッグや多角形で描いたりできます。決まった大きさのケースに入れるときに")).clicked() {
                        self.pick_tool_named("rect");
                    }
                });
                ui.separator();
                crate::widgets::section_heading(ui, "整える");
                let bar = crate::widgets::segment_row(ui, &[("ピン名をシルクへ", true)]);
                bar[0].clone().on_hover_text(i18n::tr("回路図でヘッダ(コネクタ)に付けた「ピンの名前」(F+、SA、GND など)を、基板のシルク(白い文字)として、パッドのそばに書きます。もう一度押すと書き直します。位置は、あとで「文字」ツールで動かせます"));
                if bar[0].clicked() {
                    self.write_pin_names(sch);
                }
                let (has_swap, has_map) = (!self.board.swap_specs.is_empty(), !self.board.pin_map.is_empty());
                let mut items = vec![("向きを最適化", true)];
                if has_swap {
                    items.push(("ゲートを入れ替え", true));
                }
                if has_map {
                    items.push(("回路図に反映", true));
                }
                let bar = crate::widgets::segment_row(ui, &items);
                bar[0].clone().on_hover_text(i18n::tr("配線がまだ付いていない部品を、0°・90°・180°・270° のうち、未配線の線が短く、交差が少なくなる向きに回します。回してもピンのつなぎは変わりません (回路は同じ)。ほかの部品や配線、基板の外にぶつかる向きは選びません。ロックした部品は回しません。配線を引く前の仕上げに使います。元に戻すで取り消せます"));
                let mut at = 1;
                let mut swap = false;
                let mut reflect = false;
                if has_swap {
                    bar[at].clone().on_hover_text(i18n::tr("74HC32 や 74HC14 のように、同じゲートが何個も入った IC では、どのゲートをどの信号に使っても回路は同じです (OR ゲートの 2 つの入力も同じ)。部品を置いたあとに押すと、未配線の線が短くなる割り当てを探して入れ替えます (交差の多さも点数に入れますが、長さを優先するので、交差が少し増えることもあります)。配線がまだ付いていないゲートだけ動かします。入れ替えは基板に覚えられ、「回路図から取り込み」をしても元に戻りません。回路は変わらないので、シミュレーションはそのままです。元に戻すで取り消せます"));
                    swap = bar[at].clicked();
                    at += 1;
                }
                if has_map {
                    bar[at].clone().on_hover_text(i18n::tr("「ゲートを入れ替え」で入れ替えたピンを、回路図の側にも反映します。回路図の配線の端とネットラベルが、入れ替えたピンへ移ります (部品は描き直さないので、ピンの間で配線が交差して見えることがあります。つながりは基板と同じです)。反映すると、基板が覚えていた入れ替えは要らなくなります。ほかの配線とまざってつながりが変わるときは、何も変えずに理由を出します。回路図の側の「元に戻す」で取り消せます"));
                    reflect = bar[at].clicked();
                }
                if bar[0].clicked() {
                    self.orient_parts();
                }
                if swap {
                    self.swap_gates();
                }
                if reflect {
                    self.to_schematic_request = true;
                }
                {
                    let bar = crate::widgets::segment_row(ui, &[("選択を保存…", true), ("スニペットを置く…", true)]);
                    bar[0].clone().on_hover_text(i18n::tr("選んだ部品と、その間の配線・ビアを、ファイル (.ccbsnip) に保存します。よく使う電源まわりの配置と配線などを、別の基板で使えます。部品の位置と向きは、選んだ範囲の真ん中からの相対で残ります"));
                    bar[1].clone().on_hover_text(i18n::tr("保存したスニペットを、いま見えている画面の真ん中に置きます。同じ部品番号・同じフットプリントの部品を、保存した並べ方に動かし、保存した配線とビアを足します。動かした部品につながっていた古い配線は消えます。この基板にない部品や、固定した部品は動かさず、結果の文に出します。「元に戻す」で戻せます"));
                    if bar[0].clicked() {
                        self.save_snippet_dialog();
                    }
                    if bar[1].clicked() {
                        self.load_snippet_dialog();
                    }
                }
                if !self.board.parts.is_empty() {
                    let bar = crate::widgets::segment_row(ui, &[("部品番号を位置順に", true)]);
                    bar[0].clone().on_hover_text(i18n::tr("部品番号 (R1・C2 など) を、基板の上の位置の順に振り直します。上の段から下の段へ、各段は左から右です。回路図の部品番号、要件の表、「接続しない」の印も同じに変わります。固定した部品の番号は変えません。「元に戻す」で戻せます (回路図の側も、回路図で元に戻してください)"));
                    if bar[0].clicked() {
                        self.renumber_request = true;
                    }
                }
            }) {
                opened = Some(Step::Place);
            }
            if step_header(ui, Step::Route, open_step, round, routed, "② 自動で配線", "部品どうしを銅の線でつなぎます (配線)。「ベタ」は、基板の面いっぱいに広げた銅箔のことで、GND (0V の基準) によく使います。自動で引いたあと、手で直せます", |ui| {
                let sel_net = match self.selected {
                    Some(Item::Pad(i)) => self.board.pads.get(i).map(|p| p.net.clone()).filter(|n| !n.is_empty()),
                    _ => None,
                };
                let only_tip = if sel_net.is_some() { "" } else { "

(いまは使えません: 先に基板の上のパッドを1つクリックして選んでください)" };
                crate::widgets::section_heading(ui, "自動で引く");
                ui.label(egui::RichText::new(i18n::tr("上から順に押します")).weak());
                let short_only = sel_net.as_ref().map_or("選んだネットだけ".to_string(), |n| format!("{n} だけ"));
                let bar = crate::widgets::segment_row(ui, &[("GND ベタ", true), ("全部つなぐ", true), (short_only.as_str(), sel_net.is_some())]);
                bar[0].clone().on_hover_text(i18n::tr("基板の裏面ぜんぶを GND の銅箔 (ベタ) にして、表面の GND パッドを、短い線とビアで裏のベタにつなぎます。GND の長い配線が要らなくなり、配線も楽になります。
    部品を並べる前に押しても、並べたあとに押しても使えます。先に押すと、GND を除いて部品が近づきます (「部品を並べて外形を決める」の説明を参照)。どちらも「全部」の自動配線の前に押します (四角い外形の基板で使えます)。気に入らなければ「元に戻す」で取り消せます"));
                bar[1].clone().on_hover_text(i18n::tr("未配線 (黄色の線) をすべて自動で配線します。先に部品を基板の上に並べておいてください。
    線幅・ビア径は、配線・ビアツールを選ぶと出る設定が使われ、通せなかった接続は黄色の線のまま残ります。気に入らなければ「元に戻す」で取り消せます。
    電流がまだ決まっていないときは、回路図の動作点 (DC) の電流を先に取り、電流の大きいネットは必要な太さで引きます (「③ 確認する」の「電流と電圧降下」で見直せます)。
    2層・45°配線で、ルール違反になる配線は自動的にやめます"));
                bar[2].clone().on_hover_text(i18n::trf!("パッドを選ぶと、そのパッドのネットだけを自動配線します。重要な線は手で引き、残りを任せる使い方ができます{only_tip}"));
                if bar[0].clicked() {
                    self.ground_pour();
                }
                if bar[1].clicked() {
                    self.autoroute(None);
                }
                if bar[2].clicked()
                    && let Some(n) = &sel_net {
                        self.autoroute(Some(n));
                    }
                ui.checkbox(&mut self.pour_top, i18n::tr("表面も GND のベタで埋める")).on_hover_text(i18n::tr("「GND ベタ」を押したとき、裏面に加えて、表面の空いた所も GND の銅で埋めます (自動配線は、その上を通る形で配線します。配線のあとに表面のベタを足すこともできます)。配線から隙間をあけて塗るので、配線は変わりません。表面の GND パッドは直接つながり、ノイズに強く、銅の量が表裏でそろって基板が反りにくくなります。ルール違反が増えるときは置きません"));
                self.panel_stitch(ui);
                ui.separator();
                crate::widgets::section_heading(ui, "引いた配線の手直し");
                let bar = crate::widgets::segment_row(ui, &[("配線を短くする", true), ("通らない接続", true), (short_only.as_str(), sel_net.is_some())]);
                bar[0].clone().on_hover_text(i18n::tr("遠回りしている配線や、なくてもつながるビアのある配線を、引き直してみます。短く・ビアが少なくなり、ルール違反が増えないものだけを採用します (手で引いた配線にも使えます)。時間がかかることがあります。気に入らなければ「元に戻す」で取り消せます"));
                bar[1].clone().on_hover_text(i18n::tr("自動配線で通らなかった(黄色の線が残った)接続のそばの小さい部品 (3 ピンまで) を 1〜4 mm 動かして、配線を引き直してみます。通らない接続が減り、ルール違反が増えないものだけを採用します。最長 40 秒かかります。気に入らなければ「元に戻す」で取り消せます"));
                bar[2].clone().on_hover_text(i18n::trf!("パッドを選ぶと、そのパッドのネットの配線だけを短くしてみます (まわりの配線を動かすことはあります){only_tip}"));
                if bar[0].clicked() {
                    self.shorten_routes(None);
                }
                if bar[1].clicked() {
                    self.nudge_parts();
                }
                if bar[2].clicked()
                    && let Some(n) = &sel_net {
                        self.shorten_routes(Some(n));
                    }
            }) {
                opened = Some(Step::Route);
            }
            crate::widgets::section_header(ui, egui::Id::new("board_sec_仕上"), false, "仕上げ (任意)", "パッドの根元をそろえる、配線の角を丸める、涙型など、見た目と製造のための仕上げです", |ui| {
                self.panel_pad_entries(ui);
                self.panel_smooth(ui);
                self.panel_fiducials(ui);
            });
            if step_header(ui, Step::Check, open_step, round, checked, "③ 確認する", "「DRC」は、線が近すぎないか・基板の端にはみ出していないかなど、工場で作れるかの規則 (設計ルール) を調べる機能です。電流と電圧降下・放熱もここで確かめます", |ui| {
                if crate::widgets::primary_button_wide(ui, true, "DRC を実行").on_hover_text(i18n::tr("設計ルールチェック。線同士が近すぎる、銅が基板の端にはみ出している、などを調べます。発注前に必ず実行しましょう")).clicked() {
                    self.run_drc();
                }
                self.live_drc_toggle(ui);
                self.requirements_check(ui);
                self.requirements_table(ui, &sch.sch.requirements);
                let mut jump = None;
                for v in &self.violations {
                    if ui.link(format!("({:.1}, {:.1}) {}", v.at.x, v.at.y, i18n::tr_dyn(&v.message))).clicked() {
                        jump = Some(v.at);
                    }
                }
                if let Some(at) = jump {
                    self.pan = Vec2::new(at.x as f32, at.y as f32);
                    self.zoom = self.zoom.max(25.0);
                }
                ui.separator();
                self.panel_widths(ui);
                ui.separator();
                let lacking = self.thermal_lacking();
                if lacking > 0
                    && ui
                        .button(i18n::trf!("サーマルビアを置く ({lacking} 個の放熱パッド)"))
                        .on_hover_text(i18n::tr("放熱パッド (ICの裏のパッドなど) の中に、熱を裏面へ逃がすビアを格子状に置きます。そのあと、裏面に同じネット (たいてい GND) のゾーンを作ると、熱が基板全体に広がります"))
                        .clicked()
                {
                    self.checkpoint();
                    let n = pcb_core::drc::add_thermal_vias(&mut self.board);
                    self.status = if n > 0 { format!("サーマルビアを {n} 個置きました。裏面に同じネットのゾーンを作ってください") } else { "サーマルビアを置けませんでした (パッドが小さい、または他の穴に近すぎます)".into() };
                }
            }) {
                opened = Some(Step::Check);
            }
            if step_header(ui, Step::Order, open_step, round, false, "④ 発注する", "出来上がりを 3D で見て、「発注の準備」で発注先を選び、工場に渡すデータ (ガーバー) を作ります", |ui| {
                let bar = crate::widgets::segment_row(ui, &[("3Dで見る…", true), ("発注の準備…", true)]);
                bar[0].clone().on_hover_text(i18n::tr("基板を立体で見ます。出来上がりの色 (緑のレジスト・金色のパッド・白いシルク) と、部品の高さの目安が分かります。ドラッグで回せます"));
                bar[1].clone().on_hover_text(i18n::tr("発注先を選び、チェックと出来上がりの確認をして、製造ファイルを書き出します。発注までの手順も表示します"));
                if bar[0].clicked() {
                    self.open_view3d();
                }
                if bar[1].clicked() {
                    self.open_order();
                }
                ui.label(i18n::tr("基板名 (出力ファイル名)")).on_hover_text(i18n::tr("書き出すファイルの名前のもとになります (例: board_gerber.zip)"));
                ui.text_edit_singleline(&mut self.board.name).on_hover_text(i18n::tr("製造ファイル (Gerber など) の名前に使います。半角の英数字がおすすめです"));
            }) {
                opened = Some(Step::Order);
            }
            // opening one step closes the others
            if let Some(s) = opened {
                self.step_open = Some(s);
                self.step_gen += 1;
            }
            ui.separator();
            if ui.button(i18n::tr("操作ガイド (F1)")).on_hover_text(i18n::tr("マウスとキーの操作の一覧を開きます")).clicked() {
                self.help_request = Some(crate::help::HelpTab::Board);
            }
            } else {
            ui.scope(|ui| {
                if wrap {
                    // icon-only: the icons are one wrapped block, and the picked tool's settings go under the whole block
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.horizontal_wrapped(|ui| self.tool_buttons(ui, TOOLS));
                    self.tool_options(ui);
                } else {
                    // three groups of joined bars that stay where they are; the picked tool's settings go under all of them
                    for (name, tools) in TOOL_GROUPS {
                        ui.label(egui::RichText::new(i18n::tr_dyn(name)).weak().small());
                        self.tool_bar(ui, tools, false);
                    }
                    ui.add_space(2.0);
                    self.tool_options(ui);
                }
            });
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                let half = egui::CornerRadius { nw: 4, sw: 4, ne: 0, se: 0 };
                let other = egui::CornerRadius { nw: 0, sw: 0, ne: 4, se: 4 };
                let width = ui.available_width() / 2.0;
                if crate::icons::segment_button(ui, Icon::Undo, "元に戻す", "Ctrl+Z", false, !self.undo.is_empty(), "直前の操作を取り消します (Ctrl+Z)", width, half).clicked() {
                    self.undo();
                }
                if crate::icons::segment_button(ui, Icon::Redo, "やり直し", "Ctrl+Y", false, !self.redo.is_empty(), "取り消した操作をやり直します (Ctrl+Y)", width, other).clicked() {
                    self.redo();
                }
            });
            // folded at first (rarely changed); the grid, which is, stays in sight on the band
            let title = i18n::trf!("作業の設定 (グリッド {:.2} mm)", self.grid);
            crate::widgets::section_header(ui, egui::Id::new("board_sec_作業"), false, &title, "作業する層・配線のドラッグ・範囲選択・延長線トラッキング・グリッドなど、基板を描くときの設定です", |ui| {
                ui.horizontal(|ui| {
                    ui.label(i18n::tr("作業層"));
                    crate::icons::help(ui, concat!("画面右上の「表示とロック」の上の段で切り替えます (L キーでも順に切り替わります)。",
                        "いま配線を置く銅箔の層。部品とシルクは表面か裏面にだけ置けます (内層を選んでいるときは表面)。右上の「表示とロック」は、たたんで小さくできます"));
                });
                ui.checkbox(&mut self.track_drag_on, i18n::tr("配線をドラッグで動かす")).on_hover_text(i18n::tr("選択ツールで配線をつかんで、平行に動かせるようにします (つながった配線は伸び縮みし、邪魔な配線は押しのけます)。うっかり配線を動かさないよう、普段は切ってあります。切ってあっても、配線は部品と同じようにクリックで選べ、範囲選択でまとめて動かせます"));
                ui.checkbox(&mut self.box_both_sides, i18n::tr("範囲選択は反対の面も選ぶ")).on_hover_text(i18n::tr("空いた所をドラッグして囲んだとき、いま作業している面だけでなく、反対の面の配線・ビア・部品も選びます。選んだまま動かすと、表と裏の配線が部品といっしょに動きます。オフにすると、「表示とロック」の「反対の面」のロックがかかっているとき、反対の面は選びません (クリックでは、ロックが効きます)。Ctrl を押しながらクリック・ドラッグすると、選択に足したり、外したりできます"));
                if ui.checkbox(&mut self.tracking.on, i18n::tr("延長線トラッキング (F11)")).on_hover_text(i18n::tr("配線・ビア・外形・ベタのとき、パッドやビアや配線の端にカーソルを少し置くと、その点を拾います (青い +)。そのあと、その点から横と縦に点線のガイドが出て、カーソルがガイドの近くに来ると吸い付きます。2つの点のガイドが交わる所にも吸い付きます。グリッドとは両立します (ガイドに沿う向きはグリッドのままです)。選択ツールで部品をドラッグするときも、部品の位置がガイドに吸い付きます。Esc で拾った点を消します")).changed() && !self.tracking.on {
                    self.clear_tracking();
                }
                ui.add(egui::DragValue::new(&mut self.grid).speed(0.05).range(0.0..=5.0).prefix(i18n::tr("グリッド ")).suffix(" mm")).on_hover_text(i18n::tr("カーソルが吸い付く間隔。0 なら吸い付きません。小さいほど細かく置けます (部品の足の間隔は 2.54mm や 1.27mm が多いです)。自動配線の細かさにもなり、足の間隔の狭い IC (TQFP など) で通らない線が残るときは、0.1〜0.2 に小さくします"));
            });
            // the layers of the board: how many, and what they are made of (the impedance of a track follows from it)
            self.panel_stackup(ui);
            }
        });
    }

    /// One group of tools as a joined bar (equal widths, one chosen). The short labels are for the narrow bar; the hover text has the
    /// full name, what the tool does and its number key.
    fn tool_bar(&mut self, ui: &mut egui::Ui, tools: &[(Tool, &str)], joined: bool) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            let width = ui.available_width() / tools.len() as f32;
            for (n, (t, label)) in tools.iter().enumerate() {
                let (_, full, icon) = TOOLS.iter().find(|(x, _, _)| x == t).copied().expect("every tool is in TOOLS");
                let first = if n == 0 { 4 } else { 0 };
                let last = if n + 1 == tools.len() { 4 } else { 0 };
                let (low_l, low_r) = if joined { (0, 0) } else { (first, last) };
                let corners = egui::CornerRadius { nw: first, sw: low_l, ne: last, se: low_r };
                let key = tool_key(*t).map(|k| format!(" (キー {k})")).unwrap_or_default();
                let tip = format!("{full}: {}{key}", tool_tip(*t));
                if crate::icons::segment_button(ui, icon, label, "", self.tool == *t, true, &tip, width, corners).clicked() {
                    self.set_tool(*t);
                }
            }
        });
    }

    fn tool_buttons(&mut self, ui: &mut egui::Ui, tools: &[(Tool, &'static str, Icon)]) {
        for (t, name, icon) in tools {
            let key = tool_key(*t).map(|k| k.to_string()).unwrap_or_default();
            if crate::icons::icon_button_key(ui, *icon, name, &key, self.tool == *t, true, tool_tip(*t)).clicked() {
                self.set_tool(*t);
            }
        }
    }

    /// The outline from numbers (width, height, corner radius): a rectangle, a circle or oval, or the corners of the
    /// outline rounded. In the settings box of the two outline tools.
    fn outline_numbers_ui(&mut self, ui: &mut egui::Ui) {
        ui.label(i18n::tr("数字で決める")).on_hover_text(i18n::tr("基板の大きさと形 (黄色の枠) を、幅・高さ・角R の数字から作ります。発注するときの基板サイズになります。決まった大きさのケースに入れるときに"));
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::DragValue::new(&mut self.size_w).speed(0.5).range(5.0..=500.0).prefix(i18n::tr("幅 "))).on_hover_text(i18n::tr("基板の幅 (mm)。「矩形にする」か「円・長円にする」を押すと外形に反映されます"));
            ui.add(egui::DragValue::new(&mut self.size_h).speed(0.5).range(5.0..=500.0).prefix(i18n::tr("高さ "))).on_hover_text(i18n::tr("基板の高さ (mm)。「矩形にする」か「円・長円にする」を押すと外形に反映されます"));
            ui.add(egui::DragValue::new(&mut self.size_r).speed(0.1).range(0.0..=100.0).prefix(i18n::tr("角R ")).suffix(" mm"))
                .on_hover_text(i18n::tr("角の丸み (半径)。0 なら角ばった形です。ケースに入れる基板や、手で持つ基板は、角を丸めると角が欠けにくく、安全です。大きすぎる値は、入る大きさまで自動で小さくなります"));
        });
        // the three ways to turn the numbers above into an outline: one bar
        let bar = crate::widgets::segment_row(ui, &[("矩形にする", true), ("円・長円にする", true), ("角を丸める", true)]);
        bar[0].clone().on_hover_text(i18n::tr("上の幅と高さで基板の外形を作ります。発注するときの基板サイズになります。右の「角R」が 0 より大きいと、角が丸くなります"));
        bar[1].clone().on_hover_text(i18n::tr("上の幅と高さに収まる円 (幅=高さ) や長円の外形にします。丸い基板やボタン電池サイズの基板に"));
        bar[2].clone().on_hover_text(i18n::tr("いまの基板の外形 (矩形でも、多角形でも) のすべての角を、上の「角R」の半径で丸めます。辺が短くて入らない角は、入る大きさまで小さくなります (元に戻すで取り消せます)"));
        if bar[0].clicked() {
            self.checkpoint();
            let (lo, _) = self.board.bounds().unwrap_or((Point::default(), Point::default()));
            self.set_outline_box(lo.x, lo.y, lo.x + self.size_w, lo.y + self.size_h);
        }
        if bar[1].clicked() {
            self.checkpoint();
            let (lo, _) = self.board.bounds().unwrap_or((Point::default(), Point::default()));
            self.board.outline = pcb_core::outline::ellipse(lo.x, lo.y, lo.x + self.size_w, lo.y + self.size_h);
            self.status = "基板の外形を円・長円にしました".into();
        }
        if bar[2].clicked() {
            self.round_outline(self.size_r);
        }
        if let Some((lo, hi)) = self.board.bounds() {
            ui.small(i18n::trf!("現在 {:.1} × {:.1} mm ({}点)", hi.x - lo.x, hi.y - lo.y, self.board.outline.len()));
        }
    }

    /// The "手順" tab with the step of that name open (command line `--board-step`). False for a name that is no step.
    pub fn open_step_named(&mut self, name: &str) -> bool {
        let s = match name {
            "place" => Step::Place,
            "route" => Step::Route,
            "check" => Step::Check,
            "order" => Step::Order,
            _ => return false,
        };
        self.panel_tab = PanelTab::Steps;
        self.step_open = Some(s);
        self.step_gen += 1;
        true
    }

    /// The "手で描く" tab with the tool of that name picked (command line `--board-tool`). False for a name that is no tool.
    pub fn pick_tool_named(&mut self, name: &str) -> bool {
        let t = match name {
            "select" => Tool::Select,
            "track" => Tool::Track,
            "via" => Tool::Via,
            "measure" => Tool::Measure,
            "zone" => Tool::Zone,
            "keepout" => Tool::Keepout,
            "padth" => Tool::PadTh,
            "padsmd" => Tool::PadSmd,
            "hole" => Tool::Hole,
            "rect" => Tool::OutlineRect,
            "poly" => Tool::OutlinePoly,
            "silk" => Tool::Silk,
            "text" => Tool::Text,
            _ => return false,
        };
        self.panel_tab = PanelTab::Draw;
        self.set_tool(t);
        true
    }

    pub(super) fn set_tool(&mut self, t: Tool) {
        self.tool = t;
        self.clear_tracking();
        self.pending = None;
        self.poly.clear();
        self.trunk_pads.clear();
        self.trunk_note = None;
        // what was selected stays behind: R, B and Delete must not reach it from another tool
        if t != Tool::Select {
            self.selected = None;
            self.multi.clear();
            self.boxsel = None;
        }
    }

    /// Settings of the tool that is picked, shown right under the toolbar: how a wire is laid for the wire tool,
    /// the size of a via for the via tool (autoroute uses both).
    /// Whether the picked tool has settings to show under the bar.
    fn has_tool_options(&self) -> bool {
        matches!(self.tool, Tool::Track | Tool::Via | Tool::PadTh | Tool::PadSmd | Tool::Hole | Tool::Zone | Tool::Keepout | Tool::OutlineRect | Tool::OutlinePoly)
    }

    fn tool_options(&mut self, ui: &mut egui::Ui) {
        if !self.has_tool_options() {
            return;
        }
        // a bordered box under the tool bars, headed with the tool's name: it belongs to the picked tool and goes when the tool changes
        let full = TOOLS.iter().find(|(x, _, _)| *x == self.tool).map_or("", |(_, f, _)| *f);
        crate::widgets::round_card(ui, |ui| {
            ui.label(egui::RichText::new(i18n::trf!("{}の設定", i18n::tr_dyn(full))).strong());
            self.tool_options_body(ui)
        });
        ui.add_space(4.0);
    }

    fn tool_options_body(&mut self, ui: &mut egui::Ui) {
        match self.tool {
            Tool::Track => {
        if ui.checkbox(&mut self.trunk_mode, i18n::tr("幹で引く (2パッド→太い1本)")).on_hover_text(i18n::tr("入れると、配線ツールで、同じネットで隣り合う2つのパッド (QFN の2本の LX ピンなど) から、太い1本の幹を引き出します。パッドを2つクリックして、最後に幹を伸ばす先をクリックします。パッドの近くは細い首、そのあとが太い幹になります。合流の所に空きができず、すき間を埋める必要がない形です")).changed() {
            self.trunk_pads.clear();
            self.trunk_note = None;
            self.pending = None;
        }
        if self.trunk_mode {
            self.trunk_help(ui);
        }
        ui.label(i18n::tr("引き方")).on_hover_text(i18n::tr("配線ツールで引くときの設定です。線幅・ビアは、自動配線にも使われます"));
        ui.checkbox(&mut self.auto_route, i18n::tr("自動経路 (パッドまで探す)")).on_hover_text(i18n::tr("入れると、配線ツールで、行き先のパッド・ビア・配線の端にカーソルを合わせたとき、出発点からそこまでの経路を、障害物をよけて自動で探して見せます (水色の線)。クリックで、その経路を配線します。探せないとき・行き先が何もない所のときは、いつもの曲げ方の線になります。切ると、押しのけも使わない、手で決めたとおりの配線になります"));
        // shoving only works on top of the automatic path, so it is offered only while that is on
        if self.auto_route {
            ui.indent("shove_indent", |ui| {
                ui.checkbox(&mut self.shove, i18n::tr("押しのけ配線")).on_hover_text(i18n::tr("自動経路で通れないとき・行き先が何もない所のときに、邪魔になる別のネットの配線を脇へ寄せて通します。パッドやビアに固定された配線は動かさず、寄せきれないときは普通に引きます。プレビューが橙色なら押しのけ、赤ならルール違反です"));
            });
        }
        ui.horizontal(|ui| {
            ui.label(i18n::tr("配線"));
            ui.selectable_value(&mut self.route_mode, RouteMode::Angle45, "45°").on_hover_text(i18n::tr("配線を 45° の斜めを使って曲げます。基板の標準的な引き方で、角が鈍くなり、きれいで製造もしやすくなります"));
            ui.selectable_value(&mut self.route_mode, RouteMode::Orthogonal, i18n::tr("直角")).on_hover_text(i18n::tr("配線を縦と横だけで曲げます (斜めは使いません)"));
            ui.selectable_value(&mut self.route_mode, RouteMode::Free, i18n::tr("自由")).on_hover_text(i18n::tr("クリックした点どうしを、そのまま直線でつなぎます (どんな角度でも)。鋭い角ができやすいので注意しましょう"));
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.pad_lead, i18n::tr("パッドに垂直に出入り")).on_hover_text(i18n::tr("配線をパッドから出すとき・パッドへ入れるときに、パッドの辺に垂直な短い区間を必ず付けます (そのあとは、いつもの 45° などで行き先へ向かいます)。斜めに入る根元がなくなり、きれいにそろいます。距離が近すぎるときと、丸いパッド、「自由」の配線では何もしません。長いパッドでは、パッドの両端にも吸い付くので、2本を並べて出せます"));
            ui.add_enabled(self.pad_lead, egui::DragValue::new(&mut self.pad_lead_len).speed(0.05).range(0.1..=3.0).suffix(" mm")).on_hover_text(i18n::tr("パッドの縁から、垂直に出す区間の長さ"));
        });
        ui.add(egui::DragValue::new(&mut self.track_width).speed(0.01).range(0.05..=5.0).prefix(i18n::tr("線幅 ")).suffix(" mm"))
            .on_hover_text(i18n::tr("配線の太さ。大きい電流を流す線は太く。発注先の最小線幅 (多くは 0.15〜0.2 mm) を下回らないように"));
        egui::CollapsingHeader::new(i18n::tr("高度な配線 (中級者向け)")).default_open(false).show(ui, |ui| {
                self.panel_net_classes(ui);
                self.panel_pairs(ui);
                self.panel_lengths(ui);
        }).header_response.on_hover_text(i18n::tr("中級者向け: ネットクラス・差動ペア・長さ合わせ"));
            }
            Tool::Via => {
                let nets = self.board_nets();
                let label = self.via_net.clone().unwrap_or_else(|| "自動 (置いた所のネット)".into());
                ui.horizontal(|ui| {
                    ui.label(i18n::tr("ネット"));
                    egui::ComboBox::from_id_salt("via_net").selected_text(label).show_ui(ui, |ui| {
                        if ui.selectable_label(self.via_net.is_none(), i18n::tr("自動 (置いた所のネット)")).clicked() {
                            self.via_net = None;
                        }
                        for n in &nets {
                            if ui.selectable_label(self.via_net.as_deref() == Some(n.as_str()), n).clicked() {
                                self.via_net = Some(n.clone());
                            }
                        }
                    }).response.on_hover_text(i18n::tr("置くビアを、どのネットのものにするか。「自動」は、置いた場所の パッド・ビア・配線・ベタのネットになります (何もない所ならネットなし)。ネットを選ぶと、ベタの上でも空いた所でも、そのネットのビアになり、そのネットのベタにつながり、未配線の線 (黄色) の相手にもなります"));
                });
        ui.add(egui::DragValue::new(&mut self.via_dia).speed(0.05).range(0.3..=3.0).prefix(i18n::tr("ビア径 ")).suffix(" mm")).on_hover_text(i18n::tr("ビア (表と裏をつなぐ穴) の外径。銅の丸の大きさです。穴径より 0.3〜0.4mm 大きくします。発注先の最小値を下回らないように"));
        ui.add(egui::DragValue::new(&mut self.via_drill).speed(0.05).range(0.2..=2.0).prefix(i18n::tr("ビア穴 ")).suffix(" mm"))
            .on_hover_text(i18n::tr("ビアの穴径。小さいほど基板が高くなることがあります。発注先の最小穴径を確認しましょう"));
            }
            Tool::PadTh | Tool::PadSmd => {
                ui.add(egui::DragValue::new(&mut self.pad_size).speed(0.05).range(0.3..=10.0).prefix(i18n::tr("パッド径 ")).suffix(" mm")).on_hover_text(i18n::tr("パッド (部品の足をはんだ付けする銅の丸) の直径"));
                ui.add(egui::DragValue::new(&mut self.pad_drill).speed(0.05).range(0.2..=6.0).prefix(i18n::tr("パッド穴 ")).suffix(" mm")).on_hover_text(i18n::tr("パッドの穴径 (スルーホール)。部品の足の太さ + 0.2〜0.3mm が目安です"));
            }
            Tool::OutlineRect => {
                ui.small(i18n::tr("画面をドラッグして描きます"));
                ui.separator();
                self.outline_numbers_ui(ui);
            }
            Tool::OutlinePoly => {
                self.poly_mode_ui(ui);
                ui.separator();
                self.outline_numbers_ui(ui);
            }
            Tool::Keepout => self.panel_keepouts(ui),
            Tool::Hole => {
                ui.add(egui::DragValue::new(&mut self.hole_drill).speed(0.1).range(0.5..=10.0).prefix(i18n::tr("取付穴 ")).suffix(" mm")).on_hover_text(i18n::tr("ネジ止め用の取付穴の穴径。M3 のネジなら 3.2mm 前後です"));
            }
            Tool::Zone => {
                ui.label(i18n::tr("形を描いて確定 (右クリック・ダブルクリック・Enter、または最初の点をクリック)"));
                self.poly_mode_ui(ui);
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.zone_op, pcb_core::model::ShapeOp::Add, i18n::tr("足す (+)")).on_hover_text(i18n::tr("描いた形を、ベタの面に足します。ベタを選んである (同じネット・同じ層) と、そのベタの図形として足します。選んでいなければ、新しいベタを作ります"));
                    ui.selectable_value(&mut self.zone_op, pcb_core::model::ShapeOp::Sub, i18n::tr("引く (−)")).on_hover_text(i18n::tr("描いた形を、ベタから引きます (銅のない穴・窓)。ベタを選んである (この層) と、そのベタだけから引きます。選んでいなければ、描いた範囲に触れる、この層のベタから引きます"));
                });
                // the pour that is selected (on this layer): the shape goes to it, whatever the net below says
                let target = match self.selected {
                    Some(Item::Zone(i)) => self.board.zones.get(i).filter(|z| z.side == self.side).map(|z| z.net.clone()),
                    _ => None,
                };
                if self.zone_op == pcb_core::model::ShapeOp::Sub {
                    if let Some(net) = &target {
                        ui.label(i18n::trf!("選んでいるベタ (ネット {net}) から引きます"));
                    } else {
                        ui.label(i18n::tr("選んでいるベタがないので、描いた範囲のベタから引きます"));
                    }
                    let nets = self.nets();
                    ui.add_enabled_ui(target.is_none(), |ui| egui::ComboBox::from_label(i18n::tr("引く相手のネット")).selected_text(if self.hole_net.is_empty() { "すべて".to_string() } else { self.hole_net.clone() }).show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.hole_net, String::new(), i18n::tr("すべて"));
                        for n in &nets {
                            ui.selectable_value(&mut self.hole_net, n.clone(), n);
                        }
                    }).response.on_hover_text(i18n::tr("ベタを選んでいないとき、どのネットのベタから引くか。「すべて」なら、描いた範囲に触れる、この層のベタ全部です (ネットを決めなくてよい)")));
                } else {
                    if target.as_ref().is_some_and(|n| n == &self.zone_net) {
                        ui.label(i18n::tr("選んでいるベタの図形として足します"));
                    } else {
                        ui.label(i18n::tr("新しいベタを作ります (同じネットのベタを選んでおくと、そのベタに足します)"));
                    }
                    let nets = self.nets();
                    egui::ComboBox::from_label(i18n::tr("ベタのネット")).selected_text(self.zone_net.clone()).show_ui(ui, |ui| {
                        for n in &nets {
                            ui.selectable_value(&mut self.zone_net, n.clone(), n);
                        }
                        if !nets.iter().any(|n| n == "GND") {
                            ui.selectable_value(&mut self.zone_net, "GND".to_string(), "GND");
                        }
                    }).response.on_hover_text(i18n::tr("ゾーンでつなぐネット。ふつうは GND にして、基板の余った所を GND の銅で塗ります"));
                    ui.add(egui::DragValue::new(&mut self.zone_clearance).speed(0.05).range(0.1..=2.0).prefix(i18n::tr("隙間 ")).suffix(" mm")).on_hover_text(i18n::tr("ゾーンの銅と、ほかのネットの配線・パッドの間をあける距離。小さいと短絡しやすく、大きいと銅が減ります (多くは 0.3〜0.5mm)"));
                    ui.checkbox(&mut self.zone_thermal, i18n::tr("スルーホールはサーマルリリーフ")).on_hover_text(i18n::tr("スルーホールのパッドを、ゾーンに細い十字の足 (サーマルリリーフ) でつなぎます。熱が逃げにくくなり、はんだ付けしやすくなります。オフにすると全面でつながります"));
                    if ui.button(i18n::tr("重なるベタを結合")).on_hover_text(i18n::tr("同じネット・同じ層で、重なったり接したりしているベタを、1 枚のベタにまとめ、最終の頂点 (直線) に変換します (足す・引くの図形と円弧は、頂点の列になります)。見た目はつながっていても、内部のデータが別々のときに使います")).clicked() {
                        self.checkpoint();
                        let n = pcb_core::zonefill::merge_all_zones(&mut self.board);
                        let (points, dropped) = pcb_core::zonefill::tidy_zones(&mut self.board);
                        self.status = format!("重なるベタを {n} 枚まとめ、余分な頂点 {points} 個と、小さな引く図形 {dropped} 個を整理しました");
                    }
                }
            }
            _ => {}
        }
    }

    /// What a click draws: lines, arcs, a circle or a rectangle (for the board outline, a pour and the hole of a pour).
    pub(super) fn poly_mode_ui(&mut self, ui: &mut egui::Ui) {
        let before = self.poly_mode;
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.poly_mode, PolyMode::Lines, i18n::tr("直線")).on_hover_text(i18n::tr("クリックした点を直線でつなぎます。最初の点をクリックするか、右クリック・ダブルクリック・Enter で、囲んで確定します"));
            ui.selectable_value(&mut self.poly_mode, PolyMode::ArcThrough, i18n::tr("3点円弧")).on_hover_text(i18n::tr("円弧を描きます。最後の点から、途中の点をクリックし、終点をクリックします。直線と切り替えながら描けます"));
            ui.selectable_value(&mut self.poly_mode, PolyMode::ArcTangent, i18n::tr("接線円弧")).on_hover_text(i18n::tr("直前の線 (または円弧) になめらかにつながる円弧を、終点を1回クリックするだけで描きます"));
            ui.selectable_value(&mut self.poly_mode, PolyMode::Circle, i18n::tr("円")).on_hover_text(i18n::tr("中心をクリックし、円周の点をクリックします。その場で確定します"));
            ui.selectable_value(&mut self.poly_mode, PolyMode::Rect, i18n::tr("長方形")).on_hover_text(i18n::tr("反対側の2つの角をクリックします。その場で確定します"));
        });
        if self.poly_mode == PolyMode::Rect {
            ui.add(egui::DragValue::new(&mut self.shape_round).speed(0.1).range(0.0..=50.0).prefix(i18n::tr("角の丸め R ")).suffix(" mm")).on_hover_text(i18n::tr("長方形の角の丸みの半径。0 なら角張ったままです"));
        }
        if self.poly_mode != before {
            self.arc_mid = None;
            self.pending = None;
        }
    }

    pub(super) fn selection_panel(&mut self, ui: &mut egui::Ui) {
        if self.multi.len() > 1 {
            ui.separator();
            ui.label(i18n::trf!("{} 個を選択中", self.multi.len()));
            if ui.button(i18n::tr("選択を解除 (Esc)")).on_hover_text(i18n::tr("選択をやめます (Esc キーでも同じです)")).clicked() {
                self.multi.clear();
            }
            self.panel_align(ui);
            return;
        }
        let Some(sel) = self.selected else { return };
        ui.separator();
        ui.label(i18n::tr("選択中"));
        match sel {
            Item::Track(i) if i < self.board.tracks.len() => {
                let mut w = self.board.tracks[i].width;
                let r = ui.add(egui::DragValue::new(&mut w).speed(0.01).range(0.05..=5.0).prefix(i18n::tr("線幅 ")).suffix(" mm")).on_hover_text(i18n::tr("選んだ配線の太さ。大きい電流を流す線は太くします"));
                if r.drag_started() || r.gained_focus() {
                    self.checkpoint();
                }
                if r.changed() {
                    self.board.tracks[i].width = w;
                }
                if ui.button(i18n::tr("このネットの配線を全部消す")).on_hover_text(i18n::tr("この配線がつながっているネットの、配線とビアをすべて消します (パッドは残り、未配線の線が戻ります)。引き直したいときに使います")).clicked() {
                    self.rip_up_selected_net(i);
                    return;
                }
                let mut s = self.board.tracks[i].side;
                ui.horizontal(|ui| {
                    for l in self.board.copper_layers() {
                        ui.selectable_value(&mut s, l, l.label()).on_hover_text(i18n::tr("この配線をこの層に移します"));
                    }
                });
                if s != self.board.tracks[i].side {
                    self.checkpoint();
                    self.board.tracks[i].side = s;
                }
            }
            Item::Via(i) if i < self.board.vias.len() => {
                let (mut d, mut dr) = (self.board.vias[i].diameter, self.board.vias[i].drill);
                let r1 = ui.add(egui::DragValue::new(&mut d).speed(0.05).range(0.3..=3.0).prefix(i18n::tr("径 ")).suffix(" mm")).on_hover_text(i18n::tr("選んだビアの外径 (銅の丸の大きさ)"));
                let r2 = ui.add(egui::DragValue::new(&mut dr).speed(0.05).range(0.2..=2.0).prefix(i18n::tr("穴 ")).suffix(" mm")).on_hover_text(i18n::tr("選んだビアの穴径"));
                if r1.drag_started() || r1.gained_focus() || r2.drag_started() || r2.gained_focus() {
                    self.checkpoint();
                }
                if r1.changed() || r2.changed() {
                    self.board.vias[i].diameter = d;
                    self.board.vias[i].drill = dr;
                }
                let nets = self.board_nets();
                let mut vnet = self.board.vias[i].net.clone();
                let before = vnet.clone();
                egui::ComboBox::from_id_salt("sel_via_net").selected_text(if vnet.is_empty() { "ネットなし (触れたもののネット)".to_string() } else { vnet.clone() }).show_ui(ui, |ui| {
                    ui.selectable_value(&mut vnet, String::new(), i18n::tr("ネットなし (触れたもののネット)"));
                    for n in &nets {
                        ui.selectable_value(&mut vnet, n.clone(), n);
                    }
                }).response.on_hover_text(i18n::tr("このビアのネット。ネットを決めると、ベタ (同じネット) につながり、ほかのパッドや配線に触れると短絡になります"));
                if vnet != before {
                    self.checkpoint();
                    self.board.vias[i].net = vnet;
                }
            }
            Item::Zone(i) if i < self.board.zones.len() => {
                let nets = self.nets();
                let mut net = self.board.zones[i].net.clone();
                let before = net.clone();
                egui::ComboBox::from_label(i18n::tr("ゾーンのネット")).selected_text(net.clone()).show_ui(ui, |ui| {
                    for n in &nets {
                        ui.selectable_value(&mut net, n.clone(), n);
                    }
                }).response.on_hover_text(i18n::tr("このゾーンでつなぐネット"));
                let mut c = self.board.zones[i].clearance;
                let r = ui.add(egui::DragValue::new(&mut c).speed(0.05).range(0.1..=2.0).prefix(i18n::tr("隙間 ")).suffix(" mm")).on_hover_text(i18n::tr("このゾーンと、ほかのネットの間をあける距離"));
                let mut th = self.board.zones[i].thermal;
                let rt = ui.checkbox(&mut th, i18n::tr("サーマルリリーフ")).on_hover_text(i18n::tr("スルーホールのパッドを、細い十字の足でゾーンにつなぎます (はんだ付けしやすくなります)"));
                // the shapes of the pour: what each one does, how many corners and arcs it has; one can be turned round (add / take away) or deleted
                let mut flip: Option<usize> = None;
                let mut remove: Option<usize> = None;
                let mut bake = false;
                ui.separator();
                ui.label(i18n::trf!("図形 {} 個 (足す図形の和から、引く図形の和を引いたものがベタです)", self.board.zones[i].shapes.len()));
                for (k, zs) in self.board.zones[i].shapes.iter().enumerate() {
                    ui.horizontal(|ui| {
                        let arcs = zs.shape.verts.iter().filter(|v| v.arc_mid.is_some()).count();
                        ui.label(i18n::trf!("{} {}  頂点 {} 円弧 {}", k + 1, if zs.op == pcb_core::model::ShapeOp::Add { "＋足す" } else { "−引く" }, zs.shape.verts.len(), arcs));
                        if ui.small_button(i18n::tr("足す↔引く")).on_hover_text(i18n::tr("この図形が、ベタに足すか、ベタから引くかを入れ替えます")).clicked() {
                            flip = Some(k);
                        }
                        if ui.small_button(i18n::tr("消す")).on_hover_text(i18n::tr("この図形を消します (ほかの図形は残ります)")).clicked() {
                            remove = Some(k);
                        }
                    });
                }
                if ui.button(i18n::tr("頂点に確定")).on_hover_text(i18n::tr("ベタの面 (足す図形の和から、引く図形の和を引いたもの) を、最終の頂点の列 (直線だけ) に変換します。円弧は短い直線になり、図形は面ごと・穴ごとの 1 つずつになります。あとで円弧として直すことはできなくなります")).clicked() {
                    bake = true;
                }
                if flip.is_some() || remove.is_some() || bake {
                    self.checkpoint();
                    let z = &mut self.board.zones[i];
                    if let Some(k) = flip {
                        z.shapes[k].op = if z.shapes[k].op == pcb_core::model::ShapeOp::Add { pcb_core::model::ShapeOp::Sub } else { pcb_core::model::ShapeOp::Add };
                    }
                    if let Some(k) = remove {
                        z.shapes.remove(k);
                    }
                    if bake {
                        pcb_core::zonefill::bake_zone(&mut self.board, i);
                        pcb_core::zonefill::tidy_zones(&mut self.board);
                    }
                    return;
                }
                ui.separator();
                // where pours of other nets overlap, the one in front keeps its copper and the other gives way
                let (mut front, mut back) = (false, false);
                ui.horizontal(|ui| {
                    front = ui.button(i18n::tr("最前面へ")).on_hover_text(i18n::tr("別のネットのゾーンと重なったとき、このゾーンの銅を優先します (相手が引き下がり、クリアランスの隙間があきます)")).clicked();
                    back = ui.button(i18n::tr("最背面へ")).on_hover_text(i18n::tr("別のネットのゾーンと重なったとき、相手を優先します (このゾーンが引き下がります)")).clicked();
                });
                if front || back {
                    self.checkpoint();
                    let at = self.board.move_zone(i, front);
                    self.selected = Some(Item::Zone(at));
                    return;
                }
                if net != before || r.drag_started() || r.gained_focus() || rt.changed() {
                    self.checkpoint();
                }
                let z = &mut self.board.zones[i];
                z.net = net;
                if r.changed() {
                    z.clearance = c;
                }
                z.thermal = th;
            }
            Item::Part(i) if i < self.board.parts.len() => {
                let p = &self.board.parts[i];
                let (r, side) = (p.refdes.clone(), p.side);
                ui.label(format!("{}  {}  [{}]  {}", p.refdes, p.value, p.footprint, if side == Side::Bottom { "裏面実装" } else { "表面実装" }));
                let _ = r;
                let locked = self.board.is_locked(&r);
                if ui.button(if locked { i18n::tr("固定を外す") } else { i18n::tr("固定する") }).on_hover_text(i18n::tr("固定すると、ドラッグ・回転 (R)・面の付け替え (B) ができなくなり、範囲選択に巻き込んで動かす心配もなくなります。「部品を並べて外形を決める」(自動配置) でも動かず、ほかの部品がそのまわりに並びます。コネクタのように置き場所の決まった部品や、配置が済んだ部品に使います。基板ファイルに保存されます")).clicked() {
                    self.checkpoint();
                    if locked {
                        self.board.locked_parts.retain(|x| x != &r);
                    } else {
                        self.board.locked_parts.push(r.clone());
                    }
                    self.status = if locked { format!("{r} の固定を外しました") } else { format!("{r} を固定しました") };
                }
                if ui.button(if side == Side::Bottom { i18n::tr("表面に移す (B)") } else { i18n::tr("裏面に移す (B)") }).on_hover_text(i18n::tr("部品を反対の面に付け替えます。裏面の部品は左右が反転します (B キーでも)")).clicked() {
                    self.flip_selected_part();
                }
                let mut show = !self.board.hidden_labels.contains(&r);
                if ui.checkbox(&mut show, i18n::tr("部品番号を印刷する")).on_hover_text(i18n::tr("この部品の部品番号 (R1 など) をシルクに印刷します。部品が密集して置き場がないときは外せます (組み立てるときは、部品表と位置のファイルで場所が分かります)")).changed() {
                    self.checkpoint();
                    if let Err(e) = pcb_core::silkedit::set_label(&mut self.board, &r, None, None, Some(!show)) {
                        self.status = e;
                    }
                }
            }
            Item::Hole(i) if i < self.board.holes.len() => {
                ui.label(i18n::trf!("取付穴 φ{:.2} mm", self.board.holes[i].drill));
            }
            Item::Label(i) if i < self.board.texts.len() => {
                let r = self.board.texts[i].refdes.clone();
                ui.label(i18n::trf!("{r} のシルク文字 (部品番号)"));
                ui.small(i18n::tr("ドラッグで、好きな所へ動かせます。部品を動かすと、文字も付いてきます"));
                let min = self.board.rules.min_silk_text;
                let mut h = self.board.texts[i].height;
                let resp = ui
                    .add(egui::DragValue::new(&mut h).speed(0.05).range(min..=pcb_core::silkedit::MAX_TEXT_HEIGHT).prefix(i18n::tr("文字の高さ ")).suffix(" mm"))
                    .on_hover_text(i18n::trf!("部品番号の文字の高さ。発注先が読めるように印刷できる {min:.2} mm より小さくはできません (下限は、発注の窓で選ぶ発注先で変わります)"));
                if resp.drag_started() || resp.gained_focus() {
                    self.checkpoint();
                }
                let mut result: Option<Result<String, String>> = None;
                if resp.changed() {
                    result = Some(pcb_core::silkedit::set_label(&mut self.board, &r, Some(h), None, None).map(|_| String::new()));
                }
                let (mut turn, mut all, mut hide) = (false, false, false);
                ui.horizontal(|ui| {
                    turn = ui.button(i18n::tr("回す (R)")).on_hover_text(i18n::tr("文字を 90° 回します (R キーでも)")).clicked();
                    all = ui.button(i18n::tr("全部この高さに")).on_hover_text(i18n::tr("基板のすべての部品番号の文字を、この高さにそろえます")).clicked();
                });
                ui.horizontal(|ui| {
                    hide = ui.button(i18n::tr("印刷しない")).on_hover_text(i18n::tr("この部品番号をシルクに印刷しません (部品が密集して置き場がないとき)。部品を選ぶと「部品番号を印刷する」で戻せます")).clicked();
                    let moved = self.board.label_offsets.contains_key(&r);
                    if ui.add_enabled(moved, egui::Button::new(i18n::tr("元の位置に戻す (Delete)"))).on_hover_text(i18n::tr("動かした文字を、自動で決まる位置 (部品の上) に戻します (Delete キーでも)。文字そのものは消えません")).clicked() {
                        self.delete(sel);
                    }
                });
                if turn {
                    self.checkpoint();
                    self.board.turn_label(&r);
                }
                if all {
                    self.checkpoint();
                    result = Some(pcb_core::silkedit::set_all_labels(&mut self.board, h).map(|n| format!("部品番号 {n} 個の文字を、高さ {h:.2} mm にそろえました")));
                }
                if hide {
                    self.checkpoint();
                    result = Some(pcb_core::silkedit::set_label(&mut self.board, &r, None, None, Some(true)).map(|_| format!("{r} の部品番号を、印刷しないようにしました (部品を選ぶと戻せます)")));
                }
                match result {
                    Some(Ok(s)) if !s.is_empty() => self.status = s,
                    Some(Err(e)) => self.status = e,
                    _ => {}
                }
                // the part was placed again: its text is at another index now (or gone, when hidden)
                self.selected = match self.board.texts.iter().position(|t| t.refdes == r) {
                    Some(k) => Some(Item::Label(k)),
                    None => self.board.parts.iter().position(|p| p.refdes == r).map(Item::Part),
                };
            }
            Item::Text(i) if i < self.board.texts.len() => {
                let pin = self.board.texts[i].refdes.contains('#');
                ui.label(if pin { i18n::tr("シルクの文字 (ピン名)") } else { i18n::tr("シルクの文字") });
                // what is typed stays in the box while the font cannot print it; the board gets only a text it prints
                if self.text_edit.as_ref().is_none_or(|e| e.0 != i) {
                    self.text_edit = Some((i, self.board.texts[i].text.clone(), None));
                }
                let (_, mut buf, mut err) = self.text_edit.clone().unwrap();
                let resp = ui
                    .add(egui::TextEdit::singleline(&mut buf).desired_width(160.0).hint_text(i18n::tr("例: MY BOARD V1.0")))
                    .on_hover_text(i18n::tr("印刷する文字。英字 (小文字は大文字で印刷)・数字・空白と、. , - + / : # % ( ) などの記号が使えます。日本語は印刷できません"));
                if resp.gained_focus() {
                    self.checkpoint();
                }
                if resp.changed() {
                    let c = pcb_core::silkedit::TextChange { text: Some(buf.clone()), ..Default::default() };
                    err = pcb_core::silkedit::set_text(&mut self.board, i, &c).err();
                }
                if let Some(e) = &err {
                    ui.colored_label(crate::theme::ERROR, e);
                }
                self.text_edit = Some((i, buf, err));
                let min = self.board.rules.min_silk_text;
                let mut h = self.board.texts[i].height;
                let resp = ui
                    .add(egui::DragValue::new(&mut h).speed(0.05).range(min..=pcb_core::silkedit::MAX_TEXT_HEIGHT).prefix(i18n::tr("文字の高さ ")).suffix(" mm"))
                    .on_hover_text(i18n::trf!("文字の高さ。発注先が読めるように印刷できる {min:.2} mm より小さくはできません (下限は、発注の窓で選ぶ発注先で変わります)"));
                if resp.drag_started() || resp.gained_focus() {
                    self.checkpoint();
                }
                if resp.changed() {
                    let c = pcb_core::silkedit::TextChange { height: Some(h), ..Default::default() };
                    if let Err(e) = pcb_core::silkedit::set_text(&mut self.board, i, &c) {
                        self.status = e;
                    }
                }
                let side = self.board.texts[i].side;
                let (mut turn, mut flip) = (false, false);
                ui.horizontal(|ui| {
                    turn = ui.button(i18n::tr("回す (R)")).on_hover_text(i18n::tr("文字を 90° 回します (R キーでも)")).clicked();
                    flip = ui
                        .button(if side == Side::Bottom { i18n::tr("表面に移す") } else { i18n::tr("裏面に移す") })
                        .on_hover_text(i18n::tr("文字を反対の面のシルクに移します。裏面の文字は、裏から見て読めるように左右が反転して印刷されます"))
                        .clicked();
                });
                if turn || flip {
                    self.checkpoint();
                    let t = &mut self.board.texts[i];
                    if turn {
                        t.rot = (t.rot + 1) % 4;
                    }
                    if flip {
                        t.side = if side == Side::Bottom { Side::Top } else { Side::Bottom };
                    }
                }
            }
            Item::Silk(i) if i < self.board.silk.len() => {
                let min = self.board.rules.min_silk_width;
                let mut w = self.board.silk[i].width;
                let resp = ui
                    .add(egui::DragValue::new(&mut w).speed(0.01).range(min..=pcb_core::silkedit::MAX_LINE_WIDTH).prefix(i18n::tr("線幅 ")).suffix(" mm"))
                    .on_hover_text(i18n::trf!("シルクの線の太さ。発注先が印刷できる {min:.3} mm より細くはできません (下限は、発注の窓で選ぶ発注先で変わります)。次に引く線も、この太さになります"));
                if resp.drag_started() || resp.gained_focus() {
                    self.checkpoint();
                }
                if resp.changed() {
                    match pcb_core::silkedit::set_line_width(&mut self.board, i, w) {
                        Ok(()) => self.silk_width = w,
                        Err(e) => self.status = e,
                    }
                }
            }
            _ => {}
        }
        if !matches!(sel, Item::Part(_) | Item::Label(_)) && ui.button(i18n::tr("削除 (Delete)")).on_hover_text(i18n::tr("選んだものを消します (Delete キー)。部品は消せません")).clicked() {
            self.delete(sel);
        }
    }
}
