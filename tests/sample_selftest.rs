//! Runs the sample game headlessly and checks the ERB self-test output.
use std::io::Write;
use std::process::{Command, Stdio};

#[test]
fn sample_selftest_passes() {
    let exe = env!("CARGO_BIN_EXE_emuera-rs");
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/sample");
    let mut child = Command::new(exe)
        .args([dir, "--headless"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // new game, advance a day, quit
    child.stdin.take().unwrap().write_all(b"0\n101\n999\n").unwrap();
    let out = child.wait_with_output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("OK  arith"), "{}", text);
    assert!(!text.contains("NG "), "{}", text);
    // every CHECK/CHECKS call must have run and printed OK
    let oks = text.lines().filter(|l| l.starts_with("OK  ")).count();
    assert!(text.contains(&format!("selftest: {} checks done", oks)), "{}", text);
    assert!(oks >= 52, "only {} checks ran\n{}", oks, text);
    assert!(text.contains("1日目"), "{}", text);
    assert!(!text.contains("script error"), "{}", text);
}
