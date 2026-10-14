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
}
