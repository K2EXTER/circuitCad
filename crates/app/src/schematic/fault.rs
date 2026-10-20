//! Fault simulation panel: break one part and see what changes at DC.

use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use sim_core::{assess_safety, chain_failure, dc_op, Danger, break_part, break_pin, breakable, describe_fault, fault_applicable, simulate_broken, simulate_broken_with, simulate_broken_ac, simulate_broken_tran, AcFaultReport, Broken, Fault, FaultReport, PinFaultSpec, PinHow, TranFaultReport};

/// The last fault run.
pub(super) struct FaultView {
    pub(super) report: Result<FaultReport, Vec<String>>,
}

/// How a fault is compared: at DC (voltages), in time (waveform) or in frequency (gain).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FaultDomain {
    Dc,
    Tran,
    Ac,
}

pub(super) enum FaultWave {
    Tran(TranFaultReport),
    Ac(AcFaultReport),
}

/// The last waveform fault run (time or frequency domain): the node's waveform healthy and broken, drawn on top of each other.
pub(super) struct FaultWaveView {
    pub(super) report: Result<FaultWave, Vec<String>>,
    plot: PlotState,
}

/// A scan running on another thread: how far it is, how to stop it, and where the rows will come from (`None` = cancelled).
pub(super) struct ScanRun {
    pub(super) done: Arc<AtomicUsize>,
    pub(super) total: usize,
    pub(super) cancel: Arc<AtomicBool>,
    rx: std::sync::mpsc::Receiver<Option<Vec<ScanRow>>>,
}

/// The work of a scan, as owned data: the healthy circuit, the broken ones, and the pin faults to build and solve.
struct ScanPlan {
    c: Circuit,
    sch: Schematic,
    temp: f64,
    jobs: Vec<(String, usize, Broken)>,
    pin_jobs: Vec<(String, String, String)>,
}

impl ScanPlan {
    /// Solves every fault on all the cores, in the order given (so the rows do not depend on which thread finished first). `done` counts the
    /// faults finished; once `cancel` is set no new ones are started and the result is `None`.
    fn run(self, done: &AtomicUsize, cancel: &AtomicBool) -> Option<Vec<ScanRow>> {
        let ScanPlan { c, sch, temp, jobs, pin_jobs } = self;
        let rate = |r: &FaultReport| {
            let severity = match &r.faulted {
                Err(_) => f64::INFINITY,
                Ok(_) => r.node_changes.iter().map(|(_, a, b)| (b - a).abs()).fold(0.0, f64::max),
            };
            (severity, assess_safety(&sch, r).level)
        };
        let mut rows = vec![];
        // the healthy circuit is solved once for the whole scan (it used to be solved again for every fault); the broken circuits are
        // independent of each other, so they are solved on all the cores
        let baseline = dc_op(&c);
        let solved = sim_core::par_map_progress(
            &jobs,
            |(_, _, b)| {
                match &baseline {
                    Ok(base) => simulate_broken_with(&c, b, base),
                    Err(_) => simulate_broken(&c, b),
                }
                .ok()
                .map(|r| rate(&r))
            },
            done,
            cancel,
        );
        for ((part, kind, b), result) in jobs.into_iter().zip(solved) {
            if let Some(Some((severity, danger))) = result {
                rows.push(ScanRow { part, pin: String::new(), fault_kind: kind, label: b.label, severity, danger });
            }
        }
        // every pin fault has the same healthy circuit: solve it once, from the first pin fault that can be built
        let pin_base = pin_jobs.iter().find_map(|(part, pin, label)| {
            let spec = PinFaultSpec { refdes: part.clone(), pin: pin.clone(), how: PinHow::Open };
            break_pin(&sch, temp, &spec, label).ok().and_then(|(healthy, _)| dc_op(&healthy).ok())
        });
        if let Some(base) = &pin_base {
            let solved = sim_core::par_map_progress(
                &pin_jobs,
                |(part, pin, label)| {
                    let spec = PinFaultSpec { refdes: part.clone(), pin: pin.clone(), how: PinHow::Open };
                    let (healthy, b) = break_pin(&sch, temp, &spec, label).ok()?;
                    simulate_broken_with(&healthy, &b, base).ok().map(|r| rate(&r))
                },
                done,
                cancel,
            );
            for ((part, pin, label), result) in pin_jobs.into_iter().zip(solved) {
                if let Some(Some((severity, danger))) = result {
                    rows.push(ScanRow { part, pin, fault_kind: 0, label, severity, danger });
                }
            }
        }
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        rows.sort_by(|a, b| b.danger.cmp(&a.danger).then(b.severity.total_cmp(&a.severity)));
        Some(rows)
    }
}

