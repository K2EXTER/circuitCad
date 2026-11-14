//! Autosave and recovery: while there are unsaved changes, a copy goes to the app data folder every minute; after a crash (or a power
//! cut) the next plain launch offers to take it back. A normal close, a save of everything, or a "discard" answer removes the copy.

use super::*;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SCH_FILE: &str = "sch.schjson";
const BOARD_FILE: &str = "board.pcbjson";
const META_FILE: &str = "meta.json";

/// Where the copies are kept (next to `settings.json`).
pub(super) fn default_dir() -> PathBuf {
    part_lib::default_dir().parent().map_or_else(|| PathBuf::from("autosave"), |d| d.join("autosave"))
}

/// What an earlier run left behind.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Found {
    pub sch: bool,
    pub board: bool,
    pub sch_path: Option<PathBuf>,
    pub board_path: Option<PathBuf>,
    pub project: Option<PathBuf>,
    /// Seconds since 1970 when the copy was written.
    pub saved_at: u64,
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Write the copies (`None` removes that one). With nothing to keep, the folder's files go away.
pub(super) fn write(dir: &Path, sch: Option<&str>, board: Option<&str>, paths: [Option<&Path>; 3]) -> Result<(), String> {
    if sch.is_none() && board.is_none() {
        clear(dir);
        return Ok(());
    }
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    for (name, text) in [(SCH_FILE, sch), (BOARD_FILE, board)] {
        match text {
            // written beside and renamed, so a crash in the middle cannot leave half a file in place of a good one
            Some(t) => {
                let tmp = dir.join(format!("{name}.tmp"));
                std::fs::write(&tmp, t).map_err(|e| e.to_string())?;
                std::fs::rename(&tmp, dir.join(name)).map_err(|e| e.to_string())?;
            }
            None => {
                let _ = std::fs::remove_file(dir.join(name));
            }
        }
    }
    let p = |x: Option<&Path>| x.map(|p| p.to_string_lossy().into_owned());
    let meta = serde_json::json!({ "sch_path": p(paths[0]), "board_path": p(paths[1]), "project": p(paths[2]), "saved_at": now_secs() });
    std::fs::write(dir.join(META_FILE), meta.to_string()).map_err(|e| e.to_string())
}

/// Remove everything the folder holds of an autosave.
pub(super) fn clear(dir: &Path) {
    for name in [SCH_FILE, BOARD_FILE, META_FILE, "sch.schjson.tmp", "board.pcbjson.tmp"] {
        let _ = std::fs::remove_file(dir.join(name));
    }
}

/// The copies an earlier run left, if any.
pub(super) fn find(dir: &Path) -> Option<Found> {
    let (sch, board) = (dir.join(SCH_FILE).is_file(), dir.join(BOARD_FILE).is_file());
    if !sch && !board {
        return None;
    }
    let meta = std::fs::read_to_string(dir.join(META_FILE)).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
    let get = |k: &str| meta.as_ref().and_then(|m| m.get(k)).and_then(|v| v.as_str()).map(PathBuf::from);
    let saved_at = meta.as_ref().and_then(|m| m.get("saved_at")).and_then(|v| v.as_u64()).unwrap_or(0);
    Some(Found { sch, board, sch_path: get("sch_path"), board_path: get("board_path"), project: get("project"), saved_at })
}

impl Found {
    /// "3 分前" and the like, for the question.
    pub fn age_text(&self) -> String {
        let secs = now_secs().saturating_sub(self.saved_at);
        match self.saved_at {
            0 => "前回".into(),
            _ if secs < 90 => "1 分以内".into(),
            _ if secs < 3600 => format!("{} 分前", secs / 60),
            _ if secs < 86400 => format!("{} 時間前", secs / 3600),
            _ => format!("{} 日前", secs / 86400),
        }
    }
}

impl App {
    /// Every so often (the settings' 自動保存の間隔, a minute at first; never when it is 0), keep a copy of what is not saved. Not while the question about an earlier copy is open (the editors are still
    /// empty then, and writing would remove that copy).
    pub(super) fn tick_autosave(&mut self, ctx: &egui::Context) {
        if self.recovery.is_some() {
            return;
        }
        if self.prefs.autosave_secs == 0 {
            return;
        }
        let interval = Duration::from_secs(self.prefs.autosave_secs.into());
        let elapsed = self.last_autosave.elapsed();
        if elapsed < interval {
            ctx.request_repaint_after(interval - elapsed);
            return;
        }
        self.last_autosave = std::time::Instant::now();
        let (sch, board) = (self.sch.autosave_json(), self.board_ed.autosave_json());
        let paths = [self.sch.path.as_deref(), self.board_ed.path.as_deref(), self.project.as_deref()];
        if let Err(e) = write(&default_dir(), sch.as_deref(), board.as_deref(), paths) {
            self.sch.status = format!("自動保存できませんでした: {e}");
        }
    }

