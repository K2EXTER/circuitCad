//! The sketch written as blocks (B6): the blocks are stacked like Scratch's, dragged by their title from the list at the top
//! into the start (setup) and the loop, or inside an if and a repeat; a value's slot takes a number, a pin, a variable or a
//! reading, changed from its ▼. The sketch's text stays what is saved and run (`sim_core::sketch::blocks`): each change writes the
//! blocks as code, and each frame reads the code back into blocks (so a change of the code shows in the blocks too).

use super::*;
use sim_core::sketch::Board;
use sim_core::sketch::blocks::{self as bk, BExpr, Block, BlockSketch, PinMode, VarDecl};

/// Where a list of blocks is: the start (0) or the loop (1), then for each level the block's place and which of its lists
/// (0: the body, or the `then` of an if; 1: its `else`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ListPath {
    section: u8,
    nest: Vec<(usize, u8)>,
}

impl ListPath {
    fn child(&self, i: usize, which: u8) -> ListPath {
        let mut p = self.clone();
        p.nest.push((i, which));
        p
    }
}

/// What is being dragged: a block from its place, or a new one from the list at the top.
#[derive(Clone)]
enum Drag {
    Move(ListPath, usize),
    New(Block),
}

/// What is done to the blocks once they are drawn (they are borrowed while drawn).
enum Act {
    Drop(Drag, ListPath, usize),
    Remove(ListPath, usize),
    Duplicate(ListPath, usize),
    Rename(String, String),
    RemoveVar(usize),
}

/// The kind of value a slot wants (its first choices and how a number in it is shown).
#[derive(Clone, Copy, PartialEq)]
enum Slot {
    Pin,
    Level,
    Num,
    Cond,
    Print,
}

struct Cx {
    board: Board,
    /// The variables that may be named here (the sketch's, and the counters of the repeats around).
    vars: Vec<String>,
    base: egui::Id,
    n: u64,
    acts: Vec<Act>,
    /// Where a dragged block goes if let go now (the innermost list under the pointer).
    target: Option<(ListPath, usize)>,
    dragging: bool,
    /// Where the dragged block would go as of the last frame: a space of its height is left open there (the blocks below make
    /// way), so the place shows before it is let go.
    ghost: Option<(ListPath, usize)>,
    ghost_h: f32,
    /// The block being moved (drawn faded in its place while it is dragged).
    moving: Option<(ListPath, usize)>,
    /// The block the sketch runs now, by its place in the order the blocks are drawn (`order` counts them as they are).
    running: Option<usize>,
    order: usize,
}

impl Cx {
    fn id(&mut self) -> egui::Id {
        self.n += 1;
        self.base.with(self.n)
    }
}

// the colours of the kinds of block (white words on them in either theme)
const OUT: Color32 = Color32::from_rgb(50, 105, 195);
const TIME: Color32 = Color32::from_rgb(20, 125, 145);
const CONTROL: Color32 = Color32::from_rgb(196, 118, 18);
const VARS: Color32 = Color32::from_rgb(190, 66, 92);
const SERIAL: Color32 = Color32::from_rgb(122, 78, 186);
const READ: Color32 = Color32::from_rgb(34, 128, 88);
const MATH: Color32 = Color32::from_rgb(88, 132, 36);

fn look(b: &Block) -> (Color32, &'static str) {
    match b {
        Block::PinMode { .. } => (OUT, "ピンの使い方:"),
        Block::DigitalWrite { .. } => (OUT, "出力:"),
        Block::AnalogWrite { .. } => (OUT, "PWM 出力:"),
        Block::Delay(_) => (TIME, "待つ:"),
        Block::SerialBegin(_) => (SERIAL, "シリアルを始める:"),
        Block::Print { .. } => (SERIAL, "シリアルに送る:"),
        Block::Set { .. } | Block::Change { .. } => (VARS, "変数:"),
        Block::If { .. } => (CONTROL, "もし"),
        Block::Repeat { .. } | Block::While { .. } => (CONTROL, "くり返す:"),
    }
}

fn tip(b: &Block) -> &'static str {
    match b {
        Block::PinMode { .. } => "ピンを出力 (LED などを動かす)・入力 (スイッチなどを読む)・引き上げつきの入力 (押していない間 HIGH) のどれで使うか決めます (pinMode)。ふつうは「最初に 1 回」に置きます",
        Block::DigitalWrite { .. } => "ピンから HIGH (5 V) か LOW (0 V) を出します (digitalWrite)。先にそのピンを「出力」にしておきます",
        Block::AnalogWrite { .. } => "ピンから 0〜255 の割合で速く点けたり消したりするパルスを出します (analogWrite)。LED の明るさやモーターの速さを変えられます。~ の付いたピンだけで使えます",
        Block::Delay(_) => "決めた時間だけ止まって待ちます (delay)。1000 ミリ秒で 1 秒です",
        Block::SerialBegin(_) => "シリアル (パソコンとの通信) を始めます (Serial.begin)。送る前に「最初に 1 回」に置きます",
        Block::Print { .. } => "文字や値をシリアルモニタに送ります (Serial.print / println)。「改行」で 1 行ずつに分かれます",
        Block::Set { .. } => "変数に値を入れます (x = …)",
        Block::Change { .. } => "変数の値を増やします (x += …)。マイナスの数なら減ります",
        Block::If { .. } => "条件が成り立つときだけ、中のブロックを動かします (if)。右クリックで「そうでなければ」を付けられます",
        Block::Repeat { .. } => "中のブロックを決めた回数くり返します (for)",
        Block::While { .. } => "条件が成り立つ間、中のブロックをくり返します (while)",
    }
}

/// The blocks the list at the top offers (for the board: its LED and a pin `analogWrite` pulses).
fn palette(board: Board, first_var: Option<&str>) -> Vec<Block> {
    let led = BExpr::Name("LED_BUILTIN".into());
    let pwm = board.pwm_pins().iter().copied().find(|&p| p == 9).or(board.pwm_pins().first().copied()).unwrap_or(0);
    let button = || BExpr::Bin("==".into(), Box::new(BExpr::DigitalRead(Box::new(BExpr::Int(2)))), Box::new(BExpr::Name("LOW".into())));
    let var = first_var.unwrap_or("x").to_string();
    vec![
        Block::PinMode { pin: led.clone(), mode: PinMode::Output },
        Block::DigitalWrite { pin: led, value: BExpr::Name("HIGH".into()) },
        Block::AnalogWrite { pin: BExpr::Int(pwm as i64), value: BExpr::Int(128) },
        Block::Delay(BExpr::Int(500)),
        Block::If { cond: button(), then: vec![], els: None },
        Block::If { cond: button(), then: vec![], els: Some(vec![]) },
        Block::Repeat { counter: "i".into(), times: BExpr::Int(3), body: vec![] },
        Block::While { cond: button(), body: vec![] },
        Block::Set { name: var.clone(), value: BExpr::Int(0) },
        Block::Change { name: var, by: BExpr::Int(1) },
        Block::SerialBegin(BExpr::Int(9600)),
        Block::Print { value: BExpr::Text("こんにちは".into()), newline: true },
    ]
}

