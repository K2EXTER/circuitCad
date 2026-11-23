//! The pin table of the "③ ピン" tab: one row per pin (number, name, side, the terminal of the SPICE model it is). The text in
//! `pins_text` (and `sim_pins`) stays the stored form, so the templates, the data sheet import and the saved parts keep working;
//! the rows are read from the text and written back to it when a row is edited.

use super::*;

/// Shown for a terminal that has no pin yet (also what is written into `sim_pins`, so the form says what is missing).
const OPEN: &str = "?";

const SIDES: [(&str, &str); 5] = [("", "自動"), ("left", "左"), ("right", "右"), ("top", "上"), ("bottom", "下")];

/// One row of the table. Fields are text so that a half-typed row is kept as it is.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct PinRow {
    pub number: String,
    pub name: String,
    pub side: String,
    /// Empty places before the pin on its side of the symbol (`gap=N` in the text).
    pub gap: u8,
    /// A pin of the simulation model only, with no pad (`sim` in the text).
    pub sim: bool,
    /// The electrical kind (`kind=in` in the text); None = not said.
    pub kind: Option<sch_core::PinKind>,
    /// The unit it is drawn on (`unit=B` in the text; 0 = A, Q6).
    pub unit: u8,
}

/// The rows of a pin text (a line with only a number gives a row with an empty name).
pub(super) fn rows_of(text: &str) -> Vec<PinRow> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let (t, gap, sim, kind, unit) = pin_tokens(l);
            let unit = unit.and_then(super::parse::unit_of_word).unwrap_or(0);
            let kind = kind.and_then(sch_core::PinKind::from_word);
            let side = t.get(2).and_then(|s| parse_side(s)).map_or("", side_word);
            let gap = gap.and_then(|g| g.parse::<u8>().ok()).unwrap_or(0).min(MAX_GAP);
            PinRow { number: t.first().copied().unwrap_or("").into(), name: t.get(1).copied().unwrap_or("").into(), side: side.into(), gap, sim, kind, unit }
        })
        .collect()
}

/// The pin text of the rows (a row with nothing in it is left out).
pub(super) fn text_of(rows: &[PinRow]) -> String {
    rows.iter()
        .filter(|r| !(r.number.is_empty() && r.name.is_empty()))
        .map(|r| {
            let mut l = if r.side.is_empty() { format!("{} {}", r.number, r.name) } else { format!("{} {} {}", r.number, r.name, r.side) };
            if r.gap > 0 {
                l.push_str(&format!(" gap={}", r.gap));
            }
            if r.sim {
                l.push_str(" sim");
            }
            if let Some(k) = r.kind {
                l.push_str(&format!(" kind={}", k.word()));
            }
            if r.unit > 0 {
                l.push_str(&format!(" unit={}", sch_core::CustomSymbol::unit_letter(r.unit)));
            }
            l
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Puts the kind its name says into each row that has none (`PinKind::guess_from_name`).
pub(super) fn guess_kinds(rows: &mut [PinRow]) {
    for r in rows.iter_mut().filter(|r| r.kind.is_none()) {
        r.kind = sch_core::PinKind::guess_from_name(&r.name);
    }
}

/// The columns of the CSV, as its first line names them.
const CSV_HEAD: &str = "number,name,side,kind,sim,gap";

/// The rows as CSV (with the line of the column names), for editing in a spreadsheet: the names of a 100-pin IC are
/// quicker to fix there (KiCad 10 does the same with its pin table).
pub(super) fn csv_of(rows: &[PinRow]) -> String {
    let cell = |s: &str| if s.contains([',', '"', '\n']) { format!("\"{}\"", s.replace('"', "\"\"")) } else { s.to_string() };
    let mut out = String::from(CSV_HEAD);
    for r in rows.iter().filter(|r| !(r.number.is_empty() && r.name.is_empty())) {
        let kind = r.kind.map_or("", |k| k.word());
        let gap = if r.gap > 0 { r.gap.to_string() } else { String::new() };
        out += &format!("\n{},{},{},{},{},{}", cell(&r.number), cell(&r.name), r.side, kind, if r.sim { "1" } else { "" }, gap);
    }
    out.push('\n');
    out
}

/// The cells of one CSV line ("a,\"b,c\",d": a quoted cell may hold commas and doubled quotes).
fn csv_cells(line: &str) -> Vec<String> {
    let mut cells = vec![String::new()];
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                chars.next();
                cells.last_mut().unwrap().push('"');
            }
            ('"', _) => quoted = !quoted,
            (',' | ';' | '\t', false) => cells.push(String::new()),
            (c, _) => cells.last_mut().unwrap().push(c),
        }
    }
    cells.into_iter().map(|c| c.trim().to_string()).collect()
}

