//! The settings of the selected part (left panel and right-click window) and the advice shown under them.

use super::*;

impl SchEditor {
    pub(crate) fn panel_properties(&mut self, ui: &mut egui::Ui) {
        self.properties_inner(ui, true);
    }

    /// The settings of the selected part; `heading` adds the "選択中の部品" line (the panel band has its own title).
    pub(super) fn properties_inner(&mut self, ui: &mut egui::Ui, heading: bool) {
        self.label_net_ui(ui);
        self.port_editor_ui(ui);
        let mut open_sheet: Option<usize> = None;
        if let Some(Sel::Comp(i)) = self.selected
            && i < self.sch.components.len() {
                let fresh = self.dc.as_ref().is_some_and(|d| d.signature == self.signature());
                let sub_rows: Vec<(String, String)> = self.sch.components[i].sub.as_ref().map(|s| self.sch.value_rows(s.sheet)).unwrap_or_default();
                let c = &mut self.sch.components[i];
                if heading {
                    ui.label(egui::RichText::new(i18n::tr("選択中の部品")).strong());
                }
                // the two things everyone edits first, aligned in one table
                let (has_refdes, has_value) = (!c.kind.is_power_symbol(), c.kind != Kind::Ground);
                if has_refdes || has_value {
                    egui::Grid::new(("part_main", i)).num_columns(2).spacing([8.0, 5.0]).show(ui, |ui| {
                        if has_refdes {
                            ui.label(i18n::tr("番号"));
                            ui.add(egui::TextEdit::singleline(&mut c.refdes).desired_width(f32::INFINITY)).on_hover_text(i18n::tr("部品番号 (R1、C2、U1 など)。同じ番号が2つあると、どちらかが解析やネットで区別できません"));
                            ui.end_row();
                        }
                        if has_value {
                            ui.label(i18n::tr("値"));
                            ui.add(egui::TextEdit::singleline(&mut c.value).desired_width(f32::INFINITY)).on_hover_text(value_hint(c.kind));
                            ui.end_row();
                        }
                    });
                }
                if matches!(c.kind, Kind::Resistor | Kind::Capacitor | Kind::Inductor)
                    && ui.small_button(i18n::tr("値の決め方…")).on_hover_text(i18n::tr("この部品の値を計算で決める窓を開きます (LEDの抵抗・分圧・フィルタなど)")).clicked()
                {
                    self.wiz_open = true;
                }
                if let Kind::Header(n) = c.kind {
                    egui::CollapsingHeader::new(i18n::tr("ピンの名前 (信号名)")).id_salt(("pin_names", i)).default_open(true).show(ui, |ui| {
                        ui.small(i18n::tr("ピンごとの名前を、記号のそばと基板のシルクに書きます (配線やネットは変わりません)")).on_hover_text(i18n::tr("コネクタや端子台の各ピンが何の信号かを書いておくと、配線の間違いが減ります。空にすると、そのピンには名前を書きません"));
                        c.pin_names.resize(n as usize, String::new());
                        for k in 0..n as usize {
                            ui.horizontal(|ui| {
                                ui.label(format!("{}", k + 1));
                                ui.add(egui::TextEdit::singleline(&mut c.pin_names[k]).desired_width(110.0)).on_hover_text(i18n::tr("このピンの名前 (F+、SA、GND など)"));
                            });
                        }
                        if c.pin_names.iter().all(|s| s.is_empty()) {
                            c.pin_names.clear();
                        }
                    });
                }
                // where to buy this very part: reaches the shopping list and the BOM of the board
                if !c.kind.is_power_symbol() && c.sub.is_none() && !c.sim_only && c.kind != Kind::Ground {
                    egui::CollapsingHeader::new(i18n::tr("買える店の型番")).id_salt(("shops", i)).default_open(!c.shops.is_empty()).show(ui, |ui| {
                        ui.small(i18n::tr("店のページで確かめた型番だけ入れます。購入リストと部品表に出ます (空欄なら、検索リンクになります)")).on_hover_text(i18n::tr("抵抗やコンデンサのように、ライブラリ部品ではない部品に、買う型番を付けられます。ライブラリ部品が型番を持っているときは、ここに書いた型番が優先されます。間違った型番より、空欄のほうがよいです"));
                        for (id, label, hint) in [("akizuki", "秋月", "秋月電子通商の商品コード (例: 100941)"), ("digikey", "DigiKey", "DigiKey で検索できる型番 (例: RC0805FR-071KL)"), ("lcsc", "LCSC", "LCSC の部品番号 (C + 数字。例: C25804)")] {
                            let mut v = c.shops.get(id).cloned().unwrap_or_default();
                            ui.horizontal(|ui| {
                                ui.label(label);
                                ui.add(egui::TextEdit::singleline(&mut v).desired_width(f32::INFINITY)).on_hover_text(hint);
                            });
                            if v.is_empty() {
                                c.shops.remove(id);
                            } else {
                                c.shops.insert(id.to_string(), v);
                            }
                        }
                    });
                }
                // the pins this part does not use: marked, ERC stops asking about them
                if !c.kind.is_power_symbol() && c.sub.is_none() && (c.custom.is_some() || c.kind.pins().len() >= 3) {
                    let pins: Vec<(String, String)> = match &c.custom {
                        Some(s) => s.pins.iter().map(|p| (p.number.clone(), p.name.clone())).collect(),
                        None => c.kind.pins().into_iter().map(|(n, _)| (n.clone(), n)).collect(),
                    };
                    let marked = self.sch.no_connects.iter().filter(|(r, _)| *r == c.refdes).count();
                    egui::CollapsingHeader::new(i18n::trf!("使わないピン (NC) ({marked})")).id_salt(("nc_pins", i)).default_open(marked > 0).show(ui, |ui| {
                        ui.small(i18n::tr("つながないピンに印を付けます。回路図に × が出て、ERC は「未接続」と言わなくなります")).on_hover_text(i18n::tr("データシートが「開放のまま」と言うピンや、使わない出力などに付けます。つなぎ忘れとの区別が付くので、本当のミスだけが ERC に残ります。あとから線やラベルでつなぐと、ERC が印との食い違いを知らせます"));
                        for (num, name) in &pins {
                            let mut on = self.sch.no_connects.iter().any(|(r, p)| *r == c.refdes && p == num);
                            let text = if name.is_empty() || name == num { num.clone() } else { format!("{num}  {name}") };
                            if ui.checkbox(&mut on, text).on_hover_text(i18n::tr("チェックを付けると、このピンは使わない (つながない) ピンになります")).changed() {
                                if on {
                                    self.sch.no_connects.push((c.refdes.clone(), num.clone()));
                                } else {
                                    self.sch.no_connects.retain(|(r, p)| !(*r == c.refdes && p == num));
                                }
                            }
                        }
                    });
                }
                if matches!(c.kind, Kind::Resistor | Kind::Capacitor | Kind::Inductor) {
                    let (first, rest) = match c.value.trim().split_once(char::is_whitespace) {
                        Some((f, r)) => (f.to_string(), format!(" {}", r.trim())),
                        None => (c.value.trim().to_string(), String::new()),
                    };
                    if let Some(cur) = parse_value(&first).filter(|v| *v > 0.0) {
                        // re-centre on the typed value unless the slider itself wrote it
                        if self.slider.as_ref().is_none_or(|(k, _, t)| *k != i || *t != c.value) {
                            self.slider = Some((i, cur, c.value.clone()));
                        }
                        let base = self.slider.as_ref().map_or(cur, |s| s.1);
                        // one decade each way (1/10 .. 10x): wide enough to explore, fine enough at full panel width
                        let span = 1.0;
                        let mut x = (cur / base).log10().clamp(-span, span);
                        // the slider takes the full width; the values at its two ends sit under it, so the range is readable
                        ui.spacing_mut().slider_width = (ui.available_width() - 8.0).max(80.0);
                        let changed = ui
                            .add(egui::Slider::new(&mut x, -span..=span).show_value(false))
                            .on_hover_text(
                                i18n::tr("ドラッグして値を動かします (中央 = 今の値)。動かせる範囲は今の値の 1/10 〜 10 倍で、両端の値を下に出しています。これより大きく変えたいときは、値の欄に直接打ちます。
下の「自動で再計算」をオンにすると、動かしながら波形や電圧が変わるのを見られます"),
                            )
                            .changed();
                        ui.horizontal(|ui| {
                            ui.small(format!("{}{}", format_value(base * 10f64.powf(-span)), rest));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.small(format!("{}{}", format_value(base * 10f64.powf(span)), rest));
                            });
                        });
                        if changed {
                            let v = format_value(base * 10f64.powf(x));
                            c.value = format!("{v}{rest}");
                            self.slider = Some((i, base, c.value.clone()));
                        }
                        ui.checkbox(&mut self.live, i18n::tr("自動で再計算")).on_hover_text(i18n::tr("オンにすると、値を動かすたびに、開いている解析 (DC / 過渡 / AC) をやり直します。重い回路ではオフにしてください"));
                        if self.live {
                            ui.checkbox(&mut self.live_ghost, i18n::tr("変える前の線を残す")).on_hover_text(i18n::tr("オンにすると、値を動かし始める前の波形・特性を、うすい線で残します。動かした後のこい線と重ねて、どう変わったかを比べられます。波形の「いまの波形を残す」で残した線は、これで置き換わります"));
                        }
                    }
                }
                if matches!(c.kind, Kind::VSource | Kind::ISource) {
                    // read the form again when the text changed or another source was selected
                    let stale = self.src_form.as_ref().is_none_or(|(k, t, _)| *k != i || *t != c.value);
                    if stale {
                        self.src_form = Some((i, c.value.clone(), SourceForm::parse(&c.value)));
                    }
                    if let Some((_, text, form)) = self.src_form.as_mut() {
                        ui.separator();
                        if crate::source_form::show(ui, form, c.kind == Kind::ISource) {
                            match form.to_text() {
                                Ok(t) => {
                                    c.value = t.clone();
                                    *text = t;
                                }
                                Err(msg) => {
                                    ui.colored_label(crate::theme::ERROR, msg);
                                }
                            }
                        } else if let Err(msg) = form.to_text() {
                            ui.colored_label(crate::theme::ERROR, msg);
                        }
                    }
                }
                // the board and model side of the part: rarely changed, so folded
                if !c.kind.is_power_symbol() && c.sub.is_none() {
                    egui::CollapsingHeader::new(i18n::tr("基板とモデル")).id_salt(("part_board", i)).default_open(false).show(ui, |ui| {
                        if let Some(sym) = &c.custom {
                            ui.label(i18n::trf!("フットプリント: {}", c.footprint));
                            match &sym.sim {
                                Some(m) => {
                                    ui.label(i18n::trf!("モデル: {}", m.name));
                                    // a circuit saved before the notes existed carries the model without them:
                                    // take the notes of the same library model (same name, same text)
                                    let from_library = self.library.iter().find(|d| d.name == sym.name && d.sim.as_ref().is_some_and(|x| x.text == m.text)).and_then(|d| d.sim.as_ref()).map(|x| &x.limits);
                                    limits_ui(ui, if m.limits.is_empty() { from_library.map_or(&[][..], |l| l.as_slice()) } else { &m.limits }, "placed_limits");
                                }
                                None => {
                                    ui.colored_label(crate::theme::WARN, i18n::tr("シミュレーションモデルなし"));
                                }
                            }
                        } else {
                            let current = c.footprint_name();
                            egui::ComboBox::from_label(i18n::tr("フットプリント")).selected_text(current).show_ui(ui, |ui| {
                                for n in pcb_core::footprints::names() {
                                    ui.selectable_value(&mut c.footprint, n.clone(), n);
                                }
                            }).response.on_hover_text(i18n::tr("基板に載せる部品の形 (パッドの並び)。実物の部品の寸法に合わせて選びます"));
                        }
                        ui.checkbox(&mut c.sim_only, i18n::tr("シミュレーション専用 (基板に載せない)"))
                            .on_hover_text(i18n::tr("試験用の電源や、測る相手の代わりに置いた抵抗など、実物の基板には要らない部品に付けます。付けると、基板の取り込み・部品表・購入リストに出ません (シミュレーションには使われます)"));
                    });
                }
                if let Some(sub) = c.sub.as_mut() {
                    if ui.button(i18n::tr("シートの中を開く")).on_hover_text(i18n::tr("この記号が表すシートを開きます (記号のダブルクリックでも開きます)。直すと、同じシートを使う全部の記号に効きます")).clicked() {
                        open_sheet = Some(sub.sheet);
                    }
                    if !sub_rows.is_empty() {
                        let title = format!("このコピーだけの値 ({})", sub.values.len());
                        egui::CollapsingHeader::new(title).id_salt(("sub_values", i)).default_open(!sub.values.is_empty()).show(ui, |ui| {
                            ui.small(i18n::tr("書いた部品だけ別の値 (空欄=シートのまま)")).on_hover_text(i18n::tr("同じシートを何回も使うとき、チャンネルごとに抵抗値などを変えたい部品だけ、ここに書きます。シートの中の部品は、そのままです。コピーの部品は R1_CH1 のように別の番号になり、シミュレーションにも基板にも、ここの値が使われます"));
                            egui::Grid::new(("sub_values_grid", i)).num_columns(3).spacing([8.0, 4.0]).show(ui, |ui| {
                                for (refdes, base) in &sub_rows {
                                    let mut text = sub.values.get(refdes).cloned().unwrap_or_default();
                                    ui.label(refdes);
                                    ui.small(base).on_hover_text(i18n::tr("シートの値"));
                                    if ui.add(egui::TextEdit::singleline(&mut text).hint_text(base.as_str()).desired_width(f32::INFINITY)).on_hover_text(i18n::tr("このコピーでの値。空欄にすると、シートの値に戻ります")).changed() {
                                        if text.trim().is_empty() {
                                            sub.values.remove(refdes);
                                        } else {
                                            sub.values.insert(refdes.clone(), text);
                                        }
                                    }
                                    ui.end_row();
                                }
                            });
                        });
                    }
                } else {
                    advice_ui(ui, &*c, self.dc.as_ref(), fresh, &self.library);
                }
            }
        if let Some(k) = open_sheet {
            self.enter_sheet(k);
        }
        // one row of actions on the selection: rotate, the clipboard, delete
        if self.has_selection() || !self.clipboard.is_empty() {
            let has = self.has_selection();
            ui.add_space(crate::theme::SPACE_S);
            ui.spacing_mut().item_spacing.y = crate::theme::SPACE_S;
            // two joined bars, like the buttons under the part lists: change the part, then the clipboard
            let rotatable = matches!(self.selected, Some(Sel::Comp(_)));
            let edit = crate::widgets::segment_row(ui, &[("回転 (R)", rotatable), ("複製", has), ("削除", has)]);
            if let Some(Sel::Comp(i)) = self.selected
                && self.lit(edit[0].clone(), "回転 (R)").on_hover_text(i18n::tr("部品を 90° 回します (R キーでも同じです)")).clicked()
            {
                self.rotate_req = Some(i);
            }
            if edit[1].clone().on_hover_text(i18n::tr("コピーしてすぐ貼り付けます (Ctrl+D)")).clicked() {
                self.duplicate_selection();
            }
            if self.lit(edit[2].clone(), "削除 (Delete)").on_hover_text(i18n::tr("選んだものを消します (Delete キーでも同じです)")).clicked() {
                self.delete_selected();
            }
            let has_clip = !self.clipboard.is_empty();
            let clip = crate::widgets::segment_row(ui, &[("コピー", has), ("切り取り", has), ("貼り付け", has_clip)]);
            if clip[0].clone().on_hover_text(i18n::tr("選んだ部品・線・ラベルをコピーします (Ctrl+C)。部品のピンについているネットラベルも一緒にコピーされます")).clicked() {
                self.copy_selection();
            }
            if clip[1].clone().on_hover_text(i18n::tr("コピーして、元のものを消します (Ctrl+X)")).clicked() {
                self.cut_selection();
            }
            if clip[2].clone().on_hover_text(i18n::tr("コピーしたものを、近くの空いた所に置きます (Ctrl+V)。部品番号は新しくなります。貼ったあと、ドラッグで動かせます")).clicked() {
                self.paste_clipboard();
            }
        }
    }

    /// The settings window opened by right-clicking a part: the same editor as the left panel's
    /// "選択中の部品" section, next to the part. Closes when another thing gets selected.
    pub fn part_popup_window(&mut self, ctx: &egui::Context) {
        let Some((i, pos, serial)) = self.part_popup else { return };
        if self.selected != Some(Sel::Comp(i)) || i >= self.sch.components.len() {
            self.part_popup = None;
            return;
        }
        let c = &self.sch.components[i];
        let title = if c.kind.is_power_symbol() { "部品の設定".to_string() } else { format!("部品の設定: {}", c.refdes) };
        let mut open = true;
        egui::Window::new(title)
            .id(egui::Id::new(("part_popup", serial)))
            .open(&mut open)
            .default_pos(pos + egui::vec2(12.0, 12.0))
            .default_width(260.0)
            .show(ctx, |ui| {
                self.panel_properties(ui);
            });
        if !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) || self.selected != Some(Sel::Comp(i)) {
            self.part_popup = None;
        }
    }
}

