//! Building and loading the firmware of the board (candidate W4): the commands of the outside tools, run one after another —
//! arduino-cli for an Arduino sketch, cmake for a Raspberry Pi Pico (Pico SDK) project, yosys → nextpnr-ice40 → icepack for an iCE40
//! FPGA, or the user's own commands. Nothing is bundled (docs/integration-policy.md): a tool that is not found stops with what is
//! missing and how to get it. The settings are kept with the schematic (`Schematic.firmware`), so each board of a project has its own.

use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};

/// How the firmware of this board is built and loaded. Paths are as typed: absolute, or relative to the schematic's folder.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct FirmwareBuild {
    /// "arduino", "pico", "ice40" or "custom" (`Toolchain::parse`); empty = not set up.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub toolchain: String,
    /// The folder of the sketch / the CMake project / the Verilog files.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub folder: String,
    /// Arduino: the board's FQBN ("arduino:avr:mega"). iCE40: the device and package ("up5k:sg48"). Pico: the PICO_BOARD ("pico").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub board: String,
    /// Arduino: the serial port to load through ("COM3", "/dev/ttyACM0").
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub port: String,
    /// iCE40: the top module ("top") and the constraint file ("top.pcf", from `pins export=`), in the folder.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub top: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pcf: String,
    /// "custom": the command lines to build and to load (run by the shell, in the folder).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub build_cmd: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub flash_cmd: String,
}

impl FirmwareBuild {
    pub fn is_empty(&self) -> bool {
        *self == FirmwareBuild::default()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Toolchain {
    Arduino,
    Pico,
    Ice40,
    Custom,
}

/// The tool chains: (kind, key, name on screen, what it is).
pub const TOOLCHAINS: &[(Toolchain, &str, &str, &str)] = &[
    (Toolchain::Arduino, "arduino", "Arduino (arduino-cli)", "Arduino のスケッチ (.ino) を arduino-cli でコンパイルし、シリアルポートから書き込みます"),
    (Toolchain::Pico, "pico", "Raspberry Pi Pico (Pico SDK)", "Pico SDK の CMake のプロジェクトを cmake でビルドし、picotool で .uf2 を書き込みます"),
    (Toolchain::Ice40, "ice40", "iCE40 FPGA (yosys・nextpnr)", "Verilog を yosys で合成し、nextpnr-ice40 で配置配線、icepack で .bin にして、iceprog で書き込みます"),
    (Toolchain::Custom, "custom", "自分のコマンド", "ビルドと書き込みのコマンドを自分で書きます (フォルダの中で、シェルで動かします)"),
];

impl Toolchain {
    pub fn parse(s: &str) -> Option<Toolchain> {
        TOOLCHAINS.iter().find(|t| t.1 == s).map(|t| t.0)
    }
    pub fn key(self) -> &'static str {
        TOOLCHAINS.iter().find(|t| t.0 == self).map_or("", |t| t.1)
    }
    pub fn name(self) -> &'static str {
        TOOLCHAINS.iter().find(|t| t.0 == self).map_or("", |t| t.2)
    }
}

/// One command of a build or a load.
#[derive(Clone, Debug, PartialEq)]
pub struct Step {
    /// What it does, for the log ("コンパイル").
    pub what: String,
    pub program: String,
    pub args: Vec<String>,
    pub dir: PathBuf,
}

impl Step {
    fn new(what: &str, program: &str, args: &[&str], dir: &Path) -> Step {
        Step { what: what.into(), program: program.into(), args: args.iter().map(|a| a.to_string()).collect(), dir: dir.to_path_buf() }
    }

    /// The command line as one would type it (for the log and the CLI's answer).
    pub fn line(&self) -> String {
        std::iter::once(self.program.as_str()).chain(self.args.iter().map(|a| a.as_str())).map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a.to_string() }).collect::<Vec<_>>().join(" ")
    }

    /// A user's command line, run by the shell.
    fn shell(what: &str, line: &str, dir: &Path) -> Step {
        if cfg!(windows) {
            Step { what: what.into(), program: "cmd".into(), args: vec!["/C".into(), line.into()], dir: dir.to_path_buf() }
        } else {
            Step { what: what.into(), program: "sh".into(), args: vec!["-c".into(), line.into()], dir: dir.to_path_buf() }
        }
    }
}

