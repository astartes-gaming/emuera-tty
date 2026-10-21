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
               PRINTS \"S=\" + SUBSTRING(TALENTNAME:LOCAL, 0, 9) + \" \" + TOUPPER(TALENTNAME:LOCAL)\nPRINTL\n\
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
    // so section 4's constant folding doesn't apply); TOUPPER passes it on.
    // (PRINTS: a `%…%` would get the percent fallbacks, and the memory of
    // that untranslated read would translate it.)
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

/// spec: Flagged string literals / Literal through ALL, Literal on a
/// TALENTNAME line; PRINTPLAIN's text; a constant PRINTFORM and plain
/// PRINT print as written
#[test]
fn literals_by_the_line() {
    let text = play(
        "literals",
        &[VIRGIN, ("CSV/ALL_TR.csv", "男,Man\n"), ("CSV/Abl.csv", "1,技巧\n")],
        "PRINTS \"男\"\nPRINTL\n\
         PRINTS \"K=\" + \"男\"\nPRINTL\n\
         LOCALS = N=\nPRINTS LOCALS + \"男\"\nPRINTL\n\
         PRINTS TALENTNAME:5\nPRINTL\n\
         PRINTV \"A=\", ABLNAME:1, TALENTNAME:5\nPRINTL\n\
         PRINTPLAIN P=男\nPRINTL\n\
         PRINTFORML F=男 %\"男\"%\n\
         PRINTL R=男",
    );
    assert!(text.lines().any(|l| l == "Man"), "{}", text);
    // a constant concatenation is folded first, then looked up whole
    assert!(text.contains("K=男"), "{}", text);
    // a literal next to a variable is looked up on its own
    assert!(text.contains("N=Man"), "{}", text);
    assert!(text.lines().any(|l| l == "Virgin"), "{}", text);
    // the line names ABLNAME first: both constants go through Abl_TR
    assert!(text.contains("A=技巧処女"), "{}", text);
    // PRINTPLAIN looks its whole text up
    assert!(text.contains("P=男"), "{}", text);
    assert!(text.contains("F=男 男"), "{}", text);
    assert!(text.contains("R=男"), "{}", text);
}

/// spec: Constants folded as Anchor folds them: under an operator a
/// constant name is translated (even in a condition), a constant stored
/// or returned whole stays as folded
#[test]
fn constants_fold_as_in_anchor() {
    let erb = "@SYSTEM_TITLE\nLOCAL = 5\n\
               IF TALENTNAME:5 == \"処女\"\nPRINTL C=original\nELSE\nPRINTL C=folded\nENDIF\n\
               SIF TALENTNAME:LOCAL == \"処女\"\nPRINTL V=original\n\
               LOCALS '= TALENTNAME:5\nSIF LOCALS == \"処女\"\nPRINTL S=as-written\n\
               LOCALS '= \"[\" + TALENTNAME:5 + \"]\"\nSIF LOCALS == \"[Virgin]\"\nPRINTL O=folded\n\
               SIF CONSTNAME() == \"処女\"\nPRINTL R=as-written\n\
               PRINTS \"T=\" + (LOCAL ? TALENTNAME:5 # \"\")\nPRINTL\n\
               QUIT\n\
               @CONSTNAME\n#FUNCTIONS\nRETURNF TALENTNAME:5\n";
    let text = play_erb("fold", &[VIRGIN], erb, |_| {});
    assert!(text.contains("C=folded"), "{}", text);
    assert!(text.contains("V=original"), "{}", text);
    assert!(text.contains("S=as-written"), "{}", text);
    assert!(text.contains("O=folded"), "{}", text);
    assert!(text.contains("R=as-written"), "{}", text);
    // a constant branch is folded with the flag (the ternary is an operator)
    assert!(text.contains("T=Virgin"), "{}", text);
}

/// spec: Constant names given to built-in functions / SUBSTRING of a
/// constant name, Variable index is not folded; a constant `%…%` gets no
/// percent fallbacks
#[test]
fn constant_names_given_to_functions() {
    let text = play(
        "const-fn",
        &[VIRGIN, ("CSV/Abl.csv", "2,技巧\n"), ("CSV/Abl_TR.csv", "技巧,Technique\n"), ("CSV/ALL_TR.csv", "Virgin,Maiden\n")],
        "LOCAL = 2\n\
         IF SUBSTRING(ABLNAME:2, 0, 4) == \"Tech\"\nPRINTL C=Tech\nENDIF\n\
         IF SUBSTRING(ABLNAME:LOCAL, 0, 4) == \"技巧\"\nPRINTL V=original\nENDIF\n\
         PRINTFORML P=%TALENTNAME:5%",
    );
    assert!(text.contains("C=Tech"), "{}", text);
    assert!(text.contains("V=original"), "{}", text);
    // folded at load: Talent_TR only, not then ALL_TR's Virgin → Maiden
    assert!(text.contains("P=Virgin"), "{}", text);
}

