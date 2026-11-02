//! Breaking a board on purpose, for practice in finding and mending faults: [`inject`] makes one fault the checks of the editor
//! can find, [`health`] counts what those checks find, and a board is mended ([`repaired`]) when it is no worse than it
//! was before it was broken. The same fault comes from the same seed.
//!
//! The faults are the ones a real board has: a bypass capacitor put far from its IC, a track cut through, a track too
//! thin, two nets joined by a stray track, a part off the board. Each comes with the symptom a person would see on the
//! bench and with the cause and the way to mend it (shown after the attempt).

use crate::drc::{check, connectivity, decoupling_checks, MAX_DECOUPLING_DISTANCE};
use crate::geom::inside_polygon;
use crate::model::*;
use crate::ratsnest::ratsnest;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    /// A bypass capacitor moved far from the IC whose supply it serves (its tracks are taken away, as when a part is dragged).
    BypassFar,
    /// One track of a net removed: the net is open.
    CutTrack,
    /// All the tracks of a net made thinner than the board allows.
    ThinTrack,
    /// A stray track between two nets.
    ShortCircuit,
    /// A part moved off the board (its tracks are taken away).
    PartOutside,
}

impl Fault {
    pub const ALL: [Fault; 5] = [Fault::BypassFar, Fault::CutTrack, Fault::ThinTrack, Fault::ShortCircuit, Fault::PartOutside];

    /// The name of the fault, as the learner sees it after the attempt.
    pub fn name(self) -> &'static str {
        match self {
            Fault::BypassFar => "バイパスコンデンサが IC から遠い",
            Fault::CutTrack => "配線が 1 本切れている",
            Fault::ThinTrack => "配線が細すぎる",
            Fault::ShortCircuit => "2 つのネットがショートしている",
            Fault::PartOutside => "部品が基板の外にある",
        }
    }
}

/// What was done to the board.
#[derive(Clone, Debug)]
pub struct Injected {
    pub fault: Fault,
    /// The part or net it happened to.
    pub target: String,
    /// Where (mm): for showing the place after the attempt.
    pub at: Point,
    /// What a person would notice on the bench (it does not name the cause).
    pub symptom: String,
    /// What was wrong and how to mend it.
    pub cause: String,
}

/// What the checks of the editor find on a board.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Health {
    /// Violations of the design rules (clearance, shorts, track width ...).
    pub drc: usize,
    /// Connections that are not made.
    pub unrouted: usize,
    /// Parts with a pad off the board.
    pub outside: usize,
    /// ICs whose supply has no bypass capacitor close enough.
    pub bypass: usize,
}

impl Health {
    /// The sum of everything the checks find.
    pub fn total(&self) -> usize {
        self.drc + self.unrouted + self.outside + self.bypass
    }
}

pub fn health(board: &Board) -> Health {
    let outside = if board.outline.len() < 3 {
        0
    } else {
        board.parts.iter().filter(|p| board.pads.iter().any(|pad| pad.refdes == p.refdes && !inside_polygon(&board.outline, pad.pos))).count()
    };
    Health { drc: check(board).len(), unrouted: ratsnest(board).len(), outside, bypass: decoupling_checks(board).len() }
}

/// The board is mended when none of the checks finds more than it found before the fault.
pub fn repaired(before: &Health, now: &Health) -> bool {
    now.drc <= before.drc && now.unrouted <= before.unrouted && now.outside <= before.outside && now.bypass <= before.bypass
}

/// A small generator of numbers that depend only on the seed (so the same seed breaks the same board the same way).
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// The numbers `0..n` in an order that depends on the seed.
    fn order(&mut self, n: usize) -> Vec<usize> {
        let mut v: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() {
            let j = (self.next() % (i as u64 + 1)) as usize;
            v.swap(i, j);
        }
        v
    }
}