/// The block editor of a sketch: the list to drag from, the variables, the start and the loop. True when the blocks changed (the
/// caller writes them as code).
/// `lines`: each block's line in the sketch, in the order they are drawn (`blocks::from_code_with_lines`); `running`: the line the
/// sketch runs at now (its block is lit up).
pub fn edit_blocks(ui: &mut egui::Ui, s: &mut BlockSketch, board: Board, salt: &str, lines: &[u32], running: Option<u32>) -> bool {
    // top to bottom, as wide as it is given (the rows of blocks wrap inside it)
    let width = ui.available_width();
    ui.allocate_ui_with_layout(egui::vec2(width, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.set_width(width);
        edit_blocks_in(ui, s, board, salt, lines, running)
    })
    .inner
}

fn edit_blocks_in(ui: &mut egui::Ui, s: &mut BlockSketch, board: Board, salt: &str, lines: &[u32], running: Option<u32>) -> bool {
    let before = s.clone();
    let mut cx = Cx {
        board,
        vars: s.vars.iter().map(|v| v.name.clone()).collect(),
        base: egui::Id::new(("blocks", salt)),
        n: 0,
        acts: vec![],
        target: None,
        dragging: egui::DragAndDrop::has_payload_of_type::<Drag>(ui.ctx()),
        ghost: None,
        ghost_h: 0.0,
        moving: None,
        // the block the sketch runs now: its place in the order they are drawn
        running: running.and_then(|l| lines.iter().position(|&x| x == l)),
        order: 0,
    };
    let ghost_id = cx.base.with("ghost");
    if cx.dragging {
        cx.ghost = ui.data(|d| d.get_temp::<Option<(ListPath, usize)>>(ghost_id)).flatten();
        cx.ghost_h = ui.data(|d| d.get_temp(ghost_id.with("h"))).unwrap_or(GHOST_H);
        if let Some(p) = egui::DragAndDrop::payload::<Drag>(ui.ctx())
            && let Drag::Move(l, i) = &*p
        {
            cx.moving = Some((l.clone(), *i));
        }
    }
    ui.spacing_mut().item_spacing.y = 3.0;
    let palette_rect = palette_ui(ui, s, &mut cx);
    ui.add_space(crate::theme::SPACE_S);
    vars_ui(ui, s, &mut cx);
    for (section, title, hint) in [
        (0u8, "最初に 1 回 (setup)", "電源を入れたとき (リセットしたとき) に 1 回だけ、上から順に動きます"),
        (1, "ずっとくり返す (loop)", "「最初に 1 回」のあと、上から下までを何度もくり返します"),
    ] {
        ui.add_space(crate::theme::SPACE_S);
        ui.label(egui::RichText::new(i18n::tr(title)).strong()).on_hover_text(i18n::tr(hint));
        let list = if section == 0 { &mut s.setup } else { &mut s.looped };
        egui::Frame::NONE.fill(ui.visuals().faint_bg_color).corner_radius(4.0).inner_margin(4.0).show(ui, |ui| {
            ui.set_width(ui.available_width());
            list_ui(ui, list, ListPath { section, nest: vec![] }, &mut cx);
        });
    }
    // the place it would go now is left open next frame (none over the list at the top: there it is thrown away)
    let over_palette = ui.ctx().pointer_interact_pos().is_some_and(|q| palette_rect.contains(q));
    let ghost = if cx.dragging && !over_palette { cx.target.clone() } else { None };
    ui.data_mut(|d| d.insert_temp(ghost_id, ghost));
    if !cx.dragging {
        ui.data_mut(|d| d.insert_temp(ghost_id.with("h"), GHOST_H));
    }
    // let go: where it was dragged to (back onto the list at the top: thrown away)
    if cx.dragging && ui.input(|i| i.pointer.any_released()) {
        let pointer = ui.ctx().pointer_interact_pos();
        if let Some(p) = egui::DragAndDrop::take_payload::<Drag>(ui.ctx()) {
            let p = (*p).clone();
            if pointer.is_some_and(|q| palette_rect.contains(q)) {
                if let Drag::Move(l, i) = p {
                    cx.acts.push(Act::Remove(l, i));
                }
            } else if let Some((l, i)) = cx.target.take() {
                cx.acts.push(Act::Drop(p, l, i));
            }
        }
    }
    for a in std::mem::take(&mut cx.acts) {
        apply(s, a);
    }
    *s != before
}

/// The blocks to drag in (a click puts one at the end of the loop). Its rect: a block dragged back onto it is thrown away.
fn palette_ui(ui: &mut egui::Ui, s: &BlockSketch, cx: &mut Cx) -> egui::Rect {
    let first_var = s.vars.first().map(|v| v.name.as_str());
    let items = palette(cx.board, first_var);
    let throw = cx.dragging && egui::DragAndDrop::payload::<Drag>(ui.ctx()).is_some_and(|p| matches!(*p, Drag::Move(..)));
    let r = egui::Frame::NONE.stroke(egui::Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color)).corner_radius(4.0).inner_margin(4.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(i18n::tr("ブロック")).strong());
            let note = if throw { "ここで離すと、そのブロックを消します" } else { "名前をドラッグして下に置きます (クリックで「ずっとくり返す」の最後に)" };
            ui.label(egui::RichText::new(i18n::tr(note)).small().color(NOTE));
        });
        ui.horizontal_wrapped(|ui| {
            for b in items {
                let needs_var = matches!(b, Block::Set { .. } | Block::Change { .. }) && s.vars.is_empty();
                let (color, title) = look(&b);
                let label = match &b {
                    Block::If { els: Some(_), .. } => "もし〜でなければ",
                    Block::Repeat { .. } => "n 回くり返す",
                    Block::While { .. } => "〜の間くり返す",
                    Block::Set { .. } => "変数を〜にする",
                    Block::Change { .. } => "変数を増やす",
                    _ => title.trim_end_matches(':'),
                };
                // a whole chip to the next row when it does not fit (a frame would squeeze its name instead)
                let font = egui::TextStyle::Body.resolve(ui.style());
                let w = ui.painter().layout_no_wrap(i18n::tr_dyn(label).to_string(), font, Color32::WHITE).size().x + 14.0;
                if ui.available_size_before_wrap().x < w {
                    ui.end_row();
                }
                let why_not = needs_var.then_some("先に「＋ 変数を作る」で変数を作ります");
                let r = crate::widgets::unless_ui(ui, why_not, |ui| {
                    egui::Frame::NONE.fill(color).corner_radius(5.0).inner_margin(egui::Margin::symmetric(6, 2)).show(ui, |ui| handle(ui, label, color, Drag::New(b.clone()))).inner
                });
                let r = r.on_hover_text(i18n::tr(tip(&b)));
                if !needs_var && r.clicked() {
                    cx.acts.push(Act::Drop(Drag::New(b.clone()), ListPath { section: 1, nest: vec![] }, usize::MAX));
                }
            }
        });
    });
    r.response.rect
}

/// A block's name, dragged to move it.
fn handle(ui: &mut egui::Ui, text: &str, color: Color32, payload: Drag) -> egui::Response {
    let r = ui.add(egui::Label::new(egui::RichText::new(i18n::tr_dyn(text)).strong().color(Color32::WHITE)).selectable(false).wrap_mode(egui::TextWrapMode::Extend).sense(egui::Sense::click_and_drag()));
    drag_from(ui, &r, text, color, payload);
    r
}

