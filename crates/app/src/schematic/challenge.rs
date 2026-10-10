//! "作ってみよう": a goal instead of a finished circuit. The learner places and wires the parts, and a
//! "判定する" button judges the circuit by what it does (it is simulated with loads and inputs of its own
//! choosing), not by comparing it with one model answer. A failed check says what to look at.

use super::*;

/// One line of the checklist after "判定する".
pub(super) struct CheckResult {
    pub label: &'static str,
    /// None = not judged (an earlier check failed first).
    pub ok: Option<bool>,
    pub detail: String,
}

/// The challenge being worked on.
pub(super) struct ActiveChallenge {
    pub id: &'static str,
    /// 0 = the parts are placed, only wiring is left; 1 = place the parts too.
    pub level: u8,
    pub results: Vec<CheckResult>,
    pub hints: usize,
    pub passed: bool,
}

struct Challenge {
    id: &'static str,
    /// The lesson this challenge belongs to.
    lesson: &'static str,
    title: &'static str,
    goal: &'static str,
    /// From a nudge to nearly the answer; shown one at a time.
    hints: &'static [&'static str],
    /// A circuit that passes (shown by "答えの回路を見る").
    solution: &'static str,
}

const CHALLENGES: &[Challenge] = &[Challenge {
    id: "zener",
    lesson: "zener",
    title: "ツェナーで 5V を作る",
    goal: "9V の電源から、約 5V (4.7〜5.6V) を作ります。使うのは、抵抗とツェナーダイオードだけです。出力には、負荷として 1kΩ から 330Ω をつなぐので、重い負荷でも電圧が崩れず、入力が 8〜10V に変わっても 5V 付近を保ち、部品が定格を超えない (いちばん厳しいのは 9V で負荷 330Ω のとき) ように、直列の抵抗の値を決めてください。出力のネット (抵抗とツェナーの間の点) には、ラベルツールで「VOUT」という名前を付けます",
    hints: &[
        "回路の形: 電源の + → 抵抗 → 出力 (VOUT)。出力と GND の間に、ツェナーを入れます。ツェナーは、ふつうのダイオードと逆向き (カソード = 縞のある側を出力側、アノードを GND 側) です。",
        "抵抗の値は、「学ぶ → 値の決め方」の「ツェナーダイオードで電圧を一定にする」で求められます。入力 9V、ツェナー電圧 5.1V、負荷の電流の最大 (5.1V ÷ 330Ω ≒ 15mA)、ツェナーに残す電流 5mA くらいで計算します。ツェナーは、ライブラリの 1N4733A (5.1V) を使います。",
        "答えの目安: 抵抗は 150Ω 前後 (130〜200Ω)。小さすぎると抵抗が熱で定格を超え (電力 = (9V − 5.1V)² ÷ R が、0805 のチップ抵抗の目安 0.125W 以下 → R ≧ 125Ω)、大きすぎると負荷 330Ω でツェナーに電流が残りません (R ≦ 220Ω)。電源 (9V) の + → 抵抗 → VOUT、VOUT → ツェナーのカソード、ツェナーのアノード → GND、電源の − → GND。「答えの回路を見る」で、できあがりを見られます。",
    ],
    solution: include_str!("../../../../docs/examples/zener_regulator.schjson"),
}];

/// Output range of the zener challenge (V).
const V_LO: f64 = 4.7;
const V_HI: f64 = 5.6;

fn challenge(id: &str) -> Option<&'static Challenge> {
    CHALLENGES.iter().find(|c| c.id == id)
}

/// The id of the challenge of a lesson, if it has one.
pub fn challenge_of_lesson(lesson: &str) -> Option<&'static str> {
    CHALLENGES.iter().find(|c| c.lesson == lesson).map(|c| c.id)
}

/// The key under which a passed challenge is recorded (see `learn::Progress`).
pub fn progress_key(id: &str) -> String {
    format!("{id}+build")
}

