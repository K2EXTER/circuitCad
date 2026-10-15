use super::*;

impl PartForm {
    /// The ① basic information tab (name, category, prefix, value, description, data sheet URL) and the easy templates.
    pub(super) fn tab_basic(&mut self, ui: &mut egui::Ui) {
        egui::Grid::new("part_basic").num_columns(2).show(ui, |ui| {
            ui.label("部品名");
            ui.text_edit_singleline(&mut self.name).on_hover_text("部品の名前 (例: LM358、2N3904)。部品の一覧や回路図の値に出ます");
            ui.end_row();
            ui.label("カテゴリ");
            ui.text_edit_singleline(&mut self.category).on_hover_text("一覧のグループ名 (例: オペアンプ、トランジスタ)");
            ui.end_row();
            ui.label("部品番号の頭文字");
            ui.text_edit_singleline(&mut self.prefix).on_hover_text("回路図で付く部品番号の頭文字。IC は U、トランジスタは Q、抵抗は R、ダイオードは D");
            ui.end_row();
            ui.label("値 (省略で部品名)");
            ui.text_edit_singleline(&mut self.value).on_hover_text("回路図に表示する値。空なら部品名が入ります");
            ui.end_row();
            ui.label("説明");
            ui.text_edit_singleline(&mut self.description).on_hover_text("一覧で部品にポインタを置いたときに出る説明");
            ui.end_row();
            ui.label("データシートURL");
            ui.text_edit_singleline(&mut self.datasheet).on_hover_text("データシート (メーカーの仕様書) の URL。あとで見返すためのメモです");
            ui.end_row();
        });

        ui.add_space(6.0);
        ui.label(egui::RichText::new("かんたん作成 (テンプレート)").strong());
            ui.small("チップ部品や充電ICは、データシートの数字を入れるだけで、下の欄を自動で埋められます");
            self.passive_ui(ui);
            self.template_ui(ui);
    }

