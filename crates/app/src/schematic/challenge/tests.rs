use super::*;

#[cfg(test)]
mod challenge_tests {
    use super::*;

    fn editor(s: Schematic, id: &str) -> SchEditor {
        let mut e = SchEditor::default();
        e.library = part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).collect();
        e.start_challenge(id, 0);
        e.sch = s;
        e
    }

    fn verdict(id: &str, s: Schematic) -> Vec<(Option<bool>, String)> {
        editor(s, id).judge_challenge().into_iter().map(|r| (r.ok, r.detail)).collect()
    }

    fn solution(id: &str) -> Schematic {
        solution_of(challenge(id).unwrap())
    }

    fn set(s: &mut Schematic, refdes: &str, v: &str) {
        set_value(s, refdes, v);
    }

    /// Every challenge: its answer passes every check, its start sheets are wire-free and judged without a crash.
    /// A ground placed with a fixed offset from a pin used to land on the body of a part that is drawn the other
    /// way up (the zener's anode is at the top); no ground may sit between the pins of another part.
    #[test]
    fn no_start_ground_lands_on_another_part() {
        for ch in CHALLENGES {
            for level in [0, 1] {
                let mut e = SchEditor::default();
                e.start_challenge(ch.id, level);
                for g in e.sch.components.iter().filter(|c| c.kind == Kind::Ground) {
                    for c in e.sch.components.iter().filter(|c| c.kind != Kind::Ground) {
                        let pins: Vec<(i32, i32)> = c.pins().into_iter().map(|(_, p)| p).collect();
                        if pins.len() < 2 {
                            continue;
                        }
                        let (x0, x1) = (pins.iter().map(|p| p.0).min().unwrap(), pins.iter().map(|p| p.0).max().unwrap());
                        let (y0, y1) = (pins.iter().map(|p| p.1).min().unwrap(), pins.iter().map(|p| p.1).max().unwrap());
                        let inside = g.pos.0 > x0 && g.pos.0 < x1 && g.pos.1 > y0 && g.pos.1 < y1;
                        let on_lead = (g.pos.0 == x0 && x0 == x1 && g.pos.1 > y0 && g.pos.1 < y1) || (g.pos.1 == y0 && y0 == y1 && g.pos.0 > x0 && g.pos.0 < x1);
                        assert!(!inside && !on_lead, "{} level {level}: a ground at {:?} lies on {} ({x0},{y0})-({x1},{y1})", ch.id, g.pos, c.refdes);
                    }
                }
            }
        }
    }

    #[test]
    fn every_answer_passes_and_every_start_sheet_is_bare() {
        let ids = lesson_ids();
        for ch in CHALLENGES {
            assert!(ids.contains(&ch.lesson), "{}: lesson {} exists", ch.id, ch.lesson);
            assert!(ch.hints.len() >= 3 && ch.hints.iter().all(|h| !h.is_empty()), "{}", ch.id);
            let r = verdict(ch.id, solution(ch.id));
            assert_eq!(r.len(), ch.checks.len(), "{}", ch.id);
            assert!(r.iter().all(|(ok, _)| *ok == Some(true)), "{}: {r:#?}", ch.id);
            for level in [0, 1] {
                let mut e = SchEditor::default();
                e.start_challenge(ch.id, level);
                assert!(e.sch.wires.is_empty() && e.sch.labels.is_empty(), "{} level {level}", ch.id);
                assert!(e.sch.components.iter().any(|c| c.kind.is_power_symbol()), "{}: a ground to start from", ch.id);
                let results = e.judge_challenge();
                assert_eq!(results.len(), ch.checks.len());
                assert!(results.iter().any(|x| x.ok == Some(false)), "{} level {level}: nothing is built yet", ch.id);
                assert_eq!(e.finished_challenge(), None);
            }
            // level 0 has more parts than level 1
            assert!(ch.parts0.len() > ch.parts1.len(), "{}", ch.id);
        }
    }

    #[test]
    fn the_zener_challenge_tells_hot_starved_and_missing_apart() {
        let mut s = solution("zener");
        set(&mut s, "R1", "100");
        s.components.iter_mut().find(|c| c.refdes == "R1").unwrap().footprint = "R_0805".into();
        let r = verdict("zener", s);
        assert_eq!(r[2].0, Some(true), "{r:#?}");
        assert_eq!(r[5].0, Some(false), "a hot resistor: {r:#?}");
        assert!(r[5].1.contains("R1"), "{}", r[5].1);
        let mut s = solution("zener");
        set(&mut s, "R1", "2k");
        let r = verdict("zener", s);
        assert!(r[2].1.contains("低すぎ") && r[3].0.is_none() && r[5].0.is_none(), "{r:#?}");
        let mut s = solution("zener");
        set(&mut s, "R1", "390");
        let r = verdict("zener", s);
        assert_eq!((r[2].0, r[3].0), (Some(true), Some(false)), "{r:#?}");
        assert!(r[3].1.contains("330Ω") && r[3].1.contains("目安"));
        let mut s = solution("zener");
        s.labels.retain(|l| l.text != "VOUT");
        let r = verdict("zener", s);
        assert!(r[1].0 == Some(false) && r[2..].iter().all(|(ok, _)| ok.is_none()), "{r:#?}");
        let mut s = solution("zener");
        s.components.retain(|c| c.refdes != "D1");
        assert_eq!(verdict("zener", s)[2].0, Some(false));
    }

    #[test]
    fn the_divider_challenge_notices_loading_and_waste() {
        // 51k / 100k: right with no load, but a 100k load pulls it down
        let mut s = solution("divider");
        set(&mut s, "R1", "51k");
        set(&mut s, "R2", "100k");
        let r = verdict("divider", s);
        assert_eq!(r[2].0, Some(true), "{r:#?}");
        assert_eq!(r[3].0, Some(false), "{r:#?}");
        assert!(r[3].1.contains("並列"), "{}", r[3].1);
        // 100 / 200 ohm: correct ratio, but 5 V across 300 ohm wastes 17 mA
        let mut s = solution("divider");
        set(&mut s, "R1", "100");
        set(&mut s, "R2", "200");
        let r = verdict("divider", s);
        assert_eq!((r[2].0, r[4].0), (Some(true), Some(false)), "{r:#?}");
        assert!(r[4].1.contains("むだ"), "{}", r[4].1);
        // wrong ratio: told which way to go
        let mut s = solution("divider");
        set(&mut s, "R2", "1k");
        let r = verdict("divider", s);
        assert!(r[2].1.contains("低すぎ"), "{r:#?}");
    }

    #[test]
    fn the_led_challenge_reads_the_leds_current() {
        let mut s = solution("led");
        set(&mut s, "R1", "2k");
        assert!(verdict("led", s)[1].1.contains("少なすぎ"));
        let mut s = solution("led");
        set(&mut s, "R1", "100k");
        assert!(verdict("led", s)[1].1.contains("流れていません"));
        let mut s = solution("led");
        set(&mut s, "R1", "50");
        let r = verdict("led", s);
        assert!(r[1].1.contains("多すぎ"), "{r:#?}");
        assert_eq!(r[2].0, Some(false), "55 mA in a LED is past its rating: {r:#?}");
        let mut s = solution("led");
        s.components.retain(|c| c.kind != Kind::Led);
        assert_eq!(verdict("led", s)[1].0, Some(false));
    }

    #[test]
    fn the_filter_challenge_measures_the_gain_at_three_frequencies() {
        // R x C ten times too large: the corner is at 100 Hz
        let mut s = solution("rc_lowpass");
        set(&mut s, "C1", "1.59u");
        let r = verdict("rc_lowpass", s);
        assert!(r[2].1.contains("下がりすぎ") && r[3].0 == Some(false), "{r:#?}");
        // R x C ten times too small: the corner is at 10 kHz
        let mut s = solution("rc_lowpass");
        set(&mut s, "C1", "15.9n");
        let r = verdict("rc_lowpass", s);
        assert!(r[2].1.contains("あまり下がっていません") && r[4].0 == Some(false), "{r:#?}");
        // another R / C pair with the same corner passes (the answer is not unique)
        let mut s = solution("rc_lowpass");
        set(&mut s, "R1", "10k");
        set(&mut s, "C1", "15.9n");
        assert!(verdict("rc_lowpass", s).iter().all(|(ok, _)| *ok == Some(true)));
    }

    #[test]
    fn the_follower_and_the_inverting_amplifier_challenges_diagnose_their_mistakes() {
        // follower: a huge base resistor starves the zener, so the output cannot be held
        let mut s = solution("emitter_follower");
        set(&mut s, "R1", "100k");
        let r = verdict("emitter_follower", s);
        assert!(r.iter().any(|(ok, _)| *ok == Some(false)), "{r:#?}");
        // without the transistor nothing is left on the output net: the judge asks for the VOUT net first
        let mut s = solution("emitter_follower");
        s.components.retain(|c| c.refdes != "Q1");
        let r = verdict("emitter_follower", s);
        assert_eq!(r[1].0, Some(false), "{r:#?}");
        // inverting amplifier: R2 = R1 gives -1, a non-inverting hook-up gives a positive output
        let mut s = solution("opamp_inv");
        set(&mut s, "R2", "1k");
        let r = verdict("opamp_inv", s);
        assert!(r[2].1.contains("ゲイン"), "{r:#?}");
        let mut s = solution("opamp_inv");
        set(&mut s, "R2", "100k");
        assert_eq!(verdict("opamp_inv", s)[2].0, Some(false));
    }

    /// The final challenge: the answer has a protection diode and two capacitors; without them the judge says which check fails.
    #[test]
    fn the_final_challenge_asks_for_reverse_protection_and_capacitors() {
        // no diode: the supply goes straight to the regulator, a backwards supply shows on the output
        let mut s = solution("draw_psu");
        for l in &mut s.labels {
            if l.text == "VPROT" {
                l.text = "VIN".into();
            }
        }
        s.components.retain(|c| c.refdes != "D1");
        let r = verdict("draw_psu", s);
        let failed: Vec<usize> = r.iter().enumerate().filter(|(_, (ok, _))| *ok == Some(false)).map(|(i, _)| i).collect();
        assert_eq!(failed, vec![5], "only the reverse-connection check fails: {r:#?}");
        assert!(r[5].1.contains("ダイオード"), "{r:#?}");
        // no capacitors: only the capacitor check fails
        let mut s = solution("draw_psu");
        s.components.retain(|c| c.kind != Kind::Capacitor);
        let r = verdict("draw_psu", s);
        let failed: Vec<usize> = r.iter().enumerate().filter(|(_, (ok, _))| *ok == Some(false)).map(|(i, _)| i).collect();
        assert_eq!(failed, vec![6], "{r:#?}");
        // the final challenge opens from the last lesson and from the learning path by its own id
        assert_eq!(challenge_of_lesson("protection"), Some("draw_psu"));
        assert!(challenge_exists("draw_psu"));
    }

    #[test]
    fn the_series_regulator_and_78xx_challenges_diagnose_their_mistakes() {
        // series regulator: the feedback divider missing its gain (R3 tiny) leaves the output near the reference
        let mut s = solution("series_regulator");
        set(&mut s, "R3", "100");
        let r = verdict("series_regulator", s);
        assert_eq!(r[2].0, Some(false), "{r:#?}");
        assert!(r[2].1.contains("R3"), "{r:#?}");
        // too much gain: the output is too high
        let mut s = solution("series_regulator");
        set(&mut s, "R3", "22k");
        assert_eq!(verdict("series_regulator", s)[2].0, Some(false));
        // 78xx: no capacitors, only that check fails
        let mut s = solution("regulator_78xx");
        s.components.retain(|c| c.kind != Kind::Capacitor);
        let r = verdict("regulator_78xx", s);
        let failed: Vec<usize> = r.iter().enumerate().filter(|(_, (ok, _))| *ok == Some(false)).map(|(i, _)| i).collect();
        assert_eq!(failed, vec![5], "{r:#?}");
        assert_eq!(challenge_of_lesson("series_regulator"), Some("series_regulator"));
        assert_eq!(challenge_of_lesson("regulator_78xx"), Some("regulator_78xx"));
    }

    #[test]
    fn passing_is_recorded_under_the_challenges_key() {
        let mut e = editor(solution("zener"), "zener");
        e.run_judge();
        assert_eq!(e.finished_challenge().as_deref(), Some("zener+build"));
        assert_eq!(challenge_of_lesson("zener"), Some("zener"));
        assert_eq!(challenge_of_lesson("opamp_inv"), Some("opamp_inv"));
        assert_eq!(challenge_of_lesson("buck"), None);
        // level 0 keeps the circuit's parts, level 1 only the sources
        let names = |e: &SchEditor| e.sch.components.iter().filter(|c| !c.kind.is_power_symbol()).count();
        e.start_challenge("zener", 0);
        assert_eq!(names(&e), 3);
        e.start_challenge("zener", 1);
        assert_eq!(names(&e), 1);
        e.start_challenge("opamp_inv", 1);
        assert_eq!(names(&e), 3, "the three sources: signal, +5 V and -5 V");
    }

    /// The label that sits on this pin of this part (the solution is wired with labels).
    fn label_on(s: &Schematic, refdes: &str, pin: &str) -> usize {
        let at = s.components.iter().find(|c| c.refdes == refdes).unwrap().pins().into_iter().find(|(n, _)| n == pin).unwrap().1;
        s.labels.iter().position(|l| l.pos == at).unwrap_or_else(|| panic!("no label on {refdes}.{pin}"))
    }

    /// Which checks of the Arduino challenge fail (by position in the list; 0 is the ERC).
    fn arduino_failures(s: Schematic) -> Vec<usize> {
        verdict("draw_arduino", s).iter().enumerate().filter(|(_, (ok, _))| *ok != Some(true)).map(|(i, _)| i).collect()
    }

    #[test]
    fn the_arduino_challenge_judges_the_wiring_and_the_values() {
        let ok = solution("draw_arduino");
        assert!(arduino_failures(ok.clone()).is_empty());
        let names: Vec<String> = verdict("draw_arduino", ok.clone()).into_iter().map(|(_, d)| d).collect();
        assert!(names.iter().all(|d| d.is_empty()), "{names:?}");

        // TXD / RXD not crossed: J1.4 and J1.5 swap their nets
        let mut s = ok.clone();
        let (a, b) = (label_on(&s, "J1", "4"), label_on(&s, "J1", "5"));
        let (ta, tb) = (s.labels[a].text.clone(), s.labels[b].text.clone());
        s.labels[a].text = tb;
        s.labels[b].text = ta;
        let r = verdict("draw_arduino", s);
        let failed: Vec<usize> = r.iter().enumerate().filter(|(_, (ok, _))| *ok != Some(true)).map(|(i, _)| i).collect();
        assert_eq!(failed, vec![10], "{r:#?}");
        assert!(r[10].1.contains("交差") || r[10].1.contains("同じ名前"), "{}", r[10].1);

        // the load capacitor of XTAL2 is the wrong kind of value (100 uF), and the message names the part and the range
        let mut s = ok.clone();
        set(&mut s, "C2", "100u");
        let r = verdict("draw_arduino", s);
        assert_eq!(r.iter().enumerate().filter(|(_, (ok, _))| *ok != Some(true)).map(|(i, _)| i).collect::<Vec<_>>(), vec![4]);
        assert!(r[4].1.contains("C2") && r[4].1.contains("目安の範囲"), "{}", r[4].1);

        // no pull-up on RESET: a 1 Meg resistor is out of range; no resistor at all is "not found"
        let mut s = ok.clone();
        set(&mut s, "R1", "1Meg");
        assert_eq!(arduino_failures(s), vec![5]);
        let mut s = ok.clone();
        s.components.retain(|c| c.refdes != "R1");
        assert_eq!(arduino_failures(s), vec![5]);

        // AREF without its capacitor, and only one bypass capacitor on the supply
        let mut s = ok.clone();
        s.components.retain(|c| c.refdes != "C5");
        let r = verdict("draw_arduino", s);
        assert_eq!(r.iter().enumerate().filter(|(_, (ok, _))| *ok != Some(true)).map(|(i, _)| i).collect::<Vec<_>>(), vec![7]);
        assert!(r[7].1.contains("AREF"), "{}", r[7].1);
        let mut s = ok.clone();
        s.components.retain(|c| c.refdes != "C4");
        let r = verdict("draw_arduino", s);
        assert_eq!(r.iter().enumerate().filter(|(_, (ok, _))| *ok != Some(true)).map(|(i, _)| i).collect::<Vec<_>>(), vec![7]);
        assert!(r[7].1.contains("2 個必要"), "{}", r[7].1);

        // AVCC forgotten: the power check says which pin
        let mut s = ok.clone();
        let i = label_on(&s, "U1", "18");
        s.labels[i].text = "AVCC_FLOATING".into();
        let r = verdict("draw_arduino", s);
        assert_eq!(r.iter().enumerate().filter(|(_, (ok, _))| *ok != Some(true)).map(|(i, _)| i).collect::<Vec<_>>(), vec![1]);
        assert!(r[1].1.contains("U1 の 18 番ピン"), "{}", r[1].1);

        // a board with nothing built fails everything but the ERC; the parts the learner numbers differently are still found by kind
        let mut s = ok.clone();
        for (old, new) in [("C1", "C11"), ("C2", "C12"), ("R2", "R12")] {
            s.components.iter_mut().find(|c| c.refdes == old).unwrap().refdes = new.into();
        }
        assert!(arduino_failures(s).is_empty(), "renumbered capacitors and resistors still count");

        // a library chip capacitor (C_100n_0603) and chip resistor (R_10k_0603) count as a capacitor / resistor
        let lib = |name: &str| part_lib::EXAMPLES.iter().map(|(_, t)| serde_json::from_str::<part_lib::PartDef>(t).unwrap()).find(|d| d.name == name).unwrap();
        let (cap, res) = (lib("C_100n_0603"), lib("R_10k_0603"));
        let mut s = ok.clone();
        for (refdes, def) in [("C3", &cap), ("R1", &res)] {
            let c = s.components.iter_mut().find(|c| c.refdes == refdes).unwrap();
            c.kind = Kind::Custom;
            c.custom = Some(def.symbol());
            c.value = if refdes == "C3" { "100n".into() } else { "10k".into() };
        }
        // (a chip part's pins are named 1 and 2 like the basic part's, so the labels on the solution still sit on them)
        assert!(arduino_failures(s).is_empty(), "library chip parts count too");
    }

    #[test]
    fn the_arduino_challenge_starts_with_the_board_parts_or_only_the_microcontroller() {
        let mut e = SchEditor::default();
        e.start_challenge("draw_arduino", 0);
        let on_sheet = |e: &SchEditor| e.sch.components.iter().filter(|c| !c.kind.is_power_symbol()).map(|c| c.refdes.clone()).collect::<Vec<_>>();
        assert_eq!(on_sheet(&e).len(), 13);
        e.start_challenge("draw_arduino", 1);
        assert_eq!(on_sheet(&e), ["U1", "J1", "V1"]);
        assert_eq!(challenge_of_lesson("reset"), Some("draw_arduino"));
    }
}
