//! The table of operations (name, key, what it does) and the Ctrl+K command palette built on it
//! (`docs/code-notes/app/ui-rules.md`, 2026-11-17).
//!
//! The menus, the hovers, the guide (F1) and the palette all take an operation's name and key from `ACTIONS`, so what
//! the screen says and what the key does cannot drift apart. The editors still read their own canvas keys (R, F, Del,
//! W …); the test `keys_in_the_table_are_the_ones_the_editors_read` keeps the two in step.

use eframe::egui;

/// Where an operation applies.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Place {
    Both,
    Schematic,
    Board,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    Palette,
    Find,
    New,
    Open,
    Save,
    SaveAs,
    Undo,
    Redo,
    SelectAll,
    Fit,
    Rotate,
    Delete,
    Cancel,
    Rerun,
    AddPart,
    Wire,
    NetLabel,
    Via,
    Measure,
    Layer,
    Flip,
    Erc,
    Drc,
    Help,
    Glossary,
    PowerOn,
    Settings,
    Calc,
}

pub struct Action {
    pub act: Act,
    /// The id kept for the pinned list (`settings.json`).
    pub id: &'static str,
    pub name: &'static str,
    pub desc: &'static str,
    pub group: &'static str,
    /// The key as it is written on the screen; empty = none.
    pub key: &'static str,
    pub place: Place,
}

const fn a(act: Act, id: &'static str, name: &'static str, desc: &'static str, group: &'static str, key: &'static str, place: Place) -> Action {
    Action { act, id, name, desc, group, key, place }
}

use Place::{Board as B, Both, Schematic as S};

/// Every operation with a key, and the ones the palette offers besides the analyses. A key means the same in the
/// schematic and on the board.
pub const ACTIONS: &[Action] = &[
    a(Act::Palette, "palette", "操作を探す", "名前や、したいことの言葉で操作を探して実行", "表示", "Ctrl+K", Both),
    a(Act::Find, "find", "部品・ネットを探す", "部品番号やネットの名前で探して、そこへ移る", "表示", "Ctrl+F", S),
    a(Act::New, "new", "新規", "いまの図を空にして新しく始める", "ファイル", "Ctrl+N", Both),
    a(Act::Open, "open", "開く", "保存したファイルを開く", "ファイル", "Ctrl+O", Both),
    a(Act::Save, "save", "保存", "いまのファイルに上書き", "ファイル", "Ctrl+S", Both),
    a(Act::SaveAs, "save_as", "名前を付けて保存", "別の名前で保存", "ファイル", "Ctrl+Shift+S", Both),
    a(Act::Undo, "undo", "元に戻す", "直前の操作を取り消す", "編集", "Ctrl+Z", Both),
    a(Act::Redo, "redo", "やり直し", "取り消した操作をやり直す", "編集", "Ctrl+Y", Both),
    a(Act::SelectAll, "select_all", "すべて選ぶ", "図の上のものを全部選ぶ", "編集", "Ctrl+A", Both),
    a(Act::Fit, "fit", "全体を見る", "全体が入るように拡大", "表示", "F", Both),
    a(Act::Rotate, "rotate", "回す", "選んだ部品 (置く前の部品) を 90° 回す", "編集", "R", Both),
    a(Act::Delete, "delete", "消す", "選んだものを消す", "編集", "Delete", Both),
    a(Act::Cancel, "cancel", "やめる", "描きかけ・選択をやめて、選択ツールに戻る", "編集", "Esc", Both),
    a(Act::Rerun, "rerun", "もう一度計算", "最後に動かした解析を、いまの回路で計算し直す", "解析", "F5", S),
    a(Act::AddPart, "add_part", "部品を足す", "名前で探して置く (クイック追加)", "ツール", "A", S),
    a(Act::Wire, "wire", "配線", "ピンとピンを線でつなぐ", "ツール", "W", Both),
    a(Act::NetLabel, "net_label", "ネットラベル", "線に名前を付けて、離れた所とつなぐ", "ツール", "N", S),
    a(Act::Via, "via", "ビア", "表と裏をつなぐ穴 (配線中は、そこで層を替える)", "ツール", "V", B),
    a(Act::Measure, "measure", "距離を測る", "2 点の距離を測る (Enter・X・Y で寸法線に)", "ツール", "M", B),
    a(Act::Layer, "layer", "層を替える", "配線する層を表と裏で切り替える", "ツール", "L", B),
    a(Act::Flip, "flip", "裏返す", "選んだ部品を基板の反対の面へ", "編集", "B", B),
    a(Act::Erc, "erc", "ERC (回路の点検)", "つなぎ忘れ・ショート・電源のないピンを探す", "点検", "", S),
    a(Act::Drc, "drc", "DRC (基板の点検)", "間隔・線幅・つながりを点検", "点検", "", B),
    a(Act::Help, "help", "操作ガイド", "キーと操作の一覧", "ヘルプ", "F1", Both),
    a(Act::Glossary, "glossary", "用語集", "電子回路とこのソフトの言葉を引く", "ヘルプ", "", Both),
    a(Act::Settings, "settings", "設定", "言語・画面の大きさ・色・ホイール・自動保存", "設定", "", Both),
    a(Act::PowerOn, "power_on", "最初の電源投入のガイド", "組んだ基板の電流制限の値と、正常なら流れる電流の範囲", "学ぶ", "", S),
    a(Act::Calc, "calc", "値の決め方 (計算ツール)", "LED の抵抗・分圧・フィルタなどの値を計算", "学ぶ", "", S),
];

