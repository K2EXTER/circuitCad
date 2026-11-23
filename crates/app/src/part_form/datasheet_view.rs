//! The data sheet window: a PDF opened beside the part form, to read while typing the numbers in.

use datasheet::{Datasheet, HEADINGS, Hit, Quantity, Query, Rect, looks_like_part_number};
use super::graph_reader::{Dest, GraphOut, GraphReader, Mode};
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
    path: std::path::PathBuf,
    /// A PDF of pictures only (a scan) is read by the OCR, a page a frame: the next page to read.
    ocr_next: Option<usize>,
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
    /// An outline drawing is being cut out (for SolidCraft): drag a rectangle over it.
    cut_view: Option<super::solidcraft::View>,
    /// The drawing cut out, for the form to take.
    drawing_out: Option<super::solidcraft::Drawing>,
    drag_from: Option<egui::Pos2>,
    /// The pins read from the rectangle, being checked.
    panel: Option<TablePanel>,
    /// The pins the person accepted, for the form to take.
    pins_out: Option<PinImport>,
    /// Points being read off a graph (its own window beside this one).
    graph: Option<GraphReader>,
    /// The page drawn at a fixed scale for finding curves under the pointer (page, picture, pixels per point).
    snap: Option<(usize, datasheet::PageImage, f32)>,
    /// The graph captions of the whole PDF, read the first time a field asks for its graph.
    figures: Option<Vec<datasheet::figures::Figure>>,
    /// The reading guide (Q10): the step shown, while it is open (`crate::ds_reading::STEPS`).
    guide: Option<usize>,
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
    /// Where to bring into view for each hit, when not the hit itself (a graph above or below its caption).
    focus: Vec<Rect>,
}