    /// Look for an earlier run's copy at start-up.
    pub(super) fn look_for_recovery(&mut self) {
        self.recovery = find(&default_dir());
    }

    /// The question about an earlier copy: take it back, or throw it away.
    pub(super) fn recovery_window(&mut self, ctx: &egui::Context) {
        let Some(found) = self.recovery.clone() else { return };
        let mut answer = None;
        egui::Window::new(i18n::tr("保存していなかった作業が見つかりました")).collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            let what = match (found.sch, found.board) {
                (true, true) => "回路図と基板",
                (true, false) => "回路図",
                _ => "基板",
            };
            // wide enough for the whole title (the window takes the width of its text, and the title was cut short)
            ui.set_min_width(440.0);
            ui.label(i18n::trf!("前回、{what}を保存しないまま終了したようです(自動保存は{})。", found.age_text()));
            // which work it is: the files it was opened from (a new one had no name yet)
            let name = |p: &Option<PathBuf>| p.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned());
            egui::Grid::new("recovery_files").num_columns(2).spacing([12.0, 2.0]).show(ui, |ui| {
                if let Some(p) = name(&found.project) {
                    ui.label(i18n::tr("プロジェクト"));
                    ui.strong(p);
                    ui.end_row();
                }
                if found.sch {
                    ui.label(i18n::tr("回路図"));
                    ui.strong(name(&found.sch_path).unwrap_or_else(|| i18n::tr("(まだ名前を付けていない新しい回路図)").to_string()));
                    ui.end_row();
                }
                if found.board {
                    ui.label(i18n::tr("基板"));
                    ui.strong(name(&found.board_path).unwrap_or_else(|| i18n::tr("(まだ名前を付けていない新しい基板)").to_string()));
                    ui.end_row();
                }
            });
            ui.label(i18n::tr("復元すると、その続きから作業できます。"));
            ui.horizontal(|ui| {
                if ui.button(i18n::tr("復元する")).on_hover_text(i18n::tr("自動保存したものを開きます。開いたあと、いつもどおり保存してください")).clicked() {
                    answer = Some(true);
                }
                if ui.button(i18n::tr("捨てる")).on_hover_text(i18n::tr("自動保存したものを消して、まっさらな状態で始めます")).clicked() {
                    answer = Some(false);
                }
            });
        });
        let Some(restore) = answer else { return };
        self.recovery = None;
        let dir = default_dir();
        if restore {
            if found.sch {
                self.sch.restore(&dir.join(SCH_FILE), found.sch_path.clone());
            }
            if found.board {
                self.board_ed.restore(&dir.join(BOARD_FILE), found.board_path.clone());
            }
            self.project = found.project;
            self.mode = if found.sch { Mode::Schematic } else { Mode::Board };
            // the copies stay until the next tick writes them again
            self.last_autosave = std::time::Instant::now();
        } else {
            clear(&dir);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ccad-autosave-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn what_was_written_is_found_with_its_file_names() {
        let d = temp("found");
        assert_eq!(find(&d), None);
        let (a, b) = (PathBuf::from("a.schjson"), PathBuf::from("w.ccproj"));
        write(&d, Some("{}"), None, [Some(&a), None, Some(&b)]).unwrap();
        let f = find(&d).unwrap();
        assert!(f.sch && !f.board);
        assert_eq!((f.sch_path, f.board_path, f.project), (Some(a), None, Some(b)));
        assert!(f.saved_at > 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn nothing_to_keep_removes_the_copies() {
        let d = temp("clear");
        write(&d, Some("{}"), Some("{}"), [None, None, None]).unwrap();
        assert!(find(&d).is_some());
        // the sheet was saved: only the board is kept
        write(&d, None, Some("{}"), [None, None, None]).unwrap();
        assert!(!find(&d).unwrap().sch);
        // both saved
        write(&d, None, None, [None, None, None]).unwrap();
        assert_eq!(find(&d), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_edited_sheet_is_kept_and_comes_back_as_unsaved() {
        let d = temp("roundtrip");
        let mut ed = SchEditor::default();
        assert!(ed.autosave_json().is_none(), "a clean sheet has nothing to keep");
        ed.sch.labels.push(sch_core::Label { pos: Default::default(), text: "VOUT".into() });
        let json = ed.autosave_json().expect("an edited sheet is kept");
        write(&d, Some(&json), None, [None, None, None]).unwrap();
        let mut again = SchEditor::default();
        again.restore(&d.join(SCH_FILE), Some(PathBuf::from("mine.schjson")));
        assert_eq!(again.sch.labels.len(), 1);
        assert!(again.dirty(), "a restored sheet is not saved yet");
        assert_eq!(again.path, Some(PathBuf::from("mine.schjson")));
        let _ = std::fs::remove_dir_all(&d);
    }
}
