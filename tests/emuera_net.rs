//! Emuera.NET's additions that games written for it use (eraMegatenP):
//! not in Emuera-EM/EE, so not in the ERB suite, which also runs there.
mod common;
use common::{game, run, temp_dir};

fn play(tag: &str, files: &[(&str, &str)], erb: &str) -> String {
    let dir = temp_dir(tag);
    let mut all = vec![("ERB/T.ERB", erb)];
    all.extend_from_slice(files);
    game(&dir, &all);
    let out = run(&dir, b"", None);
    let _ = std::fs::remove_dir_all(&dir);
    out
}

/// GETCSVNOBY{NAME,CALLNAME,NICKNAME,MASTERNAME}: the lowest CSV number
/// with exactly that name, -1 if none; a name the CSV doesn't set matches
/// nothing.
#[test]
fn csv_numbers_by_name() {
    let files =
        [("CSV/Chara5.csv", "番号,5\n名前,Reimu\nあだ名,Miko\n"), ("CSV/Chara2.csv", "番号,2\n名前,Reimu\n"), ("CSV/Chara7.csv", "番号,7\n名前,Marisa\n")];
    let erb = "@SYSTEM_TITLE\nPRINTFORML [{GETCSVNOBYNAME(\"Reimu\")}][{GETCSVNOBYNICKNAME(\"Miko\")}][{GETCSVNOBYNAME(\"Sanae\")}][{GETCSVNOBYNICKNAME(\"\")}]\nQUIT\n";
    let out = play("net-csvno", &files, erb);
    assert!(out.contains("[2][5][-1][-1]"), "{out}");
}

/// HASH_XXH32: XXH32 of the UTF-8 bytes.
#[test]
fn hash_xxh32() {
    let out = play("net-xxh32", &[], "@SYSTEM_TITLE\nPRINTFORML [{HASH_XXH32(\"abc\")}]\nQUIT\n");
    assert!(out.contains(&format!("[{}]", 0x32D153FFu32)), "{out}");
}

/// MATCHALL var, value[, start[, end]]: the matching indices in RESULT:1…,
/// their number in RESULT; a character variable takes its element first.
#[test]
fn matchall() {
    let erb = "@SYSTEM_TITLE\n#DIM ARR, 6\nARR = 3, 1, 3, 4, 3, 0\nMATCHALL ARR, 3\nPRINTFORML [{RESULT}:{RESULT:1},{RESULT:2},{RESULT:3}]\n\
        MATCHALL ARR, 3, 1, 4\nPRINTFORML [{RESULT}:{RESULT:1}]\n\
        ADDVOIDCHARA\nADDVOIDCHARA\nADDVOIDCHARA\nCFLAG:1:4 = 9\nCFLAG:2:4 = 9\nMATCHALL CFLAG, 4, 9\nPRINTFORML [{RESULT}:{RESULT:1},{RESULT:2}]\n\
        MATCHALL ARR, 3, 0, 9\nQUIT\n";
    let out = play("net-matchall", &[], erb);
    for want in ["[3:0,2,4]", "[1:2]", "[2:1,2]", "検索範囲が変数のサイズを超えています"] {
        assert!(out.contains(want), "{want:?} missing:\n{out}");
    }
}

/// A font in the game's font/ folder: GETFONT reports it while the
/// frontend can show its symbols as pictures; headless (no pictures) it
/// reports Emuera's default, so a game with a text fallback uses it.
/// Uses eraMegatenP's EraMTFont when that game is on this machine.
#[test]
fn game_folder_font_hidden_without_pictures() {
    let font = &std::path::Path::new(&std::env::var("EMUERA_TEST_GAMES").unwrap_or_default()).join("eraMegatenP/Data/font/EraMTFontMedium.ttf");
    let Ok(ttf) = std::fs::read(font) else { return };
    let dir = temp_dir("net-gamefont");
    game(
        &dir,
        &[
            (
                "ERB/T.ERB",
                "@SYSTEM_TITLE\nPRINTFORML [%GETFONT()%][%GETCONFIGS(\"フォント名\")%]\nSETFONT \"Some Other Font\"\nPRINTFORML [%GETFONT()%]\nQUIT\n",
            ),
            ("emuera.config", "フォント名:EraMTFont\n"),
        ],
    );
    std::fs::create_dir_all(dir.join("font")).unwrap();
    std::fs::write(dir.join("font/EraMTFontMedium.ttf"), ttf).unwrap();
    let out = run(&dir, b"", None);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(out.contains("[ＭＳ ゴシック][ＭＳ ゴシック]") && out.contains("[Some Other Font]"), "{out}");
}