impl Search {
    fn new(queries: Vec<Query>, title: Option<&'static str>) -> Option<Search> {
        if queries.is_empty() { None } else { Some(Search { queries, title, next_page: 0, hits: vec![], current: None, any_text: false, focus: vec![] }) }
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
                // pictures only (a scan): the OCR reads the pages, a page a frame, so search, tables and the draft work
                let scan = !ds.has_text(0) && datasheet::ocr::max_side().is_some();
                ds.set_ocr(scan);
                self.doc = Some(Opened {
                    ds,
                    name,
                    path: path.to_path_buf(),
                    ocr_next: scan.then_some(0),
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
                    cut_view: None,
                    drawing_out: None,
                    drag_from: None,
                    panel: None,
                    pins_out: None,
                    graph: None,
                    snap: None,
                    figures: None,
                    guide: None,
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

    /// The open data sheet itself (for reading it whole: `draft.rs`).
    pub fn datasheet(&self) -> Option<&Datasheet> {
        self.doc.as_ref().map(|d| &d.ds)
    }

    /// What the first pages suggest (part numbers and the kind of part), when a PDF is open.
    pub fn candidates(&self) -> Option<&Candidates> {
        self.doc.as_ref().map(|d| &d.candidates)
    }

    /// The candidates read again now (a scanned PDF: from the words the OCR reads, which may not have been read when it
    /// was opened).
    pub fn fresh_candidates(&self) -> Option<Candidates> {
        self.doc.as_ref().map(|d| candidates_of(&d.ds, &d.path))
    }

    /// The form asks for a word to be clicked in the PDF: the window comes forward and waits.
    pub fn start_pick(&mut self, target: Target) {
        if let Some(d) = self.doc.as_mut() {
            d.target = Some(target);
            d.picked = None;
            self.open = true;
        }
    }

    /// The form (or the window's own button) starts reading points off a graph on the page shown. `dest` is the
    /// form's field the points can be sent to.
    /// The page goes to the graph the field's curve is drawn in, found by its caption ("Figure 9. DC Current Gain"):
    /// the page shown when it has one, else the likeliest. The other graphs of that curve are listed like search hits.
    pub fn start_graph(&mut self, dest: Option<Dest>) {
        if let Some(d) = self.doc.as_mut() {
            d.target = None;
            d.selecting = false;
            let mut note = None;
            let mut axes = None;
            if let Some(dest) = dest {
                let figures = d.figures.get_or_insert_with(|| d.ds.figures());
                let found = datasheet::figures::showing(figures, dest.curve());
                if found.is_empty() {
                    note = Some(i18n::trf!("「{}」のグラフの見出しは見つかりませんでした。ページをめくってグラフを探してください", i18n::tr(dest.curve_name())));
                } else {
                    let hits: Vec<Hit> = found.iter().map(|f| Hit { page: f.page, rect: f.rect, text: f.caption.clone(), context: f.caption.clone(), in_contents: false }).collect();
                    let focus: Vec<Rect> = found.iter().map(|f| f.area).collect();
                    let at = hits.iter().position(|h| h.page == d.page).unwrap_or(0);
                    if hits[at].page != d.page || d.graph.as_ref().is_none_or(|g| g.page != d.page) {
                        d.jump = Some(focus[at]);
                    }
                    d.page = hits[at].page;
                    axes = d.ds.figure_axes(found[at]);
                    let n = hits.len();
                    d.search = Some(Search { queries: vec![], title: Some(dest.curve_name()), next_page: d.ds.page_count(), hits, current: Some(at), any_text: true, focus });
                    if n > 1 {
                        note = Some(i18n::trf!("「{}」のグラフが {n} 枚あります。違う図なら、検索の一覧から選んでください", i18n::tr(dest.curve_name())));
                    }
                }
            }
            // the same page's graph already set up: read another curve with its ticks (VBE(sat) and VCE(sat) share a figure)
            match d.graph.as_mut() {
                Some(g) if g.page == d.page && g.has_axes() => g.retarget(dest),
                _ => {
                    let mut g = GraphReader::new(d.page, dest);
                    // the axes from the tick labels around the graph: one click on the curve is left; and the curve
                    // itself when its label (or being the only one) tells which
                    if let Some(a) = &axes {
                        g.set_axes(a);
                        // an hFE graph "normalized" to 1 at one current (BC546's Figure 7): its numbers are ratios
                        let top = a.y.ticks.iter().filter_map(|t| datasheet::graph::tick_value(&t.1)).fold(0.0_f64, f64::max);
                        if dest.is_some_and(|d| d.curve() == datasheet::figures::Curve::DcGain) && top > 0.0 && top <= 5.0 {
                            g.note(i18n::trf!("この図の縦軸は {top} までで、hFE の値でなく 1 つの電流での値を 1 とした比 (normalized) のようです。表の hFE (その電流の値) を掛けて入れるか、ほかの図を使ってください"));
                        }
                        if let Some(dest) = dest
                            && g.has_axes()
                        {
                            match d.pick_curve(a, dest) {
                                datasheet::pick::Pick::One { path, by } => g.auto_trace(path, &by),
                                datasheet::pick::Pick::Several(c) => {
                                    let n = c.len();
                                    g.show_candidates(c);
                                    g.note(i18n::trf!("この図には曲線が {n} 本あり、どれを読むか決められませんでした。色を付けた候補の線から、読みたい曲線を 1 回クリックしてください (候補にない線もクリックでたどれます)"));
                                }
                                datasheet::pick::Pick::None => {}
                            }
                        }
                    }
                    d.graph = Some(g);
                }
            }
            if let (Some(g), Some(n)) = (d.graph.as_mut(), note) {
                g.note(n);
            }
            self.open = true;
        }
    }

    /// Reads a graph for a screenshot (`--form-graph "page|x=0.2,0.8|y=1.0E-04,1.0E+00|pts=0.55:1e-3,0.62:1e-2"`, page
    /// from 1): the labels are clicked where they are printed (the last one on the page with that text), the points
    /// are placed at their values. `trace=0.5:1.26e-3` follows the curve through that point (「曲線をたどる」).
    pub fn graph_for_screenshot(&mut self, spec: &str) {
        let mut parts = spec.split('|');
        let page = parts.next().and_then(|p| p.trim().parse::<usize>().ok()).unwrap_or(1).saturating_sub(1);
        self.go_to_page(page);
        self.start_graph(Some(Dest::DiodeForward));
        let Some(d) = self.doc.as_mut() else { return };
        let labels = datasheet::graph::number_labels(d.ds.words(d.page));
        let Some(mut g) = d.graph.take() else { return };
        for part in parts {
            let (key, list) = part.split_once('=').unwrap_or((part, ""));
            for item in list.split(',').filter(|s| !s.is_empty()) {
                match key {
                    "x" | "y" => {
                        g.mode = Some(if key == "x" { Mode::XTick } else { Mode::YTick });
                        // the same number is printed on several graphs: the lowest one for the horizontal axis, the
                        // rightmost for the vertical (the graph at the bottom right)
                        let same = labels.iter().filter(|l| l.0 == item);
                        let l = if key == "x" { same.max_by(|a, b| a.2.total_cmp(&b.2)) } else { same.max_by(|a, b| a.1.total_cmp(&b.1)) };
                        if let Some(l) = l {
                            g.click((l.1, l.2), Some((&l.0, (l.1, l.2))));
                            d.jump = Some(Rect { x0: l.1, y0: l.2, x1: l.1, y1: l.2 });
                        }
                    }
                    "trace" => {
                        if let Some((a, b)) = item.split_once(':')
                            && let (Ok(a), Ok(b)) = (a.parse(), b.parse())
                            && let Some((x, y)) = g.position_of(a, b)
                        {
                            let path = d.trace_at(x, y);
                            g.add_trace(path);
                        }
                    }
                    "pts" => {
                        if let Some((a, b)) = item.split_once(':')
                            && let (Ok(a), Ok(b)) = (a.parse(), b.parse())
                        {
                            g.add_point_at_values(a, b);
                        }
                    }
                    _ => {}
                }
            }
        }
        g.mode = Some(Mode::Points);
        d.graph = Some(g);
    }

    /// The capacitors the open PDF names with a value (`Datasheet::capacitors`; none when no PDF is open).
    pub fn capacitors(&self) -> Vec<datasheet::companions::FoundCap> {
        self.doc.as_ref().map(|d| d.ds.capacitors()).unwrap_or_default()
    }

    /// The ESR the open PDF asks of the capacitors (`Datasheet::esr`; none when no PDF is open).
    pub fn esr(&self) -> Option<datasheet::companions::FoundEsr> {
        self.doc.as_ref().and_then(|d| d.ds.esr())
    }

    /// The open PDF's thermal resistances (`Datasheet::thermal`; none when no PDF is open).
    pub fn thermal(&self) -> Vec<datasheet::thermal::ThermalRow> {
        self.doc.as_ref().map(|d| d.ds.thermal()).unwrap_or_default()
    }

    /// The part numbers of `name`'s family in the open PDF and the output voltages it lists as options (none when no PDF
    /// is open; `Datasheet::part_numbers`, `voltage_options`).
    pub fn part_numbers(&self, name: &str) -> (Vec<(String, usize)>, Vec<f64>) {
        self.doc.as_ref().map(|d| (d.ds.part_numbers(name), d.ds.voltage_options())).unwrap_or_default()
    }

    /// The package of each part number of the family of `name` (`Datasheet::part_packages`).
    pub fn part_packages(&self, name: &str) -> Vec<(String, String)> {
        self.doc.as_ref().map(|d| d.ds.part_packages(name)).unwrap_or_default()
    }

    /// A Darlington's resistors R1, R2 [Ω] and the page, from the open PDF's circuit drawing (`Datasheet::darlington_resistors`).
    pub fn darlington_resistors(&self) -> Option<(f64, f64, usize)> {
        self.doc.as_ref().and_then(|d| d.ds.darlington_resistors())
    }

    /// The rows of `symbol` in the open PDF's electrical characteristics tables (none when no PDF is open).
    pub fn spec_values(&self, symbol: &str) -> Vec<datasheet::spec_table::SpecValue> {
        self.doc.as_ref().map(|d| d.ds.spec_values(symbol)).unwrap_or_default()
    }

    /// `spec_values` for the part number `part`: its grade's columns where the tables print grades over them
    /// (`Datasheet::spec_values_for`: onsemi's MC7805B and MC7805C side by side).
    pub fn spec_values_for(&self, symbol: &str, part: &str) -> Vec<datasheet::spec_table::SpecValue> {
        self.doc.as_ref().map(|d| d.ds.spec_values_for(symbol, part)).unwrap_or_default()
    }

    /// The grades the open PDF's tables print over their columns (`Datasheet::grades`).
    pub fn grades(&self) -> Vec<String> {
        self.doc.as_ref().map(|d| d.ds.grades()).unwrap_or_default()
    }

    /// The temperature range of each part number of the family of `name` (`Datasheet::part_temperatures`).
    pub fn part_temperatures(&self, name: &str) -> Vec<(String, (f64, f64))> {
        self.doc.as_ref().map(|d| d.ds.part_temperatures(name)).unwrap_or_default()
    }

    /// Reads the field `dest`'s curve without a click (「表とグラフから全部読む」): the graphs with its caption in turn
    /// (at most 3), each with its axes from the tick labels and the curve picked by its label or as the only one; the
    /// first that gives points. Why none did, when none did. The window and the page shown are left as they were.
    pub fn read_curve(&mut self, dest: Dest) -> Result<GraphOut, String> {
        let d = self.doc.as_mut().ok_or_else(|| i18n::tr("PDF が開いていません").to_string())?;
        let figures = d.figures.get_or_insert_with(|| d.ds.figures()).clone();
        let found = datasheet::figures::showing(&figures, dest.curve());
        if found.is_empty() {
            return Err(i18n::tr("図の見出しが見つかりません").to_string());
        }
        let mut why = String::new();
        for f in found.iter().take(3) {
            let Some(a) = d.ds.figure_axes(f) else {
                why = i18n::trf!("p.{} の図の目盛りが読めません", f.page + 1);
                continue;
            };
            let mut g = GraphReader::new(f.page, Some(dest));
            g.set_axes(&a);
            if !g.has_axes() {
                why = i18n::trf!("p.{} の図の目盛りが読めません", f.page + 1);
                continue;
            }
            // a regulator's curve against frequency is on a log axis: a linear one is another graph's (the axes of the
            // graph beside it, LM317's Figure 6-12) or not against frequency (LP5907's "Noise Density Test")
            if matches!(dest, Dest::RegRipple | Dest::RegNoise) && !a.x.log {
                why = i18n::trf!("p.{} の図の横軸が周波数 (対数) ではありません", f.page + 1);
                continue;
            }
            // an hFE graph normalized to 1 (BC546's Figure 7) is not hFE: a transistor's is read as ratios and scaled by
            // the table's hFE in the form; a Darlington's is not used
            let top = a.y.ticks.iter().filter_map(|t| datasheet::graph::tick_value(&t.1)).fold(0.0_f64, f64::max);
            let normalized = dest.curve() == datasheet::figures::Curve::DcGain && top > 0.0 && top <= 5.0;
            if normalized && dest != Dest::BjtHfe {
                why = i18n::trf!("p.{} の図は 1 を基準にした比 (normalized) です", f.page + 1);
                continue;
            }
            match d.pick_curve_on(f.page, &a, dest) {
                datasheet::pick::Pick::One { path, by } => {
                    g.auto_trace(path, &by);
                    match g.values() {
                        Ok(points) if !points.is_empty() => {
                            // the VCE it was measured at: the picked curve's own label, else the one the graph prints
                            let vce = match dest {
                                Dest::BjtHfe | Dest::BjtVbeOn | Dest::DarlHfe | Dest::DarlVbeOn => datasheet::spec_table::condition(&by, "VCE").or_else(|| match d.ds.figure_condition(f, "VCE")[..] {
                                    [v] => Some(v),
                                    _ => None,
                                }),
                                _ => None,
                            };
                            let by = if by.is_empty() { String::new() } else { i18n::trf!("「{}」の曲線", by) };
                            return Ok(GraphOut { dest, points, page: f.page, by, normalized, vce });
                        }
                        _ => why = i18n::trf!("p.{} の図の曲線から点が取れません", f.page + 1),
                    }
                }
                // one line found that goes the wrong way for the curve: pieces of other curves joined (dashed lines), not the curve
                datasheet::pick::Pick::Several(c) if c.len() == 1 => why = i18n::trf!("p.{} の図で見つかった線が{}の曲線らしくありません (破線などの別の線をつないだ恐れ)", f.page + 1, i18n::tr(dest.curve_name())),
                datasheet::pick::Pick::Several(c) => why = i18n::trf!("p.{} の図に曲線が {} 本あり、どれか決められません。「グラフから読む」で候補の線に色が付きます", f.page + 1, c.len()),
                datasheet::pick::Pick::None => why = i18n::trf!("p.{} の図に曲線が見つかりません", f.page + 1),
            }
        }
        Err(why)
    }

    /// The points the person sent to the form since the last call.
    pub fn take_graph(&mut self) -> Option<GraphOut> {
        self.doc.as_mut().and_then(|d| d.graph.as_mut()).and_then(|g| g.take_out())
    }

    /// The form asks for an outline drawing to be cut out (for SolidCraft): the window comes forward and waits for a rectangle.
    pub fn start_drawing_cut(&mut self, view: super::solidcraft::View) {
        if let Some(d) = self.doc.as_mut() {
            d.cut_view = Some(view);
            d.selecting = false;
            d.target = None;
            self.open = true;
        }
    }

    /// Cuts the drawing in `area` (points) of `page` (from 0) as if it had been dragged round (`--form-drawing`, for screenshots).
    pub fn cut_drawing_at(&mut self, view: super::solidcraft::View, page: usize, area: Rect) {
        if let Some(d) = self.doc.as_mut() {
            d.page = page;
            d.drawing_out = cut_drawing(&d.ds, page, area, view);
            self.open = true;
        }
    }

    /// The drawing cut out since the last call.
    pub fn take_drawing(&mut self) -> Option<super::solidcraft::Drawing> {
        self.doc.as_mut().and_then(|d| d.drawing_out.take())
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
    /// Reads the table in `area` (points) of `page` (from 0) as if it had been dragged round (`--form-table`, for screenshots).
    pub fn open_table_at(&mut self, page: usize, area: Rect) {
        if let Some(d) = self.doc.as_mut() {
            d.page = page.min(d.ds.page_count().saturating_sub(1));
            d.panel = Some(TablePanel::new(d.page, area, d.ds.table_in(d.page, area)).with_drawing(drawing_pins(&d.ds, d.page, area)));
        }
    }

    /// Opens the reading guide at step `k` (0 = the first) and searches its heading (Q10; `--form-guide`).
    pub fn open_guide(&mut self, k: usize) {
        if let Some(d) = self.doc.as_mut() {
            let k = k.min(crate::ds_reading::STEPS.len() - 1);
            let s = &crate::ds_reading::STEPS[k];
            d.guide = Some(k);
            d.search = Search::new(s.terms.iter().filter_map(|t| Query::new(t)).collect(), Some(s.title));
        }
    }

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

    /// The form's button that opens a PDF (on the tab row, one for every tab), and "窓を出す" when its window was
    /// closed. Laid out by the caller's layout (right to left on the tab row). Why a PDF did not open is `message`.
    pub fn open_button(&mut self, ui: &mut egui::Ui) {
        if self.doc.is_some() && !self.open && ui.button(i18n::tr("窓を出す")).on_hover_text(i18n::tr("閉じたデータシートの窓をもう一度出します")).clicked() {
            self.open = true;
        }
        let label = if self.doc.is_some() { "データシート (PDF) を替える…" } else { "データシート (PDF) を開く…" };
        if ui.button(i18n::tr(label)).on_hover_text(i18n::tr("手元のデータシート (PDF) を、別の窓に出します。日本語でも英語でも開けます。開いておくと、各タブの「PDFから選ぶ」で、窓の数字をクリックして欄に入れられます (ピン表は囲んで取り込めます)。PDF の窓に、PDF ファイルをドラッグしても開けます。ファイルはこのパソコンの中だけで読み、どこにも送りません")).clicked()
            && let Some(p) = rfd::FileDialog::new().set_title("データシート (PDF)").add_filter("PDF", &["pdf", "PDF"]).pick_file()
        {
            self.load(&p);
        }
    }

    /// Why the last PDF did not open (nothing when it did).
    pub fn message(&self, ui: &mut egui::Ui) {
        if !self.msg.is_empty() {
            ui.colored_label(crate::theme::error(), &self.msg);
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
            d.graph = None;
        }
        // the window that reads points off a graph: over the left panel, like the pin table's
        if let Some(d) = self.doc.as_mut()
            && let Some(g) = d.graph.as_mut()
        {
            let mut keep = true;
            let mut keep_open = true;
            egui::Window::new(i18n::tr("グラフから点を読む")).open(&mut keep_open).default_pos([8.0, 56.0]).default_size([crate::PANEL_WIDTH - 20.0, 460.0]).resizable(true).show(ctx, |ui| {
                keep = g.ui(ui, d.page);
            });
            if !keep || !keep_open {
                d.graph = None;
            }
        }
        // the window that checks the pins read from the rectangle
        let current = &self.current_pins;
        if let Some(d) = self.doc.as_mut()
            && let Some(panel) = d.panel.as_mut()
        {
            let mut keep = true;
            let mut action = Action::None;
            // over the left panel (its parts list is not used while making a part), so the form's pin table and the data sheet stay
            // in sight to compare with
            egui::Window::new(i18n::tr("ピン表の取り込み")).open(&mut keep).default_pos([8.0, 56.0]).default_size([crate::PANEL_WIDTH - 20.0, 460.0]).resizable(true).show(ctx, |ui| {
                action = panel.ui(ui, current);
            });
            match action {
                Action::Apply(mode) => {
                    let (num_col, name_col) = panel.columns();
                    d.pins_out = Some(PinImport { lines: panel.text.clone(), mode, page: panel.page, area: panel.area, num_col, name_col, from_drawing: panel.as_drawing });
                    d.panel = None;
                }
                Action::Cancel => d.panel = None,
                Action::None if !keep => d.panel = None,
                Action::None => {}
            }
        }
    }
}

/// The pins of a pin drawing in `area` (the first whose numbers run 1, 2, 3 …), for the table window.
fn drawing_pins(ds: &Datasheet, page: usize, area: Rect) -> Option<Vec<(String, String)>> {
    datasheet::pin_diagram::pins_in_drawings(ds.words(page), area).into_iter().find(|p| datasheet::draft::plausible_pins(p))
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
    /// Reading a scanned PDF with the OCR: one page a frame with the progress and a button to stop; afterwards a note
    /// that the words were read off pictures. A PDF without text that is not being read offers to read it.
    fn ocr_ui(&mut self, ui: &mut egui::Ui) {
        let pages = self.ds.page_count();
        if let Some(p) = self.ocr_next {
            // the page shown first, then the rest in order
            let next = if !self.ds.ocr_done(self.page) { self.page } else { p };
            let _ = self.ds.words(next);
            let left = (0..pages).find(|&q| !self.ds.ocr_done(q));
            self.ocr_next = left;
            if left.is_none() {
                // the part names and the kind from the words now read
                self.candidates = candidates_of(&self.ds, &self.path);
            }
            let done = (0..pages).filter(|&q| self.ds.ocr_done(q)).count();
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(crate::theme::info(), i18n::trf!("画像の PDF なので、文字認識で文字を読み取っています… {done}/{pages} ページ"));
                if ui.button(i18n::tr("やめる")).on_hover_text(i18n::tr("読み取りをやめます。読み終えたページの文字はそのまま使えます")).clicked() {
                    self.ocr_next = None;
                    self.ds.set_ocr(false);
                }
            });
            ui.ctx().request_repaint();
        } else if self.ds.ocr_on() {
            ui.label(egui::RichText::new(i18n::tr("このPDFの文字は、Windows の文字認識でページの絵から読みました。読み違いがあるので、入れた値は絵と見比べてください")).small().color(crate::theme::warn()));
        } else if !self.ds.has_text(self.page) && datasheet::ocr::max_side().is_some() {
            ui.horizontal_wrapped(|ui| {
                ui.label(egui::RichText::new(i18n::tr("このページには文字がありません (画像の PDF)")).small());
                if ui.button(i18n::tr("文字を読み取る (OCR)")).on_hover_text(i18n::tr("Windows の文字認識で、文字の無いページの絵から文字を読みます (1 ページ 1 秒ほど)。読めた文字で検索・表の読み取り・下書きが使えます")).clicked() {
                    self.ds.set_ocr(true);
                    self.ocr_next = Some(0);
                }
            });
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        let pages = self.ds.page_count();
        let (pw, _) = self.ds.page_size(self.page).unwrap_or((595.0, 842.0));
        if std::mem::take(&mut self.fit) {
            self.zoom = ((ui.available_width() - 24.0) / pw).clamp(ZOOM_MIN, ZOOM_MAX);
        }
        if (self.target.is_some() || self.selecting || self.cut_view.is_some() || self.graph.as_ref().is_some_and(|g| g.mode.is_some())) && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.target = None;
            self.selecting = false;
            self.cut_view = None;
            if let Some(g) = self.graph.as_mut() {
                g.mode = None;
            }
        }
        self.ocr_ui(ui);
        ui.horizontal_wrapped(|ui| {
            if crate::widgets::unless(ui, (!(self.page > 0)).then_some("最初のページです"), egui::Button::new("◀")).on_hover_text(i18n::tr("前のページ")).clicked() {
                self.page -= 1;
            }
            let mut shown = self.page + 1;
            if ui.add(egui::DragValue::new(&mut shown).range(1..=pages).suffix(i18n::trf!(" / {pages} ページ"))).on_hover_text(i18n::tr("ページ番号。ドラッグか、数字を入れて切り替えます")).changed() {
                self.page = shown - 1;
            }
            if crate::widgets::unless(ui, (!(self.page + 1 < pages)).then_some("最後のページです"), egui::Button::new("▶")).on_hover_text(i18n::tr("次のページ")).clicked() {
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
                ui.colored_label(crate::theme::info(), t.ask());
                if ui.button(i18n::tr("やめる")).on_hover_text(i18n::tr("選ぶのをやめます (Esc キーでも)")).clicked() {
                    self.target = None;
                }
            });
        }
        if let Some(view) = self.cut_view {
            ui.horizontal(|ui| {
                ui.colored_label(crate::theme::info(), i18n::trf!("外形図の「{}」を、下のページでドラッグして囲んでください (寸法の数字と線が入るように)", i18n::tr(view.label())));
                if ui.button(i18n::tr("やめる")).on_hover_text(i18n::tr("囲むのをやめます (Esc キーでも)")).clicked() {
                    self.cut_view = None;
                }
            });
        }
        if self.selecting {
            ui.horizontal(|ui| {
                ui.colored_label(crate::theme::info(), i18n::tr("ピン表を、下のページでドラッグして囲んでください (番号と名前の列が入るように)"));
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
            // centred, but never past its top left corner (a whole graph taller than the window, or the window's size
            // not known yet in the frame it opens)
            let centre = Vec2::new(r.x0 + r.x1, r.y0 + r.y1) * 0.5 * self.zoom;
            let corner = Vec2::new(r.x0, r.y0) * self.zoom;
            scroll = scroll.scroll_offset((centre - self.viewport * 0.5).min(corner).max(Vec2::ZERO));
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
                    ui.colored_label(crate::theme::error(), e);
                    return;
                }
            }
        }
        let Some(tex_id) = self.texture.as_ref().map(|t| t.handle.id()) else { return };
        let sense = if self.selecting || self.cut_view.is_some() { egui::Sense::click_and_drag() } else { egui::Sense::click() };
        let page_size = (w, h);
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
        // cutting out the pin table or an outline drawing: a rectangle is dragged over it
        if self.selecting || self.cut_view.is_some() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            if resp.drag_started() {
                // where the button went down, not where the pointer is when egui notices the drag
                self.drag_from = ui.input(|i| i.pointer.press_origin());
            }
            if let (Some(from), Some(to)) = (self.drag_from, resp.interact_pointer_pos()) {
                let r = egui::Rect::from_two_pos(from, to);
                ui.painter().rect_filled(r, 0.0, egui::Color32::from_rgba_unmultiplied(70, 150, 255, 50));
                ui.painter().rect_stroke(r, 0.0, egui::Stroke::new(1.5, crate::theme::info()), egui::StrokeKind::Inside);
                if resp.drag_stopped() {
                    let (a, b) = ((r.min - origin) / zoom, (r.max - origin) / zoom);
                    // a click or a speck is not a table
                    if b.x - a.x > 20.0 && b.y - a.y > 10.0 {
                        let area = Rect { x0: a.x, y0: a.y, x1: b.x, y1: b.y };
                        if let Some(view) = self.cut_view.take() {
                            self.drawing_out = cut_drawing(&self.ds, self.page, area, view);
                        } else {
                            self.panel = Some(TablePanel::new(self.page, area, self.ds.table_in(self.page, area)).with_drawing(drawing_pins(&self.ds, self.page, area)));
                            self.selecting = false;
                        }
                    }
                    self.drag_from = None;
                }
            }
            if resp.drag_stopped() {
                self.drag_from = None;
            }
        }
        // picking points: the curve nearest to the pointer in its column (within 6 points) is where the point goes
        let snapped = match (self.graph.as_ref().filter(|g| g.page == self.page).and_then(|g| g.mode), resp.hover_pos()) {
            (Some(Mode::Points), Some(pos)) if self.graph.as_ref().is_some_and(|g| g.snap) => {
                let p = (pos - origin) / zoom;
                self.curve_near(p.x, p.y)
            }
            (Some(Mode::Trace), Some(pos)) => {
                let p = (pos - origin) / zoom;
                self.curve_near(p.x, p.y)
            }
            _ => None,
        };
        // following a curve: a click traces the curve under the pointer (before the reader is borrowed below)
        let traced = match (self.graph.as_ref().filter(|g| g.page == self.page).and_then(|g| g.mode), resp.hover_pos()) {
            (Some(Mode::Trace), Some(pos)) if resp.clicked() => {
                let p = (pos - origin) / zoom;
                // a candidate curve near the click is taken as found (whole, with its joined pieces)
                match self.graph.as_mut().and_then(|g| g.take_candidate((p.x, p.y))) {
                    Some(path) => Some(path),
                    None => Some(self.trace_at(p.x, p.y)),
                }
            }
            _ => None,
        };
        // reading a graph: the ticks and points are drawn; a click adds a tick (on a label: its number) or a point
        if let Some(g) = self.graph.as_mut()
            && g.page == self.page
        {
            g.paint(ui.painter(), |x, y| origin + Vec2::new(x, y) * zoom, page_size);
            if let (Some(mode), Some(pos)) = (g.mode, resp.hover_pos()) {
                let mut p = (pos - origin) / zoom;
                if let Some(y) = snapped {
                    p.y = y;
                    let c = origin + p * zoom;
                    ui.painter().line_segment([c - Vec2::new(8.0, 0.0), c + Vec2::new(8.0, 0.0)], egui::Stroke::new(1.5, egui::Color32::from_rgb(230, 40, 40)));
                }
                let label = if matches!(mode, Mode::Points | Mode::Trace) {
                    None
                } else {
                    self.ds.words(self.page).iter().find(|w| w.rect.x0 - 1.0 <= p.x && p.x <= w.rect.x1 + 1.0 && w.rect.y0 <= p.y && p.y <= w.rect.y1).and_then(|w| {
                        let (token, x0, x1) = w.token_span_at(p.x)?;
                        datasheet::graph::tick_value(token)?;
                        // the label's own centre is the tick (several labels may be read as one word)
                        let r = Rect { x0, x1, ..w.rect };
                        Some((token.to_string(), ((x0 + x1) / 2.0, (r.y0 + r.y1) / 2.0), r))
                    })
                };
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
                let tip = match (&label, mode) {
                    (Some((t, _, r)), _) => {
                        ui.painter().rect_stroke(on_screen(r).expand(1.0), 2.0, egui::Stroke::new(1.5, crate::theme::info()), egui::StrokeKind::Outside);
                        format!("目盛り {t}")
                    }
                    (None, Mode::Points | Mode::Trace) => match g.value_at((p.x, p.y)) {
                        Some((a, b)) => format!("({}, {}){}", super::graph_reader::short(a), super::graph_reader::short(b), if snapped.is_some() { "  曲線に合わせました" } else { "" }),
                        None => "先に横軸と縦軸の目盛りを 2 つずつ入れてください".into(),
                    },
                    (None, _) => "ここを目盛りにします (値はあとで入れます)".into(),
                };
                resp.clone().on_hover_text(tip);
                if let Some(path) = traced {
                    g.add_trace(path);
                } else if resp.clicked() {
                    g.click((p.x, p.y), label.as_ref().map(|(t, c, _)| (t.as_str(), *c)));
                }
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
                ui.painter().rect_stroke(on_screen(&word.rect).expand(1.0), 2.0, egui::Stroke::new(1.5, crate::theme::info()), egui::StrokeKind::Outside);
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

    /// The centre of the curve nearest to page point (x, y) in its column, within 6 points (`datasheet::graph::
    /// curve_crossings` on the page drawn at 4 pixels per point; the words on the page are not curves).
    fn curve_near(&mut self, x: f32, y: f32) -> Option<f32> {
        if self.snap.as_ref().is_none_or(|s| s.0 != self.page) {
            let img = self.ds.render(self.page, 4.0).ok()?;
            let k = img.width as f32 / self.ds.page_size(self.page)?.0;
            self.snap = Some((self.page, img, k));
        }
        let (_, img, k) = self.snap.as_ref()?;
        let skip: Vec<Rect> = self.ds.words(self.page).iter().map(|w| w.rect).collect();
        datasheet::graph::curve_crossings(img, *k, x, y - 6.0, y + 6.0, &skip).into_iter().min_by(|a, b| (a - y).abs().total_cmp(&(b - y).abs()))
    }

    /// The curve the field `dest` wants on the graph with axes `a` on the page shown (`datasheet::pick`, on the same
    /// picture as [`Self::curve_near`]).
    fn pick_curve(&mut self, a: &datasheet::axes::ReadAxes, dest: Dest) -> datasheet::pick::Pick {
        self.pick_curve_on(self.page, a, dest)
    }

    /// [`Self::pick_curve`] on the graph of page `page`.
    fn pick_curve_on(&mut self, page: usize, a: &datasheet::axes::ReadAxes, dest: Dest) -> datasheet::pick::Pick {
        if self.snap.as_ref().is_none_or(|s| s.0 != page) {
            let Ok(img) = self.ds.render(page, 4.0) else { return datasheet::pick::Pick::None };
            let Some(k) = self.ds.page_size(page).map(|s| img.width as f32 / s.0) else { return datasheet::pick::Pick::None };
            self.snap = Some((page, img, k));
        }
        // the table's conditions for that quantity name the curve wanted among several ("IL = 500mA", 候補 Q15)
        let want = self.ds.curve_conditions(dest.curve());
        let Some((_, img, k)) = self.snap.as_ref() else { return datasheet::pick::Pick::None };
        datasheet::pick::pick_with(img, *k, self.ds.words(page), a, dest.curve(), dest.label_names(), &want)
    }

    /// The curve under page point (x, y) followed to its ends (`datasheet::graph::trace_curve` on the same picture as
    /// [`Self::curve_near`]).
    fn trace_at(&mut self, x: f32, y: f32) -> Vec<(f32, f32)> {
        if self.snap.as_ref().is_none_or(|s| s.0 != self.page) {
            let Ok(img) = self.ds.render(self.page, 4.0) else { return vec![] };
            let Some(k) = self.ds.page_size(self.page).map(|s| img.width as f32 / s.0) else { return vec![] };
            self.snap = Some((self.page, img, k));
        }
        let Some((_, img, k)) = self.snap.as_ref() else { return vec![] };
        let skip: Vec<Rect> = self.ds.words(self.page).iter().map(|w| w.rect).collect();
        datasheet::graph::trace_curve(img, *k, (x, y), &skip)
    }

    /// The reading guide (Q10): one step at a time, what to look at and why, and its heading searched in this PDF.
    fn guide_ui(&mut self, ui: &mut egui::Ui) {
        let Some(k) = self.guide else { return };
        let steps = crate::ds_reading::STEPS;
        let step = &steps[k.min(steps.len() - 1)];
        let mut go: Option<usize> = None;
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong(i18n::tr(step.title));
                ui.small(format!("{}/{}", k + 1, steps.len()));
            });
            ui.label(crate::learn::spaced(step.detail));
            ui.small(i18n::tr("見るところ:"));
            for c in step.checks {
                ui.small(format!("・{}", i18n::tr_dyn(c)));
            }
            ui.horizontal(|ui| {
                if crate::widgets::unless(ui, (k == 0).then_some("いまが最初の手順です"), egui::Button::new(i18n::tr("◀ 前へ"))).on_hover_text(i18n::tr("前の手順")).clicked() {
                    go = Some(k - 1);
                }
                if ui.button(i18n::tr("このページを探す")).on_hover_text(i18n::tr("この手順の見出しを、データシートの全ページから探して、そのページを出します (英語・日本語の見出し)")).clicked() {
                    go = Some(k);
                }
                if crate::widgets::unless(ui, (k + 1 >= steps.len()).then_some("いまが最後の手順です"), egui::Button::new(i18n::tr("次へ ▶"))).on_hover_text(i18n::tr("次の手順に進み、その見出しを探します")).clicked() {
                    go = Some(k + 1);
                }
                if ui.small_button(i18n::tr("閉じる")).on_hover_text(i18n::tr("読み方の案内を閉じます")).clicked() {
                    self.guide = None;
                }
            });
        });
        if let Some(n) = go {
            self.guide = Some(n);
            let s = &steps[n];
            self.search = Search::new(s.terms.iter().filter_map(|t| Query::new(t)).collect(), Some(s.title));
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
                self.graph = None;
            }
            if ui.button(i18n::tr("グラフから点を読む")).on_hover_text(i18n::tr("いま出ているページのグラフで、軸の目盛りを 2 つずつクリックしてから曲線の上をクリックすると、点の値を読みます。読んだ点は写したり、部品作成フォームの欄に入れたりできます")).clicked() {
                self.target = None;
                self.selecting = false;
                self.graph = Some(GraphReader::new(self.page, None));
            }
            let guide_open = self.guide.is_some();
            if ui
                .selectable_label(guide_open, i18n::tr("読み方の案内"))
                .on_hover_text(i18n::tr("データシートを、どの順に・どこを読めばよいかを、8 つの手順で案内します (絶対最大定格、推奨動作条件、min・typ・max と条件、グラフ …)。手順ごとに、このデータシートのその見出しのページへ飛びます"))
                .clicked()
            {
                self.guide = if guide_open { None } else { Some(0) };
                if !guide_open {
                    let s = &crate::ds_reading::STEPS[0];
                    self.search = Search::new(s.terms.iter().filter_map(|t| Query::new(t)).collect(), Some(s.title));
                }
            }
            ui.small(i18n::tr("見出しへ"));
            for h in HEADINGS {
                if ui.small_button(h.label()).on_hover_text(i18n::trf!("{} (英語・日本語の見出しを探します)", h.help())).clicked() {
                    self.search = Search::new(h.queries(), Some(h.label()));
                }
            }
        });
        self.guide_ui(ui);
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
                "このPDFには検索できる文字がありません (画像のPDFかもしれません)。上の「文字を読み取る (OCR)」で読めます。読めないときは絵を見て探してください".to_string()
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
            self.jump = Some(s.focus.get(i).copied().unwrap_or(h.rect));
        }
    }
}