/// spec: Percent values get an ALL lookup / Whole value in ALL_TR, Width
/// turns translation on; Stripped ALL lookup / Level suffix; RETURNF
/// lines skip the fallbacks; Padding counts translated text
#[test]
fn percent_fallbacks() {
    let erb = "@SYSTEM_TITLE\nLOCAL = 5\n\
               LOCALS = 男\nPRINTFORML W=[%LOCALS%]\n\
               IF STRFORM(\"%TALENTNAME:LOCAL,10%\") == \"    Virgin\"\nPRINTL D=width\nENDIF\n\
               LOCALS = 技巧LV3\nPRINTFORML L=%LOCALS%\n\
               PRINTS \"R=\" + IN_RETURNF()\nPRINTL\n\
               PRINTFORML P=[%TALENTNAME:LOCAL, 8, LEFT%]\n\
               QUIT\n\
               @IN_RETURNF\n#FUNCTIONS\nLOCALS = 男\nRETURNF @\"%LOCALS%\"\n";
    let text = play_erb("percent", &[VIRGIN, ("CSV/ALL_TR.csv", "男,Man\n技巧,Technique\n")], erb, |_| {});
    assert!(text.contains("W=[Man]"), "{}", text);
    assert!(text.contains("D=width"), "{}", text);
    assert!(text.contains("L=TechniqueLV3"), "{}", text);
    assert!(text.contains("R=男"), "{}", text);
    assert!(text.contains("P=[Virgin  ]"), "{}", text);
}

/// spec: Memory of untranslated reads, Memory fallback / Value from a
/// variable: a name read without the flag is remembered, and a printed
/// `%…%` holding it is translated from memory. Anchor keeps 19 pairs.
#[test]
fn memory_of_untranslated_reads() {
    let mut talent = String::new();
    let mut tr = String::new();
    for i in 1..=20 {
        talent.push_str(&format!("{},名{:02}\n", i, i));
        tr.push_str(&format!("名{:02},N{:02}\n", i, i));
    }
    // READ(n): SAVESTR:0 gets name 1 untranslated (the function runs without
    // the flag: it is called from a condition), then names 2..n are read
    let erb = "@SYSTEM_TITLE\n\
               SIF READ(19)\nPRINTL never\nPRINTFORML A=[%SAVESTR:0%]\n\
               SIF READ(20)\nPRINTL never\nPRINTFORML B=[%SAVESTR:0%]\n\
               QUIT\n\
               @READ(ARG)\n#FUNCTION\nLOCAL = 1\nSAVESTR:0 '= TALENTNAME:LOCAL\n\
               FOR LOCAL, 2, ARG + 1\nLOCAL:1 += TALENTNAME:LOCAL == \"\"\nNEXT\nRETURNF 0\n";
    let text = play_erb("memory", &[("CSV/Talent.csv", &talent), ("CSV/Talent_TR.csv", &tr)], erb, |_| {});
    // 19 names read: the first is still remembered
    assert!(text.contains("A=[N01]"), "{}", text);
    // the second READ reads names 1..19 again (remembered already, no
    // change) and name 20, new: the memory reaches 20 and drops name 1
    assert!(text.contains("B=[名01]"), "{}", text);
}

/// The twentieth new pair pushes the first out (Anchor adds, then drops the
/// oldest once it holds 20).
#[test]
fn memory_keeps_nineteen() {
    let mut talent = String::new();
    let mut tr = String::new();
    for i in 1..=20 {
        talent.push_str(&format!("{},名{:02}\n", i, i));
        tr.push_str(&format!("名{:02},N{:02}\n", i, i));
    }
    let erb = "@SYSTEM_TITLE\nSIF READ(20)\nPRINTL never\nPRINTFORML B=[%SAVESTR:0%]\nQUIT\n\
               @READ(ARG)\n#FUNCTION\nLOCAL = 1\nSAVESTR:0 '= TALENTNAME:LOCAL\n\
               FOR LOCAL, 2, ARG + 1\nLOCAL:1 += TALENTNAME:LOCAL == \"\"\nNEXT\nRETURNF 0\n";
    let text = play_erb("memory19", &[("CSV/Talent.csv", &talent), ("CSV/Talent_TR.csv", &tr)], erb, |_| {});
    assert!(text.contains("B=[名01]"), "{}", text);
}

/// spec: Memory fallback: a value containing the last translation stays as
/// it is, even when the memory could translate part of it
#[test]
fn last_translation_stops_the_memory() {
    let erb = "@SYSTEM_TITLE\nLOCAL = 5\n\
               SIF READ()\nPRINTL never\n\
               PRINTV TALENTNAME:LOCAL\nPRINTL\n\
               LOCALS = Virgin・処女\nPRINTFORML K=%LOCALS%\n\
               LOCALS = 処女\nPRINTFORML M=%LOCALS%\n\
               QUIT\n\
               @READ\n#FUNCTION\nLOCAL = 5\nLOCAL:1 = TALENTNAME:LOCAL == \"\"\nRETURNF 0\n";
    let text = play_erb("last", &[VIRGIN], erb, |_| {});
    // the PRINTV made "Virgin" the last translation
    assert!(text.contains("K=Virgin・処女"), "{}", text);
    // without it, the remembered 処女 is translated
    assert!(text.contains("M=Virgin"), "{}", text);
}

/// A whole CSV name array given to a built-in (`FINDELEMENT(BASENAME, …)`)
/// stays a variable: it isn't a constant (AnonTW's BASE_BAR does this).
#[test]
fn name_array_given_whole() {
    let text = play("name-array", &[VIRGIN, ("CSV/Base.csv", "0,体力\n1,気力\n")], "PRINTFORML F={FINDELEMENT(BASENAME, \"気力\")}");
    assert!(text.contains("F=1"), "{}", text);
}
