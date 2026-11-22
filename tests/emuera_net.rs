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
    let Ok(ttf) = std::fs::read(font) else {
        eprintln!("skipped: {} not here", font.display());
        return;
    };
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

/// HTML_PRINT_ISLAND: drawn over the window, not printed in the log; its
/// buttons answer an input; HTML_PRINT_ISLAND_CLEAR with a depth clears
/// that layer only, without one every layer.
#[test]
fn html_island_layers() {
    let erb = "@SYSTEM_TITLE\nHTML_PRINT_ISLAND \"<button value='7'>island-seven</button>\"\n\
        HTML_PRINT_ISLAND \"<button value='8'>island-eight</button>\", 2\n\
        HTML_PRINT_ISLAND_CLEAR 2\nBINPUT\nPRINTFORML [got {RESULT}]\n\
        HTML_PRINT_ISLAND_CLEAR\nBINPUT\nQUIT\n";
    let dir = temp_dir("net-island");
    game(&dir, &[("ERB/T.ERB", erb)]);
    let out = run(&dir, b"8\n7\n", None);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(out.contains("[got 7]"), "{out}");
    // (the island texts, not "island-": the temp folder's name, in the
    // crash log's path, has that too)
    assert!(!out.contains("island-seven") && !out.contains("island-eight"), "the island went into the log:\n{out}");
    assert!(out.contains("no buttons to choose"), "{out}");
}

/// setting.json's UseRenameInCharaCSV: `[[name]]` in a Chara*.csv takes
/// its _Rename.csv value (eraMegatenP's skills and clothes); without the
/// setting the CSV is read as written.
#[test]
fn rename_in_chara_csv() {
    let files = |on: bool| -> Vec<(&'static str, &'static str)> {
        let mut f = vec![
            ("emuera.config", "_Rename.csvを利用する:YES\n"),
            ("CSV/_Rename.csv", "77,[[スキル:気合]]\n"),
            ("CSV/CFLAG.csv", "3,技\n"),
            ("CSV/Chara1.csv", "番号,1\n名前,A\nフラグ,技,[[スキル:気合]]\n"),
            ("ERB/T.ERB", "@SYSTEM_TITLE\nADDCHARA 1\nPRINTFORML [cflag {CFLAG:0:3}]\nQUIT\n"),
        ];
        if on {
            f.push(("setting.json", "\u{feff}{\n  \"UseRenameInCharaCSV\": true\n}"));
        }
        f
    };
    for (on, want) in [(true, "[cflag 77]"), (false, "[cflag 0]")] {
        let dir = temp_dir(if on { "net-rename-on" } else { "net-rename-off" });
        game(&dir, &files(on));
        let out = run(&dir, b"", None);
        let _ = std::fs::remove_dir_all(&dir);
        assert!(out.contains(want), "setting {on}: {want:?} missing:\n{out}");
    }
}

/// setting.json's CheckUTF8withBOM: a level-0 load warning for a script
/// file without a BOM, listed by --check when level 0 is shown.
#[test]
fn check_utf8_with_bom() {
    let dir = temp_dir("net-bom");
    game(
        &dir,
        &[
            ("emuera.config", "表示する最低警告レベル:0\n"),
            ("setting.json", "{\"CheckUTF8withBOM\":true}"),
            ("ERB/PLAIN.ERB", "@SYSTEM_TITLE\nQUIT\n"),
            ("ERB/BOM.ERB", "\u{feff}@OTHER\nRETURN\n"),
        ],
    );
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_emuera-tty")).arg(&dir).arg("--check").output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let text = String::from_utf8_lossy(&out.stdout).into_owned() + &String::from_utf8_lossy(&out.stderr);
    assert!(text.contains("PLAIN.ERB") && text.contains("UTF-8 with BOM"), "{text}");
    assert!(!text.contains("BOM.ERB:0"), "{text}");
}

/// With the game's font hidden (no pictures), setting back the name
/// GETFONT gave keeps the game's font: eraMegatenP's options screen does
/// `GETFONT`, `SETFONT "EraMTFont"`, `SETFONT RESULTS`, and the map's
/// symbols then lost their Braille drawing until a restart. Run in the TUI
/// (`--gfx none`) inside a private tmux server; skipped without tmux or
/// the font.
#[test]
fn setting_back_the_reported_font_keeps_the_game_font() {
    let font = &std::path::Path::new(&std::env::var("EMUERA_TEST_GAMES").unwrap_or_default()).join("eraMegatenP/Data/font/EraMTFontMedium.ttf");
    let Ok(ttf) = std::fs::read(font) else {
        eprintln!("skipped: {} not here", font.display());
        return;
    };
    if std::process::Command::new("tmux").arg("-V").output().map_or(true, |o| !o.status.success()) {
        eprintln!("skipped: no tmux");
        return;
    }
    let dir = temp_dir("net-setfont-back");
    game(
        &dir,
        &[
            // (U+E193: one of EraMTFont's wall tiles)
            ("ERB/T.ERB", "@SYSTEM_TITLE\nLOCALS = %GETFONT()%\nSETFONT \"EraMTFont\"\nSETFONT LOCALS\nPRINTFORML <%UNICODE(0xE193)%>\nWAIT\nQUIT\n"),
            ("emuera.config", "フォント名:EraMTFont\n"),
        ],
    );
    std::fs::create_dir_all(dir.join("font")).unwrap();
    std::fs::write(dir.join("font/EraMTFontMedium.ttf"), ttf).unwrap();
    let socket = format!("emuera-tty-test-{}", std::process::id());
    let tmux = |args: &[&str]| std::process::Command::new("tmux").args(["-L", &socket, "-f", "/dev/null"]).args(args).output().unwrap();
    let cmd = format!("{} {} --gfx none", env!("CARGO_BIN_EXE_emuera-tty"), dir.display());
    tmux(&["new-session", "-d", "-x", "100", "-y", "20", &cmd]);
    let mut screen = String::new();
    for _ in 0..50 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        screen = String::from_utf8_lossy(&tmux(&["capture-pane", "-p"]).stdout).into_owned();
        if screen.contains('<') && screen.contains('>') {
            break;
        }
    }
    tmux(&["kill-server"]);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(screen.contains("<⠶⠶>"), "the wall tile isn't drawn in Braille:\n{screen}");
}