/// Where to get a tool when it is not found.
pub fn install_hint(program: &str) -> &'static str {
    match program {
        "arduino-cli" => "arduino-cli を入れて、PATH の通ったフォルダに置いてください (https://arduino.github.io/arduino-cli/ 。ボードの種類は「arduino-cli core install arduino:avr」で入れます)",
        "cmake" => "CMake と Pico SDK を入れてください (Raspberry Pi の「Pico setup for Windows」で、cmake・Arm のコンパイラ・PICO_SDK_PATH がまとめて入ります)",
        "picotool" => "picotool を入れて、PATH の通ったフォルダに置いてください (Pico SDK のセットアップに入っています)。picotool が無いときは、BOOTSEL を押しながら USB をつなぎ、build の .uf2 を出てきたドライブにコピーしても書き込めます",
        "yosys" | "nextpnr-ice40" | "icepack" | "iceprog" => "OSS CAD Suite (https://github.com/YosysHQ/oss-cad-suite-build) を展開して、その bin を PATH に足してください。yosys・nextpnr-ice40・icepack・iceprog がまとめて入ります",
        "cmd" | "sh" => "シェルが見つかりません",
        _ => "そのコマンドを入れて、PATH の通ったフォルダに置いてください",
    }
}

/// The first of `names` found on the PATH (with the Windows endings), or None.
pub fn find_tool(program: &str) -> Option<PathBuf> {
    let p = Path::new(program);
    if p.components().count() > 1 {
        return p.is_file().then(|| p.to_path_buf());
    }
    let exts: &[&str] = if cfg!(windows) { &["", ".exe", ".cmd", ".bat"] } else { &[""] };
    let on_path = std::env::var_os("PATH")
        .and_then(|path| std::env::split_paths(&path).find_map(|d| exts.iter().map(|e| d.join(format!("{program}{e}"))).find(|f| f.is_file())));
    on_path.or_else(|| bundled(program))
}

/// A tool that comes inside another program when it is not on the PATH: arduino-cli inside the Arduino IDE 2 (which most people
/// who use an Arduino have).
fn bundled(program: &str) -> Option<PathBuf> {
    if program != "arduino-cli" {
        return None;
    }
    let inner = ["resources", "app", "lib", "backend", "resources"];
    let mut places: Vec<PathBuf> = vec![];
    if cfg!(windows) {
        for var in ["ProgramFiles", "LOCALAPPDATA"] {
            if let Some(d) = std::env::var_os(var) {
                let d = PathBuf::from(d);
                places.push(d.join("Arduino IDE"));
                places.push(d.join("Programs").join("Arduino IDE"));
            }
        }
    } else if cfg!(target_os = "macos") {
        places.push(PathBuf::from("/Applications/Arduino IDE.app/Contents"));
    }
    let name = if cfg!(windows) { "arduino-cli.exe" } else { "arduino-cli" };
    places.into_iter().map(|p| inner.iter().fold(p, |p, x| p.join(x)).join(name)).find(|f| f.is_file())
}

/// The settings file of the Arduino IDE 2 (`~/.arduinoIDE/arduino-cli.yaml`), given to arduino-cli when there is one: it says
/// where the IDE keeps the libraries and boards installed in it. Without it arduino-cli looks in its own places (on Windows the
/// Documents folder as the system names it, which OneDrive moves), and a library installed in the IDE is not found.
pub fn ide_config() -> Option<PathBuf> {
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })?;
    let p = PathBuf::from(home).join(".arduinoIDE").join("arduino-cli.yaml");
    p.is_file().then_some(p)
}

