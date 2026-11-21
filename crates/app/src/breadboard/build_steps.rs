//! "組む順と通電" (candidate B5d): the stripboard or perfboard built one section at a time (`sch_core::build_order`), and for each
//! step what a working board shows once it is powered with only the parts fitted so far: each supply's current and the limit to
//! set (`sim_core::power_on` on the built part), and the voltages to measure on the nets the step brought in. Worked out on
//! another thread (a power-on run per step); a step picked in the list lights its parts on the board and dims the ones not yet on.

use super::*;
use sch_core::build_order::{build_steps, built_so_far, rail_volts, supply_inputs, BuildStep};
use sch_core::stripboard::StripLayout;
use std::sync::mpsc::Receiver;

/// What a working board shows after one step, powered.
#[derive(Clone, Debug, Default)]
pub struct StepCheck {
    /// Each supply: its name, volts, the current with the parts at their printed values, the range a working board falls in, the
    /// limit to set (A), and whether it was put in by the name of a net that reaches a connector (no source on the sheet).
    pub supplies: Vec<(String, f64, f64, f64, f64, f64, bool)>,
    /// The voltage on each net the step brought in (V).
    pub volts: Vec<(String, f64)>,
}

type Steps = Vec<(BuildStep, Result<StepCheck, String>)>;

#[derive(Default)]
pub struct BuildState {
    /// The list shown, and the schematic's edit count and the layout it was made for.
    result: Option<(u64, StripLayout, Steps)>,
    job: Option<(u64, StripLayout, Receiver<Steps>)>,
    /// The step picked: its parts lit on the board, the later steps' parts dimmed.
    pub step: Option<usize>,
    /// The window is open.
    pub open: bool,
}

/// The steps and their checks (on another thread).
pub fn work_out(s: &sch_core::Schematic, l: &StripLayout) -> Steps {
    let steps = build_steps(s, l);
    let feeds: Vec<(String, f64)> = supply_inputs(s, l).into_iter().filter_map(|n| rail_volts(&n).map(|v| (n, v))).collect();
    (0..steps.len()).map(|k| (steps[k].clone(), check_step(s, l, &steps, k, &feeds))).collect()
}

/// The board built up to step `k`, powered: from its own supplies, else from the supply-named nets that reach a connector.
fn check_step(s: &sch_core::Schematic, l: &StripLayout, steps: &[BuildStep], k: usize, feeds: &[(String, f64)]) -> Result<StepCheck, String> {
    let built = built_so_far(s, l, steps, k);
    let mut c = sim_core::from_schematic(&built).map_err(|e| e.first().map(|e| e.to_string()).unwrap_or_default())?;
    let mut fed = vec![];
    if sim_core::supply_sources(&c).is_empty() {
        for (net, v) in feeds {
            if !c.names.iter().any(|n| n.eq_ignore_ascii_case(net)) {
                continue;
            }
            let name = format!("V_{net}");
            let p = c.node(net);
            c.elements.push(sim_core::Element::VSource { name: name.clone(), p, n: 0, wave: sim_core::Waveform::Dc(*v), ac: None });
            fed.push(name);
        }
    }
    if sim_core::supply_sources(&c).is_empty() {
        return Err(i18n::tr("電源がありません (回路図に電源も、コネクタに来る 5V・3V3 のような名前の電源の線もありません)").to_string());
    }
    let p = sim_core::power_on(&c, &[], &sim_core::PowerOnSettings::default()).map_err(|e| e.to_string())?;
    let r = sim_core::dc_op(&c).map_err(|e| e.to_string())?;
    Ok(StepCheck {
        supplies: p.supplies.iter().map(|x| (x.name.clone(), x.volts, x.nominal, x.min, x.max, x.limit, fed.contains(&x.name))).collect(),
        volts: measure_points(&steps[k].nets, |n| r.voltage(n)),
    })
}