/// Is the point `p` on a pad, a via or another track of the board (not counting track `skip`)?
fn touches(board: &Board, p: Point, skip: usize) -> bool {
    board.pads.iter().any(|q| {
        let (w, h) = q.shape.size();
        (p.x - q.pos.x).abs() <= w / 2.0 + 1e-6 && (p.y - q.pos.y).abs() <= h / 2.0 + 1e-6
    }) || board.vias.iter().any(|v| v.pos.dist(p) <= v.diameter / 2.0 + 1e-6)
        || board.tracks.iter().enumerate().any(|(i, t)| i != skip && crate::drc::point_seg(p, t.a, t.b) < 1e-6)
}

/// Move a part the way the editor does (its tracks stay where they were), then take away the tracks that were on its pads and the
/// pieces they leave hanging: the nets of the part are open, with nothing left lying about.
fn move_part_ripping(board: &mut Board, refdes: &str, pos: Point, rot: u8) {
    let old: Vec<Point> = board.pads.iter().filter(|p| p.refdes == refdes).map(|p| p.pos).collect();
    board.place_part(refdes, pos, rot);
    board.tracks.retain(|t| !old.iter().any(|o| t.a.dist(*o) < 0.01 || t.b.dist(*o) < 0.01));
    loop {
        let hanging = (0..board.tracks.len()).find(|&i| !touches(board, board.tracks[i].a, i) || !touches(board, board.tracks[i].b, i));
        match hanging {
            Some(i) => {
                board.tracks.remove(i);
            }
            None => break,
        }
    }
}

/// The faults that [`inject`] can make on this board (not every board has a bypass capacitor or a net with tracks).
pub fn possible(board: &Board) -> Vec<Fault> {
    Fault::ALL.into_iter().filter(|f| inject(&mut board.clone(), *f, 0).is_ok()).collect()
}

/// One fault of the kind `fault` on the board. `Err` (and the board as it was) when this board cannot have it.
pub fn inject(board: &mut Board, fault: Fault, seed: u64) -> Result<Injected, String> {
    let mut rng = Rng::new(seed);
    let before = health(board);
    let result = match fault {
        Fault::BypassFar => bypass_far(board, &mut rng),
        Fault::CutTrack => cut_track(board, &mut rng),
        Fault::ThinTrack => thin_track(board, &mut rng),
        Fault::ShortCircuit => short_circuit(board, &mut rng),
        Fault::PartOutside => part_outside(board, &mut rng),
    };
    // a fault that no check can see is of no use for practice
    let (inj, undo) = result?;
    if health(board).total() > before.total() {
        Ok(inj)
    } else {
        *board = undo;
        Err("この基板では、点検に引っかかる故障にならなかったので、取りやめました".into())
    }
}

fn cut_track(board: &mut Board, rng: &mut Rng) -> Result<(Injected, Board), String> {
    let undo = board.clone();
    let open = ratsnest(board).len();
    let conn = connectivity(board);
    for i in rng.order(board.tracks.len()) {
        let Some(net) = conn.track_net(i).map(str::to_string) else { continue };
        let mut trial = board.clone();
        let t = trial.tracks.remove(i);
        if ratsnest(&trial).len() > open {
            let at = Point::new((t.a.x + t.b.x) / 2.0, (t.a.y + t.b.y) / 2.0);
            *board = trial;
            return Ok((
                Injected {
                    fault: Fault::CutTrack,
                    symptom: "電源を入れても、動かない部分があります。電気が届いていない部品があるようです".into(),
                    cause: format!("ネット「{net}」の配線が 1 本、切れていました (点 {:.1}, {:.1} mm のあたり)。未配線の黄色の線が手がかりです。その線をなぞって、つなぎ直します", at.x, at.y),
                    target: net,
                    at,
                },
                undo,
            ));
        }
    }
    Err("切っても未配線にならない配線しかありません (配線がありません)".into())
}