/// What can be typed in the value field of a part of this kind (shown when the pointer rests on the field).
fn value_hint(kind: Kind) -> &'static str {
    match kind {
        Kind::Capacitor => "容量。例: 100n / 4.7u
100n ESR=20m ESL=5n … 等価直列抵抗・インダクタンスも入れられます
100u IC=3.2 … 過渡解析を始めるときの電圧 (電池の代わりに使えます)
22u BIAS=60 … 直流の電圧がかかると、セラミックコンデンサの容量は下がります。その下がる割合 (%) です (例: 16V 品の 22u は、5V で約 60% 下がります)",
        Kind::Inductor => "インダクタンス。例: 10u / 1m
10u DCR=0.1 … 巻線抵抗も入れられます",
        Kind::Resistor => "抵抗値。例: 100 / 4.7k / 1Meg (k = 1000、Meg = 100万、m = 1/1000、u = 1/100万、n、p)",
        Kind::VSource => "電源の値。例:
5 … 直流 5V
SIN(0 1 1k) … 正弦波 (中心 振幅 周波数)
PULSE(0 5 100u 1u 1u 400u 1m) … 方形波 (低 高 遅延 立上り 立下り 幅 周期)
PWL(0 0 1m 5) … 折れ線 (時間 電圧 の組)
5 AC 1 … AC解析の入力にする
NOISE(0.05 1Meg) … ノイズ (実効値 最高周波数)。5 + NOISE(0.05 1Meg) + SIN(0 0.02 100k) のように + でつなげます (+ の前後は空白)",
        Kind::ISource => "電流源の値 (正の値で + のピンから回路へ流れ出します)。例:
10m … 直流 10mA
PULSE(10m 100m 1m 1u 1u 2m 4m) … 負荷の急変 (10mA と 100mA を 2ms ずつ)
SIN(1m 0.5m 1k) … 交流 (中心 振幅 周波数)
0 AC 1 … AC解析の入力にする (インピーダンスを測るなど)",
        _ => "部品の値。単位つきの数字は 10k, 100n, 4.7u のように書けます",
    }
}

