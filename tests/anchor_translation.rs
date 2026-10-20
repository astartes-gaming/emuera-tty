//! Emuera-Anchor's translation system, scenario by scenario from
//! openspec/changes/add-anchor-translation-parity/specs/.
mod common;
use common::{game, run, temp_dir};

/// A game whose @SYSTEM_TITLE runs `body` and quits; `files` add CSVs.
fn play(tag: &str, files: &[(&str, &str)], body: &str) -> String {
    let dir = temp_dir(tag);
    let erb = format!("@SYSTEM_TITLE\n{}\nQUIT\n", body);
    let mut all: Vec<(&str, &str)> = vec![("CSV/Talent.csv", "5,処女\n"), ("ERB/T.ERB", &erb)];
    all.extend_from_slice(files);
    game(&dir, &all);
    let text = run(&dir, b"");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!text.contains("script error"), "{}", text);
    text
}

/// spec: Translation only in translated games / Game without TR files
#[test]
fn game_without_tr_files() {
    let text = play("no-tr", &[], "LOCALS '= TALENTNAME:5\nPRINTFORML [%LOCALS%]");
    assert!(text.contains("[処女]"), "{}", text);
}

/// spec: TR file problems are warnings (they're shown, and the game starts)
#[test]
fn tr_file_problems_are_load_warnings() {
    let text = play("tr-warn", &[("CSV/Talent_TR.csv", "処女,Virgin\n処女,Maiden\n")], "PRINTFORML [%TALENTNAME:5%]");
    assert!(text.contains("Talent_TR.csv:2: '処女' is already translated on line 1 (this one is ignored)"), "{}", text);
    assert!(text.contains("[Virgin]"), "{}", text);
}
