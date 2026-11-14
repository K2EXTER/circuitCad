//! Looking into a data sheet from the command line, for making a part's model from it (docs/model-policy.md):
//!
//!     cargo run -p datasheet --example ds -- text   FILE.pdf PAGE              the words of a page, line by line, with x
//!     cargo run -p datasheet --example ds -- table  FILE.pdf SYMBOL [...]      min / typ / max, unit, conditions of a symbol
//!     cargo run -p datasheet --example ds -- labels FILE.pdf PAGE [box=x0,y0,x1,y1]   numbers on the page (tick labels)
//!     cargo run -p datasheet --example ds -- render FILE.pdf PAGE OUT.png [scale=2] [box=x0,y0,x1,y1]
//!     cargo run -p datasheet --example ds -- trace  FILE.pdf PAGE box=x0,y0,x1,y1 x=L1,L2 y=L1,L2 [xlog] [ylog] [rows] [abs] [keepthin] at=V1,V2,...
//!     cargo run -p datasheet --example ds -- grid   FILE.pdf PAGE box=x0,y0,x1,y1   the grid lines (for labels that are pictures:
//!                                                                                  then trace with xt=V@X,.. yt=V@Y,..)
//!
//! Pages count from 1; positions are PDF points from the top left. `trace` reads a graph inside `box`: the axes from
//! the tick labels named in `x=` (the lowest of that text in the box) and `y=` (the leftmost), then the curves'
//! values at each `at=` value of the horizontal axis, from the page's picture (`datasheet::graph::curve_crossings`);
//! with `rows`, `at=` is a value of the vertical axis and the answer is where the curves are across (left to right).
//! Readings for the bundled parts go into `crates/part-lib/fits/*.fit.json`.

use datasheet::graph::{Axis, Graph, Tick, number_labels};
use datasheet::{Datasheet, Rect};

fn arg<'a>(args: &'a [String], key: &str) -> Option<&'a str> {
    args.iter().find_map(|a| a.strip_prefix(&format!("{key}=")))
}

fn rect(s: Option<&str>) -> Option<Rect> {
    let v: Vec<f32> = s?.split(',').filter_map(|t| t.trim().parse().ok()).collect();
    let [x0, y0, x1, y1] = v.as_slice() else { return None };
    Some(Rect { x0: *x0, y0: *y0, x1: *x1, y1: *y1 })
}

