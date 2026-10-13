//! Quick add: press A on the sheet, type a part's name (or what it is), pick with the arrow keys and Enter.
//! Searches the basic parts and the library; favorites and recently used parts come first.

use super::*;
use library::LibPrefs;

/// What a quick-add result puts on the cursor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum QuickTarget {
    Basic(Kind),
    Lib(usize),
}

pub(super) struct QuickHit {
    pub target: QuickTarget,
    pub title: String,
    pub sub: String,
}

/// State of the open quick-add box.
#[derive(Default)]
pub(super) struct QuickAdd {
    pub query: String,
    pub sel: usize,
}

const MAX_HITS: usize = 10;

/// Other names a basic part answers to (English, symbols, what people type).
fn aliases(k: Kind) -> &'static str {
    match k {
        Kind::Resistor => "resistor r 抵抗 ohm オーム",
        Kind::Capacitor => "capacitor c condenser コンデンサ キャパシタ",
        Kind::Inductor => "inductor l coil コイル インダクタ",
        Kind::Diode => "diode d ダイオード",
        Kind::Led => "led light 発光 ダイオード",
        Kind::VSource => "source supply battery power v 電源 電池 信号",
        Kind::Npn => "npn transistor bjt q トランジスタ",
        Kind::Header(_) => "header connector pin コネクタ ヘッダ ピン",
        Kind::Ground => "ground gnd 0v アース グランド",
        Kind::Power => "vcc vdd 3v3 5v rail 電源ネット",
        Kind::Custom => "",
    }
}

fn tip_line(k: Kind) -> String {
    match k {
        Kind::Header(n) => format!("基本の部品・{n}ピンのコネクタ"),
        k if k.is_power_symbol() => "基本の部品・配線につなぐ記号".into(),
        _ => "基本の部品".into(),
    }
}

/// How well `text` matches the typed word: start of a word > inside > no match (0).
fn field_score(text: &str, word: &str, weight: u32) -> u32 {
    let t = text.to_lowercase();
    if t.is_empty() {
        0
    } else if t.starts_with(word) {
        weight * 2
    } else if t.split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(word)) {
        weight * 3 / 2
    } else if t.contains(word) {
        weight
    } else {
        0
    }
}

/// Results for `query`, best first. An empty query lists favorites, then recent parts, then the basic parts.
pub(super) fn quick_matches(query: &str, basic: &[Kind], library: &[PartDef], prefs: &LibPrefs) -> Vec<QuickHit> {
    let words: Vec<String> = query.split_whitespace().map(|w| w.to_lowercase()).collect();
    let fav_pos = |name: &str| prefs.fav.iter().position(|n| n == name);
    let recent_pos = |name: &str| prefs.recent.iter().position(|n| n == name);
    let mut scored: Vec<(u32, usize, QuickHit)> = vec![];
    let mut order = 0usize;
    for &k in basic {
        let label = k.label();
        let score = score_all(&words, &[(&label, 100), (aliases(k), 70)]);
        // with no query the basic parts come after the favorites and recent parts
        if let Some(s) = score.or(words.is_empty().then_some(1)) {
            scored.push((s, order, QuickHit { target: QuickTarget::Basic(k), title: label, sub: tip_line(k) }));
        }
        order += 1;
    }
    for (i, d) in library.iter().enumerate() {
        let pins = d.pins.iter().map(|p| p.name.as_str()).collect::<Vec<_>>().join(" ");
        let score = score_all(&words, &[(&d.name, 100), (&d.category, 40), (&d.footprint_name(), 30), (&pins, 20), (&d.description, 15)]);
        let boost = fav_pos(&d.name).map(|_| 400).or(recent_pos(&d.name).map(|p| 300 - p as u32)).unwrap_or(0);
        let base = if words.is_empty() { (boost > 0).then_some(1) } else { score };
        if let Some(s) = base {
            let sim = if d.sim.is_some() { " ◎" } else { "" };
            let star = if fav_pos(&d.name).is_some() { "★ " } else if recent_pos(&d.name).is_some() { "最近 " } else { "" };
            let sub = format!("{star}{}・{}{sim}", if d.category.is_empty() { "その他" } else { &d.category }, d.footprint_name());
            scored.push((s + boost, order, QuickHit { target: QuickTarget::Lib(i), title: d.name.clone(), sub }));
        }
        order += 1;
    }
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored.into_iter().take(MAX_HITS).map(|(_, _, h)| h).collect()
}

/// Every word must be found in some field; the score adds up the best field of each word. None = a word is missing.
fn score_all(words: &[String], fields: &[(&str, u32)]) -> Option<u32> {
    let mut total = 0;
    for w in words {
        let best = fields.iter().map(|(t, weight)| field_score(t, w, *weight)).max().unwrap_or(0);
        if best == 0 {
            return None;
        }
        total += best;
    }
    (!words.is_empty()).then_some(total)
}

impl SchEditor {
    pub(super) fn open_quick_add(&mut self) {
        self.quick = Some(QuickAdd::default());
    }

    /// Open the box with `text` typed in (startup flag `--quick`).
    pub fn open_quick_add_with(&mut self, text: &str) {
        self.quick = Some(QuickAdd { query: text.to_string(), sel: 0 });
    }

