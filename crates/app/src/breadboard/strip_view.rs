//! The stripboard and perfboard views of the proto tab (B5): the layout of `sch_core::stripboard` drawn from the parts' side — the
//! copper strips with their cuts (stripboard), or the bare wires that join the holes (perfboard) — with the parts, the wire links and
//! the supplies off the board; and the list to build it from (the parts' holes, the cuts, the links).

use super::*;
use sch_core::mount::{shopping, Mount};
use sch_core::stripboard::{hole_name, strip_check, strip_layout, At, StripLayout};

/// What the person builds on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BoardKind {
    #[default]
    Breadboard,
    /// Copper strips along the rows, cut where a strip must stop.
    Stripboard,
    /// Plain holes: the strips' work is done by bare wires along the rows.
    Perfboard,
}

const COPPER: Color32 = Color32::from_rgb(196, 128, 72);
const BOARD: Color32 = Color32::from_rgb(214, 190, 140);
const WIRE: Color32 = Color32::from_rgb(190, 196, 204);
/// An adapter board's green, and a socket's frame.
const ADAPTER: Color32 = Color32::from_rgb(40, 120, 70);
const SOCKET: Color32 = Color32::from_gray(85);

/// A colour for a net's links, the supplies red and ground black.
fn net_colour(net: &str) -> Color32 {
    let u = net.to_ascii_uppercase();
    if matches!(u.as_str(), "GND" | "0" | "VSS") {
        return Color32::from_gray(30);
    }
    if u.starts_with("VCC") || u.starts_with("VDD") || u.starts_with('+') || u.starts_with("VIN") || u.contains('V') && u.starts_with(|c: char| c.is_ascii_digit()) {
        return Color32::from_rgb(210, 40, 40);
    }
    const PALETTE: [Color32; 5] = [Color32::from_rgb(40, 110, 220), Color32::from_rgb(30, 150, 70), Color32::from_rgb(230, 140, 20), Color32::from_rgb(140, 60, 190), Color32::from_rgb(20, 160, 170)];
    PALETTE[net.bytes().fold(0usize, |a, b| a.wrapping_mul(31).wrapping_add(b as usize)) % PALETTE.len()]
}