fn inside(r: &Option<Rect>, x: f32, y: f32) -> bool {
    r.as_ref().is_none_or(|r| r.x0 <= x && x <= r.x1 && r.y0 <= y && y <= r.y1)
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (Some(cmd), Some(file)) = (args.first(), args.get(1)) else {
        return Err("ds text|table|labels|render|trace FILE.pdf ... (crates/datasheet/examples/ds.rs)".into());
    };
    let ds = Datasheet::open_file(std::path::Path::new(file))?;
    let page = || -> Result<usize, String> { args.get(2).and_then(|p| p.parse::<usize>().ok()).filter(|p| *p >= 1).map(|p| p - 1).ok_or_else(|| "PAGE (from 1)".to_string()) };
    match cmd.as_str() {
        "text" => {
            let p = page()?;
            let mut w = ds.words(p).to_vec();
            w.sort_by(|a, b| ((a.rect.y0 + a.rect.y1) / 2.0).total_cmp(&((b.rect.y0 + b.rect.y1) / 2.0)).then(a.rect.x0.total_cmp(&b.rect.x0)));
            let mut last = f32::MIN;
            let mut line = String::new();
            for x in &w {
                let y = (x.rect.y0 + x.rect.y1) / 2.0;
                if (y - last).abs() > 2.5 {
                    if !line.is_empty() {
                        println!("{line}");
                    }
                    line = format!("{y:6.1}:");
                    last = y;
                }
                line += &format!(" [{:.0}]{}", x.rect.x0, x.text);
            }
            println!("{line}");
        }
        "table" => {
            for sym in &args[2..] {
                for v in ds.spec_values(sym) {
                    let f = |x: Option<f64>| x.map_or("-".to_string(), |x| format!("{x}"));
                    println!("p.{} {} | {} | min {} typ {} max {} {}", v.page + 1, v.symbol, v.conditions, f(v.min), f(v.typ), f(v.max), v.unit.as_deref().unwrap_or("?"));
                }
            }
        }
        "labels" => {
            let p = page()?;
            let b = rect(arg(&args, "box"));
            for (t, x, y) in number_labels(ds.words(p)).into_iter().filter(|l| inside(&b, l.1, l.2)) {
                println!("{t}\t{x:.1}\t{y:.1}");
            }
        }
        "render" => {
            let p = page()?;
            let out = args.get(3).ok_or("render FILE PAGE OUT.png")?;
            let scale: f32 = arg(&args, "scale").and_then(|s| s.parse().ok()).unwrap_or(2.0);
            let img = ds.render(p, scale)?;
            let k = img.width as f32 / ds.page_size(p).map_or(1.0, |s| s.0);
            let full = image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.rgba).ok_or("picture")?;
            let pic = match rect(arg(&args, "box")) {
                Some(r) => image::imageops::crop_imm(&full, (r.x0 * k) as u32, (r.y0 * k) as u32, ((r.x1 - r.x0) * k) as u32, ((r.y1 - r.y0) * k) as u32).to_image(),
                None => full,
            };
            pic.save(out).map_err(|e| e.to_string())?;
            println!("{out} ({} x {} px, {k:.2} px per point)", pic.width(), pic.height());
        }
        "trace" => {
            let p = page()?;
            let b = rect(arg(&args, "box"));
            let Some(bx) = b else { return Err("trace needs box=x0,y0,x1,y1 (the graph with its tick labels)".into()) };
            let labels: Vec<_> = number_labels(ds.words(p)).into_iter().filter(|l| inside(&b, l.1, l.2)).collect();
            let pick = |list: Option<&str>, horizontal: bool| -> Result<Axis, String> {
                let mut ticks = vec![];
                // ticks given by hand: xt= / yt= VALUE@POSITION (page points), for labels that are pictures (`ds grid`)
                for t in arg(&args, if horizontal { "xt" } else { "yt" }).unwrap_or("").split(',').filter(|s| !s.is_empty()) {
                    let (v, at) = t.split_once('@').ok_or_else(|| format!("{t}: write VALUE@POSITION"))?;
                    let value = datasheet::graph::tick_value(v).ok_or_else(|| format!("{v}: not a number"))?;
                    let value = if args.iter().any(|a| a == "abs") { value.abs() } else { value };
                    ticks.push(Tick { at: at.parse().map_err(|_| format!("{at}: not a position"))?, value });
                }
                for t in list.unwrap_or("").split(',').filter(|s| !s.is_empty()) {
                    let same = labels.iter().filter(|l| l.0 == t);
                    let l = if horizontal { same.max_by(|a, b| a.2.total_cmp(&b.2)) } else { same.min_by(|a, b| a.1.total_cmp(&b.1)) };
                    let l = l.ok_or_else(|| format!("no label '{t}' in the box"))?;
                    // `abs`: a PNP's graph labels its axes with negative numbers; its readings are written as magnitudes
                    let value = datasheet::graph::tick_value(t).ok_or("label")?;
                    let value = if args.iter().any(|a| a == "abs") { value.abs() } else { value };
                    ticks.push(Tick { at: if horizontal { l.1 } else { l.2 }, value });
                }
                let a = Axis { log: args.iter().any(|a| a == if horizontal { "xlog" } else { "ylog" }), ticks };
                match a.problem() {
                    Some(e) => Err(format!("{} axis: {e}", if horizontal { "x" } else { "y" })),
                    None => Ok(a),
                }
            };
            let g = Graph { page: p, x: pick(arg(&args, "x"), true)?, y: pick(arg(&args, "y"), false)?, points: vec![] };
            // the plot: the box (its frame lines are straight and left out, its labels are words and left out)
            let (top, bottom) = (bx.y0, bx.y1);
            let scale: f32 = arg(&args, "scale").and_then(|s| s.parse().ok()).unwrap_or(4.0);
            let img = ds.render(p, scale)?;
            let k = img.width as f32 / ds.page_size(p).map_or(1.0, |s| s.0);
            // `rows`: at= is a value of the vertical axis, and the answer the horizontal values (a forward voltage at a current)
            let rows = args.iter().any(|a| a == "rows");
            for v in arg(&args, "at").unwrap_or("").split(',').filter_map(datasheet::graph::tick_value) {
                if rows {
                    let skip: Vec<Rect> = ds.words(p).iter().map(|w| w.rect).collect();
                    let vals = g.curve_values_at(&img, k, v, bx.x0, bx.x1, &skip);
                    println!("{v}\t{}", vals.iter().map(|x| format!("{x:.4}")).collect::<Vec<_>>().join("\t"));
                    continue;
                }
                let skip: Vec<Rect> = ds.words(p).iter().map(|w| w.rect).collect();
                let vals = if args.iter().any(|a| a == "keepthin") {
                    g.curve_values_keep_thin(&img, k, v, top - 1.0, bottom + 1.0, &skip)
                } else {
                    g.curve_values(&img, k, v, top - 1.0, bottom + 1.0, &skip)
                };
                println!("{v}\t{}", vals.iter().map(|x| format!("{x:.5}")).collect::<Vec<_>>().join("\t"));
            }
        }
        "grid" => {
            let p = page()?;
            let b = rect(arg(&args, "box")).ok_or("grid needs box=x0,y0,x1,y1 (the plot, inside its frame)")?;
            let img = ds.render(p, 4.0)?;
            let k = img.width as f32 / ds.page_size(p).map_or(1.0, |s| s.0);
            for (name, vertical) in [("x (vertical lines)", true), ("y (horizontal lines)", false)] {
                let lines = datasheet::graph::grid_lines(&img, k, b, vertical);
                println!("{name}: {}", lines.iter().map(|(c, w)| format!("{c:.1}{}", if *w > 0.9 { "*" } else { "" })).collect::<Vec<_>>().join(" "));
            }
            println!("(* = thick: usually the decades or the labelled lines)");
        }
        other => return Err(format!("unknown command {other}")),
    }
    Ok(())
}