/// The picture of `area` (PDF points) of a page, drawn sharp enough to read the dimensions (about 1600 pixels across, 2 to 6 pixels a point).
fn cut_drawing(ds: &Datasheet, page: usize, area: Rect, view: super::solidcraft::View) -> Option<super::solidcraft::Drawing> {
    let (w, h) = (f64::from(area.x1 - area.x0), f64::from(area.y1 - area.y0));
    let scale = (1600.0 / w.max(h)).clamp(2.0, 6.0) as f32;
    let img = ds.render(page, scale).ok()?;
    let (x0, y0) = (((area.x0 * scale).max(0.0) as usize).min(img.width), ((area.y0 * scale).max(0.0) as usize).min(img.height));
    let (x1, y1) = (((area.x1 * scale) as usize).min(img.width), ((area.y1 * scale) as usize).min(img.height));
    if x1 <= x0 + 1 || y1 <= y0 + 1 {
        return None;
    }
    let mut rgba = Vec::with_capacity((x1 - x0) * (y1 - y0) * 4);
    for y in y0..y1 {
        rgba.extend_from_slice(&img.rgba[(y * img.width + x0) * 4..(y * img.width + x1) * 4]);
    }
    let mut d = super::solidcraft::Drawing::new(view, page, rgba, [x1 - x0, y1 - y0], w);
    // the dimensions' numbers in it, in the picture's pixels: those with a decimal point (a drawing's pin numbers and note numbers are
    // whole numbers; a whole-number dimension is typed)
    let px = |x: f32, y: f32| [x * scale - x0 as f32, y * scale - y0 as f32];
    d.numbers = ds.lengths_in(page, area).into_iter().filter(|n| n.text.contains(['.', ','])).map(|n| { let (a, b) = (px(n.rect.x0, n.rect.y0), px(n.rect.x1, n.rect.y1)); (n.value, n.inch, n.text, [a[0], a[1], b[0], b[1]]) }).collect();
    Some(d)
}