/// What dragging a block does (from its name or from its ground): it goes with the pointer as its name until it is let go.
fn drag_from(ui: &egui::Ui, r: &egui::Response, text: &str, color: Color32, payload: Drag) {
    if r.drag_started() {
        egui::DragAndDrop::set_payload(ui.ctx(), payload);
    }
    if r.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        if let Some(p) = ui.ctx().pointer_interact_pos() {
            let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("block_drag")));
            let font = egui::TextStyle::Button.resolve(ui.style());
            let galley = painter.layout_no_wrap(i18n::tr_dyn(text).to_string(), font, Color32::WHITE);
            let rect = egui::Rect::from_min_size(p + egui::vec2(10.0, -galley.size().y / 2.0 - 3.0), galley.size() + egui::vec2(12.0, 6.0));
            painter.rect_filled(rect, 5.0, color.gamma_multiply(0.92));
            painter.galley(rect.min + egui::vec2(6.0, 3.0), galley, Color32::WHITE);
        }
    } else if r.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
}

/// A word between a block's slots.
fn word(ui: &mut egui::Ui, text: &str) {
    // not selectable: a drag on it takes the block (a selectable label would take the drag to select its text)
    ui.add(egui::Label::new(egui::RichText::new(i18n::tr_dyn(text)).color(Color32::WHITE)).selectable(false).wrap_mode(egui::TextWrapMode::Extend));
}

fn vars_ui(ui: &mut egui::Ui, s: &mut BlockSketch, cx: &mut Cx) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(i18n::tr("変数")).strong()).on_hover_text(i18n::tr("値をしまっておく入れ物です。名前を付けて作り、「変数」のブロックで値を入れたり増やしたりします。値の ▼ から読めます"));
        if ui.small_button(i18n::tr("＋ 変数を作る")).on_hover_text(i18n::tr("新しい変数 (整数、最初は 0) を作ります。名前は英字・数字・_ で付けます")).clicked() {
            let name = ["count", "value", "level", "x", "y", "z"]
                .into_iter()
                .map(String::from)
                .chain((1..).map(|k| format!("v{k}")))
                .find(|n| !s.vars.iter().any(|v| v.name == *n))
                .unwrap();
            s.vars.push(VarDecl { name, float: false, init: BExpr::Int(0) });
        }
    });
    for k in 0..s.vars.len() {
        let id = cx.base.with(("var_name", k));
        egui::Frame::NONE.fill(VARS).corner_radius(5.0).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let v = &mut s.vars[k];
                egui::ComboBox::from_id_salt(id.with("ty"))
                    .selected_text(i18n::tr(if v.float { "小数" } else { "整数" }))
                    .width(56.0)
                    .show_ui(ui, |ui| {
                        for (f, t) in [(false, "整数"), (true, "小数")] {
                            if ui.selectable_label(v.float == f, i18n::tr(t)).on_hover_text(i18n::tr(if f { "0.5 のような小数も入る変数 (float)" } else { "1・2・-3 のような整数の変数 (int)" })).clicked() && v.float != f {
                                v.float = f;
                                v.init = match (&v.init, f) {
                                    (BExpr::Int(n), true) => BExpr::Float(*n as f64),
                                    (BExpr::Float(x), false) => BExpr::Int(x.round() as i64),
                                    (e, _) => e.clone(),
                                };
                            }
                        }
                    });
                // the name is typed into a copy and taken when the box is left (every use of the old name follows it)
                let mut buf: String = ui.data(|d| d.get_temp(id)).unwrap_or_else(|| v.name.clone());
                let taken = |n: &str| cx.vars.iter().enumerate().any(|(j, x)| j != k && x == n);
                let ok = bk::valid_name(&buf) && !taken(&buf);
                let mut edit = egui::TextEdit::singleline(&mut buf).desired_width(90.0).font(egui::TextStyle::Monospace);
                if !ok {
                    edit = edit.text_color(crate::theme::error());
                }
                let r = ui.add(edit).on_hover_text(i18n::tr("変数の名前 (英字・数字・_。数字で始めない)。変えると、使っているブロックの名前も変わります"));
                if r.lost_focus() {
                    if ok && buf != v.name {
                        cx.acts.push(Act::Rename(v.name.clone(), buf.clone()));
                    }
                    ui.data_mut(|d| d.remove::<String>(id));
                } else if r.has_focus() {
                    ui.data_mut(|d| d.insert_temp(id, buf));
                }
                word(ui, "最初の値");
                expr_ui(ui, &mut s.vars[k].init, Slot::Num, cx);
                if ui.small_button("×").on_hover_text(i18n::tr("この変数を消します (使っているブロックは誤りになります)")).clicked() {
                    cx.acts.push(Act::RemoveVar(k));
                }
            });
        });
    }
}

/// How tall the open space is for a new block from the list at the top.
const GHOST_H: f32 = 26.0;

fn list_ui(ui: &mut egui::Ui, list: &mut [Block], path: ListPath, cx: &mut Cx) {
    let n = list.len();
    for i in 0..n {
        ghost_ui(ui, cx, &path, i);
        let (rect, header) = block_ui(ui, &mut list[i], &path, i, cx);
        if cx.moving.as_ref().is_some_and(|(l, k)| *l == path && *k == i) {
            ui.data_mut(|d| d.insert_temp(cx.base.with("ghost").with("h"), rect.height()));
        }
        drop_spot(ui, cx, rect, header, &path, i);
    }
    ghost_ui(ui, cx, &path, n);
    // the end of the list: where a block dropped below the others goes (an empty list is only this)
    let h = ui.spacing().interact_size.y;
    let r = ui.allocate_response(egui::vec2(ui.available_width().max(40.0), if n == 0 { h } else { h * 0.6 }), egui::Sense::hover());
    if n == 0 && cx.ghost.as_ref() != Some(&(path.clone(), 0)) {
        ui.painter().text(r.rect.left_center() + egui::vec2(4.0, 0.0), egui::Align2::LEFT_CENTER, i18n::tr("(ここにブロックを置きます)"), egui::TextStyle::Small.resolve(ui.style()), NOTE);
    }
    if cx.dragging && cx.target.is_none() && ui.rect_contains_pointer(r.rect) {
        cx.target = Some((path, n));
    }
}

/// The open space where the dragged block would go (as of the last frame), before the block `i` of the list. The pointer in it
/// keeps it there.
fn ghost_ui(ui: &mut egui::Ui, cx: &mut Cx, path: &ListPath, i: usize) {
    if cx.ghost.as_ref().is_none_or(|(l, k)| l != path || *k != i) {
        return;
    }
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(60.0), cx.ghost_h), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 5.0, crate::theme::HIGHLIGHT.gamma_multiply(0.18));
    p.rect_stroke(rect, 5.0, egui::Stroke::new(1.5, crate::theme::HIGHLIGHT), egui::StrokeKind::Inside);
    if cx.target.is_none() && ui.rect_contains_pointer(rect) {
        cx.target = Some((path.clone(), i));
    }
}

