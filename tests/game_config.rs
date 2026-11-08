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

/// ユーザー関数の引数に自動的にTOSTRを補完する (off unless set): an integer
/// passed to a string parameter is Emuera's error, or TOSTR'd when on; a
/// string to an integer parameter is always an error
/// (Process.CalledFunction.cs, UserDefinedFunctionArgument).
#[test]
fn user_function_arguments_follow_the_settings() {
    let erb = "@SYSTEM_TITLE\nCALL F, 5\nQUIT\n@F, ARGS\nPRINTFORML [%ARGS%]\n";
    let off = play("cfg-tostr-off", "", &[], erb);
    assert!(off.contains("\"@F\"の1番目の引数を整数型から文字列型に変換できません") && !off.contains("[5]"), "{off}");
    let on = play("cfg-tostr-on", "ユーザー関数の引数に自動的にTOSTRを補完する:YES\n", &[], erb);
    assert!(on.contains("[5]"), "{on}");
    let str_to_int = play("cfg-tostr-str", "ユーザー関数の引数に自動的にTOSTRを補完する:YES\n", &[], "@SYSTEM_TITLE\nCALL G, \"x\"\nQUIT\n@G, ARG\n");
    assert!(str_to_int.contains("\"@G\"の1番目の引数を文字列型から整数型に変換できません"), "{str_to_int}");
    // ユーザー関数の全ての引数の省略を許可する: ARG/ARGS/private parameters
    // default to 0 / ""; another variable has no default
    let omit = "@SYSTEM_TITLE\nCALL H\nCALL K\nQUIT\n@H, ARG, ARGS\nPRINTFORML h[{ARG}][%ARGS%]\n@K, FLAG:3\nPRINTFORML k[{FLAG:3}]\n";
    let strict = play("cfg-omit-off", "", &[], omit);
    assert!(strict.contains("h[0][]") && strict.contains("\"@K\"の1番目の引数は省略できません") && !strict.contains("k["), "{strict}");
    let lax = play("cfg-omit-on", "ユーザー関数の全ての引数の省略を許可する:YES\n", &[], omit);
    assert!(lax.contains("k[0]"), "{lax}");
}

/// キャラクタ変数の引数を補完しない (off unless set): on, `CFLAG:3` (one
/// index of two) is Emuera's error instead of `CFLAG:TARGET:3`.
#[test]
fn character_variables_follow_the_no_target_setting() {
    let erb = "@SYSTEM_TITLE\nADDVOIDCHARA\nADDVOIDCHARA\nTARGET = 1\nCFLAG:1:3 = 7\nPRINTFORML [{CFLAG:1:3}][{CFLAG:3}]\nQUIT\n";
    let filled = play("cfg-notarget-off", "", &[], erb);
    assert!(filled.contains("[7][7]"), "{filled}");
    let strict = play("cfg-notarget-on", "キャラクタ変数の引数を補完しない:YES\n", &[], erb);
    assert!(strict.contains("キャラクタ配列変数\"CFLAG\"の引数は省略できません") && !strict.contains("[7]"), "{strict}");
}

/// 文字列変数の代入に文字列式を強制する (off unless set): on, `STR = text`
/// is Emuera's error and `STR '= "text"` the way to assign.
#[test]
fn string_assignment_follows_the_setting() {
    let erb = "@SYSTEM_TITLE\nRESULTS '= \"a\"\nPRINTFORML [%RESULTS%]\nRESULTS = b\nPRINTFORML [%RESULTS%]\nQUIT\n";
    assert!(play("cfg-strset-off", "", &[], erb).contains("[b]"));
    let strict = play("cfg-strset-on", "文字列変数の代入に文字列式を強制する:YES\n", &[], erb);
    assert!(strict.contains("[a]") && strict.contains("文字列代入は禁止されています") && !strict.contains("[b]"), "{strict}");
}

/// 擬似変数RANDの仕様をeramakerに合わせる (off unless set): RAND:0 is an
/// error; on, 0 gives 0 and a negative argument counts as positive.
#[test]
fn rand_follows_the_setting() {
    let erb = "@SYSTEM_TITLE\nLOCAL = 0\nPRINTFORML [{RAND:(LOCAL - 3) < 3}]\nPRINTFORML [{RAND:LOCAL}]\nQUIT\n";
    let strict = play("cfg-rand-off", "", &[], erb);
    assert!(strict.contains("RANDの引数に0以下の値(-3)が指定されました"), "{strict}");
    let compat = play("cfg-rand-on", "擬似変数RANDの仕様をeramakerに合わせる:YES\n", &[], erb);
    assert!(compat.contains("[1]\n") && compat.contains("[0]"), "{compat}");
}

/// TIMESの計算をeramakerにあわせる (off unless set): off, the factor is a
/// .NET decimal, so 100 TIMES 0.29 is 29; on, doubles make it 28.
#[test]
fn times_follows_the_setting() {
    let erb = "@SYSTEM_TITLE\nLOCAL = 100\nTIMES LOCAL, 0.29\nPRINTFORML [{LOCAL}]\nQUIT\n";
    assert!(play("cfg-times-off", "", &[], erb).contains("[29]"));
    let eramaker = play("cfg-times-on", "TIMESの計算をeramakerにあわせる:YES\n", &[], erb);
    assert!(eramaker.contains("[28]"), "{eramaker}");
}

