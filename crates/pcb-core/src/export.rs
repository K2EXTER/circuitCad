//! Assemble the full fabrication package (Gerber + drill, zipped).

use crate::{excellon, gerber, Board, Side};
use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;

/// (file name, contents). Names follow the KiCad/Protel convention that
/// JLCPCB, PCBWay, OSH Park etc. recognise automatically.
pub fn fab_files(board: &Board) -> Vec<(String, String)> {
    let n = &board.name;
    vec![
        (format!("{n}.GTL"), gerber::copper(board, Side::Top)),
        (format!("{n}.GBL"), gerber::copper(board, Side::Bottom)),
        (format!("{n}.GTS"), gerber::mask(board, Side::Top)),
        (format!("{n}.GBS"), gerber::mask(board, Side::Bottom)),
        (format!("{n}.GTO"), gerber::silk(board, Side::Top)),
        (format!("{n}.GBO"), gerber::silk(board, Side::Bottom)),
        (format!("{n}.GKO"), gerber::outline(board)),
        (format!("{n}-PTH.DRL"), excellon::plated(board)),
        (format!("{n}-NPTH.DRL"), excellon::non_plated(board)),
    ]
}

pub fn zip_bytes(board: &Board) -> zip::result::ZipResult<Vec<u8>> {
    let mut zw = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in fab_files(board) {
        zw.start_file(name, opts)?;
        zw.write_all(data.as_bytes())?;
    }
    Ok(zw.finish()?.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::*;

    fn sample() -> Board {
        let mut b = Board::new("demo", 30.0, 20.0);
        b.pads.push(Pad {
            pos: Point::new(5.0, 5.0),
            shape: PadShape::Circle { d: 1.6 },
            drill: Some(0.8),
            side: Side::Top,
        });
        b.pads.push(Pad {
            pos: Point::new(15.0, 5.0),
            shape: PadShape::Rect { w: 1.0, h: 1.5 },
            drill: None,
            side: Side::Top,
        });
        b.tracks.push(Track {
            side: Side::Top,
            a: Point::new(5.0, 5.0),
            b: Point::new(15.0, 5.0),
            width: 0.25,
        });
        b.vias.push(Via { pos: Point::new(10.0, 10.0), diameter: 0.6, drill: 0.3 });
        b.holes.push(Hole { pos: Point::new(2.0, 2.0), drill: 3.2 });
        b
    }

    #[test]
    fn gerber_copper_contents() {
        let g = gerber::copper(&sample(), Side::Top);
        assert!(g.contains("%FSLAX46Y46*%"));
        assert!(g.contains("%MOMM*%"));
        assert!(g.contains("%ADD10C,0.250000*%"));
        assert!(g.contains("X5000000Y5000000D02*"));
        assert!(g.contains("X15000000Y5000000D01*"));
        assert!(g.contains("R,1.000000X1.500000"));
        assert!(g.ends_with("M02*\n"));
    }

    #[test]
    fn smd_pad_not_on_bottom() {
        let g = gerber::copper(&sample(), Side::Bottom);
        assert!(!g.contains("R,1.000000X1.500000"));
        assert!(g.contains("C,1.600000")); // through-hole pad
    }

    #[test]
    fn vias_are_tented() {
        let g = gerber::mask(&sample(), Side::Top);
        assert!(!g.contains("X10000000Y10000000"));
    }

    #[test]
    fn drill_files() {
        let d = excellon::plated(&sample());
        assert!(d.contains("T1C0.300"));
        assert!(d.contains("T2C0.800"));
        assert!(d.contains("X5.0000Y5.0000"));
        assert!(d.ends_with("M30\n"));
        assert!(excellon::non_plated(&sample()).contains("C3.200"));
    }

    #[test]
    fn zip_has_all_files() {
        let bytes = zip_bytes(&sample()).unwrap();
        let ar = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        assert_eq!(ar.len(), 9);
    }

    #[test]
    fn drc_flags_problems() {
        let mut b = sample();
        assert!(drc::check(&b).is_empty());
        b.tracks.push(Track {
            side: Side::Top,
            a: Point::new(1.0, 1.0),
            b: Point::new(50.0, 1.0),
            width: 0.05,
        });
        let msgs: Vec<_> = drc::check(&b).into_iter().map(|v| v.message).collect();
        assert!(msgs.iter().any(|m| m.contains("track width")));
        assert!(msgs.iter().any(|m| m.contains("outside board outline")));
    }
}
