//! The data sheet window: a PDF opened beside the part form, to read while typing the numbers in.

use datasheet::{Datasheet, HEADINGS, Hit, Quantity, Query, Rect, looks_like_part_number};
use super::pin_import::{Action, PinImport, TablePanel};
use eframe::egui::{self, Vec2};
use std::path::Path;
use std::time::{Duration, Instant};

/// Time spent searching in one frame.
const SEARCH_BUDGET: Duration = Duration::from_millis(8);
/// More hits than this are not listed (a word like "the" would fill the list and the memory for nothing).
const MAX_HITS: usize = 1000;

/// Longest side of the picture handed to the GPU (pixels); a bigger texture is refused by some graphics cards.
const MAX_TEXTURE_PX: f32 = 4096.0;
const ZOOM_MIN: f32 = 0.3;
const ZOOM_MAX: f32 = 4.0;

/// What a click on a word of the page fills in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Name,
    Pitch,
    Row,
    PadLen,
    PadWidth,
    Span,
    PadDia,
    Drill,
    BodyW,
    BodyH,
    Height,
    MaxV,
    MaxI,
    MaxP,
    ThermalW,
    ThermalH,
}

impl Target {
    pub const ALL: [Target; 16] = [
        Target::Name,
        Target::Pitch,
        Target::Row,
        Target::PadLen,
        Target::PadWidth,
        Target::Span,
        Target::PadDia,
        Target::Drill,
        Target::BodyW,
        Target::BodyH,
        Target::Height,
        Target::MaxV,
        Target::MaxI,
        Target::MaxP,
        Target::ThermalW,
        Target::ThermalH,
    ];

    pub fn from_label(label: &str) -> Option<Target> {
        Target::ALL.into_iter().find(|t| t.label() == label)
    }

    pub fn label(self) -> &'static str {
        match self {
            Target::Name => "部品名",
            Target::Pitch => "ピッチ",
            Target::Row => "列間隔",
            Target::PadLen => "パッド長",
            Target::PadWidth => "パッド幅",
            Target::Span => "対向パッド間隔",
            Target::PadDia => "パッド径",
            Target::Drill => "穴径",
            Target::BodyW => "本体の幅",
            Target::BodyH => "本体の長さ",
            Target::Height => "高さ",
            Target::MaxV => "最大電圧",
            Target::MaxI => "最大電流",
            Target::MaxP => "最大電力",
            Target::ThermalW => "放熱パッドの幅",
            Target::ThermalH => "放熱パッドの長さ",
        }
    }

    /// The kind of number the field takes (None: a name).
    pub fn quantity(self) -> Option<Quantity> {
        match self {
            Target::Name => None,
            Target::MaxV => Some(Quantity::Voltage),
            Target::MaxI => Some(Quantity::Current),
            Target::MaxP => Some(Quantity::Power),
            _ => Some(Quantity::Length),
        }
    }

    fn ask(self) -> String {
        if self.quantity().is_some() { format!("「{}」の数字を、下のページでクリックしてください", self.label()) } else { format!("「{}」にする語を、下のページでクリックしてください", self.label()) }
    }
}

/// A word clicked in the page, for the form to take.
pub struct Picked {
    pub target: Target,
    pub text: String,
    /// The unit written right after it on the page ("mm" after "0.65"), if any.
    pub unit_after: Option<String>,
    /// The page (counted from 0) it was on.
    pub page: usize,
}

/// The numbers of the pages that have a land pattern or package drawing.
pub struct DimensionPages {
    pub pages: Vec<usize>,
    pub lengths: Vec<(usize, Vec<datasheet::PageLength>)>,
    /// No page had such a heading, so the page that was shown was used.
    pub fallback: bool,
}

/// What the first pages suggest, for a person to pick from.
pub struct Candidates {
    pub names: Vec<String>,
    pub category: Option<&'static str>,
}

#[derive(Default)]
pub struct SheetView {
    open: bool,
    /// The pins in the form now, as (number, name), set by the form every frame (the pin table window compares them).
    current_pins: Vec<(String, String)>,
    doc: Option<Opened>,
    /// Why the last file could not be opened.
    msg: String,
}

