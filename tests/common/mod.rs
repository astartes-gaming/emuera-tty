//! Helpers shared by the integration tests: games in temp folders, run
//! headlessly.
#![allow(dead_code)]
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// A fresh, empty temp folder for one test.
pub fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("emuera-tty-test-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A fresh copy of `sample/` (the tests write save files into it).
pub fn sample_copy(tag: &str) -> PathBuf {
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for e in std::fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            let t = to.join(e.file_name());
            if e.file_type().unwrap().is_dir() {
                copy(&e.path(), &t);
            } else {
                std::fs::copy(e.path(), t).unwrap();
            }
        }
    }
    let dir = temp_dir(tag);
    copy(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/sample")), &dir);
    dir
}

/// Write a small game into `dir`: `files` are (path under `dir`, contents),
/// e.g. `("CSV/Talent.csv", "5,処女\n")`, `("ERB/T.ERB", "@SYSTEM_TITLE\n…")`.
/// `CSV/` and `ERB/` are created even when no file goes in them.
pub fn game(dir: &Path, files: &[(&str, &str)]) {
    std::fs::create_dir_all(dir.join("CSV")).unwrap();
    std::fs::create_dir_all(dir.join("ERB")).unwrap();
    for (path, text) in files {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
}

/// Run the game in `dir` headlessly with a fixed random seed, typing
/// `input`; stdout then stderr.
pub fn run(dir: &Path, input: &[u8]) -> String {
    let exe = env!("CARGO_BIN_EXE_emuera-tty");
    let mut child = Command::new(exe)
        .arg(dir)
        .args(["--headless", "--echo"])
        .env("EMUERA_SEED", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    let out = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr)
}