/// Which column a heading names: our own words, the Japanese of the table, and KiCad's pin table ("Number", "Name",
/// "Orientation", "Electrical Type").
fn csv_column(head: &str) -> Option<usize> {
    let h = head.trim().to_lowercase();
    let h = h.as_str();
    Some(match h {
        "number" | "pin" | "pin number" | "no" | "no." | "番号" | "ピン番号" => 0,
        "name" | "pin name" | "名前" | "ピン名" => 1,
        "side" | "orientation" | "向き" => 2,
        "kind" | "type" | "electrical type" | "種類" => 3,
        "sim" => 4,
        "gap" | "空き" => 5,
        _ => return None,
    })
}

fn csv_side(s: &str) -> Option<&'static str> {
    Some(match s.trim().to_lowercase().as_str() {
        "" | "自動" | "auto" => "",
        "left" | "l" | "左" => "left",
        "right" | "r" | "右" => "right",
        "top" | "up" | "t" | "上" => "top",
        "bottom" | "down" | "b" | "下" => "bottom",
        _ => return None,
    })
}

/// A kind from its word (`in`), its name on screen (入力), or KiCad's electrical type ("input", "power_in",
/// "No connect"); an empty cell is no kind.
fn csv_kind(s: &str) -> Result<Option<sch_core::PinKind>, ()> {
    use sch_core::PinKind::*;
    let t = s.trim().to_lowercase().replace([' ', '-'], "_");
    if t.is_empty() || t == "指定なし" {
        return Ok(None);
    }
    if let Some(k) = sch_core::PinKind::from_word(&t).or_else(|| sch_core::PinKind::ALL.into_iter().find(|k| k.label() == s.trim())) {
        return Ok(Some(k));
    }
    Ok(Some(match t.as_str() {
        "input" => Input,
        "output" => Output,
        "bidirectional" | "bidir" | "i/o" => Bidir,
        "open_collector" | "open_drain" | "open_emitter" => OpenDrain,
        "power_in" | "power_input" => PowerIn,
        "power_out" | "power_output" => PowerOut,
        "passive" => Passive,
        "no_connect" | "not_connected" | "unconnected" | "未接続" => Nc,
        _ => return Err(()),
    }))
}