/// Common wattages of resistors, smallest first.
const RESISTOR_WATTS: &[f64] = &[0.0625, 0.1, 0.125, 0.25, 0.5, 1.0, 2.0, 5.0];

fn watt_label(w: f64) -> String {
    match w {
        w if (w - 0.0625).abs() < 1e-9 => "1/16W".into(),
        w if (w - 0.125).abs() < 1e-9 => "1/8W".into(),
        w if (w - 0.25).abs() < 1e-9 => "1/4W".into(),
        w if (w - 0.5).abs() < 1e-9 => "1/2W".into(),
        w => format!("{}W", sch_core_trim(w)),
    }
}

fn sch_core_trim(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// "部品選びのヒント" for the selected part: its data sheet figures, what this circuit asks of it (from the
/// DC result), the rating it would need with a margin of 2, whether it has that, and library parts that do.
fn advice_ui(ui: &mut egui::Ui, c: &Component, dc: Option<&DcView>, fresh: bool, library: &[PartDef]) {
    let lib_def = c.custom.as_ref().and_then(|s| library.iter().find(|d| d.name == s.name));
    let ratings = c.custom.as_ref().and_then(|s| s.ratings.as_ref()).or_else(|| lib_def.and_then(|d| d.ratings.as_ref()));
    if c.kind != Kind::Resistor && c.custom.is_none() {
        return;
    }
    egui::CollapsingHeader::new(i18n::tr("部品選びのヒント (この回路で必要な定格)")).id_salt(("advice", &c.refdes)).default_open(false).show(ui, |ui| {
        if let Some(r) = ratings.filter(|r| !r.is_empty()) {
            crate::widgets::section_heading(ui, "データシートの情報");
            for l in r.summary().lines() {
                ui.label(i18n::trf!("・{l}"));
            }
        } else if c.custom.is_some() {
            ui.label(i18n::tr("この部品には、定格 (データシートの情報) が入っていません。「＋ 部品を作る」の部品フォームの「データシートの情報」で入れられます"));
        }
        let Some(view) = dc else {
            ui.label(i18n::tr("DC動作点を解析すると、この回路でこの部品にかかる電流・電力と、必要な定格が出ます"));
            return;
        };
        let Ok(res) = &view.outcome else {
            ui.label(i18n::tr("DC動作点が解けていないので、必要な定格を出せません"));
            return;
        };
        if !fresh {
            ui.colored_label(crate::theme::WARN, i18n::tr("回路を変えたので、いまは前の解析結果です。DC動作点をもう一度解析してください"));
        }
        let st = sim_core::stress(res, &c.refdes);
        let (Some(i), Some(p)) = (st.i, st.p) else {
            ui.label(i18n::tr("この部品は、解析結果に個別の値がありません (IC の内部モデルなど)"));
            return;
        };
        ui.separator();
        crate::widgets::section_heading(ui, "この回路でかかる値");
        ui.label(i18n::trf!("・電流 {}  電力 {}", eng(i, "A"), eng(p, "W")));
        if matches!(c.custom.as_ref().and_then(|s| s.sim.as_ref()).map(|m| m.element.to_ascii_uppercase()).as_deref(), Some("Q" | "M" | "J")) {
            ui.label(i18n::trf!("・部品の両端の電圧 (Vce / Vds) {}", eng(st.v.unwrap_or(0.0), "V")));
        }
        ui.label(i18n::trf!("・回路の最大電圧 {}", eng(st.v_circuit, "V")));
        let req = sim_core::requirement(&st, 2.0);
        crate::widgets::section_heading(ui, "余裕を 2 倍とるなら").on_hover_text(i18n::tr("部品は、定格ぎりぎりで使うと、熱や劣化で壊れやすくなります。電圧・電流・電力は、最低でも 2 倍の余裕を見るのが、初学者の目安です"));
        if c.kind == Kind::Resistor {
            let need = req.p_min;
            let pick = RESISTOR_WATTS.iter().copied().find(|w| *w >= need);
            ui.label(i18n::trf!("・抵抗の定格電力 {} 以上 → {}", eng(need, "W"), pick.map_or("5W を超えます。電流を減らすか、抵抗を直列・並列に分けてください".to_string(), |w| format!("{} の抵抗が使えます", watt_label(w)))));
            return;
        }
        ui.label(i18n::trf!("・最大電圧 {} 以上 / 最大電流 {} 以上 / 最大電力 {} 以上", eng(req.v_min, "V"), eng(req.i_min, "A"), eng(req.p_min, "W")));
        match ratings.filter(|r| !r.is_empty()) {
            Some(r) => {
                let ok = sim_core::meets(&req, r, true);
                let (text, color) = if ok { ("✔ この部品は、2 倍の余裕を満たしています", crate::theme::OK) } else { ("✖ この部品は、2 倍の余裕を満たしていません (いずれかの最大定格が足りない、または不明です)", crate::theme::ERROR) };
                ui.colored_label(color, text);
            }
            None => {
                ui.label(i18n::tr("この部品の定格が分からないので、判定できません。データシートで、上の条件を満たすか確かめてください"));
            }
        }
        // library parts of the same kind that would do
        if let Some(def) = lib_def {
            let cat = if def.category.is_empty() { None } else { Some(def.category.as_str()) };
            let mut ok: Vec<&PartDef> = library.iter().filter(|d| d.name != def.name && cat.is_some_and(|c| d.category == c) && d.ratings.as_ref().is_some_and(|r| sim_core::meets(&req, r, true))).collect();
            ok.sort_by(|a, b| a.name.cmp(&b.name));
            if !ok.is_empty() {
                crate::widgets::section_heading(ui, "同じ種類で、条件を満たすライブラリ部品");
                ui.horizontal_wrapped(|ui| {
                    for d in ok.iter().take(8) {
                        ui.label(&d.name).on_hover_text(d.ratings.as_ref().map(|r| r.summary()).unwrap_or_default());
                    }
                });
            }
        }
    });
}