/// The starting sheet of a level: the source and the ground symbols (and, at level 0, the parts) with no wire.
fn starting_sheet(ch: &Challenge, level: u8) -> Schematic {
    let mut full: Schematic = serde_json::from_str(ch.solution).unwrap_or_default();
    full.wires.clear();
    full.labels.clear();
    full.components.retain(|c| match c.refdes.as_str() {
        "V1" => true,
        "R1" | "D1" => level == 0,
        _ => false,
    });
    full.components.retain(|c| !c.kind.is_power_symbol());
    // close together, so that the whole sheet stays clear of the window on the right
    for c in &mut full.components {
        match c.refdes.as_str() {
            "V1" => c.pos = (0, 0),
            "R1" => c.pos = (10, -6),
            "D1" => c.pos = (20, 0),
            _ => {}
        }
    }
    // a ground symbol a little below the source's minus pin, and one below the zener's anode (level 0)
    let below = |s: &Schematic, refdes: &str, pin: &str| -> Option<Pt> {
        let c = s.components.iter().find(|c| c.refdes == refdes)?;
        let (_, p) = c.pins().into_iter().find(|(n, _)| n == pin)?;
        Some((p.0, p.1 + 5))
    };
    let mut grounds = vec![];
    if let Some(p) = below(&full, "V1", "-") {
        grounds.push(p);
    }
    if level == 0
        && let Some(p) = below(&full, "D1", "A") {
            grounds.push(p);
        }
    for p in grounds {
        full.add(Kind::Ground, p, 0);
    }
    full
}

/// A copy of the circuit with an extra load of `ohms` between the net VOUT and GND (the checker's own, named RTEST).
fn with_load(s: &Schematic, ohms: &str) -> Schematic {
    let mut t = s.clone();
    let i = t.add(Kind::Resistor, (400, 400), 0);
    t.components[i].refdes = "RTEST".into();
    t.components[i].value = ohms.into();
    let pins = t.components[i].pins();
    for (name, net) in [("1", "VOUT"), ("2", "GND")] {
        if let Some((_, p)) = pins.iter().find(|(n, _)| n == name) {
            t.labels.push(Label { pos: *p, text: net.into() });
        }
    }
    t
}

/// The same with the first voltage source set to `volts`.
fn with_input(s: &Schematic, volts: &str) -> Schematic {
    let mut t = s.clone();
    if let Some(v) = t.components.iter_mut().find(|c| c.kind == Kind::VSource) {
        v.value = volts.into();
    }
    t
}

/// Output voltage of `s` (the net VOUT), or why it cannot be found.
fn vout(s: &Schematic) -> Result<f64, String> {
    let c = sim_core::from_schematic(s).map_err(|e| e.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(" / "))?;
    let dc = sim_core::dc_op(&c).map_err(|e| {
        let mut m = e.to_string();
        for h in sim_core::diagnose(&c, &e) {
            m.push_str(&format!(" ヒント: {h}"));
        }
        m
    })?;
    dc.voltage("VOUT").ok_or_else(|| "VOUT というネットが見つかりません".to_string())
}

impl SchEditor {
    /// Starts challenge `id` at `level` on a fresh sheet (the running lesson is put away).
    pub fn start_challenge(&mut self, id: &str, level: u8) {
        let Some(ch) = challenge(id) else { return };
        self.new_file();
        self.sch = starting_sheet(ch, level);
        self.reset_history();
        self.fit_pending = true;
        self.lesson = None;
        self.panel_tab = PanelTab::Draw;
        self.challenge = Some(ActiveChallenge { id: ch.id, level, results: vec![], hints: 0, passed: false });
        self.status = format!("「{}」を始めました。回路を作って、「判定する」を押してください", ch.title);
    }

    /// The key of the challenge just passed, for the progress record.
    pub fn finished_challenge(&self) -> Option<String> {
        self.challenge.as_ref().filter(|c| c.passed).map(|c| progress_key(c.id))
    }

    /// "判定する": judges the sheet and keeps the result for the window.
    pub fn run_judge(&mut self) {
        let results = self.judge_challenge();
        let passed = !results.is_empty() && results.iter().all(|r| r.ok == Some(true));
        if let Some(a) = self.challenge.as_mut() {
            a.results = results;
            a.passed = passed;
        }
        self.status = if passed { "判定: すべて合格です".into() } else { "判定: 直すところがあります (✖ の項目を見てください)".into() };
    }