/// The rows of a CSV of the pin table (`csv_of`, or a spreadsheet's). The first line names the columns when it holds
/// "number" / "番号" and "name" / "名前" (in any order, other columns ignored); otherwise the columns are number, name,
/// side, kind. A file that is not UTF-8 is refused before this (`read_pin_csv`).
pub(super) fn rows_from_csv(text: &str) -> Result<Vec<PinRow>, String> {
    let text = text.trim_start_matches('\u{feff}');
    let mut lines = text.lines().enumerate().filter(|(_, l)| !l.trim().is_empty()).peekable();
    let first = lines.peek().map(|(_, l)| csv_cells(l)).unwrap_or_default();
    let named: Vec<Option<usize>> = first.iter().map(|h| csv_column(h)).collect();
    let header = named.contains(&Some(0)) && named.contains(&Some(1));
    let cols: Vec<Option<usize>> = if header {
        lines.next();
        named
    } else {
        (0..4).map(Some).collect()
    };
    let mut rows = vec![];
    for (i, line) in lines {
        let cells = csv_cells(line);
        let get = |c: usize| cols.iter().position(|x| *x == Some(c)).and_then(|p| cells.get(p)).map_or("", String::as_str);
        let mut r = PinRow { number: get(0).to_string(), name: get(1).to_string(), ..PinRow::default() };
        clean(&mut r.number);
        clean(&mut r.name);
        if r.number.is_empty() && r.name.is_empty() {
            continue;
        }
        r.side = csv_side(get(2)).ok_or_else(|| i18n::trf!("{} 行目: 向き「{}」が分かりません。left・right・top・bottom (左・右・上・下) か空にしてください", i + 1, get(2)))?.to_string();
        r.kind = csv_kind(get(3)).map_err(|_| i18n::trf!("{} 行目: 種類「{}」が分かりません。in・out・io・od・pwr・pwrout・pas・nc (入力・出力など) か空にしてください", i + 1, get(3)))?;
        r.sim = matches!(get(4).to_lowercase().as_str(), "1" | "true" | "yes" | "sim" | "○" | "x");
        r.gap = match get(5) {
            "" => 0,
            g => g.parse::<u8>().map_err(|_| i18n::trf!("{} 行目: 空き「{}」は 0〜{} の数にしてください", i + 1, g, MAX_GAP))?.min(MAX_GAP),
        };
        rows.push(r);
    }
    if rows.is_empty() {
        return Err(i18n::tr("ピンの行がありません。1 行目に number,name (番号,名前) の列の名前、2 行目からピンを書きます").to_string());
    }
    let mut seen = std::collections::HashSet::new();
    if let Some(r) = rows.iter().find(|r| !r.number.is_empty() && !seen.insert(r.number.clone())) {
        return Err(i18n::trf!("番号 {} のピンが 2 つあります。番号は重ならないようにしてください", r.number));
    }
    Ok(rows)
}

/// The rows of a CSV file. Excel on Japanese Windows writes "CSV (コンマ区切り)" in Shift_JIS: asked to save as
/// "CSV UTF-8" instead, as the names would be garbled.
fn read_pin_csv(path: &std::path::Path) -> Result<Vec<PinRow>, String> {
    let bytes = std::fs::read(path).map_err(|e| i18n::trf!("{} を読めません: {e}", path.display()))?;
    let text = String::from_utf8(bytes).map_err(|_| i18n::tr("文字コードが UTF-8 ではありません。Excel なら「名前を付けて保存」で「CSV UTF-8 (コンマ区切り)」を選んで保存し直してください").to_string())?;
    rows_from_csv(&text)
}

fn side_word(s: PinSide) -> &'static str {
    match s {
        PinSide::Left => "left",
        PinSide::Right => "right",
        PinSide::Top => "top",
        PinSide::Bottom => "bottom",
    }
}

fn tokens(s: &str) -> Vec<String> {
    s.split(|c: char| c.is_whitespace() || c == ',').filter(|s| !s.is_empty()).map(String::from).collect()
}

/// Spaces and commas would split a field into two on the way back to the text.
fn clean(s: &mut String) {
    s.retain(|c| !c.is_whitespace() && c != ',');
}

impl PartForm {
    /// The rows follow the text: read again when something else (a template, the data sheet import, the paste box) changed it.
    fn sync_pin_rows(&mut self) {
        if self.pin_rows_of.as_deref() != Some(self.pins_text.as_str()) {
            self.pin_rows = rows_of(&self.pins_text);
            self.pin_rows_of = Some(self.pins_text.clone());
        }
    }

