use crate::breakage::*;
use crate::*;

/// A small board that is as it should be: an ATtiny85, its bypass capacitor, a reset pull-up, an LED and a power header.
fn good() -> Board {
    serde_json::from_str(include_str!("../../../docs/examples/work_in_progress/attiny_blinker_board.pcbjson")).unwrap()
}

#[test]
fn the_sample_board_has_nothing_for_the_checks_to_find() {
    assert_eq!(health(&good()), Health::default());
}

#[test]
fn every_fault_can_be_made_and_the_checks_find_it() {
    let g = good();
    let before = health(&g);
    assert_eq!(possible(&g).len(), Fault::ALL.len(), "{:?}", possible(&g));
    for fault in Fault::ALL {
        let mut b = g.clone();
        let inj = inject(&mut b, fault, 1).unwrap_or_else(|e| panic!("{fault:?}: {e}"));
        let now = health(&b);
        assert!(now.total() > before.total(), "{fault:?}: {now:?}");
        assert!(!repaired(&before, &now), "{fault:?}");
        assert_eq!(inj.fault, fault);
        assert!(!inj.symptom.is_empty() && !inj.cause.is_empty() && !inj.target.is_empty());
        // what the learner reads first must not name the cause
        assert!(!inj.symptom.contains(&inj.target) || inj.target.len() < 3, "{fault:?}: {}", inj.symptom);
    }
}

#[test]
fn the_same_seed_breaks_the_same_way() {
    for fault in Fault::ALL {
        let (mut a, mut b) = (good(), good());
        let (x, y) = (inject(&mut a, fault, 7).unwrap(), inject(&mut b, fault, 7).unwrap());
        assert_eq!((x.target, x.at.x, x.at.y), (y.target, y.at.x, y.at.y), "{fault:?}");
    }
}

#[test]
fn the_bypass_capacitor_really_is_far_and_the_board_is_mended_by_moving_it_back_and_wiring_it() {
    let g = good();
    let before = health(&g);
    let mut b = g.clone();
    let inj = inject(&mut b, Fault::BypassFar, 3).unwrap();
    assert_eq!(inj.target, "C1");
    let now = health(&b);
    assert_eq!(now.bypass, 1);
    assert!(now.unrouted > 0, "the tracks of the capacitor are gone, as when a part is dragged: {now:?}");
    // nothing is left lying about: every track still touches something at both ends
    let c1 = g.parts.iter().find(|p| p.refdes == "C1").unwrap();
    b.place_part("C1", c1.pos, c1.rot);
    assert_eq!(health(&b).bypass, 0);
    // and wired again where it is not yet
    for net in ["VCC", "GND"] {
        crate::autoroute::autoroute(&mut b, &crate::autoroute::RouteOptions { track_width: 0.25, grid: 0.1, ..Default::default() }, Some(net));
    }
    let now = health(&b);
    assert!(repaired(&before, &now), "{now:?}");
}

#[test]
fn a_part_off_the_board_leaves_no_loose_tracks() {
    let g = good();
    let mut b = g.clone();
    inject(&mut b, Fault::PartOutside, 4).unwrap();
    assert_eq!(health(&b).outside, 1);
    // every end of every track is on a pad, a via or another track
    for (i, t) in b.tracks.iter().enumerate() {
        for p in [t.a, t.b] {
            let on_pad = b.pads.iter().any(|q| {
                let (w, h) = q.shape.size();
                (p.x - q.pos.x).abs() <= w / 2.0 + 1e-6 && (p.y - q.pos.y).abs() <= h / 2.0 + 1e-6
            });
            let on_track = b.tracks.iter().enumerate().any(|(j, u)| j != i && drc::point_seg(p, u.a, u.b) < 1e-6);
            assert!(on_pad || on_track, "track {i} hangs at ({:.2}, {:.2})", p.x, p.y);
        }
    }
}

#[test]
fn a_cut_track_is_mended_by_putting_it_back() {
    let g = good();
    let mut b = g.clone();
    let inj = inject(&mut b, Fault::CutTrack, 5).unwrap();
    assert_eq!(b.tracks.len(), g.tracks.len() - 1);
    assert!(health(&b).unrouted > 0);
    // the removed track is the one that is not in the damaged board
    let missing = g.tracks.iter().find(|t| !b.tracks.iter().any(|u| u.a == t.a && u.b == t.b)).unwrap().clone();
    b.tracks.push(missing);
    assert!(repaired(&health(&g), &health(&b)), "{:?} after {}", health(&b), inj.cause);
}

#[test]
fn a_stray_track_is_the_one_to_delete() {
    let g = good();
    let mut b = g.clone();
    inject(&mut b, Fault::ShortCircuit, 2).unwrap();
    assert_eq!(b.tracks.len(), g.tracks.len() + 1);
    b.tracks.pop();
    assert!(repaired(&health(&g), &health(&b)));
}

#[test]
fn a_board_that_cannot_have_a_fault_is_left_alone() {
    let mut empty = Board::new("t", 30.0, 20.0);
    assert!(possible(&empty).is_empty());
    for fault in Fault::ALL {
        assert!(inject(&mut empty, fault, 1).is_err(), "{fault:?}");
    }
    assert!(empty.tracks.is_empty() && empty.parts.is_empty());
}