fn thin_track(board: &mut Board, rng: &mut Rng) -> Result<(Injected, Board), String> {
    let undo = board.clone();
    let thin = (board.rules.min_track_width * 0.5).max(0.05);
    let conn = connectivity(board);
    let mut nets: Vec<String> = (0..board.tracks.len()).filter_map(|i| conn.track_net(i).map(str::to_string)).collect();
    nets.sort();
    nets.dedup();
    for k in rng.order(nets.len()) {
        let net = &nets[k];
        let idx: Vec<usize> = (0..board.tracks.len()).filter(|&i| conn.track_net(i) == Some(net.as_str())).collect();
        let len: f64 = idx.iter().map(|&i| board.tracks[i].a.dist(board.tracks[i].b)).sum();
        if len < 3.0 || idx.iter().all(|&i| board.tracks[i].width <= thin) {
            continue;
        }
        let before = check(board).len();
        let mut trial = board.clone();
        for &i in &idx {
            trial.tracks[i].width = thin;
        }
        if check(&trial).len() > before {
            let mid = board.tracks[idx[0]].a;
            *board = trial;
            return Ok((
                Injected {
                    fault: Fault::ThinTrack,
                    symptom: "動くことは動きますが、電流を流すと配線が熱くなり、電圧が下がります".into(),
                    cause: format!("ネット「{net}」の配線が {thin:.2}mm と、基板の最小線幅 ({:.2}mm) より細くなっていました。DRC が示す配線を選んで、太くします", board.rules.min_track_width),
                    target: net.clone(),
                    at: mid,
                },
                undo,
            ));
        }
    }
    Err("細くしても点検に引っかかる配線がありません".into())
}

fn short_circuit(board: &mut Board, rng: &mut Rng) -> Result<(Injected, Board), String> {
    let undo = board.clone();
    // pairs of pads of different nets on one side that are not far apart
    let mut pairs: Vec<(usize, usize, f64)> = vec![];
    for (i, a) in board.pads.iter().enumerate() {
        for (j, b) in board.pads.iter().enumerate().skip(i + 1) {
            if a.net.is_empty() || b.net.is_empty() || a.net == b.net || a.side != b.side {
                continue;
            }
            let d = a.pos.dist(b.pos);
            if d <= 8.0 {
                pairs.push((i, j, d));
            }
        }
    }
    let before = check(board).len();
    for k in rng.order(pairs.len()) {
        let (i, j, _) = pairs[k];
        let (a, b) = (&board.pads[i], &board.pads[j]);
        let mut trial = board.clone();
        trial.tracks.push(Track { side: a.side, a: a.pos, b: b.pos, width: board.rules.min_track_width.max(0.2) });
        if check(&trial).len() > before {
            let at = Point::new((a.pos.x + b.pos.x) / 2.0, (a.pos.y + b.pos.y) / 2.0);
            let (na, nb) = (a.net.clone(), b.net.clone());
            *board = trial;
            return Ok((
                Injected {
                    fault: Fault::ShortCircuit,
                    symptom: "電源を入れると、すぐに熱くなる、または電源が落ちます".into(),
                    cause: format!("ネット「{na}」と「{nb}」の間に、余計な配線が 1 本入っていて、ショートしていました (点 {:.1}, {:.1} mm のあたり)。DRC が赤く示す所の余計な配線を選んで、消します", at.x, at.y),
                    target: format!("{na} / {nb}"),
                    at,
                },
                undo,
            ));
        }
    }
    Err("ショートさせられる、近くのパッドの組がありません".into())
}

fn part_outside(board: &mut Board, rng: &mut Rng) -> Result<(Injected, Board), String> {
    let undo = board.clone();
    if board.outline.len() < 3 || board.parts.is_empty() {
        return Err("基板の外形か部品がありません".into());
    }
    let max_x = board.outline.iter().map(|p| p.x).fold(f64::MIN, f64::max);
    for k in rng.order(board.parts.len()) {
        let part = board.parts[k].clone();
        let mut trial = board.clone();
        move_part_ripping(&mut trial, &part.refdes, Point::new(max_x + 6.0, part.pos.y), part.rot);
        if health(&trial).outside > health(board).outside {
            *board = trial;
            return Ok((
                Injected {
                    fault: Fault::PartOutside,
                    symptom: "基板を作ってみたら、部品が 1 つ、基板に載りません".into(),
                    cause: format!("部品 {} が基板の外 (右の外) に出ていました。基板の中の空いた所へ動かして、配線をつなぎ直します", part.refdes),
                    target: part.refdes.clone(),
                    at: Point::new(max_x + 6.0, part.pos.y),
                },
                undo,
            ));
        }
    }
    Err("基板の外へ出せる部品がありません".into())
}