struct Opened {
    ds: Datasheet,
    name: String,
    candidates: Candidates,
    page: usize,
    /// Screen points per PDF point.
    zoom: f32,
    /// Set by "fit the width": applied at the start of the next frame, when the window's width is known.
    fit: bool,
    texture: Option<Drawn>,
    /// The text in the search box.
    query: String,
    search: Option<Search>,
    /// A place to bring into view at the next frame (a hit was picked).
    jump: Option<Rect>,
    /// Size of the page area at the last frame, to centre a hit in it.
    viewport: Vec2,
    /// A field of the form waits for a word to be clicked.
    target: Option<Target>,
    /// The word that was clicked, for the form to take.
    picked: Option<Picked>,
    /// The pin table is being cut out: drag a rectangle over it.
    selecting: bool,
    drag_from: Option<egui::Pos2>,
    /// The pins read from the rectangle, being checked.
    panel: Option<TablePanel>,
    /// The pins the person accepted, for the form to take.
    pins_out: Option<PinImport>,
}

/// A search that goes on a few pages per frame, so that a long data sheet does not freeze the window.
struct Search {
    /// Any of these (the words of a heading in English and in Japanese).
    queries: Vec<Query>,
    /// The name of the heading searched, shown instead of the box text.
    title: Option<&'static str>,
    next_page: usize,
    hits: Vec<Hit>,
    /// The hit shown as picked.
    current: Option<usize>,
    /// Letters were found on at least one page (false: the PDF may be all pictures).
    any_text: bool,
}

impl Search {
    fn new(queries: Vec<Query>, title: Option<&'static str>) -> Option<Search> {
        if queries.is_empty() { None } else { Some(Search { queries, title, next_page: 0, hits: vec![], current: None, any_text: false }) }
    }
}

/// The texture of the page on screen, and what it was drawn for.
struct Drawn {
    handle: egui::TextureHandle,
    page: usize,
    key: u32,
}

