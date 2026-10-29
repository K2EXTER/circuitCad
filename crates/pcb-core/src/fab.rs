//! Ordering a board: the rules of a few low-cost fabs for 2-layer boards, and a pre-flight check
//! that tells a beginner whether the design is ready to be sent.
//!
//! The numbers are the fabs' published minimums for a standard 2-layer, 1.6 mm, 1 oz board as of
//! 2026 and are for orientation only: always look at the fab's own page before ordering (the URL
//! is in each preset).

use crate::drc;
use crate::geom::{inside_polygon, signed_area};
use crate::model::*;

#[derive(Clone, Debug)]
pub struct FabPreset {
    pub id: &'static str,
    pub name: &'static str,
    /// One line for the drop-down.
    pub summary: &'static str,
    pub min_track_width: f64,
    pub min_clearance: f64,
    pub edge_clearance: f64,
    pub min_drill: f64,
    pub min_annular_ring: f64,
    /// The size (mm, either way round) up to which the cheapest price tier applies; `None` when the price does not step with it.
    pub cheap_size: Option<(f64, f64)>,
    /// Suggested defaults when drawing (track width, via diameter, via drill).
    pub track_width: f64,
    pub via_diameter: f64,
    pub via_drill: f64,
    pub url: &'static str,
    /// Things the numbers do not capture.
    pub notes: &'static [&'static str],
    /// How to place the order, in order.
    pub steps: &'static [&'static str],
}

pub const PRESETS: &[FabPreset] = &[
    FabPreset {
        id: "standard",
        name: "標準 (安全側)",
        summary: "線幅・間隔 0.15mm / 穴 0.3mm。ほとんどの低価格2層で通る控えめな値",
        min_track_width: 0.15,
        min_clearance: 0.15,
        edge_clearance: 0.3,
        min_drill: 0.3,
        min_annular_ring: 0.13,
        cheap_size: None,
        track_width: 0.25,
        via_diameter: 0.6,
        via_drill: 0.3,
        url: "",
        notes: &["発注先が決まっていないときの値です。決まったら、その発注先を選び直してください"],
        steps: &["発注先のサイトで「ガーバーファイルをアップロード」を選び、書き出した zip を渡します", "板厚 1.6mm・2層・銅箔 1oz が標準です"],
    },
    FabPreset {
        id: "jlcpcb",
        name: "JLCPCB (2層)",
        summary: "線幅 0.127mm / 間隔 0.2mm / ビア穴 0.3mm・外径 0.6mm",
        min_track_width: 0.127,
        min_clearance: 0.2,
        edge_clearance: 0.3,
        min_drill: 0.3,
        min_annular_ring: 0.15,
        cheap_size: Some((100.0, 100.0)),
        track_width: 0.25,
        via_diameter: 0.6,
        via_drill: 0.3,
        url: "https://jlcpcb.com/capabilities/pcb-capabilities",
        notes: &[
            "線同士は 0.127mm まで近づけられますが、ビア・パッドと線の間は 0.2〜0.25mm 必要なため、間隔は 0.2mm にしています",
            "ビア穴の下限は 0.3mm、ビア外径は 0.5mm 以上。製造時にアニュラリングは 0.15mm に広げられます",
            "100×100mm 以内だと、最も安い料金の枠に入ります",
        ],
        steps: &[
            "jlcpcb.com にログインし、「今すぐ注文」→「ガーバーファイルを追加」で `基板名_gerber.zip` をアップロードします",
            "層数 2・板厚 1.6mm・銅箔 1oz は初期値のままで構いません。プレビューで外形と穴が想定どおりか確認します",
            "部品実装 (SMT) も頼むときは、`-BOM.csv` と `-CPL.csv` をアップロードします。部品は JLCPCB の部品ライブラリの番号 (LCSC番号) で指定します",
            "見積もりを確認して注文します (画面は変更されることがあります)",
        ],
    },
    FabPreset {
        id: "pcbway",
        name: "PCBWay (2層)",
        summary: "線幅・間隔 0.15mm / 穴 0.3mm / アニュラリング 0.15mm",
        min_track_width: 0.15,
        min_clearance: 0.15,
        edge_clearance: 0.3,
        min_drill: 0.3,
        min_annular_ring: 0.15,
        cheap_size: Some((100.0, 100.0)),
        track_width: 0.25,
        via_diameter: 0.6,
        via_drill: 0.3,
        url: "https://www.pcbway.com/capabilities.html",
        notes: &[
            "実際の下限は線幅・間隔 0.1mm、穴 0.2mm ほどですが、料金が上がらない余裕のある値 (0.15mm / 0.3mm) にしています",
            "アニュラリング (穴のまわりの銅の幅) は 0.15mm 以上",
        ],
        steps: &[
            "pcbway.com で「PCB クイック見積もり」を開き、層数 2・板厚 1.6mm などを選びます",
            "「ガーバーファイルを追加」で `基板名_gerber.zip` をアップロードし、プレビューで確認します",
            "部品実装も頼むときは、`-BOM.csv` と `-CPL.csv` を追加します",
            "見積もりを確認して注文します (画面は変更されることがあります)",
        ],
    },
    FabPreset {
        id: "oshpark",
        name: "OSH Park (2層)",
        summary: "線幅・間隔 6mil (0.152mm) / 穴 10mil (0.254mm) / アニュラリング 5mil",
        min_track_width: 0.152,
        min_clearance: 0.152,
        edge_clearance: 0.381,
        min_drill: 0.254,
        min_annular_ring: 0.127,
        cheap_size: None,
        track_width: 0.25,
        via_diameter: 0.6,
        via_drill: 0.3,
        url: "https://docs.oshpark.com/services/",
        notes: &["基板の面積で値段が決まり、同じ基板が3枚届きます", "基板の端から銅までは 15mil (0.381mm) 空けます"],
        steps: &[
            "oshpark.com にログインし、「New Project」で `基板名_gerber.zip` をアップロードします",
            "表示された基板の絵で、外形と穴を確認します",
            "価格を確認して注文します (画面は変更されることがあります)",
        ],
    },
];