fn bypass_far(board: &mut Board, rng: &mut Rng) -> Result<(Injected, Board), String> {
    let undo = board.clone();
    if board.outline.len() < 3 {
        return Err("基板の外形がありません".into());
    }
    // (capacitor, IC, supply net, the pads of the IC on that net) that are served within the distance now
    let mut cands: Vec<(String, String, Vec<Point>)> = vec![];
    for c in board.parts.iter().filter(|p| p.refdes.starts_with('C')) {
        for cp in board.pads.iter().filter(|p| p.refdes == c.refdes && !p.net.is_empty()) {
            for u in board.parts.iter().filter(|p| p.refdes.starts_with('U')) {
                let ic_pads: Vec<Point> = board.pads.iter().filter(|p| p.refdes == u.refdes && p.net == cp.net).map(|p| p.pos).collect();
                if !ic_pads.is_empty() && ic_pads.iter().any(|q| q.dist(cp.pos) <= MAX_DECOUPLING_DISTANCE) && !cands.iter().any(|x| x.0 == c.refdes) {
                    cands.push((c.refdes.clone(), u.refdes.clone(), ic_pads));
                }
            }
        }
    }
    let (min_x, max_x) = board.outline.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.x), hi.max(p.x)));
    let (min_y, max_y) = board.outline.iter().fold((f64::MAX, f64::MIN), |(lo, hi), p| (lo.min(p.y), hi.max(p.y)));
    for k in rng.order(cands.len()) {
        let (cref, uref, ic_pads) = &cands[k];
        let part = board.parts.iter().find(|p| &p.refdes == cref).cloned().unwrap();
        // free places inside the board, far enough from the IC, away from the pads of other parts
        let mut spots: Vec<Point> = vec![];
        let mut y = min_y + 2.0;
        while y <= max_y - 2.0 {
            let mut x = min_x + 2.0;
            while x <= max_x - 2.0 {
                let p = Point::new(x, y);
                let far = ic_pads.iter().all(|q| q.dist(p) >= MAX_DECOUPLING_DISTANCE + 3.0);
                let clear = board.pads.iter().all(|q| q.refdes == part.refdes || q.pos.dist(p) >= 3.0);
                if far && clear && inside_polygon(&board.outline, p) {
                    spots.push(p);
                }
                x += 1.0;
            }
            y += 1.0;
        }
        if spots.is_empty() {
            continue;
        }
        let spot = spots[(rng.next() % spots.len() as u64) as usize];
        let mut trial = board.clone();
        move_part_ripping(&mut trial, &part.refdes, spot, part.rot);
        let was = decoupling_checks(board).len();
        if decoupling_checks(&trial).len() > was {
            let d = ic_pads.iter().map(|q| q.dist(spot)).fold(f64::MAX, f64::min);
            *board = trial;
            return Ok((
                Injected {
                    fault: Fault::BypassFar,
                    symptom: "ときどき勝手にリセットされる、または動きが不安定です (電源の電圧が揺れているようです)".into(),
                    cause: format!(
                        "バイパスコンデンサ {cref} が、IC {uref} の電源ピンから {d:.0}mm も離れていました (目安は {MAX_DECOUPLING_DISTANCE:.0}mm 以内)。遠いと、配線のインダクタンスで、速い電流の変化に追いつけません。{uref} のすぐそばへ動かして、配線をつなぎ直します"
                    ),
                    target: cref.clone(),
                    at: spot,
                },
                undo,
            ));
        }
    }
    Err("バイパスコンデンサを遠ざけられる所がありません".into())
}