/// One line of the "break every part" table.
pub(super) struct ScanRow {
    pub(super) part: String,
    /// The pin broken ("" = the whole part).
    pub(super) pin: String,
    /// Index into `ALL_FAULTS` for whole-part rows.
    fault_kind: usize,
    pub(super) label: String,
    /// Largest voltage change at any node (V); `f64::INFINITY` when the broken circuit cannot be solved.
    pub(super) severity: f64,
    /// Whether a part ends up past its rating (see `sim_core::assess_safety`).
    pub(super) danger: Danger,
}

/// Every way to break a whole part; the panel offers the ones that fit the part.
const ALL_FAULTS: [Fault; 10] = [Fault::Open, Fault::Short, Fault::Scale(10.0), Fault::Scale(0.1), Fault::Reverse, Fault::Scale(0.5), Fault::Scale(1.5), Fault::TurnShort(0.1), Fault::TurnShort(0.2), Fault::TurnShort(0.5)];

impl SchEditor {
    pub fn set_fault(&mut self, part: &str, kind: usize) {
        self.fault_part = part.into();
        self.fault_kind = kind.min(ALL_FAULTS.len() - 1);
    }

    pub fn analyze_fault(&mut self) {
        self.run_fault();
    }

    pub fn analyze_fault_chain(&mut self) {
        self.run_fault_chain();
    }

    pub fn analyze_fault_scan(&mut self) {
        self.run_fault_scan();
    }

    /// The node to watch: the one chosen, else a likely output (OUT / VOUT / VDC ...), else the first net.
    pub(super) fn fault_watch_node(&self) -> Option<String> {
        if !self.fault_node.trim().is_empty() {
            return Some(self.fault_node.trim().to_string());
        }
        let nets: Vec<String> = extract(&self.sch).into_iter().map(|n| n.name).filter(|n| !n.contains('.') && !n.eq_ignore_ascii_case("GND")).collect();
        ["OUT", "VOUT", "VDC", "OUTPUT"].iter().find_map(|w| nets.iter().find(|n| n.eq_ignore_ascii_case(w)).cloned()).or_else(|| nets.first().cloned())
    }

    pub fn set_fault_tran(&mut self, node: &str) {
        self.fault_domain = FaultDomain::Tran;
        self.fault_node = node.into();
    }

    pub fn set_fault_ac(&mut self, node: &str) {
        self.fault_domain = FaultDomain::Ac;
        self.fault_node = node.into();
    }

    /// The temperature to simulate at (the panel's "回路の温度").
    fn fault_temp(&self) -> Result<f64, Vec<String>> {
        match self.temp_c.trim() {
            "" => Ok(T_NOM_C),
            t => t.parse::<f64>().ok().filter(|v| *v > -273.0 && *v < 1000.0).ok_or_else(|| vec![format!("温度「{t}」を数値(℃)で入力してください (例: 27)")]),
        }
    }

    /// The healthy circuit and the broken one for what the panel has selected (a whole part, or one pin).
    pub(super) fn fault_target(&self) -> Result<(Circuit, Broken, Option<Fault>), Vec<String>> {
        let part = self.fault_part.trim();
        if part.is_empty() {
            return Err(vec!["壊す部品を選んでください".into()]);
        }
        if !self.fault_pin.is_empty() {
            let how = match self.fault_pin_how {
                0 => PinHow::Open,
                1 => PinHow::Ground,
                _ => {
                    if self.fault_bridge.trim().is_empty() {
                        return Err(vec!["ブリッジでつなぐ相手のネットを選んでください".into()]);
                    }
                    PinHow::Join(self.fault_bridge.trim().to_string())
                }
            };
            let label = self.pin_fault_label(part, &self.fault_pin, &how);
            let spec = PinFaultSpec { refdes: part.to_string(), pin: self.fault_pin.clone(), how };
            let (healthy, b) = break_pin(&self.sch, self.fault_temp()?, &spec, &label).map_err(|e| vec![e.to_string()])?;
            return Ok((healthy, b, None));
        }
        let fault = ALL_FAULTS[self.fault_kind.min(ALL_FAULTS.len() - 1)];
        let c = self.sim_circuit()?;
        let b = break_part(&c, part, fault).ok_or_else(|| vec![format!("{part} には「{}」の故障を設定できません。ほかの壊し方を選ぶか、「ピン」で壊してください", fault.label())])?;
        Ok((c, b, Some(fault)))
    }

