//! Emuera-Anchor's translation system, scenario by scenario from
//! openspec/changes/add-anchor-translation-parity/specs/.
mod common;
use common::{game, run, temp_dir};
use std::path::Path;

/// A game with Talent 5 = 処女 and the ERB file `erb`; `files` add CSVs.
/// `check` looks at the game folder before it is removed.
fn play_erb(tag: &str, files: &[(&str, &str)], erb: &str, check: impl FnOnce(&Path)) -> String {
    let dir = temp_dir(tag);
    let mut all: Vec<(&str, &str)> = vec![("CSV/Talent.csv", "5,処女\n"), ("ERB/T.ERB", erb)];
    all.extend_from_slice(files);
    game(&dir, &all);
    let text = run(&dir, b"");
    check(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(!text.contains("script error"), "{}", text);
    text
}

/// A game whose @SYSTEM_TITLE runs `body` and quits; `files` add CSVs.
fn play(tag: &str, files: &[(&str, &str)], body: &str) -> String {
    play_erb(tag, files, &format!("@SYSTEM_TITLE\n{}\nQUIT\n", body), |_| {})
}

/// Talent_TR.csv: 処女 → Virgin
const VIRGIN: (&str, &str) = ("CSV/Talent_TR.csv", "処女,Virgin\n");

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

/// spec: Translate flag / Name printed with PRINTV, Name in a condition,
/// Plain PRINT text, PRINTBUTTON
#[test]
fn which_statements_translate() {
    let text = play(
        "flag-stmts",
        &[VIRGIN],
        "LOCAL = 5\n\
         PRINTV \"V=\", TALENTNAME:LOCAL\nPRINTL\n\
         IF TALENTNAME:LOCAL == \"処女\"\nPRINTL C=original\nELSE\nPRINTL C=translated\nENDIF\n\
         PRINTFORML F={TALENTNAME:LOCAL == \"処女\"}\n\
         PRINTL P=処女\n\
         PRINTBUTTON \"B=\" + TALENTNAME:LOCAL, 1\nPRINTL\n\
         PRINTDATA\nDATAFORM D=%TALENTNAME:LOCAL%\nENDDATA\n\
         HTML_PRINT \"H=\" + TALENTNAME:LOCAL\nPRINTL",
    );
    assert!(text.contains("V=Virgin"), "{}", text);
    assert!(text.contains("D=Virgin"), "{}", text);
    assert!(text.contains("H=Virgin"), "{}", text);
    assert!(text.contains("C=original"), "{}", text);
    // a comparison inside a flagged statement sees the original too
    assert!(text.contains("F=1"), "{}", text);
    assert!(text.contains("P=処女"), "{}", text);
    assert!(text.contains("B=処女"), "{}", text);
}

/// spec: What a flagged read translates / No ALL fallback for a name read,
/// RESULTS through Str_TR
#[test]
fn what_a_flagged_read_translates() {
    let text = play(
        "flag-reads",
        &[("CSV/ALL_TR.csv", "処女,Maiden\n"), ("CSV/Str_TR.csv", "はい,Yes\n")],
        "LOCAL = 5\nPRINTV \"N=\", TALENTNAME:LOCAL\nPRINTL\nRESULTS = はい\nPRINTV \"R=\", RESULTS\nPRINTL",
    );
    assert!(text.contains("N=処女"), "{}", text);
    assert!(text.contains("R=Yes"), "{}", text);
}

/// The UTF-16LE bytes a binary save holds `s` as.
fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

/// Every `.sav` file under `dir`.
fn saves(dir: &Path) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(saves(&p));
        } else if p.extension().is_some_and(|x| x == "sav") {
            out.push(std::fs::read(p).unwrap());
        }
    }
    out
}