impl SheetView {
    /// Opens a PDF file in the window.
    pub fn load(&mut self, path: &Path) {
        match Datasheet::open_file(path) {
            Ok(ds) => {
                let name = path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned());
                let candidates = candidates_of(&ds, path);
                self.doc = Some(Opened {
                    ds,
                    name,
                    candidates,
                    page: 0,
                    zoom: 1.0,
                    fit: true,
                    texture: None,
                    query: String::new(),
                    search: None,
                    jump: None,
                    viewport: Vec2::ZERO,
                    target: None,
                    picked: None,
                    selecting: false,
                    drag_from: None,
                    panel: None,
                    pins_out: None,
                });
                self.msg.clear();
                self.open = true;
            }
            Err(e) => {
                self.msg = format!("{}: {e}", path.display());
            }
        }
    }

    pub fn has_doc(&self) -> bool {
        self.doc.is_some()
    }

    /// What the first pages suggest (part numbers and the kind of part), when a PDF is open.
    pub fn candidates(&self) -> Option<&Candidates> {
        self.doc.as_ref().map(|d| &d.candidates)
    }

    /// The form asks for a word to be clicked in the PDF: the window comes forward and waits.
    pub fn start_pick(&mut self, target: Target) {
        if let Some(d) = self.doc.as_mut() {
            d.target = Some(target);
            d.picked = None;
            self.open = true;
        }
    }

    /// The form asks for the pin table to be cut out: the window comes forward and waits for a rectangle.
    pub fn start_pin_select(&mut self) {
        if let Some(d) = self.doc.as_mut() {
            d.selecting = true;
            d.target = None;
            d.panel = None;
            self.open = true;
        }
    }

    /// The form tells the window which pins it has now.
    pub fn set_current_pins(&mut self, pins: Vec<(String, String)>) {
        self.current_pins = pins;
    }

    /// A table cut out of a page again (the same rectangle as when it was accepted).
    pub fn table_at(&self, page: usize, area: Rect) -> Option<datasheet::Table> {
        self.doc.as_ref().map(|d| d.ds.table_in(page, area))
    }

    /// The numbers on the pages that have a land pattern or package drawing, for checking the dimensions typed in:
    pub fn dimension_lengths(&self) -> Option<DimensionPages> {
        let d = self.doc.as_ref()?;
        let mut pages = d.ds.dimension_pages();
        let fallback = pages.is_empty();
        if fallback {
            pages = vec![d.page];
        }
        let lengths = pages.iter().map(|&p| (p, d.ds.lengths_on(p))).collect();
        Some(DimensionPages { pages, lengths, fallback })
    }

    /// Shows a page (a result of a check is looked at).
    pub fn go_to_page(&mut self, page: usize) {
        if let Some(d) = self.doc.as_mut() {
            d.page = page.min(d.ds.page_count() - 1);
            self.open = true;
        }
    }

    /// The pins the person accepted since the last call.
    pub fn take_pins(&mut self) -> Option<PinImport> {
        self.doc.as_mut().and_then(|d| d.pins_out.take())
    }

    /// The word that was clicked since the last call, and what it was for.
    pub fn take_picked(&mut self) -> Option<Picked> {
        self.doc.as_mut().and_then(|d| d.picked.take())
    }

    /// Starts a search as if the text had been typed and "探す" pressed (`--form-find`, for screenshots).
    pub fn find(&mut self, text: &str) {
        if let Some(d) = self.doc.as_mut() {
            // the name of a heading ("ピン配置") searches that heading, like its button
            if let Some(h) = HEADINGS.iter().find(|h| h.label() == text) {
                d.search = Search::new(h.queries(), Some(h.label()));
                return;
            }
            d.query = text.to_string();
            d.search = Search::new(Query::new(text).into_iter().collect(), None);
        }
    }

    /// The button of the form that opens a PDF, and the message when that failed.
    pub fn open_button(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button(i18n::tr("データシート (PDF) を開く…")).on_hover_text(i18n::tr("手元のデータシート (PDF) を、別の窓に出します。日本語でも英語でも開けます。見ながらピンや寸法を入れられます。PDF の窓に、PDF ファイルをドラッグしても開けます。ファイルはこのパソコンの中だけで読み、どこにも送りません")).clicked()
                && let Some(p) = rfd::FileDialog::new().set_title("データシート (PDF)").add_filter("PDF", &["pdf", "PDF"]).pick_file()
            {
                self.load(&p);
            }
            if self.doc.is_some() && !self.open && ui.button(i18n::tr("窓を出す")).on_hover_text(i18n::tr("閉じたデータシートの窓をもう一度出します")).clicked() {
                self.open = true;
            }
        });
        if !self.msg.is_empty() {
            ui.colored_label(crate::theme::ERROR, &self.msg);
        }
    }

    /// Opens a PDF dropped on the app. Returns true when a file was taken.
    pub fn take_dropped(&mut self, ctx: &egui::Context) -> bool {
        let path = ctx.input(|i| i.raw.dropped_files.iter().find_map(|f| Some(f.path().to_path_buf()).filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf")))));
        match path {
            Some(p) => {
                self.load(&p);
                true
            }
            None => false,
        }
    }

    /// Draws the window (nothing when closed).
    pub fn window(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let mut open = true;
        let Some(doc) = self.doc.as_mut() else { return };
        // beside the form, which is opened at the left of the screen: the window starts at the right edge
        let at = egui::pos2((ctx.content_rect().right() - 660.0).max(0.0), 40.0);
        egui::Window::new(i18n::tr("データシート")).open(&mut open).default_pos(at).default_size([640.0, 760.0]).resizable(true).show(ctx, |ui| {
            doc.ui(ui);
        });
        self.open = open;
        if !open && let Some(d) = self.doc.as_mut() {
            d.target = None;
            d.selecting = false;
        }
        // the window that checks the pins read from the rectangle
        let current = &self.current_pins;
        if let Some(d) = self.doc.as_mut()
            && let Some(panel) = d.panel.as_mut()
        {
            let mut keep = true;
            let mut action = Action::None;
            egui::Window::new(i18n::tr("ピン表の取り込み")).open(&mut keep).default_size([440.0, 460.0]).resizable(true).show(ctx, |ui| {
                action = panel.ui(ui, current);
            });
            match action {
                Action::Apply(mode) => {
                    let (num_col, name_col) = panel.columns();
                    d.pins_out = Some(PinImport { lines: panel.text.clone(), mode, page: panel.page, area: panel.area, num_col, name_col });
                    d.panel = None;
                }
                Action::Cancel => d.panel = None,
                Action::None if !keep => d.panel = None,
                Action::None => {}
            }
        }
    }
}

/// Part numbers (from the title and from the file name) and the kind of part.
fn candidates_of(ds: &Datasheet, path: &Path) -> Candidates {
    let mut names = ds.name_candidates();
    let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    for t in stem.split(['_', ' ', '(', ')', ',']).map(|t| t.trim_matches(|c: char| !c.is_ascii_alphanumeric())) {
        if looks_like_part_number(t) && !names.iter().any(|n| n.eq_ignore_ascii_case(t)) {
            names.push(t.to_string());
        }
    }
    Candidates { names, category: ds.category_hint() }
}

impl Opened {
    fn ui(&mut self, ui: &mut egui::Ui) {
        let pages = self.ds.page_count();
        let (pw, _) = self.ds.page_size(self.page).unwrap_or((595.0, 842.0));
        if std::mem::take(&mut self.fit) {
            self.zoom = ((ui.available_width() - 24.0) / pw).clamp(ZOOM_MIN, ZOOM_MAX);
        }
        if (self.target.is_some() || self.selecting) && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.target = None;
            self.selecting = false;
        }
        ui.horizontal_wrapped(|ui| {
            if ui.add_enabled(self.page > 0, egui::Button::new("◀")).on_hover_text(i18n::tr("前のページ")).clicked() {
                self.page -= 1;
            }
            let mut shown = self.page + 1;
            if ui.add(egui::DragValue::new(&mut shown).range(1..=pages).suffix(i18n::trf!(" / {pages} ページ"))).on_hover_text(i18n::tr("ページ番号。ドラッグか、数字を入れて切り替えます")).changed() {
                self.page = shown - 1;
            }
            if ui.add_enabled(self.page + 1 < pages, egui::Button::new("▶")).on_hover_text(i18n::tr("次のページ")).clicked() {
                self.page += 1;
            }
            ui.separator();
            if ui.button(i18n::tr("－")).on_hover_text(i18n::tr("小さくする (Ctrl + マウスホイールでも変えられます)")).clicked() {
                self.zoom = (self.zoom / 1.25).max(ZOOM_MIN);
            }
            ui.label(format!("{:.0}%", self.zoom * 100.0));
            if ui.button(i18n::tr("＋")).on_hover_text(i18n::tr("大きくする (Ctrl + マウスホイールでも変えられます)")).clicked() {
                self.zoom = (self.zoom * 1.25).min(ZOOM_MAX);
            }
            if ui.button(i18n::tr("幅に合わせる")).on_hover_text(i18n::tr("窓の幅にページの幅を合わせます")).clicked() {
                self.fit = true;
            }
            ui.small(&self.name);
        });
        if let Some(t) = self.target {
            ui.horizontal(|ui| {
                ui.colored_label(crate::theme::INFO, t.ask());
                if ui.button(i18n::tr("やめる")).on_hover_text(i18n::tr("選ぶのをやめます (Esc キーでも)")).clicked() {
                    self.target = None;
                }
            });
        }
        if self.selecting {
            ui.horizontal(|ui| {
                ui.colored_label(crate::theme::INFO, i18n::tr("ピン表を、下のページでドラッグして囲んでください (番号と名前の列が入るように)"));
                if ui.button(i18n::tr("やめる")).on_hover_text(i18n::tr("囲むのをやめます (Esc キーでも)")).clicked() {
                    self.selecting = false;
                }
            });
        }
        self.search_ui(ui);
        ui.separator();
        let ctx = ui.ctx().clone();
        let mut scroll = egui::ScrollArea::both().auto_shrink([false, false]);
        if let Some(r) = self.jump.take() {
            let centre = Vec2::new(r.x0 + r.x1, r.y0 + r.y1) * 0.5 * self.zoom;
            scroll = scroll.scroll_offset((centre - self.viewport * 0.5).max(Vec2::ZERO));
        }
        let area = scroll.show(ui, |ui| self.page_image(ui, &ctx));
        self.viewport = area.inner_rect.size();
        // Ctrl + wheel (or a pinch) zooms, only while the pointer is over the page
        if area.inner_rect.contains(ctx.input(|i| i.pointer.hover_pos()).unwrap_or_default()) {
            let z = ctx.input(|i| i.zoom_delta());
            if z != 1.0 {
                self.zoom = (self.zoom * z).clamp(ZOOM_MIN, ZOOM_MAX);
            }
        }
    }

    fn page_image(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let Some((w, h)) = self.ds.page_size(self.page) else { return };
        let size = Vec2::new(w, h) * self.zoom;
        let ppp = ctx.pixels_per_point();
        let key = ((self.zoom * ppp) * 100.0).round() as u32;
        if self.texture.as_ref().is_none_or(|t| t.page != self.page || t.key != key) {
            let scale = (self.zoom * ppp).min(MAX_TEXTURE_PX / w.max(h));
            match self.ds.render(self.page, scale) {
                Ok(img) => {
                    let color = egui::ColorImage::from_rgba_unmultiplied([img.width, img.height], &img.rgba);
                    let handle = ctx.load_texture("datasheet_page", color, egui::TextureOptions::LINEAR);
                    self.texture = Some(Drawn { handle, page: self.page, key });
                }
                Err(e) => {
                    self.texture = None;
                    ui.colored_label(crate::theme::ERROR, e);
                    return;
                }
            }
        }
        let Some(tex_id) = self.texture.as_ref().map(|t| t.handle.id()) else { return };
        let sense = if self.selecting { egui::Sense::click_and_drag() } else { egui::Sense::click() };
        let resp = ui.add(egui::Image::new((tex_id, size)).sense(sense));
        let origin = resp.rect.min;
        let zoom = self.zoom;
        let on_screen = |r: &Rect| egui::Rect::from_min_max(origin + Vec2::new(r.x0 - 1.0, r.y0) * zoom, origin + Vec2::new(r.x1 + 1.0, r.y1) * zoom);
        if let Some(s) = &self.search {
            for (i, h) in s.hits.iter().enumerate().filter(|(_, h)| h.page == self.page) {
                let r = on_screen(&h.rect);
                if s.current == Some(i) {
                    ui.painter().rect_filled(r, 2.0, egui::Color32::from_rgba_unmultiplied(255, 140, 0, 110));
                    ui.painter().rect_stroke(r.expand(1.0), 2.0, egui::Stroke::new(1.5, egui::Color32::from_rgb(230, 90, 0)), egui::StrokeKind::Outside);
                } else {
                    ui.painter().rect_filled(r, 2.0, egui::Color32::from_rgba_unmultiplied(255, 230, 0, 100));
                }
            }
        }
        // cutting out the pin table: a rectangle is dragged over it
        if self.selecting {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            if resp.drag_started() {
                // where the button went down, not where the pointer is when egui notices the drag
                self.drag_from = ui.input(|i| i.pointer.press_origin());
            }
            if let (Some(from), Some(to)) = (self.drag_from, resp.interact_pointer_pos()) {
                let r = egui::Rect::from_two_pos(from, to);
                ui.painter().rect_filled(r, 0.0, egui::Color32::from_rgba_unmultiplied(70, 150, 255, 50));
                ui.painter().rect_stroke(r, 0.0, egui::Stroke::new(1.5, crate::theme::INFO), egui::StrokeKind::Inside);
                if resp.drag_stopped() {
                    let (a, b) = ((r.min - origin) / zoom, (r.max - origin) / zoom);
                    // a click or a speck is not a table
                    if b.x - a.x > 20.0 && b.y - a.y > 10.0 {
                        let area = Rect { x0: a.x, y0: a.y, x1: b.x, y1: b.y };
                        self.panel = Some(TablePanel::new(self.page, area, self.ds.table_in(self.page, area)));
                        self.selecting = false;
                    }
                    self.drag_from = None;
                }
            }
            if resp.drag_stopped() {
                self.drag_from = None;
            }
        }
        // waiting for a word: the one under the pointer is framed, and a click takes it
        if let Some(target) = self.target
            && let Some(pos) = resp.hover_pos()
        {
            let p = (pos - origin) / zoom;
            let under = self.ds.words(self.page).iter().find(|w| w.rect.x0 - 1.0 <= p.x && p.x <= w.rect.x1 + 1.0 && w.rect.y0 <= p.y && p.y <= w.rect.y1);
            if let Some(word) = under
                && let Some(token) = word.token_at(p.x)
            {
                ui.painter().rect_stroke(on_screen(&word.rect).expand(1.0), 2.0, egui::Stroke::new(1.5, crate::theme::INFO), egui::StrokeKind::Outside);
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                resp.clone().on_hover_text(format!("→ {token}"));
                if resp.clicked() {
                    // a unit on the page after the number ("0.65" then "mm"), when the number ends its word
                    let unit_after = if word.text.ends_with(token) { self.ds.unit_after(self.page, word) } else { None };
                    self.picked = Some(Picked { target, text: token.to_string(), unit_after, page: self.page });
                    self.target = None;
                }
            } else {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
        }
    }

    /// The search box, the heading buttons, the hit counter and the list of hits.
    fn search_ui(&mut self, ui: &mut egui::Ui) {
        let pages = self.ds.page_count();
        let mut start = false;
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut self.query).hint_text(i18n::tr("検索 (例: SOIC、pin 1)")).desired_width(200.0));
            // Enter in a one-line box takes the focus away
            start |= r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            r.on_hover_text(i18n::tr("データシートの中の文字を探します。大文字・小文字、全角・半角は区別しません。空白で区切ると、続いて並ぶ語を探します (例: pin 1)。Enter で探します"));
            if ui.button(i18n::tr("探す")).on_hover_text(i18n::tr("入れた文字を、全ページから探します。見つかった所は黄色の枠で示します")).clicked() {
                start = true;
            }
            if self.search.is_some() && ui.button(i18n::tr("消す")).on_hover_text(i18n::tr("検索の結果と枠を消します")).clicked() {
                self.search = None;
            }
        });
        if start {
            self.search = Search::new(Query::new(&self.query).into_iter().collect(), None);
        }
        ui.horizontal_wrapped(|ui| {
            if ui.button(i18n::tr("ピン表を囲む")).on_hover_text(i18n::tr("ページの上でピン表をドラッグで囲むと、番号と名前を読み取ります。読み取った内容は確認して、ピン表に入れます")).clicked() {
                self.selecting = true;
                self.target = None;
            }
            ui.small(i18n::tr("見出しへ"));
            for h in HEADINGS {
                if ui.small_button(h.label()).on_hover_text(i18n::trf!("{} (英語・日本語の見出しを探します)", h.help())).clicked() {
                    self.search = Search::new(h.queries(), Some(h.label()));
                }
            }
        });
        // a few pages per frame
        let mut picked = None;
        if let Some(s) = self.search.as_mut() {
            let t = Instant::now();
            while s.next_page < pages && t.elapsed() < SEARCH_BUDGET && s.hits.len() < MAX_HITS {
                s.any_text |= !self.ds.words(s.next_page).is_empty();
                let found = self.ds.search_page_any(s.next_page, &s.queries);
                s.hits.extend(found);
                s.next_page += 1;
            }
            let working = s.next_page < pages && s.hits.len() < MAX_HITS;
            // go to the first hit that is not a row of a table of contents, or to the first one when all are
            if s.current.is_none() {
                picked = s.hits.iter().position(|h| !h.in_contents).or(if working || s.hits.is_empty() { None } else { Some(0) });
            }
            let mut note = if working {
                ui.ctx().request_repaint();
                format!("探しています… {}/{} ページ", s.next_page, pages)
            } else if s.hits.is_empty() && !s.any_text {
                "このPDFには検索できる文字がありません (画像のPDFかもしれません)。絵を見て探してください".to_string()
            } else if s.hits.is_empty() {
                "見つかりませんでした。別の書き方 (略さない・省く) で探してみてください".to_string()
            } else if s.hits.len() >= MAX_HITS {
                format!("{MAX_HITS} 件以上あります。もっと長い語で絞ってください")
            } else {
                format!("{} 件", s.hits.len())
            };
            if let Some(title) = s.title {
                note = format!("「{title}」の見出し: {note}");
            }
            if !s.hits.is_empty() {
                let n = s.hits.len();
                ui.horizontal(|ui| {
                    if ui.small_button("◀").on_hover_text(i18n::tr("前の見つかった所")).clicked() {
                        picked = Some(s.current.map_or(n - 1, |c| (c + n - 1) % n));
                    }
                    if ui.small_button("▶").on_hover_text(i18n::tr("次の見つかった所")).clicked() {
                        picked = Some(s.current.map_or(0, |c| (c + 1) % n));
                    }
                    if let Some(c) = s.current {
                        note = format!("{}/{}  {note}", c + 1, n);
                    }
                    ui.small(note);
                });
                egui::ScrollArea::vertical().id_salt("datasheet_hits").max_height(110.0).auto_shrink([false, true]).show(ui, |ui| {
                    for (i, h) in s.hits.iter().enumerate() {
                        let text = format!("p.{}  {}{}", h.page + 1, if h.in_contents { "[目次] " } else { "" }, h.context);
                        if ui.selectable_label(s.current == Some(i), text).on_hover_text(i18n::tr("クリックで、そのページのその場所へ移ります")).clicked() {
                            picked = Some(i);
                        }
                    }
                });
            } else {
                ui.small(note);
            }
        }
        if let Some(i) = picked
            && let Some(s) = self.search.as_mut()
            && let Some(h) = s.hits.get(i)
        {
            s.current = Some(i);
            self.page = h.page;
            self.jump = Some(h.rect);
        }
    }
}