    /// Judges the circuit on the sheet by simulating it.
    pub(super) fn judge_challenge(&self) -> Vec<CheckResult> {
        let mut out: Vec<CheckResult> = vec![];
        let mut push = |label: &'static str, ok: Option<bool>, detail: String| out.push(CheckResult { label, ok, detail });
        let pending = |why: &str| (None, why.to_string());

        // 1. wiring mistakes
        let errors: Vec<String> = erc(&self.sch).into_iter().filter(|p| p.severity == Severity::Error).map(|p| p.message).collect();
        let wired_ok = errors.is_empty();
        push("配線ミスの点検 (ERC) にエラーがない", Some(wired_ok), if wired_ok { String::new() } else { format!("{} (「作図」タブの ERC の一覧で、場所を確かめられます)", errors[0]) });

        // 2. the output net is named
        let named = extract(&self.sch).iter().any(|n| n.name == "VOUT" && !n.pins.is_empty());
        push(
            "出力のネットに「VOUT」のラベルがある",
            Some(named),
            if named { String::new() } else { "出力 (抵抗とツェナーの間の点) に、ラベルツールで「VOUT」という名前を付けてください。判定は、このネットの電圧を測ります".into() },
        );
        if !named {
            for l in ["負荷 1kΩ で、出力が 4.7〜5.6V", "負荷 330Ω でも、出力が 4.7V 以上", "入力が 8〜10V に変わっても、出力が保たれる", "部品が定格を超えない"] {
                let (ok, d) = pending("VOUT のラベルを付けてから判定します");
                push(l, ok, d);
            }
            return out;
        }

        // 3. with a 1k load
        let base = with_load(&self.sch, "1k");
        let v1k = vout(&base);
        match &v1k {
            Ok(v) => {
                let ok = (V_LO..=V_HI).contains(v);
                let detail = if ok {
                    format!("出力 {v:.2} V")
                } else if *v > 8.0 {
                    format!("出力が {v:.2} V で、電源の電圧に近いままです。ツェナーに電流が流れていません。ツェナーが GND につながっているか、向き (カソード = 縞を出力側) を確かめてください")
                } else if *v < 1.5 {
                    format!("出力が {v:.2} V しかありません。ツェナーが、ふつうのダイオードの向き (順方向) になっていませんか。カソード (縞のある側) を出力側、アノードを GND 側にします")
                } else if *v < V_LO {
                    format!("出力が {v:.2} V で、低すぎます。直列の抵抗が大きすぎて、ツェナーに電流が残っていないか、ツェナーの電圧が 5.1V でない部品です")
                } else {
                    format!("出力が {v:.2} V で、高すぎます。ツェナーの電圧が 5.1V より高い部品ではありませんか")
                };
                push("負荷 1kΩ で、出力が 4.7〜5.6V", Some(ok), detail);
            }
            Err(e) => push("負荷 1kΩ で、出力が 4.7〜5.6V", Some(false), format!("回路を解けませんでした: {e}")),
        }
        let first_ok = matches!(v1k, Ok(v) if (V_LO..=V_HI).contains(&v));
        if !first_ok {
            for l in ["負荷 330Ω でも、出力が 4.7V 以上", "入力が 8〜10V に変わっても、出力が保たれる", "部品が定格を超えない"] {
                let (ok, d) = pending("前の項目を直してから判定します");
                push(l, ok, d);
            }
            return out;
        }

        // 4. a heavy load
        match vout(&with_load(&self.sch, "330")) {
            Ok(v) => {
                let ok = v >= V_LO;
                push(
                    "負荷 330Ω でも、出力が 4.7V 以上",
                    Some(ok),
                    if ok { format!("出力 {v:.2} V") } else { format!("負荷 330Ω では出力が {v:.2} V に落ちます。直列の抵抗を小さくして、負荷が取る電流 (5.1V ÷ 330Ω ≒ 15mA) に加えて、ツェナーに数 mA が残るようにします。目安: 抵抗 ≦ (9V − 5.1V) ÷ (15mA + 数 mA) ≒ 200Ω 前後") },
                );
            }
            Err(e) => push("負荷 330Ω でも、出力が 4.7V 以上", Some(false), e),
        }

        // 5. the input moves
        let lo = vout(&with_input(&with_load(&self.sch, "1k"), "8"));
        let hi = vout(&with_input(&with_load(&self.sch, "1k"), "10"));
        match (lo, hi) {
            (Ok(a), Ok(b)) => {
                let ok = a >= 4.5 && b <= 5.8;
                push(
                    "入力が 8〜10V に変わっても、出力が保たれる",
                    Some(ok),
                    if ok { format!("8V のとき {a:.2} V、10V のとき {b:.2} V") } else { format!("8V のとき {a:.2} V、10V のとき {b:.2} V と、動きすぎです。入力が低いとツェナーに電流が残らず (抵抗が大きすぎる)、入力が高いと電流が増えすぎて電圧が上がります (抵抗が小さすぎる)") },
                );
            }
            _ => push("入力が 8〜10V に変わっても、出力が保たれる", Some(false), "入力を変えて解けませんでした".into()),
        }

        // 6. ratings at the nominal input with the heaviest load
        let worst = self.with_ratings_of(&with_load(&self.sch, "330"));
        let over: Vec<String> = match sim_core::dc_op(&sim_core::from_schematic(&worst).unwrap_or_default_circuit()) {
            Ok(dc) => sim_core::check_ratings(&worst, &dc).into_iter().filter(|i| i.refdes != "RTEST" && i.level == sim_core::RatingLevel::Over).map(|i| i.message).collect(),
            Err(_) => vec![],
        };
        push(
            "部品が定格を超えない",
            Some(over.is_empty()),
            if over.is_empty() { "電源 9V・負荷 330Ω の、いちばん厳しい条件でも、定格の範囲です".into() } else { format!("電源 9V・負荷 330Ω のとき、{}。抵抗を大きくする (または、定格の大きい部品にする) と直ります", over[0]) },
        );
        out
    }