pub fn preset(id: &str) -> Option<&'static FabPreset> {
    PRESETS.iter().find(|p| p.id == id)
}

impl FabPreset {
    /// The design rules the DRC should enforce for this fab.
    pub fn apply_rules(&self, rules: &mut Rules) {
        rules.min_track_width = self.min_track_width;
        rules.min_clearance = self.min_clearance;
        rules.edge_clearance = self.edge_clearance;
        rules.min_drill = self.min_drill;
        rules.min_annular_ring = self.min_annular_ring;
    }

    /// True when `rules` are at least as strict as this fab's minimums.
    pub fn covered_by(&self, rules: &Rules) -> bool {
        let eps = 1e-9;
        rules.min_track_width + eps >= self.min_track_width
            && rules.min_clearance + eps >= self.min_clearance
            && rules.edge_clearance + eps >= self.edge_clearance
            && rules.min_drill + eps >= self.min_drill
            && rules.min_annular_ring + eps >= self.min_annular_ring
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Ok,
    /// Good to know, nothing to fix.
    Info,
    /// Can be ordered, but probably not what was meant.
    Warn,
    /// Must be fixed before ordering.
    Error,
}

#[derive(Clone, Debug)]
pub struct Check {
    pub level: Level,
    pub title: String,
    /// Why it matters and how to fix it.
    pub detail: String,
    pub at: Option<Point>,
    /// The other places the same note is about (`at` is the first of them), so all can be marked on the board.
    pub also: Vec<Point>,
}

fn check(level: Level, title: impl Into<String>, detail: impl Into<String>, at: Option<Point>) -> Check {
    Check { level, title: title.into(), detail: detail.into(), at, also: vec![] }
}

impl Check {
    fn with_also(mut self, places: impl IntoIterator<Item = Point>) -> Check {
        let at = self.at;
        self.also = places.into_iter().filter(|p| Some(*p) != at).collect();
        self
    }
}

/// A copper drop above this (V) at a declared current is worth a look before ordering.
const MAX_DROP_V: f64 = 0.1;

/// The check for a board that no longer matches its schematic (`summary` says how). A different net, or a part that is only on one side, makes
/// the ordered board a different circuit from the one simulated, so it blocks; a changed value or footprint only warns.
pub fn schematic_sync_check(summary: &str, blocking: bool) -> Check {
    check(
        if blocking { Level::Error } else { Level::Warn },
        "回路図と基板が合っていません",
        format!("{summary}。回路図を直したあと、基板で「回路図から取り込み」を押していないようです。このまま発注すると、シミュレーションした回路と違う基板が届きます。基板の画面の左パネルで「回路図から取り込み」を押し、必要なら配線をやり直しましょう"),
        None,
    )
}

/// Everything a beginner should look at before sending the files.
pub fn preflight(board: &Board, fab: Option<&FabPreset>) -> Vec<Check> {
    let mut out = vec![];

    // outline
    let area = if board.outline.len() >= 3 { signed_area(&board.outline).abs() } else { 0.0 };
    if area < 1.0 {
        out.push(check(Level::Error, "基板の外形がありません", "基板の形 (外形) が決まっていません。左の「矩形に設定」か、ツール「基板外形」で決めましょう。発注には外形が必要です", None));
        return out;
    }
    let (mut lo, mut hi) = (Point::new(f64::MAX, f64::MAX), Point::new(f64::MIN, f64::MIN));
    for p in &board.outline {
        lo = Point::new(lo.x.min(p.x), lo.y.min(p.y));
        hi = Point::new(hi.x.max(p.x), hi.y.max(p.y));
    }
    out.push(check(Level::Ok, format!("基板の大きさ {:.1} × {:.1} mm", hi.x - lo.x, hi.y - lo.y), "外形が決まっています", None));

    // the presets are for two-layer orders
    if board.inner_layers > 0 {
        let n = board.inner_layers as usize + 2;
        out.push(check(
            Level::Warn,
            format!("{n} 層の基板です (内層 {} 枚)", board.inner_layers),
            format!("書き出した zip には内層のファイル (.G1 …) が入ります。発注の画面で層数を {n} 層にし、層構成 (積層) と板厚を発注先の指定に合わせてください。プリセットの寸法の値は 2 層向けなので、内層のある基板は発注先の「多層」の製造能力で確かめましょう"),
            None,
        ));
    }

    // parts must be on the board
    let outside: Vec<&Part> = board.parts.iter().filter(|p| !inside_polygon(&board.outline, p.pos)).collect();
    if let Some(first) = outside.first() {
        let names: Vec<&str> = outside.iter().take(5).map(|p| p.refdes.as_str()).collect();
        out.push(check(
            Level::Error,
            format!("基板の外に部品があります: {}{}", names.join(", "), if outside.len() > 5 { " ほか" } else { "" }),
            "回路図から取り込んだ部品は、最初は基板の右の外側に並びます。ドラッグして基板の上に置きましょう",
            Some(first.pos),
        ).with_also(outside.iter().map(|p| p.pos)));
    } else if !board.parts.is_empty() {
        out.push(check(Level::Ok, format!("部品 {} 個はすべて基板の上にあります", board.parts.len()), "", None));
    }

    // routing
    let airwires = crate::ratsnest::open_connections(board);
    if let Some(w) = airwires.first() {
        out.push(check(
            Level::Error,
            format!("未配線が {} 本あります (黄色の線)", airwires.len()),
            "つながっていない接続が残っています。「配線」ツールで手でつなぐか、「自動配線」に任せましょう",
            Some(w.a),
        ).with_also(airwires.iter().map(|w| w.a)));
    } else {
        out.push(check(Level::Ok, "すべて配線されています", "", None));
    }

    // design rules, first the board's own, then the fab's
    let own = drc::check_final(board);
    if let Some(v) = own.first() {
        out.push(check(Level::Error, format!("DRC 違反が {} 件あります", own.len()), format!("例: {}。一覧の行をクリックすると場所へ移動します", v.message), Some(v.at)).with_also(own.iter().map(|x| x.at)));
    } else {
        out.push(check(Level::Ok, "DRC (設計ルールチェック) に違反はありません", "", None));
    }
    if let Some(f) = fab {
        if !f.covered_by(&board.rules) {
            let mut strict = board.clone();
            f.apply_rules(&mut strict.rules);
            // rules may also be stricter than the fab in some fields: keep the stricter value
            strict.rules.min_track_width = strict.rules.min_track_width.max(board.rules.min_track_width);
            strict.rules.min_clearance = strict.rules.min_clearance.max(board.rules.min_clearance);
            strict.rules.edge_clearance = strict.rules.edge_clearance.max(board.rules.edge_clearance);
            strict.rules.min_drill = strict.rules.min_drill.max(board.rules.min_drill);
            strict.rules.min_annular_ring = strict.rules.min_annular_ring.max(board.rules.min_annular_ring);
            let v = drc::check_final(&strict);
            if let Some(first) = v.first() {
                out.push(check(
                    Level::Error,
                    format!("{} のルールでは違反が {} 件あります", f.name, v.len()),
                    format!("例: {}。線を太く・離す、穴やパッドを大きくするなどで直します。「ルールを{}にする」を押すと、DRC もこのルールで調べます", first.message, f.name),
                    Some(first.at),
                ).with_also(v.iter().map(|x| x.at)));
            } else {
                out.push(check(Level::Warn, format!("DRC のルールが {} より緩くなっています", f.name), "今の設計は発注先のルールを満たしていますが、DRC の設定が緩いままです。「ルールを発注先に合わせる」を押しておくと、あとで直したときも守られます", None));
            }
        } else {
            out.push(check(Level::Ok, format!("{} の最小値を満たしています", f.name), "", None));
        }
    }

    // track widths against the currents the user declared
    let conn = drc::connectivity(board);
    let widths = crate::current::net_widths(board, &conn, &crate::current::CopperSpec::default());
    let thin: Vec<&crate::current::NetWidth> = widths.iter().filter(|w| w.too_thin()).collect();
    if let Some(first) = thin.first() {
        out.push(check(
            Level::Warn,
            format!("電流に対して配線が細いネットがあります: {}", thin.iter().take(4).map(|w| w.net.as_str()).collect::<Vec<_>>().join(", ")),
            format!(
                "例: {} は {:.1} A を流す設定で、{:.2} mm 以上 (銅箔 35µm・温度上昇 10℃ の目安) が必要ですが、細い所は {:.2} mm です。細い配線は熱を持ち、焼けたり基板が傷んだりします。左の「配線の太さ」で「推奨幅にする」を押しましょう",
                first.net, first.amps, first.needed, first.thinnest.unwrap_or(0.0)
            ),
            first.at,
        ));
    } else if !widths.is_empty() {
        out.push(check(Level::Ok, format!("電流を設定したネット {} 本は、配線の太さが足りています", widths.len()), "", None));
    }

    // track widths against the impedance a net class asks for (a rule of thumb: the maker's own figure is the one to trust)
    let wanted = board.net_classes.iter().filter(|c| c.impedance.is_some() && !c.nets.is_empty()).count();
    if wanted > 0 {
        let off = crate::impedance::check_targets(board, 0.1);
        if let Some(first) = off.first() {
            out.push(
                check(
                    Level::Warn,
                    format!("目標のインピーダンスから外れた配線があります: {}", off.iter().take(4).map(|i| i.net.as_str()).collect::<Vec<_>>().join(", ")),
                    format!(
                        "例: {} は {:.0} Ω を目標にしていますが、{:.2} mm の配線は約 {:.0} Ω (目安) です。ネットの設定の窓の「目標の線幅にする」で、そろえられます。値は教科書の式による ±10% ほどの目安なので、発注先の積層構成と、そこの計算の値を確かめてください",
                        first.net, first.target, first.worst_width, first.worst_z
                    ),
                    Some(first.at),
                )
                .with_also(off.iter().map(|i| i.at)),
            );
        } else {
            out.push(check(Level::Ok, format!("インピーダンスを決めたネット {} 組の配線は、目標に合っています (目安)", wanted), "", None));
        }
    }

    // silk screen
    let silk = drc::silk_checks(board);
    if let Some(first) = silk.first() {
        out.push(check(Level::Warn, format!("シルク (白い印刷) に気になる所が {} 件あります", silk.len()), format!("例: {}", first.message), Some(first.at)).with_also(silk.iter().map(|x| x.at)));
    }

    // solder mask strips between pads
    let dams = drc::mask_dam_checks(board);
    if let Some(first) = dams.first() {
        out.push(check(Level::Warn, format!("ソルダーマスクの帯が細すぎるパッドの間が {} 箇所あります", dams.len()), format!("例: {}", first.message), Some(first.at)).with_also(dams.iter().map(|x| x.at)));
    }

    // narrow gaps inside one net's copper (a thin branch beside a wide track)
    let slits = drc::slit_checks(board);
    if let Some(first) = slits.first() {
        out.push(check(Level::Warn, format!("同じネットの銅箔の間に、最小間隔より狭いすき間が {} 箇所あります", slits.len()), format!("例: {}", first.message), Some(first.at)).with_also(slits.iter().map(|x| x.at)));
    }

    // differential pairs: length and spacing
    let pairs = crate::diffpair::check_pairs(board);
    if let Some(first) = pairs.first() {
        out.push(check(Level::Warn, format!("差動ペアに気になる所が {} 件あります", pairs.len()), format!("例: {}", first.message), Some(first.at)).with_also(pairs.iter().map(|x| x.at)));
    }

    // heat pads without thermal vias
    let thermal = drc::thermal_via_checks(board);
    if let Some(first) = thermal.first() {
        out.push(check(Level::Warn, format!("放熱パッドにサーマルビアが足りない部品が {} 個あります", thermal.len()), format!("例: {}", first.message), Some(first.at)).with_also(thermal.iter().map(|x| x.at)));
    }

    // slivers and islands of the copper pours
    let pours = drc::zone_checks(board);
    if let Some(first) = pours.first() {
        out.push(check(Level::Warn, format!("ベタ (ゾーン) に、気になる所が {} 箇所あります (細いかけら・孤立した島・他のネットで分断されて、つながっていないパッド)", pours.len()), format!("例: {}", first.message), Some(first.at)).with_also(pours.iter().map(|x| x.at)));
    }

    // bypass capacitors next to IC supply pins
    let decoupling = drc::decoupling_checks(board);
    if let Some(first) = decoupling.first() {
        out.push(check(Level::Warn, format!("ICの電源ピンのそばのコンデンサに気になる所が {} 件あります", decoupling.len()), format!("例: {}", first.message), Some(first.at)).with_also(decoupling.iter().map(|x| x.at)));
    }

    // the groups and distances of the schematic's requirements table
    if !board.place_groups.is_empty() || !board.keep_apart.is_empty() || !board.bypass.is_empty() || !board.measures.is_empty() {
        let rep = crate::requirements::check(board);
        let marks = crate::requirements::marks(board);
        let at: Vec<Point> = marks.groups.iter().filter(|g| !g.ok && !g.waived).map(|g| g.at).chain(marks.apart.iter().filter(|a| !a.ok && !a.waived).map(|a| Point::new((a.a.x + a.b.x) / 2.0, (a.a.y + a.b.y) / 2.0))).chain(marks.bypass.iter().filter(|b| !b.ok && !b.waived).map(|b| Point::new((b.a.x + b.b.x) / 2.0, (b.a.y + b.b.y) / 2.0))).collect();
        if let Some(first) = rep.findings.first() {
            out.push(check(Level::Warn, format!("設計の要件を満たしていない所が {} 件あります", rep.findings.len()), format!("例: {}。回路図の「設計の要件」の表にある、近くに置く組と離す組です。部品を動かすか、表の数字を見直してください。発注はできますが、設計の意図から外れています", first.message), at.first().copied()).with_also(at.iter().copied()));
        } else if rep.checked > 0 {
            out.push(check(Level::Ok, format!("設計の要件 {} 件を満たしています", rep.checked), "", None));
        }
        // what the table accepts: not a warning, but the reasons are on the list for whoever orders the board
        if let Some((first, why)) = rep.waived.first() {
            let why = if why.trim().is_empty() { "理由は書かれていません".to_string() } else { why.clone() };
            out.push(check(Level::Info, format!("設計の要件のうち、承知した例外が {} 件あります", rep.waived.len()), format!("例: {} (承知した理由: {})。回路図の「設計の要件」の表の「承知した例外」で、一覧と理由を見られます", first.message, why), None));
        }
    }

    // trace corners
    let acute = drc::acute_angle_checks(board);
    if let Some(first) = acute.first() {
        out.push(check(Level::Warn, format!("鋭角に曲がっている配線が {} 箇所あります", acute.len()), format!("例: {}", first.message), Some(first.at)).with_also(acute.iter().map(|x| x.at)));
    }
    if drc::missing_ground_fill(board) {
        out.push(check(Level::Info, "GND のベタ (銅箔ゾーン) がありません", "2層基板では、裏面などを GND のゾーンで塗ると、信号の戻り道が短くなってノイズが減り、配線もしやすくなります。ツール「ゾーン」でネット GND を選んで囲みましょう", None));
    }

    // the price tier of the fab
    if let Some((cw, ch)) = fab.and_then(|f| f.cheap_size) {
        let (w, h) = (hi.x - lo.x, hi.y - lo.y);
        let fits = (w <= cw && h <= ch) || (w <= ch && h <= cw);
        let name = fab.map_or("", |f| f.name);
        if fits {
            out.push(check(Level::Ok, format!("{name} の最も安い料金の枠 ({cw:.0}×{ch:.0} mm 以内) に入っています"), "", None));
        } else {
            out.push(check(Level::Info, format!("基板が {cw:.0}×{ch:.0} mm より大きいので、{name} では料金の枠が上がります"), format!("いまの大きさは {w:.1}×{h:.1} mm です。{cw:.0}×{ch:.0} mm に収まるように部品を詰めるか、外形を小さくすると安くなります。大きいままでよければ、このまま進めて構いません"), None));
        }
    }

    // the copper's voltage drop, for the nets whose current was declared
    let drops = crate::irdrop::net_drops(board, &crate::irdrop::IrSpec::default());
    let bad: Vec<(&crate::irdrop::NetDrop, &crate::irdrop::PadDrop)> = drops.iter().filter_map(|d| d.worst().map(|w| (d, w))).filter(|(_, w)| w.drop_v.is_some_and(|v| v > MAX_DROP_V)).collect();
    if let Some((d, w)) = bad.first() {
        out.push(check(
            Level::Warn,
            format!("配線の電圧降下が大きいネットがあります: {}", bad.iter().take(4).map(|(d, _)| d.net.as_str()).collect::<Vec<_>>().join(", ")),
            format!(
                "例: {} は {:.1} A を流すと、{} の {} ピンで {:.2} V 落ちます (目安 {} V 以下)。電源が足りなくなり、動作が不安定になります。配線を太く・短くするか、ベタ (ゾーン) で塗りましょう。左の「配線の太さ」で確かめられます",
                d.net, d.amps, w.refdes, w.pin, w.drop_v.unwrap_or(0.0), MAX_DROP_V
            ),
            Some(w.pos),
        ));
    } else if !drops.is_empty() && drops.iter().all(|d| d.worst().is_some()) {
        out.push(check(Level::Ok, format!("配線の電圧降下は、どのネットも {MAX_DROP_V} V 以下です"), "", None));
    }

    // BOM
    let blank: Vec<&str> = board.parts.iter().filter(|p| p.value.trim().is_empty()).map(|p| p.refdes.as_str()).collect();
    if !blank.is_empty() {
        out.push(check(Level::Warn, format!("値が空の部品があります: {}", blank.iter().take(5).copied().collect::<Vec<_>>().join(", ")), "部品表 (BOM) の「値」が空になります。回路図で部品の値を入れましょう", None));
    }

    // land patterns that are an estimate
    let guessed: Vec<String> = board.parts.iter().filter_map(|p| board.land_unverified.get(&p.footprint).map(|n| format!("{} ({n})", p.refdes))).collect();
    if !guessed.is_empty() {
        out.push(check(
            Level::Warn,
            format!("ランドパターンが概算の部品があります: {}", guessed.iter().take(5).cloned().collect::<Vec<_>>().join(", ")),
            "部品ライブラリの寸法が、データシートの推奨ランドパターンから読み取ったものではありません。発注の前に、データシートの図面とパッドの位置・大きさ・ピン番号を照らし合わせてください。確認できたら、部品ファイルの land_unverified を消します",
            None,
        ));
    }

    // holes
    let mut smallest = f64::MAX;
    for p in &board.pads {
        if let Some(d) = p.drill {
            smallest = smallest.min(d);
        }
    }
    for v in &board.vias {
        smallest = smallest.min(v.drill);
    }
    if smallest < f64::MAX {
        out.push(check(Level::Info, format!("一番小さい穴は直径 {:.2} mm", smallest), "穴が小さいほど加工が難しく、値段が上がることがあります", None));
    }
    if board.holes.is_empty() {
        out.push(check(Level::Info, "取付穴はありません", "ケースにネジ止めするなら、取付穴 (ツール「取付穴」) を付けておきましょう", None));
    }
    out
}

/// True when nothing in `checks` blocks an order.
pub fn ready(checks: &[Check]) -> bool {
    !checks.iter().any(|c| c.level == Level::Error)
}
