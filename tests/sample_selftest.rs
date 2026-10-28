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
    assert!(oks >= 105, "only {} checks ran\n{}", oks, text);
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
fn typed_macros_answer_several_inputs() {
    // Emuera's input syntax: `\n` separates inputs, `\e` skips the
    // following waits, `(…)*n` repeats; ONEINPUT takes the first character
    let dir = std::env::temp_dir().join(format!("emuera-tty-test-macro-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("CSV")).unwrap();
    std::fs::create_dir_all(dir.join("ERB")).unwrap();
    let erb = "@SYSTEM_TITLE\nREPEAT 3\nINPUT\nPRINTFORML got {RESULT}\nREND\nWAIT\nPRINTL after wait\nWAIT\n\
        INPUT\nPRINTFORML got {RESULT}\nONEINPUT\nPRINTFORML one {RESULT}\nQUIT\n";
    std::fs::write(dir.join("ERB/T.ERB"), erb).unwrap();
    let text = run(&dir, b"(1\\n)*2\\n5\\e\\n7\\n42\n");
    let order = ["got 1", "got 1", "got 5", "after wait", "got 7", "one 4"];
    let mut at = 0;
    for o in order {
        let p = text[at..].find(o).unwrap_or_else(|| panic!("{:?} missing after byte {}:\n{}", o, at, text));
        at += p + o.len();
    }
    let _ = std::fs::remove_dir_all(&dir);
}

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

/// Inside a `#FUNCTION(S)`, statements without Emuera's METHOD_SAFE flag
/// (input, CALL, BEGIN, PRINTW…) are errors when they run, as in Emuera-EM
/// and Anchor; one that never runs is harmless; `--check` lists them all.
#[test]
fn commands_not_allowed_in_functions() {
    let dir = temp_dir("not-in-function");
    let erb = "@SYSTEM_TITLE\nPRINTFORML [%GOOD()%]\nPRINTFORML [%BAD()%]\nPRINTL after\nQUIT\n\
        @GOOD\n#FUNCTIONS\nSIF 0\n  WAIT\nIF 0\n  PRINTW x\nENDIF\nPRINTL printing is allowed\nRETURNF \"good\"\n\
        @BAD\n#FUNCTIONS\nBEGIN TITLE\nRETURNF \"x\"\n";
    game(&dir, &[("ERB/T.ERB", erb)]);
    let text = run(&dir, b"\n");
    assert!(text.contains("printing is allowed"), "{}", text);
    assert!(text.contains("[good]"), "{}", text);
    assert!(text.contains("BEGIN instruction cannot be used in #FUNCTION(S)"), "{}", text);
    assert!(!text.contains("after"), "{}", text);
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_emuera-tty")).arg(&dir).arg("--check").output().unwrap();
    let check = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    for c in ["WAIT", "PRINTW", "BEGIN"] {
        assert!(check.contains(&format!("{} instruction cannot be used in #FUNCTION(S)", c)), "{}: {}", c, check);
    }
    assert!(!check.contains("PRINTL instruction"), "{}", check);
    let _ = std::fs::remove_dir_all(&dir);
}

/// `X += expr,` with a trailing comma is one value, as in Emuera (eraTW's
/// SOURCE.ERB writes `TCVAR:LCOUNT:体力回復 += 10 + RAND:10,`).
#[test]
fn trailing_comma_in_assignment() {
    let dir = temp_dir("trailing-comma");
    game(&dir, &[("ERB/T.ERB", "@SYSTEM_TITLE\nLOCAL = 5\nLOCAL += 10,\nLOCAL:1 = 1, 2,\nPRINTFORML V={LOCAL} {LOCAL:1}{LOCAL:2}\nQUIT\n")]);
    let text = run(&dir, b"");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(text.contains("V=15 12"), "{}", text);
}

/// _Replace.csv's "COM_ABLE初期値" is what a command without @COM_ABLEn
/// gets: 0 hides it (eratohoReverse lists its reactions itself), and the
/// numbers shown are still the positions in TRAIN.csv, as in Emuera.
#[test]
fn com_able_default_from_replace_csv() {
    let dir = temp_dir("comable");
    let erb = "@SYSTEM_TITLE\nBEGIN TRAIN\n@SHOW_STATUS\nPRINTL status\n@SHOW_USERCOM\nPRINTL usercom list\n\
        @COM_ABLE1\nRETURN 1\n@COM1\nPRINTL com1\nRETURN 1\n@USERCOM\nPRINTFORML usercom {RESULT}\nQUIT\n";
    game(&dir, &[("CSV/Train.csv", "0,Hidden\n1,Shown\n2,AlsoHidden\n"), ("CSV/_Replace.csv", "COM_ABLE初期値 , 0\n"), ("ERB/T.ERB", erb)]);
    // 0 isn't a command any more: it goes to @USERCOM
    let text = run(&dir, b"0\n");
    assert!(text.contains("Shown[  1]") && !text.contains("Hidden"), "{}", text);
    assert!(text.contains("usercom 0") && !text.contains("com1"), "{}", text);
}

/// Saves are written safely and their history kept (savesafe.rs): the
/// first save replaced that emuera-tty didn't write is kept as an
/// original, later ones as automatic copies, each write logged.
#[test]
fn save_history() {
    let dir = temp_dir("savehist");
    let erb = "@SYSTEM_TITLE\nINPUT\nFLAG:0 = RESULT\nSAVEDATA 1, \"x\"\nQUIT\n";
    game(&dir, &[("ERB/T.ERB", erb)]);
    let name = dir.canonicalize().unwrap().file_name().unwrap().to_string_lossy().to_string();
    let root = dir.join("data/emuera-tty/backups").join(name);
    let save = |n: &str| run(&dir, format!("{}\n", n).as_bytes());
    let folders = |kind: &str| -> Vec<std::path::PathBuf> {
        let mut v: Vec<_> =
            std::fs::read_dir(&root).unwrap().flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(&format!("-{}-save01", kind))).collect();
        v.sort();
        v
    };
    save("5");
    let five = std::fs::read(dir.join("save01.sav")).unwrap();
    // a save emuera-tty has no record of (as if Emuera.exe made it)
    std::fs::remove_dir_all(dir.join("data")).unwrap();
    save("6");
    let six = std::fs::read(dir.join("save01.sav")).unwrap();
    save("7");
    let orig = folders("original");
    let auto = folders("auto");
    assert_eq!(orig.len(), 1, "{:?}", orig);
    assert_eq!(std::fs::read(orig[0].join("save01.sav")).unwrap(), five);
    assert_eq!(auto.len(), 1, "{:?}", auto);
    assert_eq!(std::fs::read(auto[0].join("save01.sav")).unwrap(), six);
    let log = std::fs::read_to_string(root.join("save-log.tsv")).unwrap();
    let kinds: Vec<&str> = log.lines().filter(|l| !l.starts_with('#')).map(|l| l.split('\t').nth(1).unwrap()).collect();
    // (the first run's line went with the data folder)
    assert_eq!(kinds, ["original", "written", "written"], "{}", log);
    assert!(log.contains(env!("CARGO_PKG_VERSION")), "{}", log);
    // no temporary file left behind
    let tmp: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().contains(".tmp")).collect();
    assert!(tmp.is_empty(), "{:?}", tmp);
    let _ = std::fs::remove_dir_all(&dir);
}

/// On a small stack (Haiku allows a thread under 16 MB; EMUERA_STACK_MB
/// stands in for it here) endless recursion is still a script error, not
/// a crash: the call-depth limit follows the stack's size.
#[test]
fn deep_recursion_on_a_small_stack() {
    let dir = temp_dir("small-stack");
    for erb in
        ["@SYSTEM_TITLE\nCALL R(0)\n@R(N)\n#DIM N\nCALL R(N + 1)\n", "@SYSTEM_TITLE\nPRINTFORML {F(0)}\n@F(N)\n#FUNCTION\n#DIM N\nRETURNF 1 + F(N + 1)\n"]
    {
        game(&dir, &[("ERB/T.ERB", erb)]);
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_emuera-tty"))
            .arg(&dir)
            .arg("--headless")
            .env("EMUERA_STACK_MB", "2")
            .env("XDG_DATA_HOME", dir.join("data"))
            .stdin(std::process::Stdio::null())
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
        assert!(text.contains("call stack overflow"), "{:?}\n{}", out.status, text);
        assert!(out.status.code().is_some(), "killed: {:?}\n{}", out.status, text);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