/// While a block is dragged over this one: it goes above or below it (by the half of its title row the pointer is in).
fn drop_spot(ui: &egui::Ui, cx: &mut Cx, rect: egui::Rect, header: egui::Rect, path: &ListPath, i: usize) {
    if !cx.dragging || cx.target.is_some() || !ui.rect_contains_pointer(rect) {
        return;
    }
    let y = ui.ctx().pointer_interact_pos().map_or(0.0, |p| p.y);
    let before = y < header.center().y;
    cx.target = Some((path.clone(), if before { i } else { i + 1 }));
}

/// A block's right-click menu: 複製 (0), 消す (1), and on an if 「そうでなければ」 on or off (2).
fn block_menu(ui: &mut egui::Ui, is_if: bool, has_else: bool, menu: &mut Option<u8>) {
    if ui.button(i18n::tr("複製")).on_hover_text(i18n::tr("このブロックの写しをすぐ下に置きます")).clicked() {
        *menu = Some(0);
        ui.close();
    }
    if ui.button(i18n::tr("消す")).on_hover_text(i18n::tr("このブロックを中のブロックごと消します (上のブロックの一覧へドラッグしても消せます)")).clicked() {
        *menu = Some(1);
        ui.close();
    }
    if is_if {
        let t = if has_else { "「そうでなければ」を外す" } else { "「そうでなければ」を付ける" };
        if ui.button(i18n::tr(t)).clicked() {
            *menu = Some(2);
            ui.close();
        }
    }
}

/// One block and the blocks inside it. Its rect and its title row's.
fn block_ui(ui: &mut egui::Ui, b: &mut Block, path: &ListPath, i: usize, cx: &mut Cx) -> (egui::Rect, egui::Rect) {
    let (color, title) = look(b);
    let mut header = egui::Rect::NOTHING;
    let mut menu: Option<u8> = None;
    let has_else = matches!(b, Block::If { els: Some(_), .. });
    let is_if = matches!(b, Block::If { .. });
    // the block's ground is grabbed too (where it was last frame; registered before what is on it, so its lists, slots and the
    // blocks inside it take the pointer first)
    let ground_id = cx.base.with(("ground", path, i));
    let ground = ui.data(|d| d.get_temp::<egui::Rect>(ground_id)).map(|r| ui.interact(r, ground_id, egui::Sense::click_and_drag()));
    let moved = cx.moving.as_ref().is_some_and(|(l, k)| l == path && *k == i);
    let lit = cx.running == Some(cx.order);
    cx.order += 1;
    // a dark edge: a block inside one of its own colour (a repeat in an if) still shows where it is
    let edge = egui::Stroke::new(1.0, Color32::from_black_alpha(110));
    let r = egui::Frame::NONE.fill(color).stroke(edge).corner_radius(5.0).inner_margin(egui::Margin::symmetric(6, 3)).show(ui, |ui| {
        if moved {
            ui.multiply_opacity(0.35);
        }
        ui.set_min_width(120.0);
        header = ui
            .horizontal_wrapped(|ui| {
                let h = handle(ui, title, color, Drag::Move(path.clone(), i)).on_hover_text(i18n::tr(tip(b)));
                h.context_menu(|ui| block_menu(ui, is_if, has_else, &mut menu));
                slots_ui(ui, b, cx);
            })
            .response
            .rect;
        // the blocks inside, in a hollow of the panel's colour (as the mouth of a C)
        let inner = |ui: &mut egui::Ui, list: &mut Vec<Block>, p: ListPath, cx: &mut Cx| {
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                egui::Frame::NONE.fill(ui.visuals().panel_fill).corner_radius(4.0).inner_margin(4.0).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.vertical(|ui| list_ui(ui, list, p, cx));
                });
            });
        };
        match b {
            Block::If { then, els, .. } => {
                inner(ui, then, path.child(i, 0), cx);
                if let Some(e) = els {
                    word(ui, "そうでなければ");
                    inner(ui, e, path.child(i, 1), cx);
                }
            }
            Block::Repeat { counter, body, .. } => {
                cx.vars.push(counter.clone());
                inner(ui, body, path.child(i, 0), cx);
                cx.vars.pop();
            }
            Block::While { body, .. } => inner(ui, body, path.child(i, 0), cx),
            _ => {}
        }
        // the bottom arm of an if or a repeat: the C closed below its blocks (a block let go on it goes after the whole C)
        if matches!(b, Block::If { .. } | Block::Repeat { .. } | Block::While { .. }) {
            ui.add_space(7.0);
        }
    });
    ui.data_mut(|d| d.insert_temp(ground_id, r.response.rect));
    if lit {
        // the block the sketch is at now (the delay it waits in): a bright edge
        ui.painter().rect_stroke(r.response.rect.expand(1.5), 6.0, egui::Stroke::new(3.0, crate::theme::HIGHLIGHT), egui::StrokeKind::Outside);
        // brought into sight when the sketch moves on to it out of sight (not every frame: the blocks may be scrolled by hand)
        let seen_id = cx.base.with("lit");
        let before: Option<usize> = ui.data(|d| d.get_temp::<Option<usize>>(seen_id)).flatten();
        if before != cx.running {
            ui.data_mut(|d| d.insert_temp(seen_id, cx.running));
            if !ui.clip_rect().contains_rect(r.response.rect) {
                ui.scroll_to_rect(r.response.rect, None);
            }
        }
    }
    if let Some(g) = ground {
        drag_from(ui, &g, title, color, Drag::Move(path.clone(), i));
        g.context_menu(|ui| block_menu(ui, is_if, has_else, &mut menu));
    }
    match menu {
        Some(0) => cx.acts.push(Act::Duplicate(path.clone(), i)),
        Some(1) => cx.acts.push(Act::Remove(path.clone(), i)),
        Some(2) => {
            if let Block::If { els, .. } = b {
                *els = if els.is_some() { None } else { Some(vec![]) };
            }
        }
        _ => {}
    }
    (r.response.rect, header)
}

