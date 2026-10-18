//! Runs the sample game headlessly: the ERB self-test, and a save / load
//! round trip through Emuera's save format.
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// A fresh copy of `sample/` (the tests write save files into it).
fn sample_copy(tag: &str) -> PathBuf {
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
    let dir = std::env::temp_dir().join(format!("emuera-tty-test-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy(Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/sample")), &dir);
    dir
}

fn run(dir: &Path, input: &[u8]) -> String {
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

#[test]
fn sample_selftest_passes() {
    let dir = sample_copy("selftest");
    // new game, advance a day, quit
    let text = run(&dir, b"0\n101\n\n999\n");
    assert!(text.contains("OK  arith"), "{}", text);
    assert!(!text.contains("NG "), "{}", text);
    // every CHECK/CHECKS call must have run and printed OK
    let oks = text.lines().filter(|l| l.starts_with("OK  ")).count();
    assert!(text.contains(&format!("selftest: {} checks done", oks)), "{}", text);
    assert!(oks >= 104, "only {} checks ran\n{}", oks, text);
    assert!(text.contains("1日目"), "{}", text);
    // Emuera-Anchor: Abl_TR.csv applies when the name is printed
    assert!(text.contains("printed: Technique"), "{}", text);
    assert!(!text.contains("script error"), "{}", text);
    let _ = std::fs::remove_dir_all(&dir);
}

/// The body of a binary save after its comment (the comment holds a
/// timestamp, so two saves of the same state differ only there).
fn after_comment(file: &[u8]) -> &[u8] {
    let mut p = 16 + 1 + 8 + 8;
    let (mut n, mut shift) = (0usize, 0);
    loop {
        let b = file[p];
        p += 1;
        n |= ((b & 0x7F) as usize) << shift;
        if b & 0x80 == 0 {
            break;
        }
        shift += 7;
    }
    &file[p + n..]
}

#[test]
fn save_load_roundtrip() {
    let dir = sample_copy("save");
    // day 1, save to slot 3, day 2, load slot 3, save the loaded state to 4
    let text = run(&dir, b"0\n101\n\n102\n3\n101\n\n103\n3\n102\n4\n999\n");
    assert!(text.contains("ロードしました"), "{}", text);
    let after_load = &text[text.rfind("ロードしました").unwrap()..];
    assert!(after_load.contains("1日目"), "loaded state isn't day 1:\n{}", after_load);
    // games without the sav-folder option save next to the game, as Emuera
    let a = std::fs::read(dir.join("save03.sav")).expect("save03.sav");
    let b = std::fs::read(dir.join("save04.sav")).expect("save04.sav");
    assert_eq!(&a[..8], &0x0A1A_0A0D_4152_4589u64.to_le_bytes(), "Emuera binary header");
    assert!(after_comment(&a) == after_comment(&b), "re-saving a loaded game changed its data");
    let _ = std::fs::remove_dir_all(&dir);
}
