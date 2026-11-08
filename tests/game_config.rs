//! The game's Emuera settings (CSV/_default.config, emuera.config,
//! CSV/_fixed.config) change how scripts are read and run, as in Emuera.
//! One small game per setting, with and without it.
mod common;
use common::{game, run, temp_dir};

/// Run `erb` (an `@SYSTEM_TITLE` that prints and QUITs) with `config` as
/// the game's emuera.config and `files` beside it; the output.
fn play(tag: &str, config: &str, files: &[(&str, &str)], erb: &str) -> String {
    let dir = temp_dir(tag);
    let mut all = vec![("emuera.config", config), ("ERB/T.ERB", erb)];
    all.extend_from_slice(files);
    game(&dir, &all);
    let out = run(&dir, b"", None);
    let _ = std::fs::remove_dir_all(&dir);
    out
}

/// `_Rename.csvを利用する` (off unless set) and `_Replace.csvを利用する`
/// (on unless set): Emuera reads each file only when its setting is on
/// (Process.Initialize).
#[test]
fn rename_and_replace_files_follow_the_settings() {
    let files = [("CSV/_Rename.csv", "ほげ,[[HOGE]]\n"), ("CSV/_Replace.csv", "お金の単位,G\n")];
    let erb = "@SYSTEM_TITLE\nPRINTFORML [[HOGE]] %MONEYSTR(5)%\nQUIT\n";
    let on = play("cfg-rename-on", "_Rename.csvを利用する:YES\n", &files, erb);
    assert!(on.contains("ほげ 5G") || on.contains("ほげ G5"), "{on}");
    // unset: no _Rename.csv (Emuera's default), _Replace.csv used
    let unset = play("cfg-rename-unset", "", &files, erb);
    assert!(unset.contains("[[HOGE]] 5G") || unset.contains("[[HOGE]] G5"), "{unset}");
    let off = play("cfg-replace-off", "USE _REPLACE.CSV FILE:NO\n", &files, erb);
    assert!(off.contains("[[HOGE]] $5"), "Emuera's own currency sign: {off}");
    // on, without the file: Emuera's message
    let missing = play("cfg-rename-missing", "_Rename.csvを利用する:YES\n", &[], "@SYSTEM_TITLE\nQUIT\n");
    assert!(missing.contains("_Rename.csvが見つかりません"), "{missing}");
}

/// 全角スペースをホワイトスペースに含める (on unless set): off, a full-width
/// space where Emuera lexes code is an error, and allowed in text: string
/// literals, FORM text, `\@ ? # \@` branches, PRINT's raw text.
#[test]
fn full_width_space_follows_the_setting() {
    let off = "全角スペースをホワイトスペースに含める:NO\n";
    let ok = "@SYSTEM_TITLE\n#DIMS S\nS = text　with a space\nPRINTL raw　text\nPRINTFORML %\"a　b\"% %\\@ 1 ? x # 　 \\@% %S%\nPRINTFORML {1 + 1}\nQUIT\n";
    let out = play("cfg-space-text", off, &[], ok);
    assert!(out.contains("raw　text") && out.contains("a　b x text　with a space") && out.contains('2'), "{out}");
    let after_name = play("cfg-space-cmd", off, &[], "@SYSTEM_TITLE\nPRINTL　x\nQUIT\n");
    assert!(after_name.contains("命令の直後に半角スペース・タブ以外の文字が来ています"), "{after_name}");
    let in_expr = play("cfg-space-expr", off, &[], "@SYSTEM_TITLE\nPRINTFORML {1　+ 1}\nQUIT\n");
    assert!(in_expr.contains("予期しない全角スペースを発見しました"), "{in_expr}");
    let lhs = play("cfg-space-lhs", off, &[], "@SYSTEM_TITLE\nLOCAL　= 1\nQUIT\n");
    assert!(lhs.contains("予期しない全角スペースを発見しました"), "{lhs}");
    // on (Emuera's default): whitespace
    let on = play("cfg-space-on", "", &[], "@SYSTEM_TITLE\nPRINTFORML {1　+ 1}\nLOCAL　= 3\nPRINTFORML {LOCAL}\nQUIT\n");
    assert!(on.contains('2') && on.contains('3') && !on.contains("全角"), "{on}");
}

/// FORM中の三連記号を展開しない (off unless set): `***` is NAME:TARGET,
/// `+++` CALLNAME:MASTER (Emuera's StrForm).
#[test]
fn triple_symbols_follow_the_setting() {
    let erb = "@SYSTEM_TITLE\nADDVOIDCHARA\nNAME:0 = Reimu\nCALLNAME:0 = Rei\nTARGET = 0\nMASTER = 0\nPRINTFORML [***][+++][**]\nQUIT\n";
    let expand = play("cfg-triple-on", "", &[], erb);
    assert!(expand.contains("[Reimu][Rei][**]"), "{expand}");
    let keep = play("cfg-triple-off", "FORM中の三連記号を展開しない:YES\n", &[], erb);
    assert!(keep.contains("[***][+++][**]"), "{keep}");
}

/// 行連結の改行コードの置換文字列: what joins the lines of a `{ … }` block.
#[test]
fn continuation_joins_with_the_setting() {
    let erb = "@SYSTEM_TITLE\n{\nPRINTFORML a\nb\n}\nQUIT\n";
    assert!(play("cfg-cont-default", "", &[], erb).contains("a b"));
    let dash = play("cfg-cont-dash", "行連結の改行コードの置換文字列:\"-\"\n", &[], erb);
    assert!(dash.contains("a-b"), "{dash}");
}