/// CALLNAMEが空文字列の時にNAMEを代入する (off unless set): a character
/// without 呼び名 has an empty CALLNAME, or its NAME when on.
#[test]
fn callname_follows_the_setting() {
    let files = [("CSV/Chara1.csv", "番号,1\n名前,Reimu\n")];
    let erb = "@SYSTEM_TITLE\nADDCHARA 1\nPRINTFORML [%CALLNAME:0%]\nQUIT\n";
    assert!(play("cfg-callname-off", "", &files, erb).contains("[]"));
    let on = play("cfg-callname-on", "CALLNAMEが空文字列の時にNAMEを代入する:YES\n", &files, erb);
    assert!(on.contains("[Reimu]"), "{on}");
}

/// イベント関数のCALLを許可する (off unless set): CALL can't reach an event
/// function; on, it runs the first definition only.
#[test]
fn calling_event_functions_follows_the_setting() {
    let erb = "@SYSTEM_TITLE\nTRYCALL EVENTEND\nPRINTL after\nCALL EVENTEND\nQUIT\n@EVENTEND\nPRINTL one\n@EVENTEND\nPRINTL two\n";
    let off = play("cfg-callevent-off", "", &[], erb);
    assert!(off.contains("after") && off.contains("@EVENTEND not found") && !off.contains("one"), "{off}");
    let on = play("cfg-callevent-on", "イベント関数のCALLを許可する:YES\n", &[], erb);
    assert!(on.matches("one").count() == 2 && !on.contains("two"), "{on}");
}

/// SPキャラを使用する (off unless set): off, ADDSPCHARA, GETSPCHARA and an
/// SP argument to the CSV functions are Emuera's error; on, a character
/// whose CFLAG:0 isn't 0 is SP, found by GETSPCHARA, not by GETCHARA.
#[test]
fn sp_characters_follow_the_setting() {
    let files = [("CSV/Chara1.csv", "番号,1\n名前,Reimu\n")];
    let erb = "@SYSTEM_TITLE\nADDCHARA 1\nADDCHARA 1\nCFLAG:0:0 = 1\nPRINTFORML [{GETCHARA(1)}][{GETCHARA(1, 1)}][%CSVNAME(1)%]\nPRINTFORML [{GETSPCHARA(1)}]\nADDSPCHARA 1\nPRINTFORML [{CHARANUM}]\nQUIT\n";
    let off = play("cfg-sp-off", "", &files, erb);
    assert!(off.contains("[0][0][Reimu]") && off.contains("SPキャラ関係の機能は標準では使用できません"), "{off}");
    let on = play("cfg-sp-on", "SPキャラを使用する:YES\n", &files, erb);
    assert!(on.contains("[1][1][Reimu]") && on.contains("[0]") && on.contains("[3]"), "{on}");
}

/// VARSIZE(name[, dimension]): the dimension from the left, 0 when left
/// out; from 1 with VARSIZEの次元指定をERD機能に合わせる.
#[test]
fn varsize_follows_the_setting() {
    let erb = "@SYSTEM_TITLE\n#DIM HOGE, 1, 2, 3\nPRINTFORML [{VARSIZE(\"HOGE\")}][{VARSIZE(\"HOGE\", 1)}][{VARSIZE(\"HOGE\", 2)}]\nPRINTFORML [{VARSIZE(\"HOGE\", 3)}]\nQUIT\n";
    let zero = play("cfg-varsize-off", "", &[], erb);
    assert!(zero.contains("[1][2][3]") && zero.contains("存在しない次元の長さを取得しようとしました"), "{zero}");
    let erd = play("cfg-varsize-on", "VARSIZEの次元指定をERD機能に合わせる:YES\n", &[], erb);
    assert!(erd.contains("[1][1][2]") && erd.contains("[3]"), "{erd}");
}

/// ERD機能を利用する (on unless set): off, `RFLAG.csv` and `*.ERD` files give
/// no index names.
#[test]
fn erd_files_follow_the_setting() {
    let files = [("CSV/RFLAG.csv", "3,さん\n"), ("ERB/T.ERH", "#DIM SAVEDATA RFLAG, 10\n")];
    let erb = "@SYSTEM_TITLE\nRFLAG:さん = 5\nPRINTFORML [{RFLAG:3}]\nQUIT\n";
    assert!(play("cfg-erd-on", "", &files, erb).contains("[5]"));
    let off = play("cfg-erd-off", "ERD機能を利用する:NO\n", &files, erb);
    assert!(!off.contains("[5]"), "{off}");
}

/// 内部で使用する東アジア言語: STRLENS counts bytes in that code page, then
/// Shift_JIS (Emuera's LangManager.GetByteCountLang): Hangul is 2 bytes in
/// Korean's 949 and 1 (`?`) in Japanese's 932.
#[test]
fn byte_lengths_follow_the_language_setting() {
    let erb = "@SYSTEM_TITLE\nPRINTFORML [{STRLENS(\"한글\")}][{STRLENS(\"あa\")}]\nQUIT\n";
    assert!(play("cfg-lang-jp", "", &[], erb).contains("[2][3]"));
    let kr = play("cfg-lang-kr", "内部で使用する東アジア言語:KOREAN\n", &[], erb);
    assert!(kr.contains("[4][3]"), "{kr}");
}