    /// "ピン 2 (OUT) を断線": the pin as the part's symbol names it, and what is done to it.
    pub(super) fn pin_fault_label(&self, part: &str, pin: &str, how: &PinHow) -> String {
        let name = self.pin_choices(part).into_iter().find(|(n, _)| n == pin).map_or_else(|| pin.to_string(), |(_, d)| d);
        match how {
            PinHow::Open => format!("ピン {name} を断線"),
            PinHow::Ground => format!("ピン {name} を GND にショート"),
            PinHow::Join(net) => format!("ピン {name} をネット {net} とブリッジ"),
        }
    }

    /// The pins of a part as (id used in the netlist, label shown): "2" / "2 IN-", "C" ...
    pub(super) fn pin_choices(&self, refdes: &str) -> Vec<(String, String)> {
        let Some(c) = self.sch.components.iter().find(|c| c.refdes == refdes) else { return vec![] };
        c.pins()
            .into_iter()
            .map(|(n, _)| {
                let label = match &c.custom {
                    Some(sym) => sym.pins.iter().find(|p| p.number == n).map_or(n.clone(), |p| if p.name.is_empty() || p.name == n { n.clone() } else { format!("{n} {}", p.name) }),
                    None => n.clone(),
                };
                (n, label)
            })
            .collect()
    }

    /// The net a pin sits on.
    fn pin_net(&self, refdes: &str, pin: &str) -> Option<String> {
        extract(&self.sch).into_iter().find(|n| n.pins.iter().any(|p| p.refdes == refdes && p.pin == pin)).map(|n| n.name)
    }

