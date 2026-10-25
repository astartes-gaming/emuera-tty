//! The ERB test suite: every `tests/erb/*.erb` runs as a small game of
//! its own (the sample game's CSV files, the shared helpers in
//! `tests/erb/_helpers.erb`, and the file), headless; its `CALL CHECK`s
//! must all print OK and it must reach `DONE`. `tests/golden/*.erb` print
//! text that must match the `.out` beside them exactly (`UPDATE_GOLDEN=1
//! cargo test --release --test erb_suite` rewrites those). Every file runs,
//! then the failures are reported together.
//!
//! Add a check for a fixed bug to the topic file it belongs to, or a new
//! `tests/erb/<topic>.erb` with a `@TEST_MAIN`.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// A fresh game folder: the sample's CSV files, the helpers and `erb`.
fn game_for(erb: &Path) -> PathBuf {
    let stem = erb.file_stem().unwrap().to_string_lossy().into_owned();
    let dir = std::env::temp_dir().join(format!("emuera-tty-suite-{}-{}", stem, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("CSV")).unwrap();
    std::fs::create_dir_all(dir.join("ERB")).unwrap();
    for e in std::fs::read_dir(Path::new(ROOT).join("sample/CSV")).unwrap() {
        let e = e.unwrap();
        std::fs::copy(e.path(), dir.join("CSV").join(e.file_name())).unwrap();
    }
    let helpers = Path::new(ROOT).join("tests/erb");
    let text = std::fs::read_to_string(erb).unwrap();
    let mut h = std::fs::read_to_string(helpers.join("_helpers.erb")).unwrap();
    if text.lines().any(|l| l.trim_start().starts_with("@SYSTEM_TITLE")) {
        // the file runs the system flow itself: keep only the helpers
        h = h.replacen("@SYSTEM_TITLE\nCALL TEST_MAIN\nCALL TEST_DONE\nQUIT\n", "", 1);
    }
    std::fs::write(dir.join("ERB/_HELPERS.ERB"), h).unwrap();
    std::fs::copy(helpers.join("_helpers.erh"), dir.join("ERB/_HELPERS.ERH")).unwrap();
    std::fs::write(dir.join("ERB").join(format!("{}.ERB", stem.to_uppercase())), text).unwrap();
    // its own header (#DEFINEs, #DIMs), if it has one
    let erh = erb.with_extension("erh");
    if erh.exists() {
        std::fs::copy(&erh, dir.join("ERB").join(format!("{}.ERH", stem.to_uppercase()))).unwrap();
    }
    dir
}

/// Run a game headless with the file's `;@input` lines typed in; its
/// stdout (and stderr after it). Killed after 30 s.
fn run(dir: &Path, erb: &Path) -> String {
    let text = std::fs::read_to_string(erb).unwrap();
    let mut input: String = text.lines().filter_map(|l| l.trim_start().strip_prefix(";@input ")).map(|l| format!("{}\n", l)).collect();
    input.push_str("\n\n\n");
    let mut child = Command::new(env!("CARGO_BIN_EXE_emuera-tty"))
        .arg(dir)
        .arg("--headless")
        .env("EMUERA_SEED", "1")
        .env("EMUERA_LOG_STYLE", "emuera")
        .env("XDG_CONFIG_HOME", dir.join("config"))
        .env("XDG_DATA_HOME", dir.join("data"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // (a game that stops on an error may exit before reading its input:
    // the write then fails, and the output still tells what happened)
    let _ = child.stdin.take().unwrap().write_all(input.as_bytes());
    let start = Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > Duration::from_secs(30) {
            let _ = child.kill();
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let out = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}

fn files(dir: &str) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(Path::new(ROOT).join(dir))
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "erb") && !p.file_name().unwrap().to_string_lossy().starts_with('_'))
        .collect();
    v.sort();
    v
}

#[test]
fn erb_checks() {
    let mut failures = Vec::new();
    let mut total = 0;
    for f in files("tests/erb") {
        let name = f.file_name().unwrap().to_string_lossy().into_owned();
        let dir = game_for(&f);
        let out = run(&dir, &f);
        let ng: Vec<&str> = out.lines().filter(|l| l.starts_with("NG ")).collect();
        let done = out.lines().find_map(|l| l.strip_prefix("DONE ").and_then(|r| r.split(' ').next()).and_then(|n| n.parse::<usize>().ok()));
        if !ng.is_empty() {
            failures.push(format!("{}:\n  {}", name, ng.join("\n  ")));
        } else if out.contains("script error") || done.is_none() {
            failures.push(format!("{}: didn't finish\n{}", name, out));
        } else {
            total += done.unwrap();
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
    assert!(failures.is_empty(), "\n{}\n", failures.join("\n"));
    eprintln!("erb suite: {} checks passed", total);
}

/// The output from the first line the game printed (the loader's line and
/// warnings on stderr left out), with the window's trailing blanks trimmed.
fn screen(out: &str) -> String {
    out.lines()
        .filter(|l| !l.starts_with("loaded ") && !l.starts_with("warning:") && !l.starts_with("note:"))
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n")
        .trim_end()
        .to_string()
        + "\n"
}

#[test]
fn golden_output() {
    let update = std::env::var_os("UPDATE_GOLDEN").is_some();
    let mut failures = Vec::new();
    for f in files("tests/golden") {
        let dir = game_for(&f);
        let got = screen(&run(&dir, &f));
        let want_path = f.with_extension("out");
        if update {
            std::fs::write(&want_path, &got).unwrap();
        } else {
            let want = std::fs::read_to_string(&want_path).unwrap_or_default();
            if got != want {
                let line = got.lines().zip(want.lines()).position(|(a, b)| a != b).unwrap_or(got.lines().count().min(want.lines().count()));
                failures.push(format!(
                    "{}: differs from {} at line {}\n  got:  {:?}\n  want: {:?}",
                    f.file_name().unwrap().to_string_lossy(),
                    want_path.file_name().unwrap().to_string_lossy(),
                    line + 1,
                    got.lines().nth(line).unwrap_or(""),
                    want.lines().nth(line).unwrap_or("")
                ));
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
    assert!(failures.is_empty(), "\n{}\n(UPDATE_GOLDEN=1 rewrites the .out files after a deliberate change)\n", failures.join("\n"));
}

/// A fixed state saved by `tests/save/save_state.erb` must give exactly the
/// bytes of `tests/save/save00.sav` and `global.sav` (Emuera's binary
/// format: a change there breaks moving saves between emuera-tty and
/// Emuera). `UPDATE_GOLDEN=1` rewrites them after a deliberate change.
#[test]
fn golden_save() {
    let f = Path::new(ROOT).join("tests/save/save_state.erb");
    let dir = game_for(&f);
    let out = run(&dir, &f);
    assert!(out.contains("DONE"), "{}", out);
    for name in ["save00.sav", "global.sav"] {
        let got = std::fs::read(dir.join(name)).unwrap_or_else(|e| panic!("{}: {}\n{}", name, e, out));
        let want_path = Path::new(ROOT).join("tests/save").join(name);
        if std::env::var_os("UPDATE_GOLDEN").is_some() {
            std::fs::write(&want_path, &got).unwrap();
            continue;
        }
        let want = std::fs::read(&want_path).unwrap_or_default();
        if got != want {
            let at = got.iter().zip(&want).position(|(a, b)| a != b).unwrap_or(got.len().min(want.len()));
            panic!("{} differs from tests/save/{} at byte {} ({} vs {} bytes); compare them with tools/savdiff.py", name, name, at, got.len(), want.len());
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}