/// The ELF of a sketch built by arduino-cli for a board (`fqbn`: "arduino:avr:nano"), to run as the chip's real instructions
/// (W5 (b)). The sketch is written to a folder of its own under the temporary folder (named after its text, so the same text is
/// built once). Waits for arduino-cli (the first build of a board takes tens of seconds; later ones a few). The error says what
/// arduino-cli said (the lines with `error`, in the sketch's line numbers) and what to do.
pub fn compile_sketch(text: &str, fqbn: &str) -> Result<Vec<u8>, String> {
    use std::hash::{Hash, Hasher};
    let tool = find_tool("arduino-cli").ok_or_else(|| format!("arduino-cli が見つかりません。{}", install_hint("arduino-cli")))?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (text, fqbn).hash(&mut h);
    let root = std::env::temp_dir().join("circuitCad-sketch").join(format!("{:016x}", h.finish()));
    let (dir, out) = (root.join("sketch"), root.join("out"));
    let elf = out.join("sketch.ino.elf");
    if let Ok(b) = std::fs::read(&elf) {
        return Ok(b);
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("一時フォルダが作れません: {e}"))?;
    std::fs::write(dir.join("sketch.ino"), text).map_err(|e| format!("スケッチが書けません: {e}"))?;
    let mut cmd = Command::new(&tool);
    cmd.args(["compile", "--fqbn", fqbn, "--output-dir"]).arg(&out).arg(&dir);
    if let Some(c) = ide_config() {
        cmd.arg("--config-file").arg(c);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // no console window flashing up while it builds
        cmd.creation_flags(0x0800_0000);
    }
    let done = cmd.output().map_err(|e| format!("arduino-cli が動きません: {e}"))?;
    if done.status.success()
        && let Ok(b) = std::fs::read(&elf)
    {
        return Ok(b);
    }
    let said = format!("{}\n{}", String::from_utf8_lossy(&done.stdout), String::from_utf8_lossy(&done.stderr));
    let ino = dir.join("sketch.ino").to_string_lossy().to_string();
    let mut lines: Vec<String> = said
        .lines()
        .filter(|l| l.contains("error") || l.contains("Error"))
        .map(|l| l.replace(&ino, "スケッチ").trim().to_string())
        .filter(|l| !l.is_empty())
        .take(6)
        .collect();
    if let Some(hint) = said.lines().find_map(explain) {
        lines.push(hint.to_string());
    }
    if lines.is_empty() {
        lines.push("arduino-cli のコンパイルが失敗しました".into());
    }
    Err(lines.join("\n"))
}

/// The folder of the firmware: `folder` as typed, against the schematic's folder `base` when relative.
pub fn folder_of(fw: &FirmwareBuild, base: &Path) -> PathBuf {
    let f = Path::new(fw.folder.trim());
    if f.is_absolute() { f.to_path_buf() } else { base.join(f) }
}

fn need<'a>(value: &'a str, what: &str) -> Result<&'a str, String> {
    let v = value.trim();
    if v.is_empty() { Err(format!("{what}を入れてください")) } else { Ok(v) }
}

/// The commands that build the firmware (in order). The settings are checked first: an empty box says which.
pub fn build_steps(fw: &FirmwareBuild, base: &Path) -> Result<Vec<Step>, String> {
    let kind = Toolchain::parse(&fw.toolchain).ok_or("ファームの種類 (Arduino・Pico・iCE40・自分のコマンド) を選んでください")?;
    need(&fw.folder, "ファームのフォルダ")?;
    let dir = folder_of(fw, base);
    if !dir.is_dir() {
        return Err(format!("ファームのフォルダ「{}」がありません。回路図のファイルの場所からの相対か、絶対のパスで入れてください", dir.display()));
    }
    let d = dir.to_string_lossy().to_string();
    Ok(match kind {
        Toolchain::Arduino => {
            let fqbn = need(&fw.board, "ボードの FQBN (例: arduino:avr:mega)")?;
            vec![Step::new("コンパイル", "arduino-cli", &["compile", "--fqbn", fqbn, &d], &dir)]
        }
        Toolchain::Pico => {
            let build = dir.join("build").to_string_lossy().to_string();
            let board = format!("-DPICO_BOARD={}", if fw.board.trim().is_empty() { "pico" } else { fw.board.trim() });
            vec![Step::new("CMake の設定", "cmake", &["-S", &d, "-B", &build, &board], &dir), Step::new("ビルド", "cmake", &["--build", &build], &dir)]
        }
        Toolchain::Ice40 => {
            let top = need(&fw.top, "いちばん上のモジュールの名前 (例: top)")?;
            let (device, package) = fw.board.trim().split_once(':').ok_or("デバイスとパッケージを「up5k:sg48」の形で入れてください")?;
            let pcf = need(&fw.pcf, "制約ファイル (.pcf、ピンの表から「ピンとファーム…」で書き出せます)")?;
            let mut sources: Vec<String> = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "v" || x == "sv")).map(|p| p.file_name().unwrap_or_default().to_string_lossy().to_string()).collect();
            sources.sort();
            if sources.is_empty() {
                return Err(format!("「{}」に Verilog のファイル (.v) がありません", dir.display()));
            }
            std::fs::create_dir_all(dir.join("build")).map_err(|e| e.to_string())?;
            let json = format!("build/{top}.json");
            let asc = format!("build/{top}.asc");
            let bin = format!("build/{top}.bin");
            let synth = format!("synth_ice40 -top {top} -json {json}");
            let mut yosys = vec!["-q", "-p", synth.as_str()];
            yosys.extend(sources.iter().map(|s| s.as_str()));
            let dev = format!("--{device}");
            vec![
                Step::new("合成", "yosys", &yosys, &dir),
                Step::new("配置配線", "nextpnr-ice40", &[&dev, "--package", package, "--json", &json, "--pcf", pcf, "--asc", &asc], &dir),
                Step::new(".bin にする", "icepack", &[&asc, &bin], &dir),
            ]
        }
        Toolchain::Custom => vec![Step::shell("ビルド", need(&fw.build_cmd, "ビルドのコマンド")?, &dir)],
    })
}

