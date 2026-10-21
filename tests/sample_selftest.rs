//! Runs the sample game headlessly: the ERB self-test, and a save / load
//! round trip through Emuera's save format.
mod common;
use common::{game, run, sample_copy, temp_dir};

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
    // Emuera-Anchor: Abl_TR.csv applies when the name is printed (not in
    // the CALL CHECKS arguments, "ablname")
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

/// DOTRAIN as in Emuera: the event it is in runs to its end, then the
/// command runs in place of what came next; inside a command it is an error.
#[test]
fn dotrain_runs_after_the_event() {
    let dir = temp_dir("dotrain");
    let erb = "@SYSTEM_TITLE\nBEGIN TRAIN\n@EVENTTRAIN\nNEXTCOM = -1\n\
        @SHOW_STATUS\nFLAG:0 += 1\nPRINTFORML status {FLAG:0}\nSIF FLAG:0 == 1\n  DOTRAIN 1\nSIF FLAG:0 == 1\n  PRINTL rest of status\nSIF FLAG:0 == 3\n  QUIT\n\
        @SHOW_USERCOM\nPRINTL usercom list\n\
        @USERCOM\nPRINTFORML usercom {RESULT}\nSIF RESULT == 50\n  DOTRAIN 999\nPRINTL rest of usercom\n\
        @COM1\nPRINTL com1\nRETURN 1\n@COM999\nPRINTL com999\nRETURN 1\n@COM0\nDOTRAIN 1\nRETURN 1\n\
        @EVENTCOMEND\nPRINTFORML comend {SELECTCOM}\n";
    game(&dir, &[("CSV/Train.csv", "0,A\n1,B\n999,C\n"), ("ERB/T.ERB", erb)]);
    let text = run(&dir, b"\n50\n\n0\n\n");
    let order =
        ["status 1", "rest of status", "com1", "comend 1", "status 2", "usercom list", "usercom 50", "rest of usercom", "com999", "comend 999", "status 3"];
    let mut at = 0;
    for o in order {
        let p = text[at..].find(o).unwrap_or_else(|| panic!("{:?} missing after byte {}:\n{}", o, at, text));
        at += p + o.len();
    }
    let _ = std::fs::remove_dir_all(&dir);
    // DOTRAIN inside @COMn: Emuera refuses it
    game(&dir, &[("CSV/Train.csv", "0,A\n1,B\n"), ("ERB/T.ERB", "@SYSTEM_TITLE\nBEGIN TRAIN\n@EVENTTRAIN\nNEXTCOM = 0\n@COM0\nDOTRAIN 1\nRETURN 1\n")]);
    let text = run(&dir, b"\n\n");
    assert!(text.contains("DOTRAIN can only be used"), "{}", text);
    let _ = std::fs::remove_dir_all(&dir);
}
