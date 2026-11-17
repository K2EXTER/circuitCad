//! The colours of the schematic canvas as a table of roles (bg, grid, part, wire, …), so that the drawing code never
//! names a colour itself. Two themes: dark (the default) and paper. The user can change any role's colour in the
//! settings window (a contrast check marks a colour that is hard to see on the ground). Pictures written out (SVG, PDF,
//! README shots) always use paper: `with_paper`.
//!
//! The current palette is per thread: egui draws on one thread, and tests that write SVG in parallel do not see each
//! other's swap to paper.

use eframe::egui::Color32;
use std::cell::Cell;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    Bg,
    Grid,
    Part,
    PartFill,
    PinNumber,
    Wire,
    Bus,
    NetLabel,
    NoConnect,
    PartName,
    Value,
    Voltage,
    Current,
    Selection,
    Error,
    Warning,
    /// The live logic view (`schematic/live_logic.rs`): a net at High, at Low, at neither (X: between the thresholds, or not
    /// driven), and a lit LED.
    LogicHigh,
    LogicLow,
    LogicX,
    LedOn,
    /// The ground under a number drawn over the lines (not offered for editing: it follows the ground).
    TextChip,
}

/// The roles in the order of the settings window, with the key kept in `settings.json`, the name shown and what it
/// colours.
pub const ROLES: &[(Role, &str, &str, &str)] = &[
    (Role::Bg, "bg", "地", "キャンバスの地"),
    (Role::Grid, "grid", "方眼", "方眼の点"),
    (Role::Part, "part", "部品", "部品の記号とピン"),
    (Role::PartFill, "part_fill", "部品の塗り", "IC などの箱の中"),
    (Role::PinNumber, "pin_number", "ピン番号", "ピンの番号"),
    (Role::Wire, "wire", "配線", "電圧の結果がないときの配線と接続点"),
    (Role::Bus, "bus", "バス", "バスの線とバスのラベル"),
    (Role::NetLabel, "net_label", "ネットラベル", "ネットの名前のラベル"),
    (Role::NoConnect, "no_connect", "未接続の印", "つながないピンの × と、つながっていないピンの印"),
    (Role::PartName, "part_name", "部品番号", "R1・U2 などの部品番号"),
    (Role::Value, "value", "値", "10k・LM358 などの値"),
    (Role::Voltage, "voltage", "電圧の数字", "解析の結果の電圧"),
    (Role::Current, "current", "電流の数字", "解析の結果の電流"),
    (Role::Selection, "selection", "選択", "選んでいるもの"),
    (Role::Error, "error", "まちがい", "ERC のまちがいの印"),
    (Role::Warning, "warning", "注意", "ERC の注意の印・故障の印"),
    (Role::LogicHigh, "logic_high", "High (1)", "デジタル回路を動かすときの、High のネット"),
    (Role::LogicLow, "logic_low", "Low (0)", "デジタル回路を動かすときの、Low のネット"),
    (Role::LogicX, "logic_x", "不定 (X)", "デジタル回路を動かすときの、High とも Low とも言えないネット"),
    (Role::LedOn, "led_on", "光った LED", "デジタル回路を動かすときの、光っている LED"),
];

const N: usize = Role::TextChip as usize + 1;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Theme {
    #[default]
    Dark,
    Paper,
}

