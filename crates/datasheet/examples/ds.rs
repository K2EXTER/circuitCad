//! Looking into a data sheet from the command line, for making a part's model from it (docs/model-policy.md):
//!
//!     cargo run -p datasheet --example ds -- text   FILE.pdf PAGE              the words of a page, line by line, with x
//!     cargo run -p datasheet --example ds -- figures FILE.pdf                  the graph captions, the curve each shows, the side of its graph
//!     cargo run -p datasheet --example ds -- axes FILE.pdf                     the axes read from the tick labels of each graph the form uses
//!     cargo run -p datasheet --example ds -- pick FILE.pdf                     the curve each graph's field would take, and the label it was taken by
//!     cargo run -p datasheet --example ds -- ocr FILE.pdf PAGE                 the words the OCR of Windows reads off the drawn page
//!     (any command) ... ocr=1                                               pages without text read by the OCR (a scanned PDF)
//!     cargo run -p datasheet --example ds -- absmax FILE.pdf                   the absolute maximum ratings read (the first part number's column)
//!     cargo run -p datasheet --example ds -- kind   FILE.pdf                   the kind of part and the part numbers it suggests
//!     cargo run -p datasheet --example ds -- table  FILE.pdf SYMBOL [...]      min / typ / max, unit, conditions of a symbol
//!     cargo run -p datasheet --example ds -- labels FILE.pdf PAGE [box=x0,y0,x1,y1]   numbers on the page (tick labels)
//!     cargo run -p datasheet --example ds -- render FILE.pdf PAGE OUT.png [scale=2] [box=x0,y0,x1,y1]
//!     cargo run -p datasheet --example ds -- trace  FILE.pdf PAGE box=x0,y0,x1,y1 x=L1,L2 y=L1,L2 [xlog] [ylog] [rows] [abs] [keepthin] at=V1,V2,...
//!     cargo run -p datasheet --example ds -- trace  FILE.pdf PAGE box=.. x=.. y=.. follow=X:Y [n=12]   follow one curve
//!     cargo run -p datasheet --example ds -- grid   FILE.pdf PAGE box=x0,y0,x1,y1   the grid lines (for labels that are pictures:
//!                                                                                  then trace with xt=V@X,.. yt=V@Y,..)
//!
//! Pages count from 1; positions are PDF points from the top left. `trace` reads a graph inside `box`: the axes from
//! the tick labels named in `x=` (the lowest of that text in the box) and `y=` (the leftmost), then the curves'
//! values at each `at=` value of the horizontal axis, from the page's picture (`datasheet::graph::curve_crossings`);
//! with `rows`, `at=` is a value of the vertical axis and the answer is where the curves are across (left to right).
//! With `follow=X:Y` (a point on a curve, in the axes' values) the one curve through it is followed to its ends
//! (`datasheet::graph::trace_curve`) and `n=` points spread along it are printed (`spread_along`), as the graph window's
//! 「曲線をたどる」 does.
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
    // `ocr=1`: pages without text (a scanned PDF) are read by the OCR of Windows
    ds.set_ocr(args.iter().any(|a| a == "ocr=1"));
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
        "absmax" => {
            for r in ds.absolute_maximum_of(ds.name_candidates().first().map(String::as_str)) {
                println!("p.{} {} {} {}", r.page + 1, r.symbol, r.value, r.unit);
            }
        }
        "figures" => {
            for f in ds.figures() {
                let side = if f.area.y0 < f.rect.y0 { "graph above" } else { "graph below" };
                println!("p.{} {:?} {} ({side})", f.page + 1, f.curves, f.caption);
            }
        }
        "thermal" => {
            // ds thermal FILE: the thermal resistances (table rows and deratings)
            for r in ds.thermal() {
                println!("p.{} {:?} {:.3} K/W cols={}{}{}  | {}", r.page + 1, r.to, r.k_per_w, r.columns, if r.from_derating { " (derating)" } else { "" }, r.derate_from.map_or(String::new(), |t| format!(" from {t} °C")), r.text);
            }
        }
        "cond" => {
            // ds cond FILE NAME: the values of NAME=… printed in each graph ("VCE")
            let name = args.get(2).map(String::as_str).unwrap_or("VCE");
            for f in ds.figures().iter().filter(|f| !f.curves.is_empty()) {
                println!("p.{} {:?} {}: {:?}", f.page + 1, f.curves, f.caption, ds.figure_condition(f, name));
            }
        }
        "axes" => {
            for f in ds.figures().iter().filter(|f| !f.curves.is_empty()) {
                let show = |a: &datasheet::axes::ReadAxis| format!("{}{} ×{} [{}]", if a.log { "log " } else { "" }, a.ticks.len(), a.prefix, a.ticks.iter().map(|t| t.1.as_str()).collect::<Vec<_>>().join(" "));
                match ds.figure_axes(f) {
                    Some(a) => println!("p.{} {:?} {}
    x: {}
    y: {}", f.page + 1, f.curves, f.caption, show(&a.x), show(&a.y)),
                    None => println!("p.{} {:?} {}
    (no axes)", f.page + 1, f.curves, f.caption),
                }
            }
        }
        "pick" => {
            for f in ds.figures().iter().filter(|f| !f.curves.is_empty()) {
                let Some(a) = ds.figure_axes(f) else { continue };
                let img = ds.render(f.page, 4.0)?;
                let k = img.width as f32 / ds.page_size(f.page).map_or(1.0, |s| s.0);
                let names: Vec<(&str, &[&str])> = if f.curves.contains(&datasheet::figures::Curve::Capacitance) { vec![("Cob", &["cob", "cobo"]), ("Cib", &["cib", "cibo"]), ("", &[])] } else { vec![("", &[])] };
                for (&c, (tag, names)) in f.curves.iter().flat_map(|c| names.iter().map(move |n| (c, n))) {
                    let picked = datasheet::pick::pick(&img, k, ds.words(f.page), &a, c, names);
                    // `save=DIR`: the page with the curve taken drawn in red, one picture a curve
                    if let (Some(dir), datasheet::pick::Pick::One { path, .. }) = (arg(&args, "save"), &picked) {
                        let mut pic = image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.rgba.clone()).ok_or("picture")?;
                        for &(x, y) in path {
                            for d in -2i32..=2 {
                                let (px, py) = ((x * k) as i32, (y * k) as i32 + d);
                                if px >= 0 && py >= 0 && (px as u32) < pic.width() && (py as u32) < pic.height() {
                                    pic.put_pixel(px as u32, py as u32, image::Rgba([255, 0, 0, 255]));
                                }
                            }
                        }
                        let out = format!("{dir}/pick_p{}_{c:?}{tag}.png", f.page + 1);
                        pic.save(&out).map_err(|e| e.to_string())?;
                    }
                    let what = match picked {
                        datasheet::pick::Pick::One { path, by } => format!("one by {by:?}: x {:.0}..{:.0} y {:.0}..{:.0}", path[0].0, path[path.len() - 1].0, path[0].1, path[path.len() - 1].1),
                        datasheet::pick::Pick::Several(c) => format!("{} curves, no label", c.len()),
                        datasheet::pick::Pick::None => "no curve".into(),
                    };
                    println!("p.{} {} {c:?}{tag}: {what}", f.page + 1, f.caption);
                }
            }
        }
        "ocr" => {
            // the words the OCR reads off a page, in page points: `box=x0,y0,x1,y1` a part of it, `px=4` pixels per
            // point, `bin=160` black and white at that grey, `thick=1` strokes made thicker by that many pixels
            let p = page()?;
            let (w, h) = ds.page_size(p).ok_or("page size")?;
            let b = rect(arg(&args, "box")).unwrap_or(Rect { x0: 0.0, y0: 0.0, x1: w, y1: h });
            let px: f32 = arg(&args, "px").and_then(|s| s.parse().ok()).unwrap_or(2400.0 / (b.x1 - b.x0).max(b.y1 - b.y0));
            let page_img = ds.render(p, px)?;
            let k = page_img.width as f32 / w;
            let (cx, cy) = ((b.x0 * k) as usize, (b.y0 * k) as usize);
            let (cw, ch) = ((((b.x1 - b.x0) * k) as usize).min(page_img.width - cx), (((b.y1 - b.y0) * k) as usize).min(page_img.height - cy));
            let mut rgba = Vec::with_capacity(cw * ch * 4);
            for y in cy..cy + ch {
                rgba.extend_from_slice(&page_img.rgba[(y * page_img.width + cx) * 4..(y * page_img.width + cx + cw) * 4]);
            }
            let mut img = datasheet::PageImage { width: cw, height: ch, rgba };
            if let Some(t) = arg(&args, "bin").and_then(|s| s.parse::<u32>().ok()) {
                for q in img.rgba.as_chunks_mut::<4>().0 {
                    let l = (299 * q[0] as u32 + 587 * q[1] as u32 + 114 * q[2] as u32) / 1000;
                    let v = if l < t { 0 } else { 255 };
                    q[..3].copy_from_slice(&[v, v, v]);
                }
            }
            if let Some(n) = arg(&args, "thick").and_then(|s| s.parse::<usize>().ok()) {
                for _ in 0..n {
                    let src = img.rgba.clone();
                    let dark = |x: usize, y: usize| src[(y * cw + x) * 4] < 128;
                    for y in 1..ch - 1 {
                        for x in 1..cw - 1 {
                            if dark(x - 1, y) || dark(x + 1, y) || dark(x, y - 1) || dark(x, y + 1) {
                                img.rgba[(y * cw + x) * 4..(y * cw + x) * 4 + 3].copy_from_slice(&[0, 0, 0]);
                            }
                        }
                    }
                }
            }
            if let Some(out) = arg(&args, "save") {
                image::RgbaImage::from_raw(cw as u32, ch as u32, img.rgba.clone()).ok_or("image")?.save(out).map_err(|e| e.to_string())?;
            }
            for o in datasheet::ocr::read(&img).ok_or("no OCR (Windows only, or the picture is too big)")? {
                println!("{:6.1} {:6.1}  {}", b.x0 + o.x0 / k, b.y0 + (o.y0 + o.y1) / 2.0 / k, o.text);
            }
        }
        "words" => {
            // every word of a page with its box and the code points of its text (to see odd characters)
            let p = page()?;
            let b = rect(arg(&args, "box"));
            for w in ds.words(p).iter().filter(|w| inside(&b, w.rect.x0, w.rect.y0)) {
                println!("{:7.2} {:7.2} {:7.2} {:7.2}  {:?} {:?}", w.rect.x0, w.rect.y0, w.rect.x1, w.rect.y1, w.text, w.text.chars().map(|c| c as u32).collect::<Vec<_>>());
            }
        }
        "kind" => {
            println!("{:?} {:?} p_type={:?}", ds.category_hint(), ds.name_candidates(), ds.p_type());
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
            if let Some(f) = arg(&args, "follow") {
                let (fx, fy) = f.split_once(':').ok_or("follow=X:Y (values of the axes)")?;
                let (fx, fy) = (datasheet::graph::tick_value(fx).ok_or("follow: X")?, datasheet::graph::tick_value(fy).ok_or("follow: Y")?);
                let seed = (g.x.position(fx).ok_or("follow: X off the axis")?, g.y.position(fy).ok_or("follow: Y off the axis")?);
                let skip: Vec<Rect> = ds.words(p).iter().map(|w| w.rect).collect();
                let path = datasheet::graph::trace_curve(&img, k, seed, &skip);
                if path.is_empty() {
                    return Err("no curve within 6 points of follow=".into());
                }
                let n: usize = arg(&args, "n").and_then(|s| s.parse().ok()).unwrap_or(12);
                println!("# traced {} steps, page x {:.1} .. {:.1}", path.len(), path[0].0, path[path.len() - 1].0);
                for (x, y) in datasheet::graph::spread_along(&path, n) {
                    if let Some((a, b)) = g.value_at(x, y) {
                        println!("{a:.5}	{b:.5}");
                    }
                }
                return Ok(());
            }
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