/// spec: Translated string assignment / Expression assignment stores
/// translation (and a save holds it), FORM assignment, Appending
#[test]
fn string_assignments_store_the_translation() {
    let erb = "@SYSTEM_TITLE\nCALL F\nQUIT\n\
               @F\nLOCAL = 5\n\
               LOCALS '= TALENTNAME:LOCAL\nSIF LOCALS == \"Virgin\"\nPRINTL E=ok\n\
               LOCALS = [%TALENTNAME:LOCAL%]\nSIF LOCALS == \"[Virgin]\"\nPRINTL F=ok\n\
               LOCALS = a\nLOCALS += TALENTNAME:LOCAL\nSIF LOCALS == \"aVirgin\"\nPRINTL A=ok\n\
               LOCALS:1 '= TALENTNAME:LOCAL, TALENTNAME:LOCAL\nSIF LOCALS:2 == \"Virgin\"\nPRINTL L=ok\n\
               SAVESTR:0 '= TALENTNAME:LOCAL\nSAVEDATA 0, \"t\"\n";
    let virgin = utf16("Virgin");
    let text = play_erb("flag-assign", &[VIRGIN], erb, |dir| {
        let s = saves(dir);
        assert!(!s.is_empty(), "no save written");
        assert!(s.iter().any(|b| b.windows(virgin.len()).any(|w| w == virgin)), "the save doesn't hold 'Virgin'");
    });
    for k in ["E=ok", "F=ok", "A=ok", "L=ok"] {
        assert!(text.contains(k), "{}: {}", k, text);
    }
}

/// spec: Flag passed through expressions / Concatenation, User function
/// result, Ternary does not pass the flag
#[test]
fn flag_through_expressions() {
    let erb = "@SYSTEM_TITLE\nLOCAL = 5\n\
               PRINTS \"C=\" + \"[\" + TALENTNAME:LOCAL + \"]\"\nPRINTL\n\
               PRINTFORML U=%NAME_OF(5)%\n\
               SIF NAME_OF(5) == \"処女\"\nPRINTL W=original\n\
               PRINTS \"T=\" + (LOCAL ? TALENTNAME:LOCAL # \"\")\nPRINTL\n\
               PRINTFORML Y=\\@ LOCAL ? %TALENTNAME:LOCAL% # x \\@\n\
               PRINTFORML S=%SUBSTRING(TALENTNAME:LOCAL, 0, 9)% %TOUPPER(TALENTNAME:LOCAL)%\n\
               CALL SHOW, TALENTNAME:LOCAL\n\
               QUIT\n\
               @NAME_OF(ARG)\n#FUNCTIONS\nRETURNF TALENTNAME:ARG\n\
               @SHOW(ARGS)\nSIF ARGS == \"処女\"\nPRINTL A=original\n";
    let text = play_erb("flag-expr", &[VIRGIN], erb, |_| {});
    assert!(text.contains("C=[Virgin]"), "{}", text);
    assert!(text.contains("U=Virgin"), "{}", text);
    // called from a condition, the function runs without the flag
    assert!(text.contains("W=original"), "{}", text);
    assert!(text.contains("T=処女"), "{}", text);
    assert!(text.contains("Y=処女"), "{}", text);
    // SUBSTRING reads without the flag (a variable index isn't a constant,
    // so section 4's constant folding doesn't apply); TOUPPER passes it on
    assert!(text.contains("S=処女 VIRGIN"), "{}", text);
    // CALL arguments are read without the flag
    assert!(text.contains("A=original"), "{}", text);
}

/// PRINT_ABL and the like print translated names (design D7: Anchor
/// prints the CSV names there).
#[test]
fn engine_lists_are_translated() {
    let text = play(
        "print-abl",
        &[("CSV/Abl.csv", "1,技巧\n"), ("CSV/Abl_TR.csv", "技巧,Technique\n"), ("CSV/Chara0.csv", "番号,0\n名前,a\n")],
        "ADDCHARA 0\nABL:0:1 = 3\nPRINT_ABL 0\nPRINTL",
    );
    assert!(text.contains("TechniqueLV3"), "{}", text);
}