/// The commands that load the built firmware into the board.
pub fn flash_steps(fw: &FirmwareBuild, base: &Path) -> Result<Vec<Step>, String> {
    let kind = Toolchain::parse(&fw.toolchain).ok_or("ファームの種類 (Arduino・Pico・iCE40・自分のコマンド) を選んでください")?;
    need(&fw.folder, "ファームのフォルダ")?;
    let dir = folder_of(fw, base);
    let d = dir.to_string_lossy().to_string();
    Ok(match kind {
        Toolchain::Arduino => {
            let fqbn = need(&fw.board, "ボードの FQBN (例: arduino:avr:mega)")?;
            let port = need(&fw.port, "書き込むシリアルポート (例: COM3)")?;
            vec![Step::new("書き込み", "arduino-cli", &["upload", "-p", port, "--fqbn", fqbn, &d], &dir)]
        }
        Toolchain::Pico => {
            // the .uf2 the build made (the first one in build/)
            let build = dir.join("build");
            let uf2 = std::fs::read_dir(&build).ok().and_then(|r| r.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "uf2"))).ok_or("build に .uf2 がありません。先にビルドしてください")?;
            vec![Step::new("書き込み", "picotool", &["load", "-x", "-f", &uf2.to_string_lossy()], &dir)]
        }
        Toolchain::Ice40 => {
            let top = need(&fw.top, "いちばん上のモジュールの名前 (例: top)")?;
            vec![Step::new("書き込み", "iceprog", &[&format!("build/{top}.bin")], &dir)]
        }
        Toolchain::Custom => vec![Step::shell("書き込み", need(&fw.flash_cmd, "書き込みのコマンド")?, &dir)],
    })
}

/// The tools of `steps` that are not on this computer, each with where to get it.
pub fn missing(steps: &[Step]) -> Vec<(String, &'static str)> {
    let mut out: Vec<(String, &'static str)> = vec![];
    for s in steps {
        if find_tool(&s.program).is_none() && !out.iter().any(|(p, _)| *p == s.program) {
            out.push((s.program.clone(), install_hint(&s.program)));
        }
    }
    out
}

/// A line of a tool's output said in the app's words, for the errors met most (None: nothing to add).
pub fn explain(line: &str) -> Option<&'static str> {
    let l = line.to_ascii_lowercase();
    if l.contains("platform not installed") || l.contains("platform '") && l.contains("not found") {
        Some("ボードの種類が arduino-cli に入っていません。「arduino-cli core install arduino:avr」(Mega・UNO など) のように入れてください")
    } else if l.contains("no such file or directory") && l.contains(".h") {
        Some("インクルードするファイル (ライブラリ) が見つかりません。Arduino IDE のライブラリマネージャか「arduino-cli lib install <名前>」で入れるか、名前の綴りを確かめてください")
    } else if l.contains("access is denied") || l.contains("can't open device") || l.contains("could not open port") || l.contains("serial port busy") {
        Some("シリアルポートが開けません。ポートの名前を確かめ、ほかのソフト (シリアルモニタなど) がポートを使っていないか見てください")
    } else if l.contains("pico_sdk_path") || l.contains("pico_sdk_import") {
        Some("Pico SDK の場所 (環境変数 PICO_SDK_PATH) が決まっていません。Pico SDK を入れて、PICO_SDK_PATH を設定してください")
    } else if l.contains("can't find ftdi") || l.contains("unable to find ftdi") || l.contains("ftdi_usb_open") {
        Some("FPGA の書き込み器 (FTDI) が見つかりません。USB のケーブルと、ドライバ (Windows では Zadig で WinUSB) を確かめてください")
    } else if l.contains("not constrained") || l.contains("io") && l.contains("unconstrained") {
        Some("制約ファイル (.pcf) にない信号があります。回路図の「ピンとファーム…」で .pcf を書き出し直してください")
    } else {
        None
    }
}