    /// The sheet with the data sheet ratings of library parts filled in for parts that were placed without them.
    fn with_ratings_of(&self, s: &Schematic) -> Schematic {
        let mut t = s.clone();
        for c in &mut t.components {
            if let Some(sym) = c.custom.as_mut()
                && sym.ratings.is_none()
                && let Some(d) = self.library.iter().find(|d| d.name == sym.name) {
                    sym.ratings = d.ratings.clone().filter(|r| !r.is_empty());
                }
        }
        t
    }

    /// Floating window of the running challenge.
    pub fn challenge_window(&mut self, ctx: &egui::Context) {
        let Some(active) = self.challenge.as_ref() else { return };
        let Some(ch) = challenge(active.id) else { return };
        let (level, hints, passed) = (active.level, active.hints, active.passed);
        let mut judge = false;
        let mut new_level = None;
        let mut more_hint = false;
        let mut show_answer = false;
        let mut back = false;
        let mut close = false;
        egui::Window::new(format!("作ってみよう: {}", ch.title)).default_width(400.0).anchor(Align2::RIGHT_TOP, Vec2::new(-8.0, 40.0)).show(ctx, |ui| {
            ui.label(ch.goal);
            ui.horizontal(|ui| {
                ui.label("むずかしさ");
                if ui.selectable_label(level == 0, "配線だけ").on_hover_text("部品は置いてあります。線でつなぐ練習です (やり直すと、最初の状態に戻ります)").clicked() && level != 0 {
                    new_level = Some(0);
                }
                if ui.selectable_label(level == 1, "部品も置く").on_hover_text("電源だけが置いてあります。部品を選んで置き、配線し、値も決めます (やり直すと、最初の状態に戻ります)").clicked() && level != 1 {
                    new_level = Some(1);
                }
            });
            ui.separator();
            let results = &self.challenge.as_ref().map(|c| &c.results[..]).unwrap_or(&[]);
            if results.is_empty() {
                ui.label("作り終えたら、「判定する」を押してください。回路を実際にシミュレーションして、次の項目を調べます");
            }
            for r in results.iter() {
                let (mark, color) = match r.ok {
                    Some(true) => ("✔", Color32::from_rgb(60, 160, 90)),
                    Some(false) => ("✖", Color32::from_rgb(230, 90, 70)),
                    None => ("○", Color32::from_gray(140)),
                };
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(color, mark);
                    ui.label(egui::RichText::new(r.label).strong());
                });
                if !r.detail.is_empty() {
                    ui.indent(("challenge_detail", r.label), |ui| {
                        ui.label(egui::RichText::new(&r.detail).color(if r.ok == Some(false) { Color32::from_rgb(230, 150, 120) } else { Color32::from_gray(170) }));
                    });
                }
            }
            if passed {
                ui.separator();
                ui.colored_label(Color32::from_rgb(60, 160, 90), "できました!");
                ui.label("自分で作った回路が、負荷や入力が変わっても 5V 付近を保ち、定格も守れています。次は、ツェナーに電流を流し続ける熱のむだと、負荷が重いときの限界を、考えてみましょう。それを解決するのが、続くエミッタフォロワと、帰還で安定化する回路です");
            }
            for (k, h) in ch.hints.iter().enumerate().take(hints) {
                ui.separator();
                ui.label(egui::RichText::new(format!("ヒント {}", k + 1)).strong());
                ui.label(*h);
            }
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if ui.button("判定する").on_hover_text("いまの回路をシミュレーションして、目標を満たしているか調べます。何度でも押せます").clicked() {
                    judge = true;
                }
                if ui.add_enabled(hints < ch.hints.len(), egui::Button::new(format!("ヒントを見る ({hints} / {})", ch.hints.len()))).on_hover_text("行き詰まったら、少しずつヒントを出します。最後のヒントは、答えの目安です").clicked() {
                    more_hint = true;
                }
                if ui.button("答えの回路を見る").on_hover_text("できあがりの回路を、いまの回路と入れ替えます (Ctrl+Z で戻せます)。答えは 1 つではありません。判定を通れば、別の値や形でも正解です").clicked() {
                    show_answer = true;
                }
            });
            ui.horizontal(|ui| {
                if ui.button("レッスンに戻る").on_hover_text("このチャレンジをやめて、レッスンの手順に戻ります").clicked() {
                    back = true;
                }
                if ui.button("閉じる").clicked() {
                    close = true;
                }
            });
        });
        if let Some(l) = new_level {
            self.start_challenge(ch.id, l);
        } else if judge {
            self.run_judge();
        } else if more_hint {
            if let Some(a) = self.challenge.as_mut() {
                a.hints = (a.hints + 1).min(ch.hints.len());
            }
        } else if show_answer {
            if let Ok(s) = serde_json::from_str::<Schematic>(ch.solution) {
                // not through `load_sample`: the history stays, so Ctrl+Z brings the learner's own circuit back
                self.sch = s;
                self.fit_pending = true;
            }
        } else if back {
            let lesson = ch.lesson;
            self.challenge = None;
            if let Some(i) = lesson_ids().iter().position(|l| *l == lesson) {
                self.start_lesson(i);
            }
        } else if close {
            self.challenge = None;
        }
    }
}