    /// The tab of the data sheet's ratings.
    pub(super) fn tab_ratings(&mut self, ui: &mut egui::Ui) {
            ui.label("部品を追加するときは、メーカーのデータシートを開いて調べます。見る表は 2 つです: 「絶対最大定格 (Absolute Maximum Ratings)」 = 超えると壊れる値、「電気的特性 (Electrical Characteristics)」 = 動作中の代表的な値。調べて入れておくと、回路のシミュレーション結果と比べて、超えそうなときに警告できます");
            // the kind chosen at the top decides which items to look up; only an IC (op-amp, regulator ...) or no choice
            // at all leaves the question open
            let decided = self.checklist_for_type();
            if let Some(k) = decided {
                self.rt_kind = k;
            }
            ui.horizontal(|ui| {
                if decided.is_some() {
                    ui.label(format!("調べる項目: {}", CHECKLISTS[self.rt_kind.min(CHECKLISTS.len() - 1)].0)).on_hover_text("上で選んだ部品の種類に合わせた項目です。変えるときは、いちばん上の「部品の種類」を選び直します");
                } else {
                    ui.label(if self.ptype == Some(PartType::Ic) { "どの IC?" } else { "部品の種類" });
                    egui::ComboBox::from_id_salt("rt_kind").selected_text(CHECKLISTS[self.rt_kind.min(CHECKLISTS.len() - 1)].0).show_ui(ui, |ui| {
                        for (k, (name, _, _)) in CHECKLISTS.iter().enumerate() {
                            ui.selectable_value(&mut self.rt_kind, k, *name);
                        }
                    }).response.on_hover_text("選ぶと、その種類で調べるとよい項目が出ます");
                }
                if ui.button("調べる項目のチェックリストを入れる").on_hover_text("下の「そのほかの情報」に、調べる項目を並べます。データシートを見て、値を書き込んでください (すでに書いた行は、そのまま残ります)").clicked() {
                    self.rt_rows = merge_checklist(&self.rt_rows, self.rt_kind);
                }
            });
            ui.label(egui::RichText::new(CHECKLISTS[self.rt_kind.min(CHECKLISTS.len() - 1)].1).weak());
            egui::Grid::new("part_ratings_grid").num_columns(2).show(ui, |ui| {
                ui.label("最大電圧 [V]").on_hover_text("部品にかけてよい最大の電圧。ダイオードなら逆耐圧 (VRRM)、トランジスタなら Vce の最大 (VCEO)、MOSFET なら Vds の最大");
                ui.add(egui::TextEdit::singleline(&mut self.rt_v).desired_width(80.0)).on_hover_text("例: 40 / 1000");
                ui.end_row();
                ui.label("最大電流 [A]").on_hover_text("流してよい最大の連続電流 (ダイオードなら平均順電流 IF、トランジスタなら Ic の最大)");
                ui.add(egui::TextEdit::singleline(&mut self.rt_i).desired_width(80.0)).on_hover_text("例: 1 / 200m");
                ui.end_row();
                ui.label("最大電力 [W]").on_hover_text("部品が熱にしてよい最大の電力 (許容損失 Ptot)。周囲温度が高いと下がります。データシートの条件を確認します");
                ui.add(egui::TextEdit::singleline(&mut self.rt_p).desired_width(80.0)).on_hover_text("例: 0.5 / 625m / 1");
                ui.end_row();
            });
            // only where it belongs: an electrolytic / tantalum capacitor, a part that already has such figures, or
            // a part whose kind was not chosen
            let filled = !self.rt_ripple.trim().is_empty() || !self.rt_life_h.trim().is_empty();
            let polar = self.pas_polar || filled;
            if polar || self.ptype.is_none() {
            egui::CollapsingHeader::new("電解コンデンサの項目 (リップル電流と寿命)").id_salt("rt_electrolytic").default_open(polar).show(ui, |ui| {
                ui.small("入れると、過渡解析の結果に、このコンデンサに流れたリップル電流が定格を超えていないか、推定寿命がどれくらいかが出ます。データシートの「定格リップル電流」「寿命 (耐久性)」「リップル電流の周波数補正係数」の表から写します");
                egui::Grid::new("rt_electrolytic_grid").num_columns(2).show(ui, |ui| {
                    ui.label("定格リップル電流 [A rms]").on_hover_text("コンデンサに流してよい、交流分の実効値 (データシートの Rated Ripple Current)。mA rms で書いてあるときは 540m のように入れます。最高温度 (105℃ など) での値です");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.rt_ripple).desired_width(80.0).hint_text("540m")).on_hover_text("例: 540m / 1.2。データシートの表の、容量と電圧に合う行の値");
                        ui.label("周波数");
                        ui.add(egui::TextEdit::singleline(&mut self.rt_ripple_f).desired_width(60.0)).on_hover_text("その値が決められている周波数。データシートの表の見出しにあります (たいてい 100kHz。120Hz のこともあります)");
                    });
                    ui.end_row();
                    ui.label("周波数の補正係数").on_hover_text("リップル電流の定格に掛ける係数の表 (Multiplier for Ripple Current)。低い周波数ほど、流してよい電流が小さくなります。1行に「周波数 係数」");
                    ui.add(egui::TextEdit::multiline(&mut self.rt_ripple_table).desired_rows(4).desired_width(160.0).hint_text("120 0.55\n1k 0.77\n10k 0.94\n100k 1.0")).on_hover_text("例: 120 0.55 / 1k 0.77 / 10k 0.94 / 100k 1.0。容量の範囲ごとに表が分かれているときは、この部品の容量の行を入れます。空のときは、周波数によらず定格のままです");
                    ui.end_row();
                    ui.label("寿命 [時間]").on_hover_text("定格電圧・定格リップル電流・最高温度で使ったときの寿命 (Endurance)。ケースの大きさで違うことがあります (例: 6000 / 10000)");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.rt_life_h).desired_width(80.0).hint_text("6000")).on_hover_text("例: 6000 / 10000");
                        ui.label("温度 [℃]");
                        ui.add(egui::TextEdit::singleline(&mut self.rt_life_t).desired_width(50.0)).on_hover_text("その寿命が決められている温度。最高使用温度 (105 など)");
                    });
                    ui.end_row();
                });
            });
            }
            ui.label("そのほかの情報 (1行に1つ。「項目 | 値 | 条件」。値が空の行は、まだ調べていない項目として残ります)");
            ui.add(egui::TextEdit::multiline(&mut self.rt_rows).desired_rows(5).desired_width(f32::INFINITY).hint_text("順方向電圧 | 0.9V | IF = 1A"))
                .on_hover_text("定格・代表値を、データシートのまま書き写します。条件 (電流や温度) も一緒に書くと、あとで比べやすくなります。部品の一覧でポインタを置いたときに出ます");
            let (max_given, rows_given, rows_open) = self.ratings_progress();
            let summary = format!("入力済み: 最大定格 {max_given} / 3 項目、そのほかの情報 {rows_given} 行{}", if rows_open > 0 { format!(" (まだ調べていない項目 {rows_open} 行)") } else { String::new() });
            if max_given == 0 && rows_given == 0 {
                ui.colored_label(Color32::from_rgb(220, 160, 40), "データシートの情報が空です。空のまま保存もできますが、定格のチェックと「部品選びのヒント」が使えません。データシートを開いて、調べてみましょう");
            } else {
                ui.label(summary);
            }
            if self.datasheet.trim().is_empty() && (max_given > 0 || rows_given > 0) {
                ui.colored_label(Color32::from_rgb(220, 160, 40), "データシートの URL が空です。調べた元を残しておくと、あとで見返せます (上の「データシートURL」)");
            }
    }

    /// The tab of the simulation model.
    pub(super) fn tab_sim(&mut self, ui: &mut egui::Ui) {
            ui.label("シミュレーションモデル (部品の特性)");
            ui.small("メーカーのサイトなどで見つけた SPICE モデル (`.model` / `.subckt`) をそのまま貼ると、種類・名前・ピンを自動で入力します");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.sim, SimChoice::None, "なし").on_hover_text("シミュレーションしない部品にします (基板づくりだけに使う部品)");
                ui.selectable_value(&mut self.sim, SimChoice::Model, ".model (ダイオード/トランジスタ等)").on_hover_text("ダイオードやトランジスタなどの 1 素子を `.model` 文で書いたモデル");
                ui.selectable_value(&mut self.sim, SimChoice::Subckt, ".subckt (IC マクロモデル)").on_hover_text("IC の中身を、複数の素子の組み合わせ (`.subckt`) で書いたモデル");
            });
            let mut autofill = false;
            let pasted = ui.add(
                egui::TextEdit::multiline(&mut self.sim_text)
                    .desired_rows(if self.sim == SimChoice::None { 3 } else { 5 })
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("ここに SPICE モデルを貼る: .model D1 D(IS=1n)  /  .subckt NAME 1 2 3 ... .ends"),
            ).on_hover_text("貼り付けると、モデルの種類・名前・端子からフォームを自動で埋めます。ピン表を自分で書いてあるときは、その名前 (A K / C B E など) に合わせて対応づけます");
            // a paste into an empty model box fills the form right away
            if pasted.changed() && self.sim == SimChoice::None && !self.sim_text.trim().is_empty() {
                autofill = true;
            }
            ui.horizontal(|ui| {
                if ui.button("貼ったモデルから自動入力").on_hover_text("上の SPICE 文を読んで、種類・モデル名・素子記号・ピン表・ノード順を入力します。何度押しても大丈夫です").clicked() {
                    autofill = true;
                }
                if ui.button("試し読み").on_hover_text("このモデルをシミュレータに読ませて、動くかを保存の前に確かめます。読めない書き方があれば、直し方を出します").clicked() {
                    self.trial_message = Some(self.trial_read());
                }
                if ui.button("SPICEファイルを読む…").on_hover_text("メーカーからダウンロードした .lib / .mod / .sub / .cir / .txt のファイルを読み込み、上の欄に入れて自動入力します").clicked()
                    && let Some(p) = rfd::FileDialog::new().add_filter("SPICE モデル", &["lib", "mod", "sub", "cir", "spice", "txt", "inc"]).pick_file()
                {
                    match std::fs::read_to_string(&p) {
                        Ok(t) => {
                            self.sim_text = t;
                            autofill = true;
                        }
                        Err(e) => self.spice_message = Some(Err(format!("ファイルを読めませんでした: {e}。文字コードが UTF-8 でないときは、テキストエディタで開いて貼り付けてください"))),
                    }
                }
            });
            if autofill {
                self.spice_message = Some(self.autofill_from_spice());
                self.trial_message = None;
            }
            match &self.trial_message {
                Some(super::trial::Trial::Ok(m)) => {
                    ui.colored_label(Color32::from_rgb(60, 170, 90), m);
                }
                Some(super::trial::Trial::Warn(m)) => {
                    ui.colored_label(Color32::from_rgb(220, 160, 40), m);
                }
                Some(super::trial::Trial::Err(m)) => {
                    ui.colored_label(Color32::from_rgb(230, 60, 60), m);
                }
                None => {}
            }
            match &self.spice_message {
                Some(Ok(m)) => {
                    ui.colored_label(Color32::from_rgb(60, 170, 90), m);
                }
                Some(Err(e)) => {
                    ui.colored_label(Color32::from_rgb(230, 60, 60), e);
                }
                None => {}
            }
            if self.sim != SimChoice::None {
                ui.horizontal(|ui| {
                    if self.sim == SimChoice::Model {
                        ui.label("素子記号");
                        ui.add(egui::TextEdit::singleline(&mut self.sim_element).desired_width(30.0)).on_hover_text("素子の種類の記号。ダイオード D、バイポーラトランジスタ Q、MOSFET M、スイッチ S");
                    }
                    ui.label("モデル名");
                    ui.add(egui::TextEdit::singleline(&mut self.sim_name).desired_width(120.0)).on_hover_text("`.model` や `.subckt` に付けた名前。上の本文と同じ名前にします");
                    ui.label("ノード順のピン番号");
                    ui.add(egui::TextEdit::singleline(&mut self.sim_pins).desired_width(160.0).hint_text("例: A K")).on_hover_text("モデルの端子の順番に、部品のピン番号を並べます。例: ダイオードなら A K (アノード カソード)、トランジスタなら C B E の順");
                });
                ui.label("このモデルの限界 (1行に1つ。実物との違いを書くと、部品を選んだときに表示されます)");
                ui.add(
                    egui::TextEdit::multiline(&mut self.sim_limits)
                        .desired_rows(3)
                        .desired_width(f32::INFINITY)
                        .hint_text("例: 温度による変化は扱いません。
    接合容量は扱いません。"),
                );
            }

    }

    /// The tab of the pins.
    pub(super) fn tab_pins(&mut self, ui: &mut egui::Ui) {
            ui.label("ピン (1行に「番号 名前 [left|right|top|bottom]」。向き省略で前半が左・後半が右。NC は未接続ピン扱い)");
            ui.add(egui::TextEdit::multiline(&mut self.pins_text).desired_rows(6).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace)).on_hover_text("ピンの定義を1行に1つ書きます。例: `1 IN+ left` (番号 名前 向き)。向きは left / right / top / bottom で、省略すると前半が左、後半が右になります。NC は未接続のピンです");
    }

    /// The tab of the board shape: package, thermal pad, body, extra pads and holes.
    pub(super) fn tab_package(&mut self, ui: &mut egui::Ui) {
            ui.label("パッケージ (データシートの推奨ランドパターン寸法 mm。パッド数はピン数に合わせます)");
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.pkg, PkgKind::Chip2, "2端子チップ").on_hover_text("抵抗・コンデンサ・ダイオードなど、足が2つのチップ部品");
                ui.selectable_value(&mut self.pkg, PkgKind::Dual, "デュアル(SOIC/DIP)").on_hover_text("足が左右の2列に並ぶ IC (SOIC、DIP など)");
                ui.selectable_value(&mut self.pkg, PkgKind::Quad, "クアッド(QFP)").on_hover_text("足が四方に並ぶ IC (QFP、QFN など)");
                ui.selectable_value(&mut self.pkg, PkgKind::Row, "1列(ヘッダ)").on_hover_text("足が1列に並ぶ部品 (ピンヘッダ、コネクタなど)");
            });
            ui.horizontal(|ui| {
                ui.label("標準のフットプリント");
                let shown = if self.fp_preset.is_empty() { "(下の寸法を使う)".to_string() } else { self.fp_preset.clone() };
                egui::ComboBox::from_id_salt("fp_preset").selected_text(shown).width(200.0).show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.fp_preset, String::new(), "(下の寸法を使う)");
                    for n in pcb_core::footprints::names() {
                        ui.selectable_value(&mut self.fp_preset, n.clone(), n);
                    }
                }).response.on_hover_text("0603 や 0805 のチップ部品、DIP・ピンヘッダなど、あらかじめ用意された形です。選ぶと、下の寸法の代わりにこの形を使います。ピン番号は、形ごとのピン名 (チップは 1 と 2、ダイオードは K と A、トランジスタは E B C など) に合わせます");
            });
            let dv = |ui: &mut egui::Ui, label: &str, v: &mut f64| {
                ui.add(egui::DragValue::new(v).speed(0.01).range(0.0..=100.0).prefix(format!("{label} ")).suffix(" mm")).on_hover_text(pkg_tip(label));
            };
            ui.horizontal_wrapped(|ui| {
                dv(ui, "ピッチ", &mut self.pitch);
                match self.pkg {
                    PkgKind::Chip2 => {
                        dv(ui, "パッド長", &mut self.len);
                        dv(ui, "パッド幅", &mut self.width);
                    }
                    PkgKind::Dual => {
                        dv(ui, "列間隔", &mut self.row);
                        dv(ui, "パッド長", &mut self.len);
                        dv(ui, "パッド幅", &mut self.width);
                        ui.checkbox(&mut self.tht, "スルーホール(DIP)").on_hover_text("足を基板の穴に通す形 (DIP) にします。オフなら面実装 (表面にはんだ付け)");
                        if self.tht {
                            dv(ui, "穴径", &mut self.drill);
                        }
                    }
                    PkgKind::Quad => {
                        dv(ui, "対向パッド間隔", &mut self.span);
                        dv(ui, "パッド長", &mut self.len);
                        dv(ui, "パッド幅", &mut self.width);
                    }
                    PkgKind::Row => {
                        dv(ui, "パッド径", &mut self.pad_d);
                        dv(ui, "穴径", &mut self.drill);
                    }
                }
            });
            if matches!(self.pkg, PkgKind::Dual | PkgKind::Quad) {
                ui.checkbox(&mut self.thermal_on, "放熱パッド (ICの裏の大きなパッド) を付ける").on_hover_text("QFN・ESOP・SOT-223 などの、裏や中央にある大きなパッド。ここで大きさとピン番号を入れると、パッケージの中心に置きます。ピン表にもそのピンの行を足してください (例: 9 GND)。発注前チェックで、サーマルビアが足りないと注意します");
                if self.thermal_on {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("ピン番号");
                        ui.add(egui::TextEdit::singleline(&mut self.thermal_pin).desired_width(40.0).hint_text("9")).on_hover_text("放熱パッドのピン番号。ピン表にある番号にします (GND につなぐことが多い)");
                        ui.add(egui::DragValue::new(&mut self.thermal_w).speed(0.01).range(0.1..=50.0).prefix("幅 ").suffix(" mm")).on_hover_text("データシートの推奨ランドパターンの、放熱パッドの幅");
                        ui.add(egui::DragValue::new(&mut self.thermal_h).speed(0.01).range(0.1..=50.0).prefix("長さ ").suffix(" mm")).on_hover_text("データシートの推奨ランドパターンの、放熱パッドの長さ");
                    });
                }
            }
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::DragValue::new(&mut self.height).speed(0.01).range(0.0..=100.0).prefix("高さ ").suffix(" mm")).on_hover_text("データシートの側面図の、基板からの本体の高さ。0 のままなら、3D表示は名前から推測します。ケースに入るかの確認と、3D表示の本体の高さに使います");
                ui.small("側面図の高さ (0 = 未入力)");
            });
            ui.checkbox(&mut self.body_on, "本体の外形 (上面図の幅×長さ) を描く").on_hover_text("データシートの外形寸法 (3面図の上面図) の、部品の本体の大きさを入れます。基板のシルク (白い線) に本体の輪郭が出て、向きの印 (1番ピンの点・カソードの棒・＋) も付きます。パッドに重なる所は自動で避けます。3D表示でも、この幅×長さ×上の高さの本体になります");
            if self.body_on {
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut self.body_circle, false, "四角").on_hover_text("四角い本体 (IC、チップ部品)");
                    ui.selectable_value(&mut self.body_circle, true, "円").on_hover_text("丸い本体 (ラジアルの電解コンデンサ、LED)");
                    let dv = |ui: &mut egui::Ui, label: &str, v: &mut f64, lo: f64| {
                        ui.add(egui::DragValue::new(v).speed(0.01).range(lo..=100.0).prefix(format!("{label} ")).suffix(" mm")).on_hover_text(body_tip(label));
                    };
                    dv(ui, if self.body_circle { "直径" } else { "幅" }, &mut self.body_w, 0.1);
                    if !self.body_circle {
                        dv(ui, "長さ", &mut self.body_h, 0.1);
                    }
                    dv(ui, "中心X", &mut self.body_x, -100.0);
                    dv(ui, "中心Y", &mut self.body_y, -100.0);
                });
                ui.horizontal_wrapped(|ui| {
                    ui.label("向きの印");
                    for (m, label, tip) in [
                        (BodyMark::None, "なし", "印を付けません"),
                        (BodyMark::Dot, "点", "本体の角の内側に点を打ちます。ICの1番ピンの印です"),
                        (BodyMark::Bar, "棒", "本体の端の近くに線を引きます。ダイオードのカソードや、LEDの平らな側の印です"),
                        (BodyMark::Plus, "＋", "極性のあるコンデンサの ＋ 側の印です"),
                    ] {
                        ui.selectable_value(&mut self.body_mark, m, label).on_hover_text(tip);
                    }
                    if self.body_mark != BodyMark::None {
                        ui.label("のピン");
                        ui.add(egui::TextEdit::singleline(&mut self.body_pin).desired_width(40.0).hint_text("1")).on_hover_text("印を付けるピンの番号。空なら最初のパッド (1番ピン)。ダイオードのカソードなら K、電解コンデンサの＋なら＋側のピン番号");
                    }
                });
            }
            egui::CollapsingHeader::new("追加のパッドと穴 (USBの固定脚・位置決めの穴・長穴)").id_salt("part_extras").default_open(!self.extras.trim().is_empty()).show(ui, |ui| {
                ui.small("USB・DCジャック・ピンソケットなどは、信号のピンのほかに、基板に固定するための脚や穴があります。ここに足すと、部品を置いたとき一緒に付き、部品を動かしたり回したりするとついてきます。製造ファイルのドリルにも出ます");
                ui.small(EXTRAS_HELP);
                ui.horizontal_wrapped(|ui| {
                    ui.label("例を足す:");
                    for (label, line, tip) in EXTRAS_EXAMPLES {
                        if ui.small_button(*label).on_hover_text(format!("{tip}
    数字は例です。データシートの推奨ランドパターンの寸法に直してください")).clicked() {
                            if !self.extras.is_empty() && !self.extras.ends_with('\n') {
                                self.extras.push('\n');
                            }
                            self.extras.push_str(line);
                        }
                    }
                });
                ui.add(egui::TextEdit::multiline(&mut self.extras).desired_rows(4).desired_width(f32::INFINITY).font(egui::TextStyle::Monospace).hint_text("脚 SH -6.5 0 2.0 3.0 1.2"))
                    .on_hover_text("追加のパッドと穴を1行に1つ書きます。# で始まる行は無視します。上の説明のとおりの形で書きます");
            });
    }
}