fn slots_ui(ui: &mut egui::Ui, b: &mut Block, cx: &mut Cx) {
    match b {
        Block::PinMode { pin, mode } => {
            expr_ui(ui, pin, Slot::Pin, cx);
            word(ui, "は");
            let label = |m: PinMode| match m {
                PinMode::Output => "出力",
                PinMode::Input => "入力",
                PinMode::InputPullup => "入力 (引き上げ)",
            };
            combo(ui, cx, i18n::tr(label(*mode)), |ui| {
                for m in PinMode::ALL {
                    ui.selectable_value(mode, m, i18n::tr(label(m)));
                }
            });
        }
        Block::DigitalWrite { pin, value } => {
            expr_ui(ui, pin, Slot::Pin, cx);
            word(ui, "を");
            expr_ui(ui, value, Slot::Level, cx);
            word(ui, "にする");
        }
        Block::AnalogWrite { pin, value } => {
            expr_ui(ui, pin, Slot::Pin, cx);
            word(ui, "に");
            expr_ui(ui, value, Slot::Num, cx);
            word(ui, "(0〜255)");
        }
        Block::Delay(ms) => {
            expr_ui(ui, ms, Slot::Num, cx);
            word(ui, "ミリ秒");
        }
        Block::SerialBegin(baud) => {
            expr_ui(ui, baud, Slot::Num, cx);
            word(ui, "bps");
        }
        Block::Print { value, newline } => {
            expr_ui(ui, value, Slot::Print, cx);
            ui.checkbox(newline, egui::RichText::new(i18n::tr("改行")).color(Color32::WHITE)).on_hover_text(i18n::tr("送ったあとに改行します (Serial.println)。外すと、次に送るものが同じ行に続きます (Serial.print)"));
        }
        Block::Set { name, value } => {
            var_combo(ui, name, cx);
            word(ui, "を");
            expr_ui(ui, value, Slot::Num, cx);
            word(ui, "にする");
        }
        Block::Change { name, by } => {
            var_combo(ui, name, cx);
            word(ui, "を");
            expr_ui(ui, by, Slot::Num, cx);
            word(ui, "だけ増やす");
        }
        Block::If { cond, .. } => {
            expr_ui(ui, cond, Slot::Cond, cx);
            word(ui, "なら");
        }
        Block::Repeat { counter, times, .. } => {
            expr_ui(ui, times, Slot::Num, cx);
            word(ui, "回");
            ui.add(egui::Label::new(egui::RichText::new(i18n::trf!("(数える変数 {counter})")).small().color(Color32::from_white_alpha(200))).selectable(false))
                .on_hover_text(i18n::tr("くり返した回数 (0 から) が入る変数です。中のブロックの値の ▼ から読めます"));
        }
        Block::While { cond, .. } => {
            expr_ui(ui, cond, Slot::Cond, cx);
            word(ui, "の間");
        }
    }
}

/// Adds something that is laid out in a frame of its own (a list to choose from, a reading with slots): on the next row of the
/// block when it does not fit in what is left of this one (its width as it was drawn last frame). A frame in a wrapping row
/// would otherwise stick out of the panel.
fn wrapped<R>(ui: &mut egui::Ui, id: egui::Id, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let w: f32 = ui.data(|d| d.get_temp(id)).unwrap_or(0.0);
    let at_start = ui.cursor().min.x <= ui.max_rect().min.x + 1.0;
    if w > 0.0 && !at_start && ui.available_size_before_wrap().x < w {
        ui.end_row();
    }
    let r = ui.scope(add);
    ui.data_mut(|d| d.insert_temp(id, r.response.rect.width()));
    r.inner
}

/// A list to choose from in a slot (wrapped to the next row when it does not fit).
fn combo(ui: &mut egui::Ui, cx: &mut Cx, shown: impl Into<egui::WidgetText>, add: impl FnOnce(&mut egui::Ui)) {
    combo_w(ui, cx, shown, None, add);
}

/// A list to choose from, `width` wide (else egui's width for a list).
fn combo_w(ui: &mut egui::Ui, cx: &mut Cx, shown: impl Into<egui::WidgetText>, width: Option<f32>, add: impl FnOnce(&mut egui::Ui)) {
    let id = cx.id();
    wrapped(ui, id.with("w"), |ui| {
        let mut c = egui::ComboBox::from_id_salt(id).selected_text(shown).height(360.0);
        if let Some(w) = width {
            c = c.width(w);
        }
        c.show_ui(ui, add);
    });
}

fn var_combo(ui: &mut egui::Ui, name: &mut String, cx: &mut Cx) {
    let known = cx.vars.contains(name);
    let shown = if known { egui::RichText::new(name.as_str()) } else { egui::RichText::new(name.as_str()).color(crate::theme::error()) };
    let vars = cx.vars.clone();
    combo(ui, cx, shown, |ui| {
        for v in vars {
            ui.selectable_value(name, v.clone(), v);
        }
    });
}

/// The board's pins as a slot offers them: the LED, the digital pins (~ where `analogWrite` pulses), the analog ones.
fn pin_choices(board: Board) -> Vec<(String, BExpr)> {
    let mut v = vec![(format!("LED ({})", board.pin_name(board.led())), BExpr::Name("LED_BUILTIN".into()))];
    let digital = if board == Board::Tiny85 { board.pins() as u8 } else { board.a0() };
    for p in 0..digital {
        let pwm = if board.pwm_pins().contains(&p) { "~" } else { "" };
        v.push((format!("{}{pwm}", board.pin_name(p)), BExpr::Int(p as i64)));
    }
    for k in 0..board.analog_pins() {
        v.push((format!("A{k}"), BExpr::Name(format!("A{k}"))));
    }
    v
}

fn is_pin(e: &BExpr, board: Board) -> bool {
    pin_choices(board).iter().any(|(_, x)| x == e)
}

/// How a name shows in a slot's list (HIGH and LOW with their volts).
fn name_label(x: &str) -> String {
    match x {
        "HIGH" => "HIGH (5 V)".into(),
        "LOW" => "LOW (0 V)".into(),
        "true" => i18n::tr("true (成り立つ)").to_string(),
        "false" => i18n::tr("false (成り立たない)").to_string(),
        v => v.to_string(),
    }
}

/// A list of choices with "the other values" under them (a pin's or a name's slot).
fn list_with_others(ui: &mut egui::Ui, choices: Vec<(String, BExpr)>, cur: &BExpr, pick: &mut Option<BExpr>, others: &[Option<(&str, &str, BExpr)>]) {
    for (label, x) in choices {
        if ui.selectable_label(*cur == x, label).clicked() {
            *pick = Some(x);
        }
    }
    ui.separator();
    ui.label(egui::RichText::new(i18n::tr("ほかの値に替える")).small().color(NOTE));
    others_ui(ui, others, pick);
}

/// A reading or calculation in a slot, with its own slots (wrapped to the next row when it does not fit). A condition (a comparison,
/// and / or, not) is a hexagon, as the slots of an if and a while that want one; a value is rounded.
fn chip(ui: &mut egui::Ui, cx: &mut Cx, color: Color32, body: &mut dyn FnMut(&mut egui::Ui, &mut Cx)) {
    let id = cx.id();
    let hex = color == CONTROL;
    wrapped(ui, id, |ui| {
        // the shape is drawn under its contents once their size is known
        let under = ui.painter().add(egui::Shape::Noop);
        let side = if hex { 11 } else { 6 };
        let r = egui::Frame::NONE.inner_margin(egui::Margin { left: side, right: side, top: 2, bottom: 2 }).show(ui, |ui| {
            // a long one wraps inside itself (a frame of its own does not wrap with the row around it)
            ui.horizontal_wrapped(|ui| body(ui, cx));
        });
        let rect = r.response.rect;
        let stroke = egui::Stroke::new(1.0, Color32::from_white_alpha(90));
        let shape = if hex {
            let k = (rect.height() / 2.0).min(10.0);
            let (l, rt, t, b, c) = (rect.left(), rect.right(), rect.top(), rect.bottom(), rect.center().y);
            let pts = vec![egui::pos2(l, c), egui::pos2(l + k, t), egui::pos2(rt - k, t), egui::pos2(rt, c), egui::pos2(rt - k, b), egui::pos2(l + k, b)];
            egui::Shape::convex_polygon(pts, color, stroke)
        } else {
            egui::Shape::rect_filled(rect, (rect.height() / 2.0).min(10.0), color)
        };
        ui.painter().set(under, shape);
        if !hex {
            ui.painter().rect_stroke(rect, (rect.height() / 2.0).min(10.0), stroke, egui::StrokeKind::Inside);
        }
    });
}