impl BreadboardView {
    /// The left panel of the stripboard / perfboard views.
    pub(super) fn strip_panel(&mut self, ui: &mut egui::Ui, sch: &mut SchEditor) {
        let l = strip_layout(&sch.sch);
        let strip = self.kind == BoardKind::Stripboard;
        ui.label(
            spaced(if strip {
                "銅の帯が横に並んだ基板 (ストリップ基板) に組む並べ方です。上の帯は 1 本ずつ 1 つのネット (左に名前)。2 本足の部品は、つなぐ 2 つの帯をまたいで縦に差します。IC とトランジスタは下の段に置き、× の穴で帯を切って足ごとに分け、色の線で自分のネットの帯につなぎます。帯を切るのは、裏から穴の銅を削ります (ドリルの刃を手で回す・専用のカッター)"
            } else {
                "穴だけの基板 (ユニバーサル基板) に組む並べ方です。ストリップ基板と同じ置き方で、銅の帯の代わりに、灰色の線 (すずめっき線) を裏で穴から穴へはんだ付けします。上の線は 1 本ずつ 1 つのネット (左に名前)。色の線は表を通すビニール線です"
            })
            .color(NOTE),
        );
        let problems = strip_check(&sch.sch, &l);
        ui.add_space(4.0);
        if problems.is_empty() {
            ui.colored_label(crate::theme::OK, i18n::tr("✔ 回路図どおりにつながっています"));
        } else {
            for p in &problems {
                ui.colored_label(BAD, i18n::trf!("・{}", i18n::tr_dyn(p)));
            }
        }
        ui.small(i18n::trf!("大きさ: {} 列 × {} 行 (2.54mm の間隔で {:.0} × {:.0} mm)", l.cols, l.rows, l.cols as f32 * 2.54, l.rows as f32 * 2.54));
        ui.separator();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.label(egui::RichText::new(i18n::tr("部品と差す穴")).strong());
            for p in &l.parts {
                ui.horizontal_top(|ui| {
                    ui.add_sized([44.0, 18.0], egui::Label::new(egui::RichText::new(&p.refdes).strong()));
                    ui.vertical(|ui| {
                        ui.add(egui::Label::new(p.leads.iter().map(|(pin, at)| format!("{pin}:{}", hole_name(*at))).collect::<Vec<_>>().join("  ")).wrap());
                        // a socket or an adapter under the part (a leaded twin is in the list below, once for all)
                        let how = if p.mount == Mount::Leaded { String::new() } else { p.mount.describe() };
                        if !how.is_empty() {
                            ui.small(egui::RichText::new(i18n::tr_dyn(&how)).color(NOTE));
                        }
                    });
                });
            }
            let list = shopping(&l.parts.iter().map(|p| (p.refdes.clone(), p.mount.clone())).collect::<Vec<_>>());
            if !list.is_empty() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(i18n::tr("ソケットと変換基板")).strong()).on_hover_text(i18n::tr(
                    "IC は基板に直接はんだ付けせず、丸ピンのソケットに差します。こての熱で傷めず、壊れたら差し替えられます。足の細かい表面実装の IC (SOP・QFN・QFP・SOT-23) は、変換基板にはんだ付けしてから、そのピンを丸ピンの連結ソケットに差します。変換基板はチップの 1 番ピンが変換基板の 1 番ピン (DIP の数え方) になるので、上の穴の表は、部品のピンの番号のままで差せます。チップ抵抗・コンデンサは、同じ値のリード部品で組めます",
                ));
                for (item, n, refs) in &list {
                    ui.small(format!("{}  ×{n}  ({})", i18n::tr_dyn(item), refs.join(", ")));
                }
            }
            if strip && !l.cuts.is_empty() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(i18n::trf!("帯を切る穴 ({} か所)", l.cuts.len())).strong()).on_hover_text(i18n::tr("この穴で銅の帯を切り、左右を分けます。切ったら、テスターの抵抗のレンジで、左右がつながっていないことを確かめます"));
                ui.add(egui::Label::new(l.cuts.iter().map(|c| hole_name(*c)).collect::<Vec<_>>().join(" ")).wrap());
            }
            if !strip {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(i18n::tr("裏のすずめっき線")).strong()).on_hover_text(i18n::tr("同じ行の穴を、端から端まで 1 本の線でつないで、はんだ付けします"));
                for (a, b) in perf_wires(&l) {
                    ui.small(format!("{} – {}", hole_name(a), hole_name(b)));
                }
            }
            if !l.links.is_empty() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(i18n::trf!("表の線 ({} 本)", l.links.len())).strong());
                for k in &l.links {
                    ui.small(format!("{} – {}  ({})", hole_name(k.a), hole_name(k.b), k.net));
                }
            }
            if !l.off.is_empty() {
                ui.add_space(6.0);
                ui.label(egui::RichText::new(i18n::tr("基板の外 (電源・電池・信号源)")).strong());
                for (r, pins) in &l.off {
                    ui.small(format!("{r}  {}", pins.iter().map(|(pin, net, at)| format!("{pin} ({net}) → {}", hole_name(*at))).collect::<Vec<_>>().join("、")));
                }
            }
        });
    }

    /// The board drawn.
    pub(super) fn strip_canvas(&mut self, ui: &mut egui::Ui, sch: &mut SchEditor) {
        let l = strip_layout(&sch.sch);
        let (response, painter) = ui.allocate_painter(ui.available_size(), egui::Sense::hover());
        let rect = response.rect;
        painter.rect_filled(rect, 0.0, crate::canvas_theme::col(crate::canvas_theme::Role::Bg));
        if l.parts.is_empty() {
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, i18n::tr("回路図に部品がありません"), egui::FontId::proportional(16.0), Color32::from_gray(200));
            return;
        }
        // a margin on the left for the strips' names and the supplies off the board
        let left = 9.0;
        let w = left + l.cols as f32 + 1.0;
        let h = l.rows as f32 + 1.5;
        let pitch = ((rect.width() - 40.0) / w).min((rect.height() - 40.0) / h).clamp(5.0, 30.0);
        let origin = Pos2::new(rect.left() + (rect.width() - w * pitch) / 2.0 + left * pitch, rect.top() + 30.0 + pitch);
        let to = |a: At| origin + Vec2::new(a.1 as f32, a.0 as f32) * pitch;
        let board = Rect::from_min_max(to((0, 0)) - Vec2::splat(pitch * 0.8), to((l.rows - 1, l.cols - 1)) + Vec2::splat(pitch * 0.8));
        painter.rect_filled(board, 3.0, BOARD);
        let font = egui::FontId::monospace((pitch * 0.55).clamp(8.0, 13.0));
        let strip = self.kind == BoardKind::Stripboard;
        if strip {
            for (r, a, b) in l.pieces() {
                painter.rect_filled(Rect::from_min_max(to((r, a)) - Vec2::new(pitch * 0.45, pitch * 0.32), to((r, b)) + Vec2::new(pitch * 0.45, pitch * 0.32)), 1.0, COPPER.gamma_multiply(0.75));
            }
        }
        for r in 0..l.rows {
            for c in 0..l.cols {
                painter.circle_filled(to((r, c)), pitch * 0.13, Color32::from_gray(70));
            }
        }
        if strip {
            for cut in &l.cuts {
                let p = to(*cut);
                let d = pitch * 0.3;
                let s = Stroke::new(2.0, Color32::from_rgb(200, 30, 30));
                painter.line_segment([p - Vec2::splat(d), p + Vec2::splat(d)], s);
                painter.line_segment([p + Vec2::new(-d, d), p + Vec2::new(d, -d)], s);
            }
        } else {
            for (a, b) in perf_wires(&l) {
                painter.line_segment([to(a), to(b)], Stroke::new(pitch * 0.18, WIRE));
            }
        }
        // the home strips' names, and the row letters and column numbers
        for (r, net) in l.homes.iter().enumerate() {
            // ground's black would not show on the dark canvas: its name in grey
        let named = if net_colour(net) == Color32::from_gray(30) { Color32::from_gray(170) } else { net_colour(net) };
        painter.text(to((r as u16, 0)) - Vec2::new(pitch * 1.0, 0.0), egui::Align2::RIGHT_CENTER, net, font.clone(), named);
        }
        for c in (0..l.cols).step_by(5) {
            painter.text(to((0, c)) - Vec2::new(0.0, pitch * 1.0), egui::Align2::CENTER_BOTTOM, format!("{}", c + 1), font.clone(), Color32::from_gray(150));
        }
        // the parts
        for p in &l.parts {
            let pts: Vec<Pos2> = p.leads.iter().map(|(_, a)| to(*a)).collect();
            if pts.is_empty() {
                continue;
            }
            let bb = Rect::from_points(&pts);
            // an adapter board (green) or a socket's frame under the chip
            if p.mount.width().is_some() || p.mount.one_row() {
                let fill = if matches!(p.mount, Mount::Socket { .. }) { SOCKET } else { ADAPTER };
                painter.rect_filled(bb.expand2(Vec2::new(pitch * 0.45, pitch * 0.5)), 2.0, fill);
            }
            match p.look {
                sch_core::breadboard::Look::Dip | sch_core::breadboard::Look::Row => {
                    let body = bb.expand2(Vec2::new(pitch * 0.35, pitch * 0.45));
                    match &p.mount {
                        // the small chip in the middle of its adapter, the package's name under the reference
                        Mount::Adapter { package, .. } => {
                            let side = (body.width().min(body.height()) * 0.5).clamp(pitch * 0.8, pitch * 2.5);
                            let chip = Rect::from_center_size(body.center(), Vec2::splat(side));
                            painter.rect_filled(chip, 1.0, Color32::from_gray(30));
                            painter.text(chip.center(), egui::Align2::CENTER_CENTER, &p.refdes, font.clone(), Color32::WHITE);
                            painter.text(chip.center_bottom() + Vec2::new(0.0, 2.0), egui::Align2::CENTER_TOP, package, egui::FontId::proportional((pitch * 0.45).clamp(7.0, 11.0)), Color32::from_gray(225));
                        }
                        _ => {
                            painter.rect_filled(if p.look == sch_core::breadboard::Look::Dip { body.shrink2(Vec2::new(pitch * 0.55, 0.0)) } else { body }, 2.0, Color32::from_gray(35));
                            painter.text(body.center(), egui::Align2::CENTER_CENTER, &p.refdes, font.clone(), Color32::WHITE);
                        }
                    }
                }
                _ => {
                    for w in pts.windows(2) {
                        painter.line_segment([w[0], w[1]], Stroke::new(1.5, Color32::from_gray(110)));
                    }
                    let mid = bb.center();
                    let body = if bb.height() >= bb.width() { Rect::from_center_size(mid, Vec2::new(pitch * 0.55, (bb.height() * 0.5).clamp(pitch * 0.8, pitch * 3.0))) } else { Rect::from_center_size(mid, Vec2::new((bb.width() * 0.5).max(pitch * 0.8), pitch * 0.55)) };
                    painter.rect_filled(body, 3.0, Color32::from_rgb(230, 205, 160));
                    painter.rect_stroke(body, 3.0, Stroke::new(1.0, Color32::from_gray(80)), egui::StrokeKind::Inside);
                    painter.text(body.right_center() + Vec2::new(2.0, 0.0), egui::Align2::LEFT_CENTER, &p.refdes, font.clone(), Color32::from_gray(20));
                }
            }
            for q in &pts {
                painter.circle_filled(*q, pitch * 0.18, Color32::from_gray(150));
            }
        }
        // the links over the parts, and the supplies off the board
        for k in &l.links {
            let c = net_colour(&k.net);
            painter.line_segment([to(k.a), to(k.b)], Stroke::new(pitch * 0.16, c));
            for e in [k.a, k.b] {
                painter.circle_filled(to(e), pitch * 0.17, c);
            }
        }
        for (k, (r, pins)) in l.off.iter().enumerate() {
            let box_at = origin + Vec2::new(-left * pitch + pitch * 0.5, (l.homes.len() as f32 + 1.5 + k as f32 * 2.2) * pitch);
            let body = Rect::from_min_size(box_at, Vec2::new(pitch * 3.0, pitch * 1.6));
            painter.rect_filled(body, 3.0, Color32::from_gray(60));
            painter.text(body.center(), egui::Align2::CENTER_CENTER, r, font.clone(), Color32::WHITE);
            for (pin, net, at) in pins {
                let _ = pin;
                painter.line_segment([body.right_center(), to(*at)], Stroke::new(pitch * 0.12, net_colour(net)));
            }
        }
    }
}

/// The bare wires of a perfboard: along each piece of strip, from its first used hole to its last.
pub fn perf_wires(l: &StripLayout) -> Vec<(At, At)> {
    let used: Vec<At> = l.used().into_iter().map(|(a, _)| a).collect();
    l.pieces()
        .into_iter()
        .filter_map(|(r, a, b)| {
            let cols: Vec<u16> = used.iter().filter(|u| u.0 == r && u.1 >= a && u.1 <= b).map(|u| u.1).collect();
            let (lo, hi) = (cols.iter().min()?, cols.iter().max()?);
            (lo < hi).then_some(((r, *lo), (r, *hi)))
        })
        .collect()
}