pub fn action(act: Act) -> &'static Action {
    ACTIONS.iter().find(|x| x.act == act).expect("every Act is in ACTIONS")
}

/// "配線 (W)": a name with its key, for a hover or a menu.
#[cfg_attr(not(test), allow(dead_code))]
pub fn with_key(act: Act) -> String {
    let x = action(act);
    if x.key.is_empty() { i18n::tr(x.name).to_string() } else { format!("{} ({})", i18n::tr(x.name), x.key) }
}

/// What a palette row does when chosen.
#[derive(Clone, PartialEq, Debug)]
pub enum Run {
    Act(Act),
    /// An analysis section of the schematic (`ANALYSIS_SECTIONS` key).
    Analysis(&'static str),
    Part(String),
    Net(String),
    Term(&'static str),
}

#[derive(Clone, Debug)]
pub struct Entry {
    /// Kept in the pinned list and the recent list.
    pub id: String,
    pub name: String,
    pub desc: String,
    pub group: String,
    pub key: String,
    /// Why it cannot be used now (shown instead of hiding the row).
    pub why_not: Option<String>,
    pub run: Run,
}

/// The palette's state while it is open.
#[derive(Default)]
pub struct Palette {
    pub open: bool,
    /// Only the parts and nets (Ctrl+F).
    pub find: bool,
    pub query: String,
    pub sel: usize,
}

/// What the user did in the palette this frame.
pub enum Outcome {
    None,
    Run(Entry),
    TogglePin(String),
}

/// Does the row match all the words typed (in its name, description, group or key; letters in any case)?
pub fn matches(e: &Entry, q: &str) -> bool {
    let hay = format!("{} {} {} {} {}", e.name, e.desc, e.group, e.key, e.id).to_lowercase();
    q.split_whitespace().all(|w| hay.contains(&w.to_lowercase()))
}

/// The rows to show: with nothing typed, the recent and the pinned rows (each under its heading); otherwise every match,
/// names that start with the words first.
pub fn rows<'a>(all: &'a [Entry], q: &str, recent: &[String], pins: &[String]) -> Vec<(Option<&'static str>, &'a Entry)> {
    let q = q.trim();
    if q.is_empty() {
        let pick = |ids: &[String]| ids.iter().filter_map(|id| all.iter().find(|e| &e.id == id)).collect::<Vec<_>>();
        let mut out = vec![];
        for (k, e) in pick(recent).into_iter().take(5).enumerate() {
            out.push(((k == 0).then_some("最近使った"), e));
        }
        for (k, e) in pick(pins).into_iter().enumerate() {
            out.push(((k == 0).then_some("★ ピン留め"), e));
        }
        if out.is_empty() {
            out = all.iter().take(8).map(|e| (None, e)).collect();
        }
        return out;
    }
    let ql = q.to_lowercase();
    let mut hits: Vec<&Entry> = all.iter().filter(|e| matches(e, q)).collect();
    hits.sort_by_key(|e| (!e.name.to_lowercase().starts_with(&ql), !e.name.to_lowercase().contains(&ql)));
    hits.into_iter().take(40).map(|e| (None, e)).collect()
}

const ROW_H: f32 = 28.0;
/// The width kept for the name column, so the descriptions line up.
const NAME_W: f32 = 190.0;

/// The palette window: one line per row (name, description, group, key), ↑↓ to move, Enter to run, Ctrl+Enter to pin,
/// Esc to close.
pub fn show(ctx: &egui::Context, p: &mut Palette, all: &[Entry], recent: &[String], pins: &[String]) -> Outcome {
    if !p.open {
        return Outcome::None;
    }
    let shown = rows(all, &p.query, recent, pins);
    let (up, down, enter, pin, esc) = ctx.input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp),
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
            i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
        )
    });
    if esc {
        p.open = false;
        return Outcome::None;
    }
    let n = shown.len();
    if n > 0 {
        if down {
            p.sel = (p.sel + 1) % n;
        }
        if up {
            p.sel = (p.sel + n - 1) % n;
        }
        p.sel = p.sel.min(n - 1);
    }
    let mut out = Outcome::None;
    if let Some((_, e)) = shown.get(p.sel) {
        if pin {
            out = Outcome::TogglePin(e.id.clone());
        } else if enter && e.why_not.is_none() {
            out = Outcome::Run((*e).clone());
        }
    }
    let mut close = false;
    egui::Area::new(egui::Id::new("command_palette")).order(egui::Order::Foreground).anchor(egui::Align2::CENTER_TOP, [0.0, 70.0]).show(ctx, |ui| {
        egui::Frame::popup(ui.style()).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
            ui.set_width(640.0);
            let hint = if p.find { i18n::tr("部品番号・ネットの名前 (例: R1、VCC)") } else { i18n::tr("操作を探す… (名前や、したいことの言葉で)") };
            let before = p.query.clone();
            let r = ui.add(egui::TextEdit::singleline(&mut p.query).desired_width(f32::INFINITY).font(egui::FontId::proportional(16.0)).hint_text(hint));
            r.request_focus();
            if p.query != before {
                p.sel = 0;
            }
            ui.add_space(4.0);
            if shown.is_empty() {
                ui.label(egui::RichText::new(i18n::tr("見つかりません。別の言葉で探してください")).weak());
            }
            egui::ScrollArea::vertical().max_height(ROW_H * 14.0).show(ui, |ui| {
                for (k, (heading, e)) in shown.iter().enumerate() {
                    if let Some(h) = heading {
                        ui.add_space(2.0);
                        ui.label(egui::RichText::new(i18n::tr(h)).small().weak());
                    }
                    let resp = row(ui, e, p.query.trim(), k == p.sel, pins.contains(&e.id));
                    if k == p.sel && (up || down) {
                        resp.scroll_to_me(None);
                    }
                    if resp.clicked() && e.why_not.is_none() {
                        out = Outcome::Run((*e).clone());
                    }
                    if resp.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                        p.sel = k;
                    }
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.small(i18n::tr("↑↓ 選ぶ   Enter 実行   Esc 閉じる   Ctrl+Enter ★に留める・外す"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.small(egui::RichText::new(i18n::tr("Ctrl+K でいつでも")).weak());
                });
            });
        });
        if ui.input(|i| i.pointer.any_pressed()) && !ui.rect_contains_pointer(ui.min_rect()) {
            close = true;
        }
    });
    if close || matches!(out, Outcome::Run(_)) {
        p.open = false;
    }
    out
}