/// The voltages worth reading after a step: the supplies first, then the other nets with a voltage on them (a net at about 0 V
/// with nothing driving it yet is left out), at most `MEASURE_MAX`.
fn measure_points(nets: &[String], volts: impl Fn(&str) -> Option<f64>) -> Vec<(String, f64)> {
    let mut rails: Vec<(String, f64)> = vec![];
    let mut others: Vec<(String, f64)> = vec![];
    for n in nets {
        let Some(v) = volts(n) else { continue };
        if sch_core::section_plan::supply_name(n) {
            rails.push((n.clone(), v));
        } else if v.abs() > 0.01 {
            others.push((n.clone(), v));
        }
    }
    rails.extend(others);
    rails.truncate(MEASURE_MAX);
    rails
}

const MEASURE_MAX: usize = 8;

/// A supply's current as read on a bench supply's display: below a microamp (the parts drawn have no model of their own current)
/// it is "about 0".
fn amps_or_none(a: f64) -> String {
    if a.abs() < 1e-6 {
        i18n::tr("ほぼ 0").to_string()
    } else {
        amps(a)
    }
}

fn amps(a: f64) -> String {
    format!("{}A", sim_core::format_value(a))
}

/// ①②③…
fn circled(k: usize) -> String {
    char::from_u32(0x2460 + k as u32).filter(|_| k < 20).map(String::from).unwrap_or_else(|| format!("({})", k + 1))
}