/// Hover text of a package dimension box of the part form (data sheet "recommended land pattern").
pub(super) fn pkg_tip(label: &str) -> &'static str {
    match label {
        "ピッチ" => "隣り合う足の中心どうしの間隔 (mm)。例: 1.27、0.65、2.54",
        "パッド長" => "はんだ付けする銅の四角 (パッド) の長さ。データシートの推奨ランドパターンの値",
        "パッド幅" => "パッドの幅。足の幅より少し広くします",
        "列間隔" => "向かい合う足の列の中心どうしの間隔 (mm)。DIP の 0.3 インチ幅なら 7.62",
        "対向パッド間隔" => "向かい合うパッドの外側から外側まで (または中心から中心まで) の距離。データシートの推奨寸法を見ます",
        "パッド径" => "パッド (丸い銅) の直径",
        "穴径" => "スルーホールの穴の直径。足の太さ + 0.2〜0.3mm",
        _ => "データシートの推奨ランドパターンの寸法 (mm)",
    }
}

/// Hover text of a body dimension box (data sheet "package outline").
pub(super) fn body_tip(label: &str) -> &'static str {
    match label {
        "幅" => "本体の横の大きさ (データシートの外形寸法の E など)。パッドの列と直角の向きではなく、基板の X 方向です",
        "直径" => "丸い本体の直径 (データシートの D)",
        "長さ" => "本体の縦の大きさ (基板の Y 方向)",
        "中心X" => "本体の中心の、部品の原点 (パッケージの中心) からのずれ (X)。ふつうは 0",
        _ => "本体の中心の、部品の原点 (パッケージの中心) からのずれ (Y)。ふつうは 0",
    }
}