fn row(ui: &mut egui::Ui, e: &Entry, q: &str, sel: bool, pinned: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW_H), egui::Sense::click());
    let p = ui.painter();
    let vis = ui.visuals();
    if sel {
        p.rect_filled(rect, 4.0, crate::theme::SELECTED_BG);
    }
    let off = e.why_not.is_some();
    let strong = if off { vis.weak_text_color() } else { vis.strong_text_color() };
    let weak = vis.weak_text_color();
    let y = rect.center().y;
    let mut right = rect.right() - 8.0;
    if !e.key.is_empty() {
        let g = p.layout_no_wrap(e.key.clone(), egui::FontId::monospace(13.0), strong);
        let w = g.size().x + 12.0;
        let chip = egui::Rect::from_min_size(egui::pos2(right - w, y - 10.0), egui::vec2(w, 20.0));
        p.rect(chip, 4.0, vis.extreme_bg_color, egui::Stroke::new(1.0, weak), egui::StrokeKind::Inside);
        p.galley(chip.center() - g.size() / 2.0, g, strong);
        right = chip.left() - 8.0;
    }
    let group = format!("{}{}", if pinned { "★ " } else { "" }, i18n::tr_dyn(&e.group));
    let g = p.layout_no_wrap(group, egui::FontId::proportional(12.0), weak);
    p.galley(egui::pos2(right - g.size().x, y - g.size().y / 2.0), g.clone(), weak);
    right -= g.size().x + 12.0;
    let x = rect.left() + 10.0;
    let name = p.layout_job(marked(&i18n::tr_dyn(&e.name), q, egui::FontId::proportional(14.0), strong));
    let name_w = name.size().x;
    p.galley(egui::pos2(x, y - name.size().y / 2.0), name, strong);
    // the description in its own column, cut with … when the row is narrow (the name, group and key never are)
    let dx = x + name_w.max(NAME_W) + 12.0;
    let (desc, col) = match &e.why_not {
        Some(why) => (i18n::trf!("使えない: {}", i18n::tr_dyn(why)), crate::theme::WARN),
        None => (i18n::tr_dyn(&e.desc).to_string(), weak),
    };
    let mut job = marked(&desc, q, egui::FontId::proportional(12.0), col);
    job.wrap = egui::text::TextWrapping::truncate_at_width((right - dx).max(0.0));
    let g = p.layout_job(job);
    p.galley(egui::pos2(dx, y - g.size().y / 2.0), g, col);
    resp
}