impl Theme {
    pub fn key(self) -> &'static str {
        match self {
            Theme::Dark => "dark",
            Theme::Paper => "paper",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "dark" => Some(Theme::Dark),
            "paper" => Some(Theme::Paper),
            _ => None,
        }
    }
    pub fn base(self) -> Palette {
        match self {
            Theme::Dark => DARK,
            Theme::Paper => PAPER,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Palette {
    c: [Color32; N],
    /// The voltage steps: 0 V, five above (warm), five below (cool).
    pub zero: Color32,
    pub above: [Color32; 5],
    pub below: [Color32; 5],
}

impl Palette {
    pub fn get(&self, r: Role) -> Color32 {
        self.c[r as usize]
    }
    pub fn set(&mut self, r: Role, c: Color32) {
        self.c[r as usize] = c;
    }
}

const fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

const fn pal(roles: [Color32; N], zero: Color32, above: [Color32; 5], below: [Color32; 5]) -> Palette {
    Palette { c: roles, zero, above, below }
}

/// Dark ground, light gray parts: colour is kept for results (the voltage steps, currents, errors).
pub const DARK: Palette = pal(
    [
        rgb(30, 32, 36),                                // Bg
        rgb(68, 72, 80),                                // Grid
        rgb(200, 202, 208),                             // Part
        rgb(44, 48, 56),                                // PartFill
        Color32::from_gray(130),                        // PinNumber
        rgb(96, 196, 120),                              // Wire
        rgb(120, 150, 240),                             // Bus
        rgb(90, 205, 205),                              // NetLabel
        rgb(130, 160, 240),                             // NoConnect
        rgb(140, 180, 240),                             // PartName
        Color32::from_gray(190),                        // Value
        rgb(120, 185, 255),                             // Voltage
        rgb(255, 165, 70),                              // Current
        rgb(110, 170, 255),                             // Selection
        rgb(244, 110, 200),                             // Error
        rgb(236, 170, 50),                              // Warning
        rgb(255, 96, 80),                               // LogicHigh
        rgb(70, 140, 255),                              // LogicLow
        rgb(235, 200, 60),                              // LogicX
        rgb(255, 70, 40),                               // LedOn
        Color32::from_rgba_premultiplied(18, 19, 22, 210), // TextChip
    ],
    rgb(118, 122, 130),
    [rgb(226, 214, 110), rgb(238, 186, 84), rgb(244, 150, 82), rgb(238, 112, 98), rgb(226, 92, 150)],
    [rgb(110, 206, 226), rgb(100, 180, 246), rgb(110, 150, 250), rgb(140, 132, 246), rgb(176, 120, 236)],
);

/// Paper: the look of a printed schematic (and of every picture written out).
pub const PAPER: Palette = pal(
    [
        rgb(250, 248, 235),                                   // Bg
        Color32::from_gray(190),                              // Grid
        Color32::from_gray(60),                               // Part
        rgb(226, 232, 240),                                   // PartFill
        Color32::from_gray(110),                              // PinNumber
        rgb(0, 110, 30),                                      // Wire
        rgb(40, 70, 170),                                     // Bus
        rgb(0, 100, 100),                                     // NetLabel
        rgb(60, 90, 170),                                     // NoConnect
        rgb(20, 60, 140),                                     // PartName
        Color32::from_gray(60),                               // Value
        rgb(10, 60, 200),                                     // Voltage
        rgb(200, 90, 0),                                      // Current
        rgb(0, 80, 220),                                      // Selection
        rgb(230, 0, 200),                                     // Error
        rgb(190, 110, 0),                                     // Warning
        rgb(210, 30, 20),                                     // LogicHigh
        rgb(20, 80, 210),                                     // LogicLow
        rgb(160, 120, 0),                                     // LogicX
        rgb(230, 40, 20),                                     // LedOn
        Color32::from_rgba_premultiplied(255, 255, 255, 190), // TextChip
    ],
    Color32::from_gray(150),
    [rgb(140, 125, 0), rgb(200, 115, 0), rgb(220, 90, 20), rgb(200, 40, 40), rgb(170, 20, 110)],
    [rgb(0, 150, 180), rgb(0, 110, 200), rgb(40, 80, 210), rgb(90, 60, 200), rgb(120, 40, 170)],
);

thread_local! {
    static CURRENT: Cell<Palette> = const { Cell::new(DARK) };
}

/// The colour of a role in the palette the canvas is drawn with now.
pub fn col(r: Role) -> Color32 {
    CURRENT.with(|p| p.get().get(r))
}

pub fn palette() -> Palette {
    CURRENT.with(|p| p.get())
}

pub fn set_palette(p: Palette) {
    CURRENT.with(|c| c.set(p));
}

/// Runs `f` with the paper palette (a picture written out), then puts the palette back.
pub fn with_paper<T>(f: impl FnOnce() -> T) -> T {
    let before = palette();
    set_palette(PAPER);
    let out = f();
    set_palette(before);
    out
}

/// The theme's palette with the user's own colours over it (keys of `ROLES`; unknown keys are ignored).
pub fn build(theme: Theme, own: &[(String, [u8; 3])]) -> Palette {
    let mut p = theme.base();
    for (k, [r, g, b]) in own {
        if let Some((role, ..)) = ROLES.iter().find(|(_, key, ..)| key == k) {
            p.set(*role, rgb(*r, *g, *b));
        }
        if k == "bg" {
            // the chip under numbers follows the ground
            let [r, g, b, _] = rgb(*r, *g, *b).to_array();
            p.set(Role::TextChip, Color32::from_rgba_unmultiplied(r, g, b, 200));
        }
    }
    p
}

fn luminance(c: Color32) -> f32 {
    let f = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
}

/// The WCAG contrast ratio of two colours (1 = the same, 21 = black on white).
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (x, y) = (luminance(a), luminance(b));
    (x.max(y) + 0.05) / (x.min(y) + 0.05)
}

/// Lines and marks need 3:1 against the ground to be seen (WCAG 1.4.11); the grid and the part fill are meant to be faint.
pub fn hard_to_see(p: &Palette, r: Role) -> bool {
    !matches!(r, Role::Bg | Role::Grid | Role::PartFill | Role::TextChip) && contrast(p.get(r), p.get(Role::Bg)) < 3.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_role_of_both_themes_stands_out_from_the_ground() {
        for t in [Theme::Dark, Theme::Paper] {
            let p = t.base();
            for (r, k, ..) in ROLES {
                assert!(!hard_to_see(&p, *r), "{t:?} {k}: {:.2}", contrast(p.get(*r), p.get(Role::Bg)));
            }
            for c in p.above.iter().chain(&p.below) {
                assert!(contrast(*c, p.get(Role::Bg)) >= 3.0, "{t:?} voltage step {c:?}");
            }
        }
    }

    #[test]
    fn own_colours_go_over_the_theme_and_paper_is_only_for_the_picture() {
        let p = build(Theme::Dark, &[("wire".into(), [1, 2, 3]), ("nonsense".into(), [9, 9, 9])]);
        assert_eq!(p.get(Role::Wire), rgb(1, 2, 3));
        assert_eq!(p.get(Role::Part), DARK.get(Role::Part));
        set_palette(p);
        assert_eq!(with_paper(|| col(Role::Bg)), PAPER.get(Role::Bg));
        assert_eq!(col(Role::Wire), rgb(1, 2, 3), "back to the user's palette");
        assert!(hard_to_see(&build(Theme::Dark, &[("part".into(), [40, 42, 46])]), Role::Part));
    }
}