    pub(super) fn run_fault_wave(&mut self) {
        let ac = self.fault_domain == FaultDomain::Ac;
        let report = (|| {
            let node = self.fault_watch_node().ok_or_else(|| vec!["見る点 (ノード) がありません。回路に名前のついた点 (ラベルなど) を付けてください".to_string()])?;
            let (c, b, _) = self.fault_target()?;
            if ac {
                let (Some(f0), Some(f1), Some(ppd)) = (parse_value(&self.ac_fstart), parse_value(&self.ac_fstop), parse_value(&self.ac_ppd)) else {
                    return Err(vec!["AC解析の開始・終了周波数と点数を数値で入力してください (例: 10, 1Meg, 20)".to_string()]);
                };
                let set = AcSettings { fstart: f0, fstop: f1, sweep: AcSweep::Decade(ppd.max(0.0) as usize) };
                simulate_broken_ac(&c, &b, &set, &node).map(FaultWave::Ac).map_err(|e| vec![e.to_string()])
            } else {
                let Some((tstop, tstep)) = self.tran_times() else {
                    return Err(vec!["過渡解析の終了時間と刻みを数値で入力してください (例: 10m, 10u)".to_string()]);
                };
                simulate_broken_tran(&c, &b, &TranSettings { tstop, tstep, uic: self.tran_uic }, &node).map(FaultWave::Tran).map_err(|e| vec![e.to_string()])
            }
        })();
        self.status = match &report {
            Ok(FaultWave::Tran(r)) => format!("故障シミュレーション (過渡): {} を「{}」", r.element, r.label),
            Ok(FaultWave::Ac(r)) => format!("故障シミュレーション (AC): {} を「{}」", r.element, r.label),
            Err(e) => format!("故障シミュレーションに失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        let plot = if ac {
            let (f0, f1) = (parse_value(&self.ac_fstart).unwrap_or(10.0), parse_value(&self.ac_fstop).unwrap_or(1e6));
            PlotState::log(f0.max(1e-30), f1.max(f0 * 1.001))
        } else {
            PlotState::new(parse_value(&self.tran_tstop).unwrap_or(1.0))
        };
        self.fault_wave = Some(FaultWaveView { report, plot });
        self.wave_mode = WaveMode::Fault;
    }

    pub(super) fn fault_wave_panel(&mut self, ui: &mut egui::Ui) {
        let Some(view) = self.fault_wave.as_mut() else { return };
        let mut close = false;
        let FaultWaveView { report, plot: st } = view;
        ui.horizontal(|ui| {
            ui.strong(match report {
                Ok(FaultWave::Ac(_)) => "故障シミュレーション (AC): 周波数特性の比較",
                _ => "故障シミュレーション (過渡): 正常と故障の比較",
            });
            if ui.button("閉じる").clicked() {
                close = true;
            }
            ui.small("白い線 = 正常な回路 / 色つきの線 = 部品を壊した回路。クリックで測定カーソル");
        });
        match report {
            Err(msgs) => {
                for m in msgs {
                    ui.colored_label(Color32::from_rgb(230, 60, 60), m);
                }
            }
            Ok(r) => {
                let (lines, window) = match r {
                    FaultWave::Tran(t) => (t.summary(), Some(t.window)),
                    FaultWave::Ac(a) => (a.summary(), None),
                };
                ui.horizontal_top(|ui| {
                    ui.vertical(|ui| {
                        ui.set_width(300.0);
                        for (k, l) in lines.iter().enumerate() {
                            if k == 0 {
                                ui.strong(l);
                            } else {
                                ui.label(l);
                            }
                        }
                        if let Some(w) = window {
                            ui.small(format!("測った範囲: {} 〜 {}", eng(w.0, "s"), eng(w.1, "s")));
                        }
                    });
                    let (rect, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
                    match r {
                        FaultWave::Tran(r) => {
                            let (h_name, f_name) = (format!("V({}) 正常", r.node), format!("V({}) {}", r.node, r.label));
                            let mut traces = vec![Trace { name: &h_name, color: Color32::from_gray(225), t: &r.healthy_time, v: &r.healthy }];
                            if let Ok((t, v)) = &r.faulted {
                                traces.push(Trace { name: &f_name, color: PALETTE[0], t, v });
                            }
                            let style = Style { unit: "V", x_unit: "s", x_name: "t", plain_y: false };
                            plot::draw_group(ui, rect, egui::Id::new("fault_plot"), &traces, st, 0, &style, true);
                        }
                        FaultWave::Ac(r) => {
                            let (h_name, f_name) = (format!("V({}) 正常", r.node), format!("V({}) {}", r.node, r.label));
                            let mut traces = vec![Trace { name: &h_name, color: Color32::from_gray(225), t: &r.freq, v: &r.healthy_db }];
                            if let Ok(v) = &r.faulted_db {
                                traces.push(Trace { name: &f_name, color: PALETTE[0], t: &r.freq, v });
                            }
                            let style = Style { unit: " dB", x_unit: "Hz", x_name: "f", plain_y: true };
                            plot::draw_group(ui, rect, egui::Id::new("fault_plot_ac"), &traces, st, 0, &style, true);
                        }
                    }
                });
            }
        }
        if close {
            self.fault_wave = None;
        }
    }

    pub(super) fn run_fault(&mut self) {
        if self.fault_domain != FaultDomain::Dc {
            self.run_fault_wave();
            return;
        }
        let report = self.fault_target().and_then(|(c, b, fault)| {
            simulate_broken(&c, &b).map(|mut r| {
                r.fault = fault;
                r
            }).map_err(|e| vec![e.to_string()])
        });
        self.status = match &report {
            Ok(r) => format!("故障シミュレーション: {} を「{}」", r.element, r.label),
            Err(e) => format!("故障シミュレーションに失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        self.fault_view = Some(FaultView { report });
    }

    /// Breaks the chosen part, then lets every part that goes past its rating fail in turn (transient run).
    pub(super) fn run_fault_chain(&mut self) {
        let report = (|| {
            if !self.fault_pin.is_empty() {
                return Err(vec!["連鎖故障は、部品全体の故障で調べます。「壊し方」を「部品全体」にしてください".to_string()]);
            }
            let part = self.fault_part.trim();
            if part.is_empty() {
                return Err(vec!["壊す部品を選んでください".to_string()]);
            }
            let fault = ALL_FAULTS[self.fault_kind.min(ALL_FAULTS.len() - 1)];
            let Some((tstop, tstep)) = self.tran_times() else {
                return Err(vec!["過渡解析の終了時間と刻みを数値で入力してください (例: 10m, 10u)".to_string()]);
            };
            let c = self.sim_circuit()?;
            chain_failure(&self.sch, &c, part, fault, &TranSettings { tstop, tstep, uic: self.tran_uic }).map_err(|e| vec![e.to_string()])
        })();
        self.status = match &report {
            Ok(r) => format!("連鎖故障: {} 個の部品が壊れました", r.steps.len()),
            Err(e) => format!("連鎖故障の解析に失敗: {}", e.first().cloned().unwrap_or_default()),
        };
        self.fault_chain = Some(report);
    }

    /// Everything the "break every part" scan needs, copied out of the editor so that it can run on another thread.
    fn scan_plan(&self) -> Option<ScanPlan> {
        let Ok(c) = self.sim_circuit() else {
            return None;
        };
        let mut jobs = vec![];
        for part in breakable(&c) {
            for (kind, fault) in ALL_FAULTS.iter().enumerate() {
                if let Some(b) = break_part(&c, &part, *fault) {
                    jobs.push((part.clone(), kind, b));
                }
            }
        }
        // ICs and transistors: every pin disconnected in turn (the usual solder / contact failure)
        let pin_parts: Vec<String> = self.sch.components.iter().filter(|x| matches!(x.kind, Kind::Custom | Kind::Npn)).map(|x| x.refdes.clone()).collect();
        let mut pin_jobs = vec![];
        for part in pin_parts {
            for (pin, _) in self.pin_choices(&part) {
                let label = self.pin_fault_label(&part, &pin, &PinHow::Open);
                pin_jobs.push((part.clone(), pin, label));
            }
        }
        Some(ScanPlan { c, sch: self.sch.clone(), temp: self.fault_temp().unwrap_or(T_NOM_C), jobs, pin_jobs })
    }

    /// The scan, to the end, on the calling thread (command line, tests).
    pub(super) fn run_fault_scan(&mut self) {
        let Some(plan) = self.scan_plan() else {
            self.status = "回路を計算できないため、故障を試せません。先に DC動作点を解析してエラーを直してください".into();
            return;
        };
        let rows = plan.run(&AtomicUsize::new(0), &AtomicBool::new(false)).unwrap_or_default();
        self.status = format!("故障シミュレーション: {} 通りを試しました", rows.len());
        self.fault_scan = rows;
    }

    /// For the command line: the scan on its own thread, and the fault panel open.
    pub fn start_fault_scan_bg(&mut self) {
        self.panel_tab = PanelTab::Analyze;
        self.sec_mode = 1;
        self.start_fault_scan();
    }

    /// The scan on its own thread, with a progress bar and a cancel button (the panel's button).
    pub(super) fn start_fault_scan(&mut self) {
        if self.scan_run.is_some() {
            return;
        }
        let Some(plan) = self.scan_plan() else {
            self.status = "回路を計算できないため、故障を試せません。先に DC動作点を解析してエラーを直してください".into();
            return;
        };
        let (done, cancel) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicBool::new(false)));
        let total = plan.jobs.len() + plan.pin_jobs.len();
        let (tx, rx) = std::sync::mpsc::channel();
        let (d, c, options) = (done.clone(), cancel.clone(), sim_core::options());
        std::thread::spawn(move || {
            // the simulation options are a per-thread setting
            sim_core::set_options(options);
            let _ = tx.send(plan.run(&d, &c));
        });
        self.scan_run = Some(ScanRun { done, total, cancel, rx });
        self.status = format!("故障シミュレーション: {total} 通りを試しています…");
    }

    /// Called every frame: takes the finished scan, and keeps the window redrawing while it runs.
    pub(super) fn poll_fault_scan(&mut self, ctx: &egui::Context) {
        let Some(run) = &self.scan_run else { return };
        match run.rx.try_recv() {
            Ok(Some(rows)) => {
                self.status = format!("故障シミュレーション: {} 通りを試しました", rows.len());
                self.fault_scan = rows;
                self.scan_run = None;
            }
            Ok(None) => {
                self.status = "故障シミュレーションを中止しました".into();
                self.scan_run = None;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => ctx.request_repaint_after(std::time::Duration::from_millis(100)),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.status = "故障シミュレーションが途中で止まりました".into();
                self.scan_run = None;
            }
        }
    }

    /// Shows the broken circuit's voltages on the sheet.
    pub(super) fn show_fault_on_sheet(&mut self) {
        let Some(FaultView { report: Ok(r) }) = &self.fault_view else { return };
        let Ok(f) = &r.faulted else { return };
        let label = format!("{} を「{}」", r.element, r.label);
        let view = self.dc_view(Ok(f.clone()), Some(label));
        self.dc = Some(view);
        self.show_v = true;
    }

    pub(super) fn panel_fault(&mut self, ui: &mut egui::Ui) {
        let circuit = self.sim_circuit().ok();
        let parts: Vec<String> = self.sch.components.iter().filter(|c| !c.kind.is_power_symbol()).map(|c| c.refdes.clone()).collect();
        ui.horizontal(|ui| {
            ui.label("部品");
            let shown = if self.fault_part.is_empty() { "選ぶ".to_string() } else { self.fault_part.clone() };
            let before = self.fault_part.clone();
            egui::ComboBox::from_id_salt("fault_part")
                .width(70.0)
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for p in &parts {
                        ui.selectable_value(&mut self.fault_part, p.clone(), p);
                    }
                })
                .response
                .on_hover_text("壊す部品。抵抗・コンデンサ・コイル・ダイオード・トランジスタ・電源は部品全体を、ICなどはピン単位で壊せます");
            if let Some(Sel::Comp(i)) = self.selected
                && let Some(c) = self.sch.components.get(i)
                && parts.contains(&c.refdes)
                && c.refdes != self.fault_part
                && ui.small_button("選択中の部品").on_hover_text("回路図で選んでいる部品を壊す対象にします").clicked()
            {
                self.fault_part = c.refdes.clone();
            }
            if self.fault_part != before {
                self.fault_pin.clear();
            }
        });
        let whole: Vec<Fault> = circuit
            .as_ref()
            .and_then(|c| c.elements.iter().find(|e| e.name() == self.fault_part))
            .map(fault_applicable)
            .unwrap_or_default();
        ui.horizontal(|ui| {
            ui.label("壊し方");
            if ui.selectable_label(self.fault_pin.is_empty(), "部品全体").on_hover_text("部品そのものが壊れた/付け間違えた状態にします").clicked() {
                self.fault_pin.clear();
            }
            let pins = self.pin_choices(&self.fault_part);
            if ui.selectable_label(!self.fault_pin.is_empty(), "ピン単位").on_hover_text("1本のピンだけ、はんだ不良 (断線) や、隣とのはんだブリッジ (ショート) にします。ICのピンは、この方法で壊します").clicked() && self.fault_pin.is_empty()
                && let Some((n, _)) = pins.first() {
                    self.fault_pin = n.clone();
                }
        });
        if self.fault_pin.is_empty() {
            if whole.is_empty() && !self.fault_part.is_empty() {
                ui.small("この部品は部品全体としては壊せません (ICなど)。「ピン単位」を選んでください");
            }
            ui.horizontal_wrapped(|ui| {
                for (k, f) in ALL_FAULTS.iter().enumerate() {
                    if !whole.contains(f) {
                        continue;
                    }
                    let tip = match f {
                        Fault::Open => "足が外れた・はんだ不良・パターン切れ。その部品が回路から消えた状態にします (電源は 0V になります)",
                        Fault::Short => "足同士がつながった (はんだブリッジ・部品の破壊)。ごく小さな抵抗でつなぎます",
                        Fault::TurnShort(_) => "コイルの巻線の層と層の絶縁がやぶれて、巻線の一部がショートした状態 (レアショート)。インダクタンスが下がって電流が増え、ショートした部分には大きな電流が回って発熱します。「連鎖故障を追う」で、そのあと何が壊れていくかを見られます",
                        Fault::Reverse => "向きを逆につけた (ダイオード/LED のアノードとカソード、トランジスタのコレクタとエミッタ)。部品の向き間違いは、初心者の失敗でいちばん多いものの一つです",
                        Fault::Scale(x) if *x > 1.0 => "値の読み間違い・部品の取り違え・電源電圧の上昇。値を大きくします",
                        _ => "値の読み間違い・部品の取り違え・容量抜け・電源電圧の低下。値を小さくします",
                    };
                    let label = circuit.as_ref().map_or(f.label(), |c| describe_fault(c, &self.fault_part, *f));
                    ui.selectable_value(&mut self.fault_kind, k, label).on_hover_text(tip);
                }
            });
        } else {
            let pins = self.pin_choices(&self.fault_part);
            ui.horizontal(|ui| {
                ui.label("ピン");
                let shown = pins.iter().find(|(n, _)| *n == self.fault_pin).map_or(self.fault_pin.clone(), |(_, d)| d.clone());
                let shown = match self.pin_net(&self.fault_part, &self.fault_pin) {
                    Some(net) => format!("{shown} ({net})"),
                    None => shown,
                };
                egui::ComboBox::from_id_salt("fault_pin").width(150.0).selected_text(shown).show_ui(ui, |ui| {
                    for (n, d) in &pins {
                        let net = self.pin_net(&self.fault_part, n).map_or(String::new(), |x| format!(" ({x})"));
                        ui.selectable_value(&mut self.fault_pin, n.clone(), format!("{d}{net}"));
                    }
                }).response.on_hover_text("壊すピン。カッコの中は、そのピンがつながっているネットの名前です");
            });
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut self.fault_pin_how, 0, "断線 (オープン)").on_hover_text("ピンの半田がはがれた・接触不良。そのピンだけ回路から切り離します。電源ピンなら「電源が来ない」、出力ピンなら「出力が出ない」状態です");
                ui.selectable_value(&mut self.fault_pin_how, 1, "GND にショート").on_hover_text("ピンがGNDにつながってしまった (はんだくず・ブリッジ)");
                ui.selectable_value(&mut self.fault_pin_how, 2, "別のネットにブリッジ").on_hover_text("隣のピンやパターンとはんだでつながってしまった状態。つなぐ相手のネットを選びます");
            });
            if self.fault_pin_how == 2 {
                ui.horizontal(|ui| {
                    ui.label("相手のネット");
                    let nets: Vec<String> = extract(&self.sch).into_iter().map(|n| n.name).filter(|n| !n.contains('.')).collect();
                    egui::ComboBox::from_id_salt("fault_bridge").width(100.0).selected_text(if self.fault_bridge.is_empty() { "選ぶ".to_string() } else { self.fault_bridge.clone() }).show_ui(ui, |ui| {
                        for n in &nets {
                            ui.selectable_value(&mut self.fault_bridge, n.clone(), n);
                        }
                    }).response.on_hover_text("ブリッジ (ピンを別のネットとつなぐ故障) の相手のネット");
                });
            }
        }
        ui.horizontal(|ui| {
            ui.label("比べる");
            ui.selectable_value(&mut self.fault_domain, FaultDomain::Dc, "DC (電圧)").on_hover_text("落ち着いたあとの電圧と電流を比べます。素早く、どこが危ないか分かります");
            ui.selectable_value(&mut self.fault_domain, FaultDomain::Tran, "過渡 (波形)").on_hover_text("時間を追った波形を、正常な回路と重ねて比べます。リップルや発振、点滅の変化など、DCでは見えない故障が分かります。過渡解析の終了時間・刻みの設定を使います");
            ui.selectable_value(&mut self.fault_domain, FaultDomain::Ac, "AC (周波数特性)").on_hover_text("周波数ごとのゲインを、正常な回路と重ねて比べます。フィルタのカットオフが動く、増幅度が変わる、共振が現れるなどが分かります。AC解析の周波数の設定を使います。入力の電源は「AC 1」にしておきます");
        });
        if self.fault_domain != FaultDomain::Dc {
            ui.horizontal(|ui| {
                ui.label("見る点");
                let nets: Vec<String> = extract(&self.sch).into_iter().map(|n| n.name).filter(|n| !n.contains('.')).collect();
                let shown = self.fault_watch_node().unwrap_or_else(|| "選ぶ".to_string());
                egui::ComboBox::from_id_salt("fault_node")
                    .width(80.0)
                    .selected_text(shown)
                    .show_ui(ui, |ui| {
                        for n in &nets {
                            ui.selectable_value(&mut self.fault_node, n.clone(), n);
                        }
                    })
                    .response
                    .on_hover_text("波形を比べるノード (ネット) の名前。出力にあたる点を選びます。選ばないときは OUT などを自動で探します");
            });
        }
        ui.horizontal(|ui| {
            if ui.button("故障を試す").on_hover_text("選んだ部品やピンを壊して解析し、変わった電圧と電流を日本語で説明します。回路そのものは変わりません").clicked() {
                self.run_fault();
            }
            if ui.button("連鎖故障を追う").on_hover_text("選んだ部品を壊したあと、過渡解析で電流・電力が定格を超えた部品を順に壊していき、何がどの順に壊れるかを日本語で説明します (部品全体の故障で調べます。過渡解析の終了時間・刻みの設定を使います。コイルのレアショートが典型例です)").clicked() {
                self.run_fault_chain();
            }
            if ui.add_enabled(self.scan_run.is_none(), egui::Button::new("全部品を順に壊す")).on_hover_text("すべての部品を、断線・ショート・逆向き・値違い・電源低下で、ICとトランジスタは全ピンの断線でも壊して、部品が定格を超える危険なものから順に並べます (DCで比べます)。クリックで詳しく見られます。大きな回路では時間がかかりますが、別の作業スレッドで走るので画面は止まらず、途中でやめられます").clicked() {
                self.start_fault_scan();
            }
        });
        if let Some(run) = &self.scan_run {
            let done = run.done.load(Ordering::Relaxed);
            ui.horizontal(|ui| {
                let text = if done == 0 { "正常な回路を計算しています…".to_string() } else { format!("故障を試しています: {done} / {}", run.total) };
                ui.add(egui::ProgressBar::new(done as f32 / run.total.max(1) as f32).text(text).desired_width(220.0));
                if ui.button("中止").on_hover_text("まだ始めていない故障は試さずにやめます。すでに解いている途中のものは、終わるまで待ちます").clicked() {
                    run.cancel.store(true, Ordering::Relaxed);
                }
            });
        }
        match &self.fault_chain {
            Some(Err(msgs)) => {
                for m in msgs {
                    ui.colored_label(Color32::from_rgb(230, 60, 60), m);
                }
            }
            Some(Ok(r)) => {
                ui.separator();
                for (k, l) in r.summary().iter().enumerate() {
                    if k == 0 {
                        ui.strong(l);
                    } else {
                        ui.label(l);
                    }
                }
                if ui.small_button("閉じる").on_hover_text("連鎖故障の説明を消します").clicked() {
                    self.fault_chain = None;
                }
            }
            None => {}
        }
        let mut show_sheet = false;
        let mut restore = false;
        if let Some(view) = &self.fault_view {
            match &view.report {
                Err(msgs) => {
                    for m in msgs {
                        ui.colored_label(Color32::from_rgb(230, 60, 60), m);
                    }
                }
                Ok(r) => {
                    let lines = r.summary();
                    for (k, l) in lines.iter().enumerate() {
                        if k == 0 {
                            ui.strong(l);
                        } else {
                            ui.label(l);
                        }
                    }
                    let safety = assess_safety(&self.sch, r);
                    let col = match safety.level {
                        Danger::Danger => Color32::from_rgb(230, 70, 60),
                        Danger::Caution => Color32::from_rgb(230, 170, 50),
                        Danger::Safe => Color32::from_rgb(90, 180, 110),
                    };
                    ui.separator();
                    for (k, l) in safety.summary().iter().enumerate() {
                        let t = egui::RichText::new(l);
                        ui.label(if k == 0 { t.strong().color(col) } else { t });
                    }
                    ui.horizontal(|ui| {
                        if r.faulted.is_ok() && ui.button("故障時の電圧を回路図に表示").on_hover_text("回路図の電圧表示を、壊れた状態の値に切り替えます。元に戻すには右のボタンか、DC解析を実行します").clicked() {
                            show_sheet = true;
                        }
                        if ui.button("元に戻す (DC解析)").on_hover_text("壊した仮定の結果をやめて、いまの回路のDC解析の結果に戻します").clicked() {
                            restore = true;
                        }
                    });
                }
            }
        }
        if show_sheet {
            self.show_fault_on_sheet();
        }
        if restore {
            self.run_dc();
        }
        if !self.fault_scan.is_empty() {
            let mut pick = None;
            egui::CollapsingHeader::new("危険な順 (定格超え → 電圧の変化)").default_open(true).show(ui, |ui| {
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    for (k, row) in self.fault_scan.iter().enumerate() {
                        let effect = if row.severity.is_infinite() {
                            "計算できない (大電流/不定)".to_string()
                        } else if row.severity < 0.01 {
                            "DCでは変化なし".to_string()
                        } else {
                            format!("電圧が最大 {:.2} V 変化", row.severity)
                        };
                        let mark = match row.danger {
                            Danger::Danger => " 🔥定格超え",
                            Danger::Caution => " ⚠余裕なし",
                            Danger::Safe => "",
                        };
                        let text = format!("{} {}: {}{}", row.part, row.label, effect, mark);
                        let col = if row.severity.is_infinite() || row.danger == Danger::Danger { Color32::from_rgb(230, 90, 70) } else if row.severity < 0.01 { Color32::from_gray(140) } else { Color32::from_rgb(220, 190, 80) };
                        if ui.selectable_label(false, egui::RichText::new(text).color(col)).on_hover_text("クリックで、この故障の詳しい結果を表示します").clicked() {
                            pick = Some(k);
                        }
                    }
                });
            });
            if let Some(k) = pick {
                let row = &self.fault_scan[k];
                (self.fault_part, self.fault_pin, self.fault_kind, self.fault_pin_how) = (row.part.clone(), row.pin.clone(), row.fault_kind, 0);
                self.run_fault();
            }
        }
    }
}