impl BreadboardView {
    /// The steps for the layout shown (none until worked out): started on another thread when the schematic or the layout changed.
    fn build_result(&mut self, sch: &SchEditor, l: &StripLayout) -> Option<&Steps> {
        let fresh = |r: u64, k: &StripLayout| r == sch.rev && k == l && sch.settled();
        if !self.build.result.as_ref().is_some_and(|(r, k, _)| fresh(*r, k)) {
            if let Some((r, k, rx)) = &self.build.job {
                match rx.try_recv() {
                    Ok(steps) => {
                        let (r, k) = (*r, k.clone());
                        self.build.job = None;
                        self.build.result = Some((r, k, steps));
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => self.build.job = None,
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
            }
            if !self.build.job.as_ref().is_some_and(|(r, k, _)| fresh(*r, k)) && !self.build.result.as_ref().is_some_and(|(r, k, _)| fresh(*r, k)) {
                let (tx, rx) = std::sync::mpsc::channel();
                let (s, lay) = (sch.sch.clone(), l.clone());
                std::thread::spawn(move || {
                    let _ = tx.send(work_out(&s, &lay));
                });
                self.build.job = Some((sch.rev, l.clone(), rx));
                // the tests wait for it
                if cfg!(test) {
                    let got = self.build.job.as_ref().and_then(|(_, _, rx)| rx.recv_timeout(std::time::Duration::from_secs(120)).ok());
                    if let Some(steps) = got {
                        self.build.job = None;
                        self.build.result = Some((sch.rev, l.clone(), steps));
                    }
                }
            }
        }
        self.build.result.as_ref().filter(|(r, k, _)| fresh(*r, k)).map(|(_, _, s)| s)
    }

    /// The button for "組む順と通電" and its window (the panel has no room left for a list this long).
    pub(super) fn build_ui(&mut self, ui: &mut egui::Ui, sch: &SchEditor, l: &StripLayout) {
        if ui
            .selectable_label(self.build.open, i18n::tr("組む順と通電 (区画ごと)"))
            .on_hover_text(i18n::tr("区画を 1 つずつ組んで、そのたびに電源を入れて確かめる順と、各段で正常なら流れる電流・電流制限・測る電圧を、窓に出します"))
            .clicked()
        {
            self.build.open = !self.build.open;
        }
        if !self.build.open {
            self.build.step = None;
            return;
        }
        let mut open = true;
        let ctx = ui.ctx().clone();
        egui::Window::new(i18n::tr("組む順と通電")).id(egui::Id::new("strip_build_steps")).open(&mut open).default_pos(crate::widgets::beside_panel()).default_width(460.0).default_height(560.0).vscroll(true).show(&ctx, |ui| self.build_contents(ui, sch, l));
        if !open {
            self.build.open = false;
            self.build.step = None;
        }
    }

    /// Being worked out on another thread (the screenshots wait for it).
    pub fn build_busy(&self) -> bool {
        self.build.open && self.build.job.is_some()
    }

    /// The steps, each with its parts in the order to solder them and what to read once it is powered.
    fn build_contents(&mut self, ui: &mut egui::Ui, sch: &SchEditor, l: &StripLayout) {
        {
            ui.label(spaced("区画を 1 つずつ組んで、そのたびに電源を入れて確かめます。おかしな所があっても、いま付けた区画の数個の部品を見れば済みます。電源の入口の区画から順に並べ、区画の中は背の低い部品から (コネクタ・リレーは最後)。電源を入れる前に、テスターの抵抗レンジで電源の線と GND がショートしていないことを確かめ、電流を制限できる電源で入れます。").color(NOTE));
            let Some(steps) = self.build_result(sch, l).cloned() else {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(i18n::tr("組む順と、各段で流れる電流を計算しています…"));
                });
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                return;
            };
            for (k, (st, check)) in steps.iter().enumerate() {
                ui.add_space(4.0);
                let picked = self.build.step == Some(k);
                let title = i18n::trf!("{} {} (部品 {} 個・表の線 {} 本)", circled(k), i18n::tr_dyn(&st.name), st.parts.len(), st.links.len());
                if ui
                    .selectable_label(picked, egui::RichText::new(title).strong())
                    .on_hover_text(i18n::tr("選ぶと、基板の上でこの段の部品が光り、まだ付けない段の部品が薄くなります。もう一度押すと戻ります"))
                    .clicked()
                {
                    self.build.step = if picked { None } else { Some(k) };
                }
                let mut fit = vec![];
                if !st.links.is_empty() {
                    fit.push(i18n::trf!("表の線 {} 本", st.links.len()));
                }
                fit.extend(st.parts.iter().map(|r| match l.parts.iter().find(|p| &p.refdes == r).map(|p| &p.mount) {
                    Some(sch_core::mount::Mount::Socket { .. }) => i18n::trf!("{} (ソケット)", r),
                    Some(sch_core::mount::Mount::Module { .. }) => i18n::trf!("{} (ピンヘッダ)", r),
                    _ => r.clone(),
                }));
                ui.add(egui::Label::new(egui::RichText::new(i18n::trf!("付ける順: {}", fit.join(" → "))).small()).wrap());
                match check {
                    Ok(c) => {
                        for (name, volts, nominal, min, max, limit, fed) in &c.supplies {
                            let from = if *fed { i18n::trf!("{} (コネクタから入れる)", name.trim_start_matches("V_")) } else { name.clone() };
                            let text = if nominal.abs() < 1e-6 {
                                i18n::trf!("通電: {} {}V で ほぼ 0 (電流を流す部品のモデルが無い)。電流制限は {}", from, sim_core::format_value(*volts), amps(*limit))
                            } else {
                                // the range only when the tolerances move it (a part's own current fixed by its model alone does not)
                                let spread = (max - min) > 0.01 * nominal.abs();
                                let range = if spread { format!(" ({}〜{})", amps_or_none(*min), amps_or_none(*max)) } else { String::new() };
                                i18n::trf!("通電: {} {}V で {}{}。電流制限は {}", from, sim_core::format_value(*volts), amps_or_none(*nominal), range, amps(*limit))
                            };
                            ui.label(text)
                                .on_hover_text(i18n::tr("この段までの部品だけを付けて電源を入れたとき、正常なら流れる電流です (抵抗などの許容差とデータシートの min / max の範囲)。電流が範囲を外れたら、この段で付けた部品の向き・値・はんだのブリッジを見ます。モデルの無い部品の電流は入っていません。点滅・発振する回路は、直流の動作点 (止まった状態) の値です。電源の電流制限は、この値にしておくと、ショートしても部品を壊しません"));
                        }
                        if !c.volts.is_empty() {
                            let list: Vec<String> = c.volts.iter().map(|(n, v)| format!("{n} {}V", sim_core::format_value(*v))).collect();
                            ui.add(egui::Label::new(i18n::trf!("測る (GND から): {}", list.join("、"))).wrap()).on_hover_text(i18n::tr("この段で初めてつながるネットの電圧です (GND に黒い棒を当てて)。まだ付けていない段の部品は無いものとして計算しています"));
                        }
                    }
                    Err(e) => {
                        ui.colored_label(crate::theme::WARN, i18n::trf!("この段の通電は計算できませんでした: {}", i18n::tr_dyn(e)));
                    }
                }
            }
        }
    }