/// A run going on another thread: the lines so far, whether it ended (and how), and the process to stop.
#[derive(Default)]
pub struct Run {
    pub lines: Mutex<Vec<String>>,
    /// None while running; Some(Ok) when every step went well, Some(Err(message)) when one failed or was stopped.
    pub done: Mutex<Option<Result<(), String>>>,
    child: Mutex<Option<std::process::Child>>,
    stop: std::sync::atomic::AtomicBool,
}

impl Run {
    /// Stops the step running now and the ones after it.
    pub fn stop(&self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(c) = self.child.lock().unwrap().as_mut() {
            let _ = c.kill();
        }
    }

    fn say(&self, line: impl Into<String>) {
        self.lines.lock().unwrap().push(line.into());
    }
}

/// Runs `steps` in order on another thread, the output of each line by line into `run.lines`; stops at the first that fails (its
/// exit code and, for the errors met most, what they mean are said at the end). A tool that is not found is said before anything
/// runs.
pub fn start(steps: Vec<Step>) -> Arc<Run> {
    let run = Arc::new(Run::default());
    let r = run.clone();
    std::thread::spawn(move || {
        let out = run_steps(&r, &steps);
        *r.done.lock().unwrap() = Some(out);
    });
    run
}

/// `start`, then the commands `more` makes once those went well (a Pico's load: its .uf2 is there only after the build).
pub fn start_then(steps: Vec<Step>, more: impl FnOnce() -> Result<Vec<Step>, String> + Send + 'static) -> Arc<Run> {
    let run = Arc::new(Run::default());
    let r = run.clone();
    std::thread::spawn(move || {
        let out = run_steps(&r, &steps).and_then(|_| {
            let next = more().inspect_err(|e| r.say(e.clone()))?;
            run_steps(&r, &next)
        });
        *r.done.lock().unwrap() = Some(out);
    });
    run
}

/// The same, on this thread (the CLI): returns the log with the result.
pub fn run_now(steps: &[Step]) -> (Vec<String>, Result<(), String>) {
    let run = Run::default();
    let out = run_steps(&run, steps);
    (std::mem::take(&mut *run.lines.lock().unwrap()), out)
}

