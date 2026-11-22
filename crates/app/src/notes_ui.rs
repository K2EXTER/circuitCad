//! Review notes (付箋, U6) on the schematic and the board: the numbered marks on the canvas and the window that lists them, edits
//! one, takes replies and marks it settled. The notes are saved in the file (`Schematic.notes`, `Board.notes`,
//! `design_spec::Note`).

use eframe::egui::{self, Color32, Pos2, Rect, Stroke};

/// The window's state: open or not, the note chosen, the reply being typed, and whether settled notes are listed.
#[derive(Default)]
pub struct NotesUi {
    pub open: bool,
    pub sel: Option<usize>,
    reply: String,
    show_resolved: bool,
}

/// What the window asks of the editor this frame.
pub enum NoteAction {
    /// Show the place of note `k` (move the view there).
    Jump(usize),
}

/// The side of a mark, screen pixels.
const MARK: f32 = 18.0;

/// The mark of note `k` (its number on a small square) at `at`: yellow while open, grey once settled, ringed when chosen.
fn paint_mark(painter: &egui::Painter, at: Pos2, k: usize, resolved: bool, chosen: bool) -> Rect {
    let r = Rect::from_min_size(at - egui::vec2(0.0, MARK), egui::vec2(MARK, MARK));
    let fill = if resolved { Color32::from_gray(120) } else { Color32::from_rgb(240, 210, 80) };
    painter.rect_filled(r, 3.0, fill);
    // a folded corner, so the mark reads as a sticky note and not as a part
    painter.add(egui::Shape::convex_polygon(vec![r.right_top() + egui::vec2(-6.0, 0.0), r.right_top(), r.right_top() + egui::vec2(0.0, 6.0)], Color32::from_gray(40), Stroke::NONE));
    painter.text(r.center(), egui::Align2::CENTER_CENTER, (k + 1).to_string(), egui::FontId::proportional(12.0), Color32::from_gray(20));
    if chosen {
        painter.rect_stroke(r.expand(2.0), 4.0, Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Outside);
    }
    r
}