    /// The step picked: (its parts, the parts of the later steps), for the board to light and dim.
    pub(super) fn build_marks(&self) -> Option<(BTreeSet<String>, BTreeSet<String>)> {
        let k = self.build.step?;
        let (_, _, steps) = self.build.result.as_ref()?;
        let now = steps.get(k)?.0.parts.iter().cloned().collect();
        let later = steps.iter().skip(k + 1).flat_map(|(st, _)| st.parts.iter().cloned()).collect();
        Some((now, later))
    }
}

/// Lights the picked step's parts and dims the later steps' ones (`to`: a hole's place on screen, front or back).
pub(super) fn draw_build_marks(painter: &egui::Painter, l: &StripLayout, marks: &(BTreeSet<String>, BTreeSet<String>), to: impl Fn(sch_core::stripboard::At) -> Pos2, pitch: f32) {
    for p in &l.parts {
        let pts: Vec<Pos2> = p.leads.iter().map(|(_, a)| to(*a)).collect();
        if pts.is_empty() {
            continue;
        }
        let r = Rect::from_points(&pts).expand(pitch * 0.5);
        if marks.1.contains(&p.refdes) {
            painter.rect_filled(r, 2.0, super::strip_view::BOARD.gamma_multiply(0.8));
        } else if marks.0.contains(&p.refdes) {
            painter.rect_stroke(r, 3.0, Stroke::new(2.5, crate::theme::OK), egui::StrokeKind::Outside);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn load(name: &str) -> sch_core::Schematic {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/examples").join(name);
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn sections(s: &mut sch_core::Schematic, groups: &[(&str, &[&str])]) {
        s.requirements.groups = groups.iter().map(|(name, parts)| design_spec::PlaceGroup { name: name.to_string(), parts: parts.iter().map(|p| p.to_string()).collect(), section: true, ..Default::default() }).collect();
    }

    /// Three LEDs, each on its own supply, in two sections: after the first only V1 gives current; once the second is on, V2 does
    /// too. The 555 blinker's timer (it takes the supply) comes before the LED's section, its supply's own voltage measured.
    #[test]
    fn each_step_is_powered_with_the_parts_fitted_so_far() {
        let mut s = load("led_basics.schjson");
        sections(&mut s, &[("赤", &["R1", "D1"]), ("ほか", &["R2", "D2", "R3", "D3"])]);
        let steps = work_out(&s, &sch_core::stripboard::strip_layout(&s));
        let current = |k: usize, v: &str| steps[k].1.as_ref().unwrap().supplies.iter().find(|x| x.0 == v).map_or(0.0, |x| x.2);
        assert!(current(0, "V1") > 1e-3 && current(0, "V2") < 1e-6, "only the first LED is on: {:?}", steps[0].1);
        assert!(current(1, "V2") > 1e-3, "the second is on now: {:?}", steps[1].1);
        let mut s = load("ne555_blinker.schjson");
        sections(&mut s, &[("表示", &["R3", "D1"]), ("タイマ", &["U1", "R1", "R2", "C1", "C2"])]);
        let steps = work_out(&s, &sch_core::stripboard::strip_layout(&s));
        assert_eq!(steps.iter().map(|(st, _)| st.name.as_str()).collect::<Vec<_>>(), ["タイマ", "表示"]);
        let first = steps[0].1.as_ref().unwrap();
        assert!(first.volts.iter().any(|(n, v)| n == "VCC" && (*v - 5.0).abs() < 0.01), "{:?}", first.volts);
        assert!(!first.supplies[0].6, "the sheet's own supply, not one put in");
    }

    #[test]
    fn a_net_with_nothing_driving_it_is_not_a_place_to_measure() {
        let nets: Vec<String> = ["5V", "D22", "OUT", "N$3"].iter().map(|n| n.to_string()).collect();
        let v = |n: &str| Some(match n { "5V" => 5.0, "D22" => 1e-9, _ => 2.5 });
        assert_eq!(measure_points(&nets, v).into_iter().map(|x| x.0).collect::<Vec<_>>(), ["5V", "OUT", "N$3"]);
    }
}