/// A value in its slot: a pin or a name chosen from a list, a number to drag or type, text, or a reading or calculation with slots
/// of its own. What else may go in the slot is at the end of the list (a pin or a name) or in the ▼ after it (the rest).
fn expr_ui(ui: &mut egui::Ui, e: &mut BExpr, slot: Slot, cx: &mut Cx) {
    let board = cx.board;
    let others = replacements(e, slot, cx);
    let mut pick: Option<BExpr> = None;
    let mut menu = true;
    match e {
        _ if slot == Slot::Pin && is_pin(e, board) => {
            let choices = pin_choices(board);
            let shown = choices.iter().find(|(_, x)| x == e).map(|c| c.0.clone()).unwrap_or_default();
            let cur = e.clone();
            combo(ui, cx, shown, |ui| list_with_others(ui, choices, &cur, &mut pick, &others));
            menu = false;
        }
        BExpr::Name(n) => {
            let mut names: Vec<String> = cx.vars.clone();
            names.extend(["HIGH", "LOW", "true", "false"].map(String::from));
            if !names.contains(n) {
                names.insert(0, n.clone());
            }
            let known = cx.vars.contains(n) || ["HIGH", "LOW", "true", "false"].contains(&n.as_str()) || is_pin(&BExpr::Name(n.clone()), board);
            let shown = if known { egui::RichText::new(name_label(n)) } else { egui::RichText::new(name_label(n)).color(crate::theme::error()) };
            let choices: Vec<(String, BExpr)> = names.into_iter().map(|x| (name_label(&x), BExpr::Name(x))).collect();
            let cur = e.clone();
            combo(ui, cx, shown, |ui| list_with_others(ui, choices, &cur, &mut pick, &others));
            menu = false;
        }
        BExpr::Int(n) => {
            ui.add(egui::DragValue::new(n).speed(if slot == Slot::Num { 1.0 } else { 0.1 }));
        }
        BExpr::Float(x) => {
            ui.add(egui::DragValue::new(x).speed(0.01).max_decimals(4));
        }
        BExpr::Text(t) => {
            let font = egui::TextStyle::Body.resolve(ui.style());
            let w = (ui.painter().layout_no_wrap(t.clone(), font, Color32::WHITE).size().x + 16.0).clamp(40.0, 220.0);
            ui.add(egui::TextEdit::singleline(t).desired_width(w)).on_hover_text(i18n::tr("送る文字"));
        }
        BExpr::DigitalRead(p) => chip(ui, cx, READ, &mut |ui, cx| {
            word(ui, "ピン");
            expr_ui(ui, p, Slot::Pin, cx);
            word(ui, "の入力");
        }),
        BExpr::AnalogRead(p) => chip(ui, cx, READ, &mut |ui, cx| {
            word(ui, "アナログ");
            expr_ui(ui, p, Slot::Pin, cx);
            word(ui, "の読み (0〜1023)");
        }),
        BExpr::Millis => chip(ui, cx, TIME, &mut |ui, _| word(ui, "動き始めてからのミリ秒")),
        BExpr::Random(a, b) => chip(ui, cx, MATH, &mut |ui, cx| {
            word(ui, "乱数");
            expr_ui(ui, a, Slot::Num, cx);
            word(ui, "〜");
            expr_ui(ui, b, Slot::Num, cx);
            word(ui, "の手前");
        }),
        BExpr::Map(a) => chip(ui, cx, MATH, &mut |ui, cx| {
            let [v, a0, a1, b0, b1] = &mut **a;
            expr_ui(ui, v, Slot::Num, cx);
            word(ui, "を");
            expr_ui(ui, a0, Slot::Num, cx);
            word(ui, "〜");
            expr_ui(ui, a1, Slot::Num, cx);
            word(ui, "から");
            expr_ui(ui, b0, Slot::Num, cx);
            word(ui, "〜");
            expr_ui(ui, b1, Slot::Num, cx);
            word(ui, "に");
        }),
        BExpr::Bin(op, a, b) => {
            let logic = op == "&&" || op == "||";
            let compare = ["==", "!=", "<", "<=", ">", ">="].contains(&op.as_str());
            let inner = if logic { Slot::Cond } else { Slot::Num };
            let color = if logic || compare { CONTROL } else { MATH };
            chip(ui, cx, color, &mut |ui, cx| {
                expr_ui(ui, a, inner, cx);
                let group: &[&str] = if logic { &["&&", "||"] } else if compare { &["==", "!=", "<", "<=", ">", ">="] } else { &["+", "-", "*", "/", "%"] };
                combo_w(ui, cx, op_label(op), Some(if logic { 64.0 } else { 36.0 }), |ui| {
                    for o in group {
                        ui.selectable_value(op, o.to_string(), op_label(o));
                    }
                });
                expr_ui(ui, b, inner, cx);
            });
        }
        BExpr::Not(a) => chip(ui, cx, CONTROL, &mut |ui, cx| {
            expr_ui(ui, a, Slot::Cond, cx);
            word(ui, "でない");
        }),
    }
    if menu {
        ui.menu_button(egui::RichText::new("▼").small().color(Color32::WHITE), |ui| others_ui(ui, &others, &mut pick))
            .response
            .on_hover_text(i18n::tr("ここに入れる値を替えます (数・変数・ピンの読み・計算・比べる)"));
    }
    if let Some(mut x) = pick {
        // a number in a pin's slot: the board's LED pin rather than D0
        if slot == Slot::Pin && matches!(x, BExpr::Int(0)) {
            x = BExpr::Int(board.led() as i64);
        }
        *e = x;
    }
}

fn op_label(op: &str) -> String {
    match op {
        "+" => "+".into(),
        "-" => "−".into(),
        "*" => "×".into(),
        "/" => "÷".into(),
        "%" => i18n::tr("の余り").to_string(),
        "==" => "=".into(),
        "!=" => "≠".into(),
        "<=" => "≦".into(),
        ">=" => "≧".into(),
        "&&" => i18n::tr("かつ").to_string(),
        "||" => i18n::tr("または").to_string(),
        o => o.into(),
    }
}