/// Draws the marks of the notes shown on this canvas (`shown` says which) and makes them clickable: a click chooses the note and
/// opens the window; the first line of the text shows on hover. `to_screen` maps a note's place to the screen.
pub fn marks(ui: &mut egui::Ui, id: &str, notes: &[design_spec::Note], state: &mut NotesUi, shown: impl Fn(&design_spec::Note) -> bool, to_screen: impl Fn(f64, f64) -> Pos2) {
    let painter = ui.painter().clone();
    for (k, n) in notes.iter().enumerate().filter(|(_, n)| shown(n)) {
        let r = paint_mark(&painter, to_screen(n.x, n.y), k, n.resolved, state.sel == Some(k));
        let resp = ui.interact(r, ui.id().with((id, "note", k)), egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        let first = n.text.lines().next().unwrap_or("");
        let resp = resp.on_hover_text(if n.replies.is_empty() { format!("付箋 {}: {first}", k + 1) } else { format!("付箋 {}: {first}\n(返事 {} 件)", k + 1, n.replies.len()) });
        if resp.clicked() {
            state.sel = Some(k);
            state.open = true;
        }
    }
}

/// A new note at `(x, y)` on `sheet`, chosen and with the window open (its text is typed there).
pub fn add(notes: &mut Vec<design_spec::Note>, state: &mut NotesUi, x: f64, y: f64, sheet: usize) {
    notes.push(design_spec::Note { x, y, sheet, ..Default::default() });
    state.sel = Some(notes.len() - 1);
    state.open = true;
}

/// The "付箋" window: the list (open notes first; the settled ones on request), and the chosen note's text, replies, settled box and
/// delete. `where_` names the canvas ("回路図" / "基板"); `sheet_name` gives a note's sheet for the list (None = one sheet).
pub fn window(ctx: &egui::Context, where_: &str, notes: &mut Vec<design_spec::Note>, state: &mut NotesUi, sheet_name: impl Fn(usize) -> Option<String>) -> Option<NoteAction> {
    if !state.open {
        return None;
    }
    let mut action = None;
    let mut open = true;
    egui::Window::new(i18n::trf!("付箋 ({where_})"))
        .id(egui::Id::new(("notes_window", where_)))
        .open(&mut open)
        .default_pos(crate::widgets::beside_panel())
        .default_width(340.0)
        .resizable(true)
        .show(ctx, |ui| {
            let open_count = notes.iter().filter(|n| !n.resolved).count();
            ui.horizontal(|ui| {
                ui.label(i18n::trf!("まだの付箋 {open_count} 枚 / 全部で {} 枚", notes.len()));
                crate::icons::help(
                    ui,
                    "図の上の位置にコメントを置きます (授業の添削や、人に見てもらうとき)。置くのは、図の空いた所を右クリックして「ここに付箋を置く」。付箋には返事を付けられ、済んだら「解決済み」にします。付箋はファイルに保存されます",
                );
            });
            ui.checkbox(&mut state.show_resolved, i18n::tr("解決済みも出す")).on_hover_text(i18n::tr("済んだ付箋も一覧に出します (図の上では灰色の印です)"));
            if notes.is_empty() {
                ui.weak(i18n::tr("付箋はまだありません。図の空いた所を右クリックして「ここに付箋を置く」で置きます"));
            }
            // the list: open notes first, in the order they were put
            let mut order: Vec<usize> = (0..notes.len()).filter(|&k| state.show_resolved || !notes[k].resolved).collect();
            order.sort_by_key(|&k| notes[k].resolved);
            egui::ScrollArea::vertical().id_salt(("notes_list", where_)).max_height(160.0).show(ui, |ui| {
                for k in order {
                    let n = &notes[k];
                    let first = n.text.lines().next().filter(|s| !s.trim().is_empty()).unwrap_or("(まだ何も書いていません)");
                    let mut line = format!("{}. {first}", k + 1);
                    if let Some(s) = sheet_name(n.sheet) {
                        line = format!("{line}  [{s}]");
                    }
                    if !n.replies.is_empty() {
                        line = format!("{line}  (返事 {})", n.replies.len());
                    }
                    let text = if n.resolved { egui::RichText::new(format!("✔ {line}")).weak() } else { egui::RichText::new(line) };
                    if ui.selectable_label(state.sel == Some(k), text).on_hover_text(i18n::tr("押すと、この付箋を選んで、図のその場所を見せます")).clicked() {
                        state.sel = Some(k);
                        action = Some(NoteAction::Jump(k));
                    }
                }
            });
            let Some(k) = state.sel.filter(|&k| k < notes.len()) else { return };
            ui.separator();
            let mut delete = false;
            {
                let n = &mut notes[k];
                ui.horizontal(|ui| {
                    crate::widgets::heading_inline(ui, format!("付箋 {}", k + 1));
                    if ui.small_button(i18n::tr("その場所を見る")).on_hover_text(i18n::tr("図をこの付箋の場所へ動かします")).clicked() {
                        action = Some(NoteAction::Jump(k));
                    }
                });
                ui.add(egui::TextEdit::multiline(&mut n.text).desired_rows(3).desired_width(f32::INFINITY).hint_text(i18n::tr("気づいたこと・直してほしいこと")))
                    .on_hover_text(i18n::tr("付箋の本文。図の上の印にマウスを置くと、1 行目が出ます"));
                for (j, r) in n.replies.iter().enumerate() {
                    ui.horizontal_wrapped(|ui| {
                        ui.weak(i18n::trf!("返事 {}:", j + 1));
                        ui.label(r);
                    });
                }
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut state.reply).desired_width(220.0).hint_text(i18n::tr("返事"))).on_hover_text(i18n::tr("この付箋への返事 (直したこと・質問など)"));
                    let empty = state.reply.trim().is_empty();
                    if crate::widgets::unless(ui, empty.then_some("返事を書くと押せます"), egui::Button::new(i18n::tr("返事を付ける"))).clicked() {
                        n.replies.push(state.reply.trim().to_string());
                        state.reply.clear();
                    }
                });
                ui.horizontal(|ui| {
                    ui.checkbox(&mut n.resolved, i18n::tr("解決済み")).on_hover_text(i18n::tr("済んだ付箋にします。図の上の印が灰色になり、一覧では「解決済みも出す」のときだけ出ます"));
                    if ui.small_button(i18n::tr("この付箋を消す")).on_hover_text(i18n::tr("付箋を消します (元に戻すは Ctrl+Z)")).clicked() {
                        delete = true;
                    }
                });
            }
            if delete {
                notes.remove(k);
                state.sel = None;
            }
        });
    if !open {
        state.open = false;
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A note put on the schematic and on the board is chosen with its window open, and both files keep it (text, replies,
    /// settled, sheet); a file without notes reads as none.
    #[test]
    fn notes_are_put_and_kept_in_the_files() {
        let mut sch = sch_core::Schematic::default();
        let mut st = NotesUi::default();
        add(&mut sch.notes, &mut st, 12.0, 7.0, 1);
        assert_eq!((st.sel, st.open), (Some(0), true));
        sch.notes[0].text = "R1 の値を見直す".into();
        sch.notes[0].replies.push("10k にしました".into());
        sch.notes[0].resolved = true;
        let back: sch_core::Schematic = serde_json::from_str(&serde_json::to_string(&sch).unwrap()).unwrap();
        assert_eq!(back.notes, sch.notes);
        let plain = serde_json::to_string(&sch_core::Schematic::default()).unwrap();
        assert!(!plain.contains("notes"), "no notes, no key: older versions read the file as before");
        let mut board = pcb_core::model::Board::new("t", 50.0, 30.0);
        add(&mut board.notes, &mut st, 10.0, 5.0, 0);
        board.notes[0].text = "C3 をもっと U1 の近くに".into();
        let back: pcb_core::model::Board = serde_json::from_str(&serde_json::to_string(&board).unwrap()).unwrap();
        assert_eq!(back.notes, board.notes);
    }
}