fn run_steps(run: &Run, steps: &[Step]) -> Result<(), String> {
    let gone = missing(steps);
    if !gone.is_empty() {
        let text = gone.iter().map(|(p, how)| format!("「{p}」が見つかりません。{how}")).collect::<Vec<_>>().join("\n");
        run.say(text.clone());
        return Err(text);
    }
    for s in steps {
        if run.stop.load(std::sync::atomic::Ordering::Relaxed) {
            return Err("止めました".into());
        }
        run.say(format!("▶ {}: {}", s.what, s.line()));
        let mut cmd = Command::new(find_tool(&s.program).unwrap_or_else(|| PathBuf::from(&s.program)));
        cmd.args(&s.args).current_dir(&s.dir).stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
        if s.program == "arduino-cli"
            && let Some(c) = ide_config()
        {
            cmd.arg("--config-file").arg(c);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // no console window flashing up over the app
            cmd.creation_flags(0x0800_0000);
        }
        let mut child = cmd.spawn().map_err(|e| format!("{} を起動できませんでした: {e}", s.program))?;
        let (out, err) = (child.stdout.take(), child.stderr.take());
        *run.child.lock().unwrap() = Some(child);
        let hints = Mutex::new(Vec::<&'static str>::new());
        std::thread::scope(|sc| {
            let read = |pipe: Box<dyn std::io::Read + Send>| {
                for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                    if let Some(h) = explain(&line) {
                        let mut hs = hints.lock().unwrap();
                        if !hs.contains(&h) {
                            hs.push(h);
                        }
                    }
                    run.say(line);
                }
            };
            if let Some(o) = out {
                sc.spawn(move || read(Box::new(o)));
            }
            if let Some(e) = err {
                sc.spawn(move || read(Box::new(e)));
            }
        });
        let status = run.child.lock().unwrap().take().map(|mut c| c.wait());
        let ok = matches!(status, Some(Ok(st)) if st.success());
        if !ok {
            let code = match status {
                Some(Ok(st)) => st.code().map_or("止められました".to_string(), |c| format!("終了コード {c}")),
                _ => "止められました".into(),
            };
            let mut text = format!("{} で止まりました ({code})", s.what);
            for h in hints.into_inner().unwrap() {
                text.push_str(&format!("\nヒント: {h}"));
            }
            if run.stop.load(std::sync::atomic::Ordering::Relaxed) {
                text = "止めました".into();
            }
            run.say(text.clone());
            return Err(text);
        }
    }
    run.say("✔ 終わりました");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fw(kind: &str, folder: &str) -> FirmwareBuild {
        FirmwareBuild { toolchain: kind.into(), folder: folder.into(), ..Default::default() }
    }

    /// The commands of each tool chain, from the settings, with the folder against the schematic's; an empty box says which.
    #[test]
    fn the_commands_of_each_tool_chain() {
        let base = std::env::temp_dir().join("ccad_fw_test");
        let dir = base.join("blink");
        std::fs::create_dir_all(&dir).unwrap();
        let mut a = fw("arduino", "blink");
        assert!(build_steps(&a, &base).unwrap_err().contains("FQBN"));
        a.board = "arduino:avr:mega".into();
        let b = build_steps(&a, &base).unwrap();
        assert_eq!(b[0].program, "arduino-cli");
        assert_eq!(b[0].args[..3], ["compile", "--fqbn", "arduino:avr:mega"]);
        assert!(flash_steps(&a, &base).unwrap_err().contains("ポート"));
        a.port = "COM3".into();
        assert_eq!(flash_steps(&a, &base).unwrap()[0].args[..3], ["upload", "-p", "COM3"]);
        // iCE40: the Verilog files of the folder, the device and package, the constraint file
        std::fs::write(dir.join("top.v"), "module top(); endmodule").unwrap();
        let mut f = fw("ice40", "blink");
        f.top = "top".into();
        f.board = "up5k:sg48".into();
        f.pcf = "top.pcf".into();
        let s = build_steps(&f, &base).unwrap();
        assert_eq!(s.iter().map(|x| x.program.as_str()).collect::<Vec<_>>(), ["yosys", "nextpnr-ice40", "icepack"]);
        assert!(s[0].args.contains(&"top.v".to_string()) && s[1].args.contains(&"--up5k".to_string()));
        assert_eq!(flash_steps(&f, &base).unwrap()[0].line(), "iceprog build/top.bin");
        // a folder that is not there, an unknown kind
        assert!(build_steps(&fw("arduino", "nothing_here"), &base).unwrap_err().contains("ありません"));
        assert!(build_steps(&fw("", "blink"), &base).is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A user's commands run by the shell in the folder: the output comes back line by line, and a failing command stops the run with
    /// its exit code; a tool that is not there stops before anything runs, saying how to get it.
    #[test]
    fn commands_run_and_say_why_they_stop() {
        let base = std::env::temp_dir();
        let mut c = fw("custom", ".");
        c.build_cmd = "echo hello".into();
        let (log, out) = run_now(&build_steps(&c, &base).unwrap());
        assert!(out.is_ok(), "{log:?}");
        assert!(log.iter().any(|l| l.trim() == "hello"), "{log:?}");
        c.build_cmd = "exit 3".into();
        let (_, out) = run_now(&build_steps(&c, &base).unwrap());
        assert!(out.unwrap_err().contains("終了コード 3"));
        let gone = [Step::new("合成", "no-such-tool-ccad", &[], &base)];
        let (_, out) = run_now(&gone);
        assert!(out.unwrap_err().contains("no-such-tool-ccad"));
        assert_eq!(explain("Error: platform not installed"), explain("Platform 'arduino:avr' not found"));
    }

    #[test]
    fn settings_round_trip_and_stay_out_of_files_without_them() {
        let f = FirmwareBuild { toolchain: "arduino".into(), folder: "fw".into(), board: "arduino:avr:mega".into(), port: "COM3".into(), ..Default::default() };
        let back: FirmwareBuild = serde_json::from_str(&serde_json::to_string(&f).unwrap()).unwrap();
        assert_eq!(back, f);
        assert_eq!(serde_json::to_string(&FirmwareBuild::default()).unwrap(), "{}");
    }
}