/// The text with the first match of each typed word underlined.
fn marked(s: &str, q: &str, font: egui::FontId, col: egui::Color32) -> egui::text::LayoutJob {
    let fmt = |c: egui::Color32, u: bool| egui::TextFormat { font_id: font.clone(), color: c, underline: if u { egui::Stroke::new(1.0, c) } else { egui::Stroke::NONE }, ..Default::default() };
    let lower = s.to_lowercase();
    // byte ranges of the matches (lowercasing keeps the byte offsets for the scripts used here)
    let mut spans: Vec<(usize, usize)> = q.split_whitespace().filter_map(|w| lower.find(&w.to_lowercase()).map(|at| (at, at + w.len()))).filter(|(a, b)| s.is_char_boundary(*a) && s.is_char_boundary(*b) && lower.len() == s.len()).collect();
    spans.sort();
    let mut job = egui::text::LayoutJob::default();
    let mut at = 0;
    for (a, b) in spans {
        if a < at {
            continue;
        }
        job.append(&s[at..a], 0.0, fmt(col, false));
        job.append(&s[a..b], 0.0, fmt(crate::theme::HIGHLIGHT, true));
        at = b;
    }
    job.append(&s[at..], 0.0, fmt(col, false));
    job
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, name: &str, desc: &str) -> Entry {
        Entry { id: id.into(), name: name.into(), desc: desc.into(), group: "解析".into(), key: String::new(), why_not: None, run: Run::Act(Act::Help) }
    }

    #[test]
    fn words_match_name_or_description_and_names_come_first() {
        let all = vec![entry("fft", "FFT", "波形にどの周波数が入っているか"), entry("ac", "AC解析", "周波数特性を描く"), entry("f", "周波数", "用語")];
        let hits: Vec<&str> = rows(&all, "周波数", &[], &[]).iter().map(|(_, e)| e.id.as_str()).collect();
        assert_eq!(hits, ["f", "fft", "ac"], "the name that starts with the word first, then the ones found in their descriptions");
        assert!(rows(&all, "fft 周波数", &[], &[]).len() == 1, "every word must match");
        assert!(rows(&all, "ac", &[], &[]).iter().any(|(_, e)| e.id == "ac"), "letters in any case");
    }

    #[test]
    fn empty_query_shows_recent_then_pinned_under_headings() {
        let all = vec![entry("a", "A", ""), entry("b", "B", ""), entry("c", "C", "")];
        let r = rows(&all, "", &["b".into()], &["c".into(), "a".into()]);
        assert_eq!(r.iter().map(|(h, e)| (*h, e.id.as_str())).collect::<Vec<_>>(), [(Some("最近使った"), "b"), (Some("★ ピン留め"), "c"), (None, "a")]);
    }

    #[test]
    fn every_act_has_one_row_and_keys_are_not_shared() {
        let mut keys: Vec<&str> = ACTIONS.iter().map(|x| x.key).filter(|k| !k.is_empty()).collect();
        keys.sort();
        let n = keys.len();
        keys.dedup();
        assert_eq!(n, keys.len(), "a key means one thing");
        let mut ids: Vec<&str> = ACTIONS.iter().map(|x| x.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), ACTIONS.len());
        assert_eq!(with_key(Act::Wire), "配線 (W)");
    }
}