trait UnwrapOrDefaultCircuit {
    fn unwrap_or_default_circuit(self) -> sim_core::Circuit;
}

impl UnwrapOrDefaultCircuit for Result<sim_core::Circuit, Vec<sim_core::ConvertError>> {
    fn unwrap_or_default_circuit(self) -> sim_core::Circuit {
        self.unwrap_or_else(|_| sim_core::Circuit::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The zener sample without its own load: just the two parts a learner has to add, wired and labelled.
    fn solution() -> Schematic {
        let mut s: Schematic = serde_json::from_str(CHALLENGES[0].solution).unwrap();
        let pins: Vec<Pt> = s.components.iter().find(|c| c.refdes == "R2").unwrap().pins().into_iter().map(|(_, p)| p).collect();
        s.labels.retain(|l| !pins.contains(&l.pos));
        s.wires.retain(|w| !pins.contains(&w.a) && !pins.contains(&w.b));
        s.components.retain(|c| c.refdes != "R2");
        s
    }

    fn editor(s: Schematic) -> SchEditor {
        let mut e = SchEditor::default();
        e.sch = s;
        e.library = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).collect();
        e
    }

    fn set(s: &mut Schematic, refdes: &str, v: &str) {
        s.components.iter_mut().find(|c| c.refdes == refdes).unwrap().value = v.into();
    }

    fn verdict(s: Schematic) -> Vec<(Option<bool>, String)> {
        editor(s).judge_challenge().into_iter().map(|r| (r.ok, r.detail)).collect()
    }

    #[test]
    fn a_working_zener_regulator_passes_every_check() {
        let r = verdict(solution());
        assert_eq!(r.len(), 6);
        assert!(r.iter().all(|(ok, _)| *ok == Some(true)), "{r:#?}");
    }

    #[test]
    fn a_resistor_that_is_too_small_runs_hot_and_one_that_is_too_large_starves_the_zener() {
        // 100 ohm: the 0805 resistor dissipates ~0.14 W (limit 0.125 W): the ratings check fails, the voltage checks pass
        let mut s = solution();
        set(&mut s, "R1", "100");
        s.components.iter_mut().find(|c| c.refdes == "R1").unwrap().footprint = "R_0805".into();
        let r = verdict(s);
        assert_eq!(r[2].0, Some(true), "{r:#?}");
        assert_eq!(r[5].0, Some(false), "a hot resistor: {r:#?}");
        assert!(r[5].1.contains("R1"), "{}", r[5].1);
        // 2 k: the zener gets no current with a 1 k load
        let mut s = solution();
        set(&mut s, "R1", "2k");
        let r = verdict(s);
        assert_eq!(r[2].0, Some(false));
        assert!(r[2].1.contains("低すぎ"), "{}", r[2].1);
        assert!(r[3].0.is_none() && r[5].0.is_none(), "later checks wait for the earlier ones");
        // 390 ohm: fine with 1 k, but a 330 ohm load starves it
        let mut s = solution();
        set(&mut s, "R1", "390");
        let r = verdict(s);
        assert_eq!(r[2].0, Some(true), "{r:#?}");
        assert_eq!(r[3].0, Some(false), "{r:#?}");
        assert!(r[3].1.contains("330Ω") && r[3].1.contains("目安"), "{}", r[3].1);
    }

    #[test]
    fn missing_pieces_are_explained() {
        // no VOUT label: the checks that need it wait
        let mut s = solution();
        s.labels.retain(|l| l.text != "VOUT");
        let r = verdict(s);
        assert_eq!(r[1].0, Some(false));
        assert!(r[1].1.contains("VOUT") && r[2..].iter().all(|(ok, _)| ok.is_none()), "{r:#?}");
        // no zener: the output follows the supply
        let mut s = solution();
        s.components.retain(|c| c.refdes != "D1");
        let r = verdict(s);
        assert_eq!(r[2].0, Some(false), "{r:#?}");
        // an empty sheet does not panic and fails with a reason
        let r = verdict(Schematic::default());
        assert!(r.iter().any(|(ok, _)| *ok == Some(false)));
    }

    #[test]
    fn the_two_levels_start_from_a_sheet_without_wires() {
        let mut e = SchEditor::default();
        e.start_challenge("zener", 0);
        let names = |e: &SchEditor| e.sch.components.iter().map(|c| c.refdes.clone()).collect::<Vec<_>>();
        assert!(e.sch.wires.is_empty() && e.sch.labels.is_empty());
        assert!(names(&e).contains(&"V1".to_string()) && names(&e).contains(&"R1".to_string()) && names(&e).contains(&"D1".to_string()));
        assert_eq!(e.sch.components.iter().filter(|c| c.kind.is_power_symbol()).count(), 2, "a ground under the source and one under the zener");
        assert!(e.lesson.is_none() && e.challenge.is_some());
        // level 1: only the source and a ground
        e.start_challenge("zener", 1);
        assert_eq!(e.sch.components.iter().filter(|c| !c.kind.is_power_symbol()).count(), 1);
        // an empty start is judged without crashing, and nothing is recorded as passed
        let r = e.judge_challenge();
        assert!(r.iter().any(|x| x.ok == Some(false)));
        assert_eq!(e.finished_challenge(), None);
        assert_eq!(challenge_of_lesson("zener"), Some("zener"));
        assert_eq!(challenge_of_lesson("divider"), None);
        assert_eq!(progress_key("zener"), "zener+build");
    }

    #[test]
    fn passing_is_recorded_and_hints_never_run_out_of_text() {
        let mut e = editor(solution());
        e.start_challenge("zener", 0);
        e.sch = solution();
        let results = e.judge_challenge();
        let passed = results.iter().all(|r| r.ok == Some(true));
        e.challenge.as_mut().unwrap().results = results;
        e.challenge.as_mut().unwrap().passed = passed;
        assert_eq!(e.finished_challenge().as_deref(), Some("zener+build"));
        assert!(CHALLENGES[0].hints.len() >= 3 && CHALLENGES[0].hints.iter().all(|h| !h.is_empty()));
    }
}