    fn pick_quick(&mut self, t: QuickTarget) {
        match t {
            QuickTarget::Basic(k) => {
                self.tool = Tool::Place;
                self.place_lib = None;
                self.place = k;
                self.pending = None;
            }
            QuickTarget::Lib(i) => self.choose_library_part(i),
        }
        self.quick = None;
    }

    /// The quick-add box: a search field and up to ten results under it.
    pub(super) fn quick_add_ui(&mut self, ui: &mut egui::Ui) {
        let Some(mut q) = self.quick.take() else { return };
        let ctx = ui.ctx().clone();
        let hits = quick_matches(&q.query, Kind::PALETTE, &self.library, &self.lib_prefs);
        let (down, up, enter, esc) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
            )
        });
        if down && !hits.is_empty() {
            q.sel = (q.sel + 1) % hits.len();
        }
        if up && !hits.is_empty() {
            q.sel = (q.sel + hits.len() - 1) % hits.len();
        }
        q.sel = q.sel.min(hits.len().saturating_sub(1));
        if esc {
            return;
        }
        if enter
            && let Some(h) = hits.get(q.sel) {
                self.pick_quick(h.target);
                return;
            }
        let mut chosen = None;
        let mut keep = true;
        egui::Area::new(egui::Id::new("quick_add")).order(egui::Order::Foreground).anchor(egui::Align2::CENTER_TOP, [0.0, 70.0]).show(&ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_width(380.0);
                let before = q.query.clone();
                let r = ui.add(egui::TextEdit::singleline(&mut q.query).desired_width(f32::INFINITY).hint_text("置きたい部品の名前 (例: 抵抗、LM358、0603、op amp)"));
                r.request_focus();
                r.on_hover_text("名前・種類・説明・ピン名・パッケージで探せます。↑↓で選び、Enter で置く部品にします。Esc でやめます");
                if q.query != before {
                    q.sel = 0;
                }
                if hits.is_empty() {
                    ui.small("見つかりません。別の言葉で探すか、左の「新規作成…」で部品をつくれます");
                }
                for (n, h) in hits.iter().enumerate() {
                    let label = format!("{}    {}", h.title, h.sub);
                    if ui.selectable_label(n == q.sel, label).clicked() {
                        chosen = Some(h.target);
                    }
                }
                ui.small("↑↓ 選ぶ / Enter 決める / Esc やめる");
            });
            // a click outside closes it
            if ui.input(|i| i.pointer.any_pressed()) && !ui.rect_contains_pointer(ui.min_rect()) {
                keep = false;
            }
        });
        if let Some(t) = chosen {
            self.pick_quick(t);
        } else if keep {
            self.quick = Some(q);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lib() -> Vec<PartDef> {
        part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<PartDef>(t).unwrap()).collect()
    }

    fn titles(q: &str, prefs: &LibPrefs) -> Vec<String> {
        quick_matches(q, Kind::PALETTE, &lib(), prefs).into_iter().map(|h| h.title).collect()
    }

    #[test]
    fn names_aliases_and_japanese_all_find_the_part() {
        let p = LibPrefs::default();
        assert_eq!(titles("抵抗", &p)[0], "抵抗");
        assert_eq!(titles("resistor", &p)[0], "抵抗");
        assert_eq!(titles("coil", &p)[0], "コイル");
        assert_eq!(titles("lm358", &p)[0], "LM358");
        assert!(titles("0603", &p).iter().any(|t| t == "C_100n_0603"), "found through its package");
        assert!(titles("zzzz-no-such", &p).is_empty());
    }

    #[test]
    fn every_word_has_to_match() {
        let p = LibPrefs::default();
        let both = titles("100n 0603", &p);
        assert_eq!(both.first().map(String::as_str), Some("C_100n_0603"));
        assert!(!both.iter().any(|t| t == "R_10k_0603"));
    }

    #[test]
    fn an_empty_query_lists_favorites_then_recent_then_basic_parts() {
        let mut p = LibPrefs::default();
        p.toggle_fav("2N7000");
        p.touch("LM358");
        let t = titles("", &p);
        assert_eq!(&t[..2], ["2N7000", "LM358"]);
        assert_eq!(t[2], Kind::PALETTE[0].label());
        assert_eq!(t.len(), MAX_HITS);
    }

    #[test]
    fn picking_a_result_puts_the_part_on_the_cursor() {
        let mut e = SchEditor::default();
        e.open_quick_add();
        assert!(e.quick.is_some());
        e.pick_quick(QuickTarget::Basic(Kind::Led));
        assert!(e.quick.is_none() && e.tool == Tool::Place && e.place == Kind::Led && e.place_lib.is_none());
        let i = e.library.iter().position(|d| d.name == "LM358").unwrap();
        e.open_quick_add_with("lm");
        e.pick_quick(QuickTarget::Lib(i));
        assert!(e.tool == Tool::Place && e.place_lib == Some(i));
        assert_eq!(e.lib_prefs.recent.first().map(String::as_str), Some("LM358"), "it counts as recently used");
    }

    #[test]
    fn a_favorite_wins_a_tie() {
        let mut p = LibPrefs::default();
        let plain = titles("diode", &p);
        let last = plain.iter().find(|t| t.starts_with("1N")).cloned().unwrap_or_default();
        p.toggle_fav(&last);
        assert!(titles("diode", &p).iter().position(|t| *t == last) <= plain.iter().position(|t| *t == last));
    }
}
