//! Command line: `circuitCad [file.schjson | file.pcbjson | file.ccproj] [options]`. Mostly useful for scripts, for screenshots and
//! for checking a build. The options are read in a fixed order (files are opened before the analyses on them, the part form's
//! `--form-save` and the `--pdf` / `--svg` exports quit the app), so each topic has its own file and `apply_args` calls them in turn:
//!
//! - `start.rs`   lessons, challenges, the settings window, the part form's new part, the windows opened for screenshots
//! - `form.rs`    the part form (`--edit-part`, `--form-*`)
//! - `projects.rs`the multi-board sample and `.ccproj`
//! - `sheet.rs` `.schjson` with its analyses (`--dc`, `--tran`, `--ac`, ...), and the `.asc` / `.kicad_sch` imports
//! - `board.rs`   `.pcbjson` and the options that act on the board (`--drc`, `--3d`, `--order`, ...)
//! - `proto.rs`   the prototype and bench tabs (`--proto*`, `--bench*`, `--power-on`)
//!
//! A new option is usually one `if` in the file of its topic, with the helpers of [`Args`].

use super::*;

mod board;
mod form;
mod projects;
mod proto;
mod sheet;
mod start;

pub(super) fn apply_args(app: &mut App, args: &[String]) {
    let a = Args(args);
    start::lessons(app, &a);
    projects::multiboard_sample(app, &a);
    form::apply(app, &a);
    start::windows(app, &a);
    projects::apply(app, &a);
    sheet::imports(app, &a);
    sheet::apply(app, &a);
    board::apply_file(app, &a);
    proto::apply(app, &a);
    export_sheet(app, &a);
}

/// The words of the command line, with lookups by option name.
pub(super) struct Args<'a>(&'a [String]);

impl<'a> Args<'a> {
    /// The option is given.
    fn has(&self, flag: &str) -> bool {
        self.0.iter().any(|a| a == flag)
    }

    /// Where the option is (its first time).
    fn pos(&self, flag: &str) -> Option<usize> {
        self.0.iter().position(|a| a == flag)
    }

    /// Every place the option is (a repeatable option).
    fn all<'s>(&'s self, flag: &'s str) -> impl Iterator<Item = usize> + 's {
        self.0.iter().enumerate().filter(move |(_, a)| *a == flag).map(|(i, _)| i)
    }

    /// The word at `i`, whatever it is.
    fn get(&self, i: usize) -> Option<&'a str> {
        self.0.get(i).map(String::as_str)
    }

    /// The word at `i` when it is not another option (an optional value).
    fn word(&self, i: usize) -> Option<&'a str> {
        self.get(i).filter(|w| !w.starts_with("--"))
    }

    /// The word after the option (`--flag VALUE`).
    fn val(&self, flag: &str) -> Option<&'a str> {
        self.pos(flag).and_then(|i| self.get(i + 1))
    }

    /// The word after the option when it is not another option (`--flag [VALUE]`).
    fn after(&self, flag: &str) -> Option<&'a str> {
        self.pos(flag).and_then(|i| self.word(i + 1))
    }

    /// The word after the option, read as a number.
    fn num<T: std::str::FromStr>(&self, flag: &str) -> Option<T> {
        self.val(flag).and_then(|v| v.parse().ok())
    }

    /// The `N` words after the option, all of them there (`--flag A B C`).
    fn vals<const N: usize>(&self, flag: &str) -> Option<[&'a str; N]> {
        self.pos(flag).and_then(|i| self.words(i + 1))
    }

    /// The `N` words from `from`, all of them there.
    fn words<const N: usize>(&self, from: usize) -> Option<[&'a str; N]> {
        let s = self.0.get(from..from + N)?;
        Some(std::array::from_fn(|k| s[k].as_str()))
    }

    /// The file named on the command line with that ending (`.schjson`, `.pcbjson`, ...).
    fn file(&self, ending: &str) -> Option<&'a str> {
        self.0.iter().map(String::as_str).find(|a| a.ends_with(ending))
    }
}

/// `--pdf file.pdf` / `--svg file.svg`: the shown sheet as a one-page PDF or an SVG picture, then quit (after the other options, so
/// `--dc` figures are in it).
fn export_sheet(app: &mut App, a: &Args) {
    for (flag, what) in [("--pdf", "PDF"), ("--svg", "SVG")] {
        let Some(path) = a.val(flag) else { continue };
        let bytes = if flag == "--pdf" { app.sch.sheet_pdf() } else { app.sch.sheet_svg().map(String::into_bytes) };
        let Some(bytes) = bytes else {
            eprintln!("回路図が空なので、{what} に書き出せません");
            std::process::exit(1);
        };
        if let Err(e) = std::fs::write(path, bytes) {
            eprintln!("{what} を保存できませんでした: {e}。保存先のフォルダに書き込めるか確認してください");
            std::process::exit(1);
        }
        println!("{what} を保存しました: {path}");
        std::process::exit(0);
    }
}