    /// The table. Writes `pins_text`, and `sim_pins` for the model terminal column.
    pub(super) fn pin_table_ui(&mut self, ui: &mut egui::Ui) {
        self.sync_pin_rows();
        let roles = guess_spice(&self.sim_text).map(|g| g.roles).unwrap_or_default();
        let mut cur = tokens(&self.sim_pins);
        if !roles.is_empty() {
            cur.resize(roles.len().max(cur.len()), OPEN.into());
        }
        let mut rows = self.pin_rows.clone();
        let mut sim = cur.clone();
        let mut remove: Option<usize> = None;
        let mut swap: Option<(usize, usize)> = None;
        let n = rows.len();
        let numbers: Vec<String> = rows.iter().map(|r| r.number.clone()).collect();
        ui.horizontal_wrapped(|ui| {
            crate::icons::help_label(ui, "ピン", "1行が1本のピンです。番号はパッケージのパッドの番号 (データシートのピン番号)、名前は回路図に出る名前です。向きは記号のどちらの辺に出すかで、「自動」なら前半が左、後半が右になります。NC は未接続のピンです");
            if !roles.is_empty() {
                ui.small(i18n::tr("「モデルの端子」は、貼った SPICE モデルの端子が、どのピンにあたるかです (選ばないピンは、シミュレーションではどこにもつながりません)"));
            }
        });
        egui::Grid::new("pin_table").num_columns(if roles.is_empty() { 8 } else { 9 }).spacing([8.0, 4.0]).striped(true).show(ui, |ui| {
            ui.small("");
            ui.small(i18n::tr("番号"));
            ui.small(i18n::tr("名前"));
            ui.small(i18n::tr("向き"));
            ui.small(i18n::tr("ユニット")).on_hover_text(i18n::tr("1 つの IC の中の同じ回路 (デュアルのオペアンプの 1 つ目と 2 つ目) を別々の記号で置くとき、そのピンがどれ (A・B …) にあるか。電源のピンは A に。ふつうの部品は全部 A のまま"));
            ui.small(i18n::tr("種類")).on_hover_text(i18n::tr("ピンの電気的な種類 (入力・出力・電源など)。回路図の点検 (ERC) が、出力どうしのつながり・何も駆動しない入力・つないだ NC のピンを見つけるのに使います。「自動」は指定なし (名前による点検だけ)"));
            ui.small(i18n::tr("sim")).on_hover_text(i18n::tr("シミュレーション専用のピン (基板にパッドを出しません)"));
            if !roles.is_empty() {
                ui.small(i18n::tr("モデルの端子"));
            }
            ui.small("");
            ui.end_row();
            for i in 0..n {
                ui.small(format!("{}", i + 1));
                let twice = !rows[i].number.is_empty() && numbers.iter().filter(|x| **x == rows[i].number).count() > 1;
                let before = rows[i].number.clone();
                // min_size: a text box in a grid column (not the last) otherwise stays as narrow as the column's first frame
                let r = ui
                    .add(egui::TextEdit::singleline(&mut rows[i].number).desired_width(52.0).min_size(egui::vec2(52.0, 0.0)).font(egui::TextStyle::Monospace))
                    .on_hover_text(if twice { i18n::tr("同じ番号のピンがあります。番号は重ならないようにします") } else { i18n::tr("パッドの番号 (データシートのピン番号。BGA なら A1 のような名前)。同じ番号は2つ使えません") });
                if r.changed() {
                    clean(&mut rows[i].number);
                    // the model terminals follow the renamed pin
                    if !before.is_empty() {
                        for s in sim.iter_mut().filter(|s| **s == before) {
                            *s = rows[i].number.clone();
                        }
                    }
                }
                let name_edit = ui
                    .add(egui::TextEdit::singleline(&mut rows[i].name).desired_width(130.0).min_size(egui::vec2(130.0, 0.0)).font(egui::TextStyle::Monospace))
                    .on_hover_text(i18n::tr("ピンの名前 (IN・GND・OUT・IN+ など)。回路図の部品に出ます"));
                if name_edit.changed() {
                    clean(&mut rows[i].name);
                }
                let shown = SIDES.iter().find(|(k, _)| *k == rows[i].side).map_or("自動", |(_, l)| *l);
                egui::ComboBox::from_id_salt(("pin_side", i))
                    .selected_text(i18n::tr(shown))
                    .width(56.0)
                    .show_ui(ui, |ui| {
                        for (k, label) in SIDES {
                            ui.selectable_value(&mut rows[i].side, k.to_string(), i18n::tr(label));
                        }
                    })
                    .response
                    .on_hover_text(i18n::tr("記号のどちらの辺にこのピンを出すか。「自動」なら、前半が左、後半が右です"));
                egui::ComboBox::from_id_salt(("pin_unit", i))
                    .selected_text(sch_core::CustomSymbol::unit_letter(rows[i].unit).to_string())
                    .width(36.0)
                    .show_ui(ui, |ui| {
                        for u in 0..8u8 {
                            ui.selectable_value(&mut rows[i].unit, u, sch_core::CustomSymbol::unit_letter(u).to_string());
                        }
                    })
                    .response
                    .on_hover_text(i18n::tr("このピンを描くユニット。A のままなら、ふつうの 1 つの記号です"));
                let guess = sch_core::PinKind::guess_from_name(&rows[i].name);
                let shown = match (rows[i].kind, guess) {
                    (Some(k), _) => i18n::tr(k.label()).to_string(),
                    (None, Some(g)) => i18n::trf!("指定なし ({}?)", i18n::tr(g.label())),
                    (None, None) => i18n::tr("指定なし").to_string(),
                };
                egui::ComboBox::from_id_salt(("pin_kind", i))
                    .selected_text(shown)
                    .width(96.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut rows[i].kind, None, i18n::tr("指定なし"));
                        for k in sch_core::PinKind::ALL {
                            ui.selectable_value(&mut rows[i].kind, Some(k), i18n::tr(k.label()));
                        }
                    })
                    .response
                    .on_hover_text(i18n::tr("このピンの電気的な種類。「?」は名前からの推測で、まだ入っていません (表の下の「名前から種類を入れる」で入ります)"));
                ui.checkbox(&mut rows[i].sim, "").on_hover_text(i18n::tr("シミュレーション専用のピン: 回路図の記号とモデルにはあり、基板にパッドを出しません (フォトダイオードの光の強さの IRRAD など)"));
                if !roles.is_empty() {
                    let mine = if rows[i].number.is_empty() { None } else { sim.iter().position(|s| *s == rows[i].number) };
                    let label = |j: usize| i18n::trf!("端子 {} ({})", j + 1, roles.get(j).map_or("", |s| s.as_str()));
                    let mut pick = mine;
                    egui::ComboBox::from_id_salt(("pin_term", i))
                        .selected_text(mine.map_or(i18n::tr("(使わない)").to_string(), label))
                        .width(130.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut pick, None, i18n::tr("(使わない)"));
                            for j in 0..roles.len() {
                                ui.selectable_value(&mut pick, Some(j), label(j));
                            }
                        })
                        .response
                        .on_hover_text(i18n::tr("このピンが、モデルのどの端子にあたるか。同じ端子を2つのピンに選ぶと、前に選んだピンは「使わない」に戻ります"));
                    if pick != mine && !rows[i].number.is_empty() {
                        if let Some(j) = mine {
                            sim[j] = OPEN.into();
                        }
                        if let Some(j) = pick {
                            sim[j] = rows[i].number.clone();
                        }
                    }
                }
                ui.horizontal(|ui| {
                    if crate::widgets::unless(ui, (!(i > 0)).then_some("いちばん上です"), egui::Button::new("↑").small()).on_hover_text(i18n::tr("1つ上へ")).clicked() {
                        swap = Some((i, i - 1));
                    }
                    if crate::widgets::unless(ui, (!(i + 1 < n)).then_some("いちばん下です"), egui::Button::new("↓").small()).on_hover_text(i18n::tr("1つ下へ")).clicked() {
                        swap = Some((i, i + 1));
                    }
                    if ui.add(egui::Button::new(i18n::tr("消す")).small()).on_hover_text(i18n::tr("このピンを消します")).clicked() {
                        remove = Some(i);
                    }
                });
                ui.end_row();
            }
        });
        if let Some((a, b)) = swap {
            rows.swap(a, b);
        }
        if let Some(i) = remove {
            let gone = rows.remove(i).number;
            if !gone.is_empty() {
                for s in sim.iter_mut().filter(|s| **s == gone) {
                    *s = OPEN.into();
                }
            }
        }
        ui.horizontal(|ui| {
            if ui.button(i18n::tr("＋ ピンを足す")).on_hover_text(i18n::tr("表の最後に、空のピンを1行足します。番号は、いまの最大の番号の次になります")).clicked() {
                let next = rows.iter().filter_map(|r| r.number.parse::<u32>().ok()).max().map_or(1, |m| m + 1);
                // when every pin is on one side (a header), the new one goes there too instead of splitting the box
                let side = rows.first().map(|r| r.side.clone()).filter(|s| !s.is_empty() && rows.iter().all(|r| r.side == *s)).unwrap_or_default();
                // a 2-row header's table alternates left / right: the new pin takes its turn
                let alternating = rows.len() >= 2 && rows.iter().enumerate().all(|(i, r)| r.side == if i % 2 == 0 { "left" } else { "right" });
                let side = if alternating { if rows.len().is_multiple_of(2) { "left" } else { "right" }.to_string() } else { side };
                rows.push(PinRow { number: next.to_string(), side, ..PinRow::default() });
            }
            let guessable = rows.iter().any(|r| r.kind.is_none() && sch_core::PinKind::guess_from_name(&r.name).is_some());
            if crate::widgets::unless(ui, (!guessable).then_some("名前から種類が分かるピン (種類が空のもの) がありません"), egui::Button::new(i18n::tr("名前から種類を入れる")))
                .on_hover_text(i18n::tr("種類が「指定なし」のピンに、名前から推測した種類 (VCC・GND は電源入力、NC は未接続、OUT は出力、IN・EN は入力など) を入れます。推測できない名前 (FB・SW など) と、もう種類のあるピンはそのままです。データシートで確かめてください"))
                .clicked()
            {
                guess_kinds(&mut rows);
            }
            if crate::widgets::unless(ui, rows.is_empty().then_some("ピン表が空です"), egui::Button::new(i18n::tr("CSV に書き出す…"))).on_hover_text(i18n::tr("ピン表を CSV にします (番号・名前・向き・種類・sim・空き)。Excel などで名前をまとめて直し、「CSV から読む」で戻せます")).clicked()
                && let Some(p) = rfd::FileDialog::new().add_filter("CSV", &["csv"]).set_file_name(format!("{}_pins.csv", if self.name.is_empty() { "part" } else { self.name.as_str() })).save_file()
            {
                // with a byte order mark, so that Excel reads the Japanese as UTF-8
                self.pin_csv_msg = Some(std::fs::write(&p, format!("\u{feff}{}", csv_of(&rows))).map(|_| i18n::trf!("{} に書き出しました", p.display())).map_err(|e| i18n::trf!("{} に書けません: {e}", p.display())));
            }
            if ui.button(i18n::tr("CSV から読む…")).on_hover_text(i18n::tr("CSV のピン表で、この表を置き換えます。1 行目に列の名前 (number,name,side,kind … か 番号,名前,向き,種類。KiCad のピン表の書き出しも可)、無ければ 番号・名前・向き・種類 の順と見なします。UTF-8 で保存したものを読みます")).clicked()
                && let Some(p) = rfd::FileDialog::new().add_filter("CSV", &["csv", "txt"]).pick_file()
            {
                match read_pin_csv(&p) {
                    Ok(new) => {
                        self.pin_csv_msg = Some(Ok(i18n::trf!("{} から {} 本のピンを読みました", p.display(), new.len())));
                        rows = new;
                    }
                    Err(e) => self.pin_csv_msg = Some(Err(e)),
                }
            }
            if !roles.is_empty() && ui.button(i18n::tr("名前で合わせる")).on_hover_text(i18n::tr("ピンの名前 (か番号) が、モデルの端子の名前と同じものを、自動で選びます。合うものがない端子は、選び直しになります")).clicked() {
                for (j, role) in roles.iter().enumerate() {
                    sim[j] = rows.iter().find(|p| p.name.eq_ignore_ascii_case(role) || p.number.eq_ignore_ascii_case(role)).map_or(OPEN.into(), |p| p.number.clone());
                }
            }
        });
        match &self.pin_csv_msg {
            Some(Ok(m)) => {
                ui.small(m);
            }
            Some(Err(e)) => {
                ui.colored_label(crate::theme::error(), e);
            }
            None => {}
        }
        if !roles.is_empty() {
            let unused: Vec<&str> = rows.iter().filter(|p| !p.number.is_empty() && !sim.contains(&p.number)).map(|p| p.name.as_str()).collect();
            if !unused.is_empty() {
                ui.small(i18n::trf!("モデルに使わないピン: {}", unused.join(" "))).on_hover_text(i18n::tr("これらのピンは、シミュレーションでは何にもつながりません (基板では、パッドとしてそのまま使えます)。つながない部品なら、回路図で「使わないピン (NC)」にします"));
            }
        }
        if rows != self.pin_rows {
            self.pins_text = text_of(&rows);
            self.pin_rows = rows;
            self.pin_rows_of = Some(self.pins_text.clone());
        }
        if sim != cur {
            if !roles.is_empty() {
                sim.truncate(roles.len());
            }
            self.sim_pins = sim.join(" ");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pin_table_goes_to_csv_and_back() {
        let rows = rows_of("1 IN+ left gap=1 kind=in\n2 \"V-\" right\n3 OUT kind=out\n4 IRRAD bottom sim");
        let csv = csv_of(&rows);
        assert!(csv.starts_with("number,name,side,kind,sim,gap\n1,IN+,left,in,,1\n"), "{csv}");
        assert_eq!(rows_from_csv(&format!("\u{feff}{csv}")).unwrap(), rows, "with Excel's byte order mark too");
    }

    /// A spreadsheet's CSV: headings in Japanese or KiCad's, columns in another order, quoted cells, sides and kinds as
    /// words; without headings the columns are number, name, side, kind.
    #[test]
    fn a_spreadsheet_or_kicad_csv_is_read() {
        let ja = rows_from_csv("名前,番号,種類,向き\n\"VCC, 5V\",8,電源入力,右\nGND,4,pwr,左\n").unwrap();
        assert_eq!((ja[0].number.as_str(), ja[0].name.as_str(), ja[0].side.as_str(), ja[0].kind), ("8", "VCC5V", "right", Some(sch_core::PinKind::PowerIn)));
        assert_eq!(ja[1].side, "left");
        let kicad = rows_from_csv("Number,Name,Electrical Type,Graphic Style,Orientation\n1,EN,Input,Line,Left\n2,NC,No connect,Line,\n").unwrap();
        assert_eq!((kicad[0].kind, kicad[0].side.as_str(), kicad[1].kind), (Some(sch_core::PinKind::Input), "left", Some(sch_core::PinKind::Nc)));
        let bare = rows_from_csv("1,A\n2,K,right\n").unwrap();
        assert_eq!((bare.len(), bare[1].side.as_str()), (2, "right"));
    }

    #[test]
    fn a_csv_that_cannot_be_a_pin_table_says_which_line() {
        let side = rows_from_csv("number,name,side\n1,A,left\n2,B,middle\n").unwrap_err();
        assert!(side.contains("3 行目") && side.contains("middle"), "{side}");
        let kind = rows_from_csv("number,name,kind\n1,A,analog\n").unwrap_err();
        assert!(kind.contains("2 行目"), "{kind}");
        assert!(rows_from_csv("number,name\n1,A\n1,B\n").unwrap_err().contains("番号 1"));
        assert!(rows_from_csv("number,name\n").is_err());
    }
}