/// What else may go in a slot: (name, what it is, the value), None for a line between groups. A calculation, a comparison, "not"
/// and "map" take the value there as their first part; a value with parts may be taken off, keeping its first part.
fn replacements(e: &BExpr, slot: Slot, cx: &Cx) -> Vec<Option<(&'static str, &'static str, BExpr)>> {
    let b = || Box::new(e.clone());
    let mut items = vec![];
    if slot == Slot::Pin && !is_pin(e, cx.board) {
        items.push(Some(("ピン", "ボードのピンを選びます", BExpr::Name("LED_BUILTIN".into()))));
    }
    items.push(Some(("数", "整数 (ドラッグか、クリックして打ちます)", BExpr::Int(0))));
    items.push(Some(("小数", "0.5 のような小数", BExpr::Float(0.5))));
    if slot == Slot::Print {
        items.push(Some(("文字", "そのまま送る文字", BExpr::Text(String::new()))));
    }
    if slot != Slot::Pin && !matches!(e, BExpr::Name(_)) {
        items.push(Some(("HIGH / LOW・変数", "HIGH (5 V、成り立つ)・LOW (0 V)・変数を選びます", BExpr::Name(cx.vars.first().cloned().unwrap_or("HIGH".into())))));
    }
    if slot == Slot::Pin && !cx.vars.is_empty() {
        items.push(Some(("変数", "変数に入れたピンの番号", BExpr::Name(cx.vars[0].clone()))));
    }
    if slot != Slot::Pin {
        items.push(None);
        items.push(Some(("デジタル入力を読む", "ピンが HIGH (1) か LOW (0) かを読みます (digitalRead)", BExpr::DigitalRead(Box::new(BExpr::Int(2))))));
        items.push(Some(("アナログを読む", "ピンの電圧を 0〜1023 で読みます (analogRead。5 V で 1023)", BExpr::AnalogRead(Box::new(BExpr::Name("A0".into()))))));
        items.push(Some(("動き始めてからの時間", "動き始めてからのミリ秒 (millis)", BExpr::Millis)));
        items.push(Some(("乱数", "決めた範囲の数をでたらめに 1 つ (random)", BExpr::Random(Box::new(BExpr::Int(0)), Box::new(BExpr::Int(10))))));
    }
    items.push(None);
    items.push(Some(("計算 (+ − × ÷)", "いまの値を左にして計算します", BExpr::Bin("+".into(), b(), Box::new(BExpr::Int(1))))));
    if slot != Slot::Pin {
        items.push(Some(("比べる (= < >)", "いまの値を左にして比べます (成り立つと 1)", BExpr::Bin("==".into(), b(), Box::new(BExpr::Name("HIGH".into()))))));
        items.push(Some(("かつ・または", "2 つの条件を合わせます", BExpr::Bin("&&".into(), b(), Box::new(BExpr::Name("true".into()))))));
        items.push(Some(("〜でない", "条件を逆にします", BExpr::Not(b()))));
        items.push(Some(("範囲を変える (map)", "0〜1023 の読みを 0〜255 にするように、値の範囲を変えます", BExpr::Map(Box::new([e.clone(), BExpr::Int(0), BExpr::Int(1023), BExpr::Int(0), BExpr::Int(255)])))));
    }
    let first = match e {
        BExpr::DigitalRead(a) | BExpr::AnalogRead(a) | BExpr::Not(a) | BExpr::Bin(_, a, _) | BExpr::Random(a, _) => Some((**a).clone()),
        BExpr::Map(a) => Some(a[0].clone()),
        _ => None,
    };
    if let Some(f) = first {
        items.push(None);
        items.push(Some(("外す (中の最初の値を残す)", "この読み・計算を外して、中の最初の値だけを残します", f)));
    }
    items
}

fn others_ui(ui: &mut egui::Ui, items: &[Option<(&str, &str, BExpr)>], pick: &mut Option<BExpr>) {
    for it in items {
        match it {
            None => {
                ui.separator();
            }
            Some((label, hint, x)) => {
                if ui.selectable_label(false, i18n::tr_dyn(label)).on_hover_text(i18n::tr_dyn(hint)).clicked() {
                    *pick = Some(x.clone());
                    ui.close();
                }
            }
        }
    }
}

/// A free name for a new repeat's counter: not a variable's, not a repeat's around it.
fn free_counter(s: &BlockSketch, to: &ListPath) -> String {
    let mut used: Vec<String> = s.vars.iter().map(|v| v.name.clone()).collect();
    let mut list = if to.section == 0 { &s.setup } else { &s.looped };
    for &(i, which) in &to.nest {
        let Some(b) = list.get(i) else { break };
        if let Block::Repeat { counter, .. } = b {
            used.push(counter.clone());
        }
        list = match (b, which) {
            (Block::If { then, .. }, 0) => then,
            (Block::If { els: Some(e), .. }, 1) => e,
            (Block::Repeat { body, .. } | Block::While { body, .. }, 0) => body,
            _ => break,
        };
    }
    ["i", "j", "k", "m", "n", "p", "q"].into_iter().map(String::from).find(|c| !used.contains(c)).unwrap_or_else(|| format!("i{}", to.nest.len()))
}

fn list_mut<'a>(s: &'a mut BlockSketch, p: &ListPath) -> Option<&'a mut Vec<Block>> {
    let mut v = if p.section == 0 { &mut s.setup } else { &mut s.looped };
    for &(i, which) in &p.nest {
        v = match (v.get_mut(i)?, which) {
            (Block::If { then, .. }, 0) => then,
            (Block::If { els: Some(e), .. }, 1) => e,
            (Block::Repeat { body, .. } | Block::While { body, .. }, 0) => body,
            _ => return None,
        };
    }
    Some(v)
}

