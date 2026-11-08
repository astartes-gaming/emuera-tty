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