fn apply(s: &mut BlockSketch, act: Act) {
    match act {
        Act::Drop(Drag::New(mut b), to, at) => {
            if let Block::Repeat { counter, .. } = &mut b {
                *counter = free_counter(s, &to);
            }
            if let Some(v) = list_mut(s, &to) {
                let at = at.min(v.len());
                v.insert(at, b);
            }
        }
        Act::Drop(Drag::Move(from, i), mut to, mut at) => {
            // into a list inside the block itself: no; a list after it in the same list moves up one
            let k = from.nest.len();
            if from.section == to.section && to.nest.len() > k && to.nest[..k] == from.nest[..] {
                if to.nest[k].0 == i {
                    return;
                }
                if to.nest[k].0 > i {
                    to.nest[k].0 -= 1;
                }
            }
            if from == to && at > i {
                at -= 1;
            }
            let Some(v) = list_mut(s, &from) else { return };
            if i >= v.len() {
                return;
            }
            let b = v.remove(i);
            match list_mut(s, &to) {
                Some(v) => {
                    let at = at.min(v.len());
                    v.insert(at, b);
                }
                None => {
                    if let Some(v) = list_mut(s, &from) {
                        v.insert(i.min(v.len()), b);
                    }
                }
            }
        }
        Act::Remove(p, i) => {
            if let Some(v) = list_mut(s, &p)
                && i < v.len()
            {
                v.remove(i);
            }
        }
        Act::Duplicate(p, i) => {
            if let Some(v) = list_mut(s, &p)
                && i < v.len()
            {
                let b = v[i].clone();
                v.insert(i + 1, b);
            }
        }
        Act::Rename(from, to) => bk::rename(s, &from, &to),
        Act::RemoveVar(k) => {
            if k < s.vars.len() {
                s.vars.remove(k);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sketch() -> BlockSketch {
        BlockSketch {
            vars: vec![],
            setup: vec![Block::Delay(BExpr::Int(1))],
            looped: vec![
                Block::If { cond: BExpr::Name("true".into()), then: vec![Block::Delay(BExpr::Int(2))], els: None },
                Block::Delay(BExpr::Int(3)),
                Block::Repeat { counter: "i".into(), times: BExpr::Int(2), body: vec![] },
            ],
        }
    }
    fn top(section: u8) -> ListPath {
        ListPath { section, nest: vec![] }
    }

    #[test]
    fn a_block_moves_into_and_out_of_another() {
        let mut s = sketch();
        // the delay(3) into the if, after its delay(2)
        apply(&mut s, Act::Drop(Drag::Move(top(1), 1), top(1).child(0, 0), 1));
        let Block::If { then, .. } = &s.looped[0] else { panic!() };
        assert_eq!(then, &vec![Block::Delay(BExpr::Int(2)), Block::Delay(BExpr::Int(3))]);
        assert_eq!(s.looped.len(), 2);
        // the if (with them) into the repeat after it: the repeat's place moves up as the if leaves
        apply(&mut s, Act::Drop(Drag::Move(top(1), 0), top(1).child(1, 0), 0));
        assert_eq!(s.looped.len(), 1);
        let Block::Repeat { body, .. } = &s.looped[0] else { panic!() };
        assert!(matches!(body[0], Block::If { .. }));
        // a block into itself is not moved
        let before = s.clone();
        apply(&mut s, Act::Drop(Drag::Move(top(1), 0), top(1).child(0, 0), 0));
        assert_eq!(s, before);
        // out to the start, and down within one list
        apply(&mut s, Act::Drop(Drag::Move(top(1).child(0, 0), 0), top(0), 0));
        assert!(matches!(s.setup[0], Block::If { .. }));
        apply(&mut s, Act::Drop(Drag::Move(top(0), 0), top(0), 2));
        assert_eq!(s.setup[0], Block::Delay(BExpr::Int(1)));
    }

    /// The blocks drawn in a window of 400 × 900 with `events`; the blocks after the frame and where each text was drawn.
    fn frame(ctx: &egui::Context, s: &mut BlockSketch, events: Vec<egui::Event>) -> Vec<(String, egui::Pos2)> {
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 900.0));
        let mut out = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), events, ..Default::default() }, |ui| {
            edit_blocks(ui, s, Board::Nano, "t", &[], None);
        });
        out.textures_delta.clear();
        out.shapes
            .iter()
            .filter_map(|c| match &c.shape {
                egui::Shape::Text(t) => Some((t.galley.text().to_string(), t.pos + t.galley.rect.center().to_vec2())),
                _ => None,
            })
            .collect()
    }

    /// Drags from the text `from` to the text `to` (both as drawn now) with the mouse, frame by frame.
    fn drag(ctx: &egui::Context, s: &mut BlockSketch, from: &str, to: &str) {
        let texts = frame(ctx, s, vec![]);
        let at = |t: &str| texts.iter().find(|(x, _)| x == t).unwrap_or_else(|| panic!("no {t} in {texts:?}")).1;
        let (a, b) = (at(from), at(to));
        let button = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        frame(ctx, s, vec![egui::Event::PointerMoved(a)]);
        frame(ctx, s, vec![button(a, true)]);
        for k in 1..=6 {
            frame(ctx, s, vec![egui::Event::PointerMoved(a + (b - a) * (k as f32 / 6.0))]);
        }
        frame(ctx, s, vec![button(b, false)]);
        frame(ctx, s, vec![]);
    }

    #[test]
    fn blocks_are_dragged_in_from_the_list_and_back_out() {
        let ctx = egui::Context::default();
        let mut s = BlockSketch { vars: vec![], setup: vec![], looped: vec![Block::Delay(BExpr::Int(3))] };
        // from the list at the top into the empty start
        drag(&ctx, &mut s, "待つ", "(ここにブロックを置きます)");
        assert_eq!(s.setup, vec![Block::Delay(BExpr::Int(500))], "{s:?}");
        assert_eq!(s.looped.len(), 1);
        // a block dragged back onto the list is thrown away
        drag(&ctx, &mut s, "ピンの使い方", "ブロック");
        assert_eq!(s.setup.len(), 1, "the list's own block is not one of the sketch's: nothing goes");
        let texts = frame(&ctx, &mut s, vec![]);
        let handles: Vec<_> = texts.iter().filter(|(t, _)| t == "待つ:").map(|(_, p)| *p).collect();
        assert_eq!(handles.len(), 2, "{texts:?}");
        // the loop's delay (the lower one) onto the list
        let lower = handles.iter().copied().fold(egui::Pos2::ZERO, |m, p| if p.y > m.y { p } else { m });
        let list = texts.iter().find(|(t, _)| t == "ブロック").unwrap().1;
        let button = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        frame(&ctx, &mut s, vec![egui::Event::PointerMoved(lower)]);
        frame(&ctx, &mut s, vec![button(lower, true)]);
        for k in 1..=6 {
            frame(&ctx, &mut s, vec![egui::Event::PointerMoved(lower + (list - lower) * (k as f32 / 6.0))]);
        }
        frame(&ctx, &mut s, vec![button(list, false)]);
        frame(&ctx, &mut s, vec![]);
        assert!(s.looped.is_empty(), "{s:?}");
        assert_eq!(s.setup.len(), 1);
    }

    /// A block is taken by its ground too (a word on it, not only its name); while it is dragged, the place it would go is left
    /// open (here the empty start's "put a block here" makes way for it).
    #[test]
    fn a_block_is_grabbed_by_its_ground_and_its_place_opens() {
        let ctx = egui::Context::default();
        let mut s = BlockSketch { vars: vec![], setup: vec![], looped: vec![Block::Delay(BExpr::Int(3)), Block::Delay(BExpr::Int(4))] };
        let texts = frame(&ctx, &mut s, vec![]);
        let at = |t: &str| texts.iter().filter(|(x, _)| x == t).map(|(_, p)| *p).fold(None, |m: Option<egui::Pos2>, p| Some(m.map_or(p, |m| if p.y < m.y { p } else { m }))).unwrap();
        let (from, to) = (at("ミリ秒"), at("(ここにブロックを置きます)"));
        let button = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        frame(&ctx, &mut s, vec![egui::Event::PointerMoved(from)]);
        frame(&ctx, &mut s, vec![button(from, true)]);
        for k in 1..=6 {
            frame(&ctx, &mut s, vec![egui::Event::PointerMoved(from + (to - from) * (k as f32 / 6.0))]);
        }
        let shown = frame(&ctx, &mut s, vec![]);
        assert!(!shown.iter().any(|(t, _)| t == "(ここにブロックを置きます)"), "the place is open while it is dragged over it");
        frame(&ctx, &mut s, vec![button(to, false)]);
        frame(&ctx, &mut s, vec![]);
        assert_eq!(s.setup, vec![Block::Delay(BExpr::Int(3))], "{s:?}");
        assert_eq!(s.looped, vec![Block::Delay(BExpr::Int(4))]);
    }

    #[test]
    fn a_new_repeat_inside_another_counts_with_another_name() {
        let mut s = sketch();
        apply(&mut s, Act::Drop(Drag::New(Block::Repeat { counter: "x".into(), times: BExpr::Int(3), body: vec![] }), top(1).child(2, 0), 0));
        let Block::Repeat { body, .. } = &s.looped[2] else { panic!() };
        assert!(matches!(&body[0], Block::Repeat { counter, .. } if counter == "j"));
        let code = bk::to_code(&s);
        sim_core::sketch::compile(&code, Board::Nano).unwrap();
    }
}
