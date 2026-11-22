//! The game's Emuera settings: read from `CSV/_default.config`, the game's
//! `emuera.config` and `CSV/_fixed.config` (which wins), as Emuera does,
//! at start and when they change; and the model of the settings page that
//! edits them like Emuera's own settings window. Changes go where Emuera
//! saves them, the game's `emuera.config`, so Emuera.exe and emuera-tty
//! share them.

use crate::config_names::{canonical, NAMES};
use crate::console::Console;
use crate::vm::save::SaveOpts;
use std::path::{Path, PathBuf};

/// Apply the settings emuera-tty's console and saves use, line by line
/// in the order the files were read (later lines win; a value that doesn't
/// parse keeps the one before it). Returns the configured window width in
/// pixels, if any.
pub fn load(cfg: &Values, con: &mut Console, text_exts: &mut Vec<String>, saves: &mut SaveOpts) -> Option<f32> {
    let mut window_px = None;
    let yes = |v: &str| v.eq_ignore_ascii_case("YES") || v.eq_ignore_ascii_case("TRUE");
    let mut zip = false;
    let mut binary = false;
    {
        for (k, v) in cfg.lines() {
            let v = v.trim();
            let rgb = |v: &str| -> Option<crate::console::Rgb> {
                let n: Vec<u8> = v.split(',').filter_map(|x| x.trim().parse().ok()).collect();
                if n.len() == 3 {
                    Some(crate::console::Rgb(n[0], n[1], n[2]))
                } else {
                    None
                }
            };
            // (keys already Japanese: Values::read)
            match k.as_str() {
                "ウィンドウ幅" => window_px = v.parse().ok().or(window_px),
                "無限ループ警告までのミリ秒数" => con.infinite_loop_alert_time = v.parse().unwrap_or(5000),
                "無限ループ警告を無効にする" => con.disable_infinite_loop_detection = yes(v),
                "PRINTCを並べる数" => con.printc_per_line = v.parse().unwrap_or(con.printc_per_line),
                "フォント名" => {
                    con.default_font = v.to_string();
                    con.font_name = v.to_string();
                }
                "ボタンの途中で行を折りかえさない" => con.button_wrap = yes(v),
                "ver1739以前の非ボタン折り返しを再現する" => con.wrap_1739 = yes(v),
                "マウスを使用する" => con.use_mouse = !(v.eq_ignore_ascii_case("NO") || v.eq_ignore_ascii_case("FALSE")),
                "スクロール行数" => con.scroll_lines = v.parse::<usize>().ok().filter(|&n| n > 0).or(con.scroll_lines),
                "キーボードマクロを使用する" => con.key_macro = yes(v),
                "セーブデータをsavフォルダ内に作成する" => saves.in_folder = yes(v),
                "セーブデータをバイナリ形式で保存する" => binary = yes(v),
                "セーブデータを圧縮して保存する" => zip = yes(v),
                "オートセーブを行なう" => saves.autosave = yes(v),
                // Emuera keeps it within 20..80
                "表示するセーブデータ数" => saves.slots = v.parse::<i64>().map_or(saves.slots, |n| n.clamp(20, 80)),
                "LOADTEXTとSAVETEXTで使える拡張子" => {
                    for e in v.split(',').map(|e| e.trim().trim_start_matches('.').to_lowercase()).filter(|e| !e.is_empty()) {
                        if !text_exts.contains(&e) {
                            text_exts.push(e);
                        }
                    }
                }
                "フォントサイズ" => con.font_size = v.parse().unwrap_or(con.font_size),
                "一行の高さ" => con.line_height = v.parse().unwrap_or(con.line_height),
                "文字色" => {
                    if let Some(c) = rgb(v) {
                        con.default_fg = c;
                        con.style.fg = c;
                    }
                }
                "背景色" => {
                    if let Some(c) = rgb(v) {
                        con.default_bg = c;
                        con.screen_bg = c;
                    }
                }
                "選択中文字色" => {
                    if let Some(c) = rgb(v) {
                        con.focus_fg = c
                    }
                }
                "履歴文字色" => {
                    if let Some(c) = rgb(v) {
                        con.log_fg = c
                    }
                }
                "PRINTCを並べる文字数" | "PRINTCの文字数" => con.printc_len = v.parse().unwrap_or(con.printc_len),
                "表示するログの最大行数" | "履歴ログの行数" => con.max_lines = v.parse().unwrap_or(con.max_lines),
                // Emuera-EE's clipboard options
                other => {
                    con.clip.set(other, v);
                }
            }
        }
    }
    // Emuera compresses only binary saves; emuera-tty always writes binary
    // (Emuera reads it whatever its config says)
    saves.zip = zip && binary;

    // Environment variable override (takes precedence over config files)
    if let Ok(v) = std::env::var("EMUERA_INFINITE_LOOP_ALERT_TIME") {
        if let Ok(ms) = v.parse::<u64>() {
            con.infinite_loop_alert_time = ms;
        }
    }
    if let Ok(v) = std::env::var("EMUERA_DISABLE_INFINITE_LOOP_DETECTION") {
        con.disable_infinite_loop_detection = v == "1" || v.eq_ignore_ascii_case("YES") || v.eq_ignore_ascii_case("TRUE");
    }

    window_px
}

/// What kind of value a setting holds.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Bool,
    /// a number from `min` to `max`
    Int(i64, i64),
    /// `R,G,B`
    Color,
    Text,
    /// one of a list, as Emuera's drop-down boxes: (value written, label)
    Choice(&'static [(&'static str, &'static str)]),
}

/// When a change on the page reaches the running game.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Effect {
    /// at once (read again when the page closes)
    Now,
    /// read while the game loads: the next time it starts
    Restart,
    /// emuera-tty doesn't use it (a window, an editor, Rikaichan…); it is
    /// kept for Emuera.exe, and scripts can read it with GETCONFIG
    ExeOnly,
}

/// The file a setting is saved in.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Store {
    /// the game's `emuera.config`, under the setting's key
    Config,
    /// the `setting.json` beside Emuera.exe (Emuera-EM/EE's JSONConfig),
    /// as a true/false property of this name
    Json(&'static str),
}

/// One setting of Emuera's settings window.
pub struct Item {
    /// the key (Japanese, as Emuera names it)
    pub jp: &'static str,
    pub label: &'static str,
    /// the tab of Emuera's settings window it is on
    pub tab: &'static str,
    pub kind: Kind,
    /// Emuera's own default
    pub default: &'static str,
    pub effect: Effect,
    pub store: Store,
}

const fn item(tab: &'static str, jp: &'static str, label: &'static str, kind: Kind, default: &'static str) -> Item {
    Item { jp, label, tab, kind, default, effect: Effect::Now, store: Store::Config }
}

impl Item {
    const fn restart(self) -> Item {
        Item { effect: Effect::Restart, ..self }
    }
    const fn exe_only(self) -> Item {
        Item { effect: Effect::ExeOnly, ..self }
    }
    const fn json(self, property: &'static str) -> Item {
        Item { store: Store::Json(property), ..self }
    }
}

/// The tabs, in the order of Emuera's settings window.
pub const TABS: &[&str] =
    &[n!("Environment"), n!("Display"), n!("Window"), n!("Font"), n!("System"), n!("System2"), n!("Compatibility"), n!("Debug"), n!("Clipboard"), n!("Rikai")];

/// Emuera's 関数が見つからない / 呼び出されなかった警告の扱い choices.
const WARNING_HANDLING: &[(&str, &str)] =
    &[("IGNORE", n!("Ignore")), ("LATER", n!("Total count only")), ("ONCE", n!("Once per file")), ("DISPLAY", n!("Show"))];

/// Every setting of Emuera's settings window, tab by tab in the window's
/// order (ConfigDialog), plus emuera-tty's own 無限ループ警告を無効にする.
/// Those emuera-tty doesn't use are still here, so a player can set them
/// for Emuera.exe; the page says which ones (`Effect`). The window's
/// buttons that fill in several settings at once are left out.
pub const ITEMS: &[Item] = &[
    item("Environment", "マウスを使用する", n!("Use the mouse"), Kind::Bool, "YES"),
    item("Environment", "メニューを使用する", n!("Show the menu bar"), Kind::Bool, "YES").exe_only(),
    item("Environment", "デバッグコマンドを使用する", n!("Allow debug commands"), Kind::Bool, "NO"),
    item("Environment", "多重起動を許可する", n!("Allow several copies running at once"), Kind::Bool, "YES").exe_only(),
    item("Environment", "キーボードマクロを使用する", n!("Use keyboard macros (F1…F12; off: other Fn keys take over)"), Kind::Bool, "YES"),
    item("Environment", "オートセーブを行なう", n!("Make autosaves"), Kind::Bool, "YES"),
    item("Environment", "セーブデータをsavフォルダ内に作成する", n!("Keep saves in the sav folder"), Kind::Bool, "NO"),
    item("Environment", "CONFIGファイルの内容を英語で保存する", n!("Write this file's keys in English"), Kind::Bool, "NO"),
    item("Environment", "履歴ログの行数", n!("Lines in history log"), Kind::Int(100, 1000000), "5000"),
    item("Environment", "無限ループ警告までのミリ秒数", n!("Milliseconds for infinite loop warning"), Kind::Int(0, 60000), "5000"),
    item(
        "Environment",
        "無限ループ警告を無効にする",
        n!("Disable infinite loop detection (useful for games with intentional long-running functions)"),
        Kind::Bool,
        "NO",
    ),
    item("Environment", "表示するセーブデータ数", n!("Number of save slots"), Kind::Int(20, 80), "20"),
    item("Environment", "Ctrl-Zで元に戻す機能を有効にする", n!("Undo with Ctrl-Z"), Kind::Bool, "NO").exe_only(),
    item("Environment", "関連づけるテキストエディタ", n!("Text editor"), Kind::Text, "notepad").exe_only(),
    item(
        "Environment",
        "テキストエディタコマンドライン指定",
        n!("Text editor's command line"),
        Kind::Choice(&[("SAKURA", "Sakura"), ("TERAPAD", "TeraPad"), ("EMEDITOR", "EmEditor"), ("USER_SETTING", n!("Your own"))]),
        "USER_SETTING",
    )
    .exe_only(),
    item("Environment", "エディタに渡す行指定引数", n!("Text editor's line argument (your own)"), Kind::Text, "").exe_only(),
    item(
        "Display",
        "描画インターフェース",
        n!("Drawing interface"),
        Kind::Choice(&[("WINAPI", "WINAPI"), ("TEXTRENDERER", "TEXTRENDERER"), ("GRAPHICS", "GRAPHICS")]),
        "TEXTRENDERER",
    )
    .exe_only(),
    item("Display", "フレーム毎秒", n!("Frames per second"), Kind::Int(1, 240), "5").exe_only(),
    item("Display", "PRINTCを並べる数", n!("PRINTC items per line"), Kind::Int(1, 100), "3"),
    item("Display", "PRINTCの文字数", n!("PRINTC item width"), Kind::Int(1, 400), "25"),
    item("Display", "ボタンの途中で行を折りかえさない", n!("Don't break lines inside a button"), Kind::Bool, "NO"),
    // (the language of Emuera's own messages: emuera_lang.rs)
    item("Display", "Emueraの表示言語", n!("Emuera's interface language"), Kind::Text, "").restart(),
    item("Display", "UseButtonFocusBackgroundColor", n!("Highlight the background of the button under the pointer"), Kind::Bool, "NO")
        .exe_only()
        .json("UseButtonFocusBackgroundColor"),
    // the width is the picture-scaling reference, taken at start
    item("Window", "ウィンドウ幅", n!("Window width"), Kind::Int(128, 5000), "760").restart(),
    item("Window", "ウィンドウ高さ", n!("Window height"), Kind::Int(128, 5000), "480").exe_only(),
    item("Window", "ウィンドウの高さを可変にする", n!("Window height can change"), Kind::Bool, "YES").exe_only(),
    item("Window", "起動時にウィンドウを最大化する", n!("Maximise the window at start"), Kind::Bool, "NO").exe_only(),
    item("Window", "起動時のウィンドウ位置を指定する", n!("Fixed window position at start"), Kind::Bool, "NO").exe_only(),
    item("Window", "ウィンドウ位置X", n!("Window position X"), Kind::Int(0, 10000), "0").exe_only(),
    item("Window", "ウィンドウ位置Y", n!("Window position Y"), Kind::Int(0, 10000), "0").exe_only(),
    item("Window", "スクロール行数", n!("Lines per mouse-wheel step"), Kind::Int(1, 10), "1"),
    // the terminal's own background is kept; history lines aren't coloured
    item("Font", "背景色", n!("Background colour"), Kind::Color, "0,0,0").exe_only(),
    item("Font", "文字色", n!("Text colour"), Kind::Color, "192,192,192"),
    item("Font", "選択中文字色", n!("Selected button colour"), Kind::Color, "255,255,0"),
    item("Font", "履歴文字色", n!("History text colour"), Kind::Color, "192,192,192").exe_only(),
    item("Font", "フォント名", n!("Font (for text drawn into pictures)"), Kind::Text, "ＭＳ ゴシック"),
    item("Font", "フォントサイズ", n!("Font size (games place pictures by it)"), Kind::Int(8, 144), "18"),
    item("Font", "一行の高さ", n!("Line height (games place pictures by it)"), Kind::Int(8, 144), "19"),
    item("System", "大文字小文字の違いを無視する", n!("Ignore case"), Kind::Bool, "YES").restart(),
    item("System", "_Rename.csvを利用する", n!("Use _Rename.csv"), Kind::Bool, "NO").restart(),
    item("System", "_Replace.csvを利用する", n!("Use _Replace.csv"), Kind::Bool, "YES").restart(),
    item("System", "サブディレクトリを検索する", n!("Search subfolders"), Kind::Bool, "NO").restart(),
    item("System", "読み込み順をファイル名順にソートする", n!("Load files in file-name order"), Kind::Bool, "NO").restart(),
    item("System", "システム関数の上書きを許可する", n!("Allow overriding system functions"), Kind::Bool, "YES").restart(),
    item("System", "システム関数が上書きされたとき警告を表示する", n!("Warn when a system function is overridden"), Kind::Bool, "YES").restart(),
    item("System", "同名の非イベント関数が複数定義されたとき警告する", n!("Warn about non-event functions defined twice"), Kind::Bool, "NO").restart(),
    item("System", "全角スペースをホワイトスペースに含める", n!("Full-width space counts as white space"), Kind::Bool, "YES").restart(),
    item(
        "System",
        "内部で使用する東アジア言語",
        n!("East Asian language used inside"),
        Kind::Choice(&[
            ("JAPANESE", n!("Japanese")),
            ("KOREAN", n!("Korean")),
            ("CHINESE_HANS", n!("Simplified Chinese")),
            ("CHINESE_HANT", n!("Traditional Chinese")),
        ]),
        "JAPANESE",
    )
    .restart(),
    item("System2", "FORM中の三連記号を展開しない", n!("Don't expand triple symbols in FORM"), Kind::Bool, "NO").restart(),
    item("System2", "セーブデータをバイナリ形式で保存する", n!("Save in the binary format"), Kind::Bool, "NO"),
    item("System2", "セーブデータを圧縮して保存する", n!("Compress save data"), Kind::Bool, "NO"),
    item("System2", "キャラクタ変数の引数を補完しない", n!("Don't fill in character variables' argument"), Kind::Bool, "NO"),
    item("System2", "UPDATECHECKを許可しない", n!("Don't allow UPDATECHECK"), Kind::Bool, "NO").exe_only(),
    item("System2", "ERD機能を利用する", n!("Use ERD files"), Kind::Bool, "YES").restart(),
    item("System2", "VARSIZEの次元指定をERD機能に合わせる", n!("VARSIZE counts dimensions as ERD does"), Kind::Bool, "NO"),
    item("System2", "LOADTEXTとSAVETEXTで使える拡張子", n!("Extensions LOADTEXT and SAVETEXT may use"), Kind::Text, "txt"),
    item("System2", "行連結の改行コードの置換文字列", n!("What a line break in a { } join becomes"), Kind::Text, "\" \"").restart(),
    item("System2", "外部プラグインが有効時に警告を表示する", n!("Warn when plugins are enabled"), Kind::Bool, "YES").exe_only(),
    // emuera-tty always has them (VARI / VARS) and always uses its own RNG
    item("System2", "UseNewRandom", n!("Use the new random number generator"), Kind::Bool, "NO").exe_only().json("UseNewRandom"),
    item("System2", "UseScopedVariableInstruction", n!("Allow VARI / VARS (block-scoped variables)"), Kind::Bool, "NO")
        .exe_only()
        .json("UseScopedVariableInstruction"),
    item("Compatibility", "解釈不可能な行があっても実行する", n!("Run even with lines that can't be read"), Kind::Bool, "NO").exe_only(),
    item("Compatibility", "CALLNAMEが空文字列の時にNAMEを代入する", n!("Use NAME when CALLNAME is empty"), Kind::Bool, "NO").restart(),
    item("Compatibility", "擬似変数RANDの仕様をeramakerに合わせる", n!("RAND as in eramaker"), Kind::Bool, "NO"),
    item("Compatibility", "TIMESの計算をeramakerにあわせる", n!("TIMES as in eramaker"), Kind::Bool, "NO"),
    item("Compatibility", "関数・属性については大文字小文字を無視しない", n!("Don't ignore case in function names and attributes"), Kind::Bool, "NO").restart(),
    item("Compatibility", "イベント関数のCALLを許可する", n!("Allow CALL on event functions"), Kind::Bool, "NO"),
    item("Compatibility", "SPキャラを使用する", n!("Use SP characters"), Kind::Bool, "NO"),
    item("Compatibility", "ver1739以前の非ボタン折り返しを再現する", n!("Wrap text as before version 1739"), Kind::Bool, "NO"),
    item("Compatibility", "ユーザー関数の全ての引数の省略を許可する", n!("Allow leaving out any argument of a user function"), Kind::Bool, "NO"),
    item("Compatibility", "ユーザー関数の引数に自動的にTOSTRを補完する", n!("Add TOSTR to user function arguments by itself"), Kind::Bool, "NO"),
    item("Debug", "eramaker互換性に関する警告を表示する", n!("Show eramaker compatibility warnings"), Kind::Bool, "YES").exe_only(),
    item("Debug", "ロード時にレポートを表示する", n!("Show a report while loading"), Kind::Bool, "NO").restart(),
    item(
        "Debug",
        "ロード時に引数を解析する",
        n!("Analyse arguments while loading"),
        Kind::Choice(&[("NO", n!("Never")), ("ONCE", n!("When updated")), ("YES", n!("Always"))]),
        "NO",
    )
    .exe_only(),
    item(
        "Debug",
        "表示する最低警告レベル",
        n!("Lowest warning level shown"),
        Kind::Choice(&[
            ("0", n!("0: non-standard syntax")),
            ("1", n!("1: ignorable errors")),
            ("2", n!("2: errors that stop a line")),
            ("3", n!("3: fatal errors")),
        ]),
        "1",
    )
    .restart(),
    item("Debug", "呼び出されなかった関数を無視する", n!("Ignore functions never called"), Kind::Bool, "YES").exe_only(),
    item("Debug", "関数が見つからない警告の扱い", n!("Warnings for functions not found"), Kind::Choice(WARNING_HANDLING), "IGNORE").restart(),
    item("Debug", "関数が呼び出されなかった警告の扱い", n!("Warnings for functions never called"), Kind::Choice(WARNING_HANDLING), "IGNORE").restart(),
    item("Debug", "ERDで定義した識別子とローカル変数の重複を確認する", n!("Check ERD names against local variables"), Kind::Bool, "NO").restart(),
    item("Clipboard", "表示したテキストをクリップボードにコピーする", n!("Copy printed text to the clipboard"), Kind::Bool, "NO"),
    item("Clipboard", "新しい行のみコピーする", n!("Only new lines"), Kind::Bool, "YES"),
    item("Clipboard", "クリップボードに貼り付ける行数", n!("Lines copied (when not only new ones)"), Kind::Int(1, 10000), "25"),
    item("Clipboard", "総バッファサイズ", n!("Lines remembered"), Kind::Int(1, 100000), "300"),
    item("Clipboard", "スクロールの行数", n!("Lines scrolled per key"), Kind::Int(1, 100), "5").exe_only(),
    item("Clipboard", "クリップボードの更新間隔(ミリ秒)", n!("Minimum time between copies (ms)"), Kind::Int(0, 60000), "800"),
    item("Clipboard", "テキスト中の<>タグを無視する", n!("Replace pictures (<> tags) …"), Kind::Bool, "NO"),
    item("Clipboard", "<>を次の文で置き換える", n!("… with this text"), Kind::Text, "."),
    item("Clipboard", "画面のリフレッシュ時にクリップボードとバッファを消去する", n!("Forget lines when the game clears the screen"), Kind::Bool, "NO"),
    item("Clipboard", "左クリックをトリガーにする", n!("Copy on a left click"), Kind::Bool, "YES"),
    item("Clipboard", "ホイールクリックをトリガーにする", n!("Copy on a middle click"), Kind::Bool, "NO"),
    item("Clipboard", "ダブルクリックをトリガーにする", n!("Copy on a double click"), Kind::Bool, "NO"),
    item("Clipboard", "WAITをトリガーにする", n!("Copy at a WAIT"), Kind::Bool, "NO"),
    item("Clipboard", "INPUTをトリガーにする", n!("Copy at an input"), Kind::Bool, "YES"),
    item("Rikai", "Rikaichanを使用する", n!("Use Rikaichan (word look-up)"), Kind::Bool, "NO").exe_only(),
    item("Rikai", "Rikaichanのファイルパス", n!("Rikaichan's dictionary file"), Kind::Text, "Emuera-Rikai-edict.txt-eucjp").exe_only(),
    item("Rikai", "翻訳中の語句を強調表示する", n!("Highlight the word looked up"), Kind::Bool, "YES").exe_only(),
    item("Rikai", "ポップアップの背景色", n!("Pop-up background colour"), Kind::Color, "0,0,139").exe_only(),
    item("Rikai", "ポップアップの文字色", n!("Pop-up text colour"), Kind::Color, "255,255,255").exe_only(),
];

/// The label of a choice's value (the value itself when it isn't one).
pub fn choice_label(options: &[(&str, &'static str)], value: &str) -> String {
    match options.iter().find(|(v, _)| v.eq_ignore_ascii_case(value.trim())) {
        Some((_, label)) => crate::i18n::tr(label),
        None => value.to_string(),
    }
}

/// The value `step` places after `value` in a choice's list, going round
/// (a value not in the list starts from the first).
pub fn step_choice(options: &[(&'static str, &str)], value: &str, step: i64) -> String {
    let n = options.len() as i64;
    let at = options.iter().position(|(v, _)| v.eq_ignore_ascii_case(value.trim()));
    let next = match at {
        Some(i) => (i as i64 + step).rem_euclid(n),
        None => 0,
    };
    options[next as usize].0.to_string()
}

/// Where a setting's value comes from.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Source {
    /// Emuera's default
    Emuera,
    /// the game's CSV/_default.config
    Game,
    /// emuera.config (set by the player, here or in Emuera.exe)
    Player,
    /// CSV/_fixed.config: the game doesn't allow changing it
    Fixed,
}

/// The game's settings as the config files have them.
pub struct Values {
    /// key (Japanese) → (value, where from)
    vals: std::collections::HashMap<String, (String, Source)>,
    /// the game's emuera.config (it may not exist yet)
    pub path: PathBuf,
    /// write keys in English (the game's files do)
    english: bool,
    /// every (key, value) line read, in file order, keys Japanese: for
    /// readers that apply each line as it comes, later lines winning
    lines: Vec<(String, String)>,
    /// the `setting.json` the `Store::Json` settings are in (it may not
    /// exist yet), and its text
    json_path: PathBuf,
    json: String,
}

/// Where Emuera.exe keeps `setting.json`: beside itself, the game's folder
/// or the one above (`loader::net_settings` reads the same file).
fn setting_json_path(game: &Path) -> PathBuf {
    let beside_exe = std::iter::once(game).chain(game.parent()).map(|d| d.join("setting.json"));
    beside_exe.clone().find(|p| p.is_file()).unwrap_or_else(|| game.join("setting.json"))
}

/// `text` (a JSON object) with the true/false property `name` set to
/// `on`, the rest of the text kept: an existing value is replaced, a new
/// property goes first in the object. Errors when `text` holds something
/// but no object, so a file that isn't Emuera's is never overwritten.
fn json_with_bool(text: &str, name: &str, on: bool) -> Result<String, String> {
    let quoted = format!("\"{}\"", name);
    let word = if on { "true" } else { "false" };
    let mut from = 0;
    while let Some(i) = text[from..].find(&quoted) {
        let after_name = from + i + quoted.len();
        let rest = &text[after_name..];
        if let Some(value) = rest.trim_start().strip_prefix(':') {
            let start = text.len() - value.trim_start().len();
            for old in ["true", "false"] {
                if text[start..].starts_with(old) {
                    return Ok(format!("{}{}{}", &text[..start], word, &text[start + old.len()..]));
                }
            }
        }
        from = after_name;
    }
    match text.find('{') {
        Some(i) => {
            let empty = text[i + 1..].trim_start().starts_with('}');
            Ok(format!("{}{}:{}{}{}", &text[..=i], quoted, word, if empty { "" } else { "," }, &text[i + 1..]))
        }
        None if text.trim_start_matches('\u{feff}').trim().is_empty() => Ok(format!("{{{}:{}}}", quoted, word)),
        None => Err(t!("{file} isn't a JSON object; left as it is", file = "setting.json")),
    }
}

/// `key:value` lines of a config file (BOM, blank and `;` lines skipped).
fn pairs(text: &str) -> impl Iterator<Item = (&str, &str)> {
    text.lines().filter_map(|l| {
        let l = l.trim_start_matches('\u{feff}');
        if l.trim_start().starts_with(';') {
            return None;
        }
        l.split_once(':').map(|(k, v)| (k.trim(), v.trim()))
    })
}

/// Does this key name an English one (`WINDOW WIDTH`)?
fn is_english(key: &str) -> bool {
    NAMES.iter().any(|(_, en)| key.eq_ignore_ascii_case(en))
}

impl Values {
    /// No config files: Emuera's defaults throughout (tests).
    #[cfg(test)]
    pub fn emuera() -> Values {
        Values { vals: Default::default(), path: PathBuf::new(), english: false, lines: Vec::new(), json_path: PathBuf::new(), json: String::new() }
    }

    pub fn read(game: &Path) -> Values {
        let csv = crate::loader::find_dir_ci(game, "CSV");
        let path = crate::loader::find_file_ci(game, "emuera.config").unwrap_or_else(|| game.join("emuera.config"));
        let json_path = setting_json_path(game);
        let json = std::fs::read(&json_path).map(|b| String::from_utf8_lossy(&b).into_owned()).unwrap_or_default();
        let mut v = Values { vals: Default::default(), path: path.clone(), english: false, lines: Vec::new(), json_path, json };
        // Emuera's order, the last file winning (Config.cs); EE also reads
        // config/emuera.config
        let files = [
            (csv.as_deref().and_then(|c| crate::loader::find_file_ci(c, "_default.config")), Source::Game),
            (Some(path), Source::Player),
            (crate::loader::find_dir_ci(game, "config").and_then(|d| crate::loader::find_file_ci(&d, "emuera.config")), Source::Player),
            (csv.as_deref().and_then(|c| crate::loader::find_file_ci(c, "_fixed.config")), Source::Fixed),
        ];
        let mut seen_keys = (0, 0);
        for (p, src) in files {
            let Some(text) = p.and_then(|p| crate::loader::read_text(&p).ok()) else { continue };
            for (k, val) in pairs(&text) {
                if src != Source::Fixed {
                    if is_english(k) {
                        seen_keys.1 += 1;
                    } else {
                        seen_keys.0 += 1;
                    }
                }
                v.vals.insert(canonical(k).to_string(), (val.to_string(), src));
                v.lines.push((canonical(k).to_string(), val.to_string()));
            }
        }
        // write the way the game's files are written (Emuera reads both),
        // or as CONFIGファイルの内容を英語で保存する says if a file sets it
        v.english = match v.vals.get("CONFIGファイルの内容を英語で保存する") {
            Some((x, _)) => x.trim().eq_ignore_ascii_case("YES") || x.trim().eq_ignore_ascii_case("TRUE"),
            None => seen_keys.1 > seen_keys.0,
        };
        v
    }

    /// The value of `jp` (its Japanese key) in effect: the config files',
    /// else Emuera's default (`config_names::DEFAULTS`).
    pub fn value(&self, jp: &str) -> &str {
        self.vals.get(jp).map_or_else(|| crate::config_names::default(jp), |(v, _)| v.as_str())
    }

    /// Is the yes/no setting `jp` on, Emuera's default when unset? A value
    /// that isn't yes or no counts as the default, as Emuera's
    /// `ConfigItem.TryParse` failing leaves it.
    pub fn bool(&self, jp: &str) -> bool {
        let yes = |x: &str| match x.trim().to_uppercase().as_str() {
            "YES" | "TRUE" => Some(true),
            "NO" | "FALSE" => Some(false),
            _ => None,
        };
        self.vals.get(jp).and_then(|(x, _)| yes(x)).or_else(|| yes(crate::config_names::default(jp))).unwrap_or(false)
    }

    /// Every (key, value) line of the files, in the order read.
    pub fn lines(&self) -> &[(String, String)] {
        &self.lines
    }

    /// The value in effect, and where it comes from.
    pub fn get(&self, it: &Item) -> (String, Source) {
        if let Store::Json(name) = it.store {
            // Emuera writes every property on its first start: a value
            // only counts as changed when it isn't the default
            let on = crate::loader::json_bool(&self.json, name);
            let value = if on.unwrap_or(false) { "YES" } else { "NO" };
            return (value.to_string(), if value == it.default { Source::Emuera } else { Source::Player });
        }
        match self.vals.get(it.jp) {
            Some((v, s)) => (v.clone(), *s),
            None => (it.default.to_string(), Source::Emuera),
        }
    }

    /// What a running game applies on reading the files again, as [`load`]
    /// applies only the lines there are: these lines, after the default of
    /// each setting on the page that `before` had and these don't, so that
    /// one taken out of emuera.config goes back rather than staying as it was.
    pub fn after(&self, before: &Values) -> Values {
        let mut lines: Vec<(String, String)> = ITEMS
            .iter()
            .filter(|it| before.vals.contains_key(it.jp) && !self.vals.contains_key(it.jp))
            .map(|it| (it.jp.to_string(), it.default.to_string()))
            .collect();
        lines.extend(self.lines.iter().cloned());
        Values { vals: self.vals.clone(), path: self.path.clone(), english: self.english, lines, json_path: self.json_path.clone(), json: self.json.clone() }
    }

    /// Set a value in emuera.config (None: take it out, back to the
    /// game's default). Other lines of the file stay as they are.
    pub fn set(&mut self, it: &Item, value: Option<&str>) -> Result<(), String> {
        if let Store::Json(name) = it.store {
            return self.set_json(name, value.unwrap_or(it.default));
        }
        let raw = std::fs::read(&self.path).unwrap_or_default();
        let sjis = !raw.is_empty() && crate::loader::is_sjis(&raw);
        let text = crate::loader::decode(&raw);
        let mut lines: Vec<String> = text.lines().map(|l| l.trim_start_matches('\u{feff}').to_string()).collect();
        let at = lines.iter().position(|l| l.split_once(':').is_some_and(|(k, _)| canonical(k) == it.jp));
        match (at, value) {
            (Some(i), Some(v)) => {
                let key = lines[i].split_once(':').map(|(k, _)| k.to_string()).unwrap_or_default();
                lines[i] = format!("{}:{}", key, v);
            }
            (Some(i), None) => {
                lines.remove(i);
            }
            (None, Some(v)) => {
                let key = if self.english { NAMES.iter().find(|(jp, _)| *jp == it.jp).map_or(it.jp, |(_, en)| en).to_uppercase() } else { it.jp.to_string() };
                lines.push(format!("{}:{}", key, v));
            }
            (None, None) => {}
        }
        let mut out = lines.join("\r\n");
        if !out.is_empty() {
            out.push_str("\r\n");
        }
        // Emuera writes UTF-8 with a BOM; an old Shift_JIS file stays so
        let bytes = if sjis { encoding_rs::SHIFT_JIS.encode(&out).0.into_owned() } else { [&[0xEF, 0xBB, 0xBF][..], out.as_bytes()].concat() };
        std::fs::write(&self.path, bytes).map_err(|e| format!("{}: {}", self.path.display(), e))?;
        // what is in effect now
        let csv = self.path.parent().map(Values::read);
        if let Some(n) = csv {
            self.vals = n.vals;
        }
        Ok(())
    }

    /// Set a `setting.json` property ("YES"/"NO") the way Emuera's window
    /// saves it, the file's other properties kept.
    fn set_json(&mut self, name: &str, value: &str) -> Result<(), String> {
        let on = value.eq_ignore_ascii_case("YES") || value.eq_ignore_ascii_case("TRUE");
        let text = json_with_bool(&self.json, name, on)?;
        std::fs::write(&self.json_path, &text).map_err(|e| format!("{}: {}", self.json_path.display(), e))?;
        self.json = text;
        Ok(())
    }
}

/// A typed value made into what the config file holds, or why it can't be.
pub fn normalize(it: &Item, s: &str) -> Result<String, String> {
    let s = s.trim();
    match it.kind {
        Kind::Bool => match s.to_uppercase().as_str() {
            "YES" | "Y" | "TRUE" | "ON" | "1" => Ok("YES".into()),
            "NO" | "N" | "FALSE" | "OFF" | "0" => Ok("NO".into()),
            _ => Err(t!("YES or NO")),
        },
        Kind::Int(lo, hi) => match s.parse::<i64>() {
            Ok(n) if (lo..=hi).contains(&n) => Ok(n.to_string()),
            _ => Err(t!("a number from {lo} to {hi}", lo = lo, hi = hi)),
        },
        Kind::Color => {
            // R,G,B or #RRGGBB
            let n: Vec<i64> = if let Some(h) = s.strip_prefix('#').filter(|h| h.len() == 6) {
                (0..3).filter_map(|i| i64::from_str_radix(&h[i * 2..i * 2 + 2], 16).ok()).collect()
            } else {
                s.split(',').filter_map(|x| x.trim().parse().ok()).collect()
            };
            match n[..] {
                [r, g, b] if [r, g, b].iter().all(|c| (0..=255).contains(c)) => Ok(format!("{},{},{}", r, g, b)),
                _ => Err(t!("R,G,B (0-255 each) or #RRGGBB")),
            }
        }
        Kind::Text => Ok(s.to_string()),
        Kind::Choice(options) => match options.iter().find(|(v, _)| v.eq_ignore_ascii_case(s)) {
            Some((v, _)) => Ok(v.to_string()),
            None => Err(options.iter().map(|(v, _)| *v).collect::<Vec<_>>().join(", ")),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_setting_taken_out_goes_back_on_reading_again() {
        let d = std::env::temp_dir().join(format!("emuera-cfg-out-{}", std::process::id()));
        std::fs::create_dir_all(d.join("CSV")).unwrap();
        std::fs::write(d.join("CSV/_default.config"), "PRINTCを並べる数:5\n").unwrap();
        std::fs::write(d.join("emuera.config"), "表示するセーブデータ数:30\r\nPRINTCを並べる数:4\r\n").unwrap();
        let before = Values::read(&d);
        std::fs::write(d.join("emuera.config"), "").unwrap();
        let after = Values::read(&d).after(&before);
        // Emuera's default for the save slots, and the game's own PRINTC, not the player's old values
        assert_eq!(after.lines(), [("表示するセーブデータ数".to_string(), "20".to_string()), ("PRINTCを並べる数".to_string(), "5".to_string())]);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn edit_keeps_the_file() {
        let d = std::env::temp_dir().join(format!("emuera-cfg-{}", std::process::id()));
        std::fs::create_dir_all(d.join("CSV")).unwrap();
        std::fs::write(d.join("CSV/_default.config"), "ITEMS PER LINE FOR PRINTC:5\nFONT SIZE:16\nSAVE DATA COUNT PER PAGE:200\n").unwrap();
        std::fs::write(d.join("CSV/_fixed.config"), "NUMBER OF ITEM CHARACTERS FOR PRINTC:16\n").unwrap();
        let it = |jp: &str| ITEMS.iter().find(|i| i.jp == jp).unwrap();
        let mut v = Values::read(&d);
        assert_eq!(v.get(it("PRINTCを並べる数")), ("5".to_string(), Source::Game));
        assert_eq!(v.get(it("PRINTCの文字数")).1, Source::Fixed);
        assert_eq!(v.get(it("文字色")), ("192,192,192".to_string(), Source::Emuera));
        // the game writes English keys: so does a new emuera.config
        v.set(it("PRINTCを並べる数"), Some("4")).unwrap();
        v.set(it("文字色"), Some(&normalize(it("文字色"), "#102030").unwrap())).unwrap();
        let raw = std::fs::read(&v.path).unwrap();
        assert!(raw.starts_with(&[0xEF, 0xBB, 0xBF]));
        let text = String::from_utf8_lossy(&raw[3..]).to_string();
        assert_eq!(text, "ITEMS PER LINE FOR PRINTC:4\r\nTEXT COLOR:16,32,48\r\n");
        assert_eq!(v.get(it("PRINTCを並べる数")), ("4".to_string(), Source::Player));
        // back to the game's value
        v.set(it("PRINTCを並べる数"), None).unwrap();
        assert_eq!(v.get(it("PRINTCを並べる数")), ("5".to_string(), Source::Game));
        // and what the game loads
        let mut con = Console::new();
        let (mut ext, mut so) = (Vec::new(), SaveOpts::default());
        load(&Values::read(&d), &mut con, &mut ext, &mut so);
        assert_eq!((con.default_fg.0, con.default_fg.1, con.default_fg.2), (16, 32, 48));
        assert_eq!(so.slots, 80, "clamped as Emuera does");
        assert!(normalize(it("表示するセーブデータ数"), "100").is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    /// Every emuera.config setting on the page is a key Emuera knows (so
    /// it can be written in English), with Emuera's default
    #[test]
    fn page_settings_are_emueras() {
        for it in ITEMS.iter().filter(|it| it.store == Store::Config && it.jp != "無限ループ警告を無効にする") {
            assert!(NAMES.iter().any(|(jp, _)| *jp == it.jp), "{} has no English name", it.jp);
            let theirs = crate::config_names::default(it.jp);
            if !theirs.is_empty() || it.default.is_empty() {
                assert!(theirs.eq_ignore_ascii_case(it.default), "{}: page says {:?}, Emuera {:?}", it.jp, it.default, theirs);
            }
            if let Kind::Choice(options) = it.kind {
                assert!(options.iter().any(|(v, _)| *v == it.default), "{}: default not a choice", it.jp);
            }
        }
        assert!(TABS.iter().all(|t| ITEMS.iter().any(|it| it.tab == *t)), "an empty tab");
        assert!(ITEMS.iter().all(|it| TABS.contains(&it.tab)), "a setting on no tab");
    }

    #[test]
    fn choices_go_round() {
        let opts: &[(&str, &str)] = &[("IGNORE", ""), ("LATER", ""), ("ONCE", ""), ("DISPLAY", "")];
        assert_eq!(step_choice(opts, "ignore", 1), "LATER");
        assert_eq!(step_choice(opts, "DISPLAY", 1), "IGNORE");
        assert_eq!(step_choice(opts, "IGNORE", -1), "DISPLAY");
        // a value Emuera wouldn't take: back to the first
        assert_eq!(step_choice(opts, "LOUD", 1), "IGNORE");
        let level = ITEMS.iter().find(|i| i.jp == "表示する最低警告レベル").unwrap();
        assert_eq!(normalize(level, " 0 ").unwrap(), "0");
        assert!(normalize(level, "4").is_err());
    }

    /// setting.json keeps what Emuera.exe (or Emuera.NET) wrote around
    /// the property changed
    #[test]
    fn setting_json_edits() {
        let em = "{\"UseButtonFocusBackgroundColor\":false,\"UseNewRandom\":false,\"UseScopedVariableInstruction\":false}";
        assert_eq!(
            json_with_bool(em, "UseNewRandom", true).unwrap(),
            "{\"UseButtonFocusBackgroundColor\":false,\"UseNewRandom\":true,\"UseScopedVariableInstruction\":false}"
        );
        let net = "\u{feff}{\n  \"UseRenameInCharaCSV\" : true\n}";
        assert_eq!(json_with_bool(net, "UseNewRandom", true).unwrap(), "\u{feff}{\"UseNewRandom\":true,\n  \"UseRenameInCharaCSV\" : true\n}");
        assert_eq!(json_with_bool("{ }", "UseNewRandom", false).unwrap(), "{\"UseNewRandom\":false }");
        assert_eq!(json_with_bool("", "UseNewRandom", true).unwrap(), "{\"UseNewRandom\":true}");
        // a name inside a value isn't the property
        assert_eq!(json_with_bool("{\"a\":\"\\\"UseNewRandom\\\"\"}", "UseNewRandom", true).unwrap(), "{\"UseNewRandom\":true,\"a\":\"\\\"UseNewRandom\\\"\"}");
        assert!(json_with_bool("not json", "UseNewRandom", true).is_err());

        let d = std::env::temp_dir().join(format!("emuera-cfg-json-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let it = ITEMS.iter().find(|i| i.store == Store::Json("UseNewRandom")).unwrap();
        let mut v = Values::read(&d);
        assert_eq!(v.get(it), ("NO".to_string(), Source::Emuera));
        v.set(it, Some("YES")).unwrap();
        assert_eq!(std::fs::read_to_string(d.join("setting.json")).unwrap(), "{\"UseNewRandom\":true}");
        assert_eq!(Values::read(&d).get(it), ("YES".to_string(), Source::Player));
        // "back to the game's": the default, false
        v.set(it, None).unwrap();
        assert_eq!(Values::read(&d).get(it), ("NO".to_string(), Source::Emuera));
        assert!(!d.join("emuera.config").exists(), "emuera.config untouched");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn values_in_effect() {
        let d = std::env::temp_dir().join(format!("emuera-cfg-vals-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("CSV")).unwrap();
        std::fs::write(
            d.join("CSV/_default.config"),
            "\u{feff}; a comment:YES\nWHITESPACE INCLUDES FULL-WIDTH SPACE:NO\nVALID EXTENSIONS FOR LOADTEXT AND SAVETEXT:xml\nスクロール行数:3\n",
        )
        .unwrap();
        std::fs::write(d.join("emuera.config"), "全角スペースをホワイトスペースに含める:YES\nスクロール行数:many\n_Rename.csvを利用する:maybe\n").unwrap();
        std::fs::write(d.join("CSV/_fixed.config"), "Whitespace includes full-width space:no\n").unwrap();
        let v = Values::read(&d);
        // the fixed file wins, English keys any case
        assert!(!v.bool("全角スペースをホワイトスペースに含める"));
        // unset: Emuera's default
        assert!(v.bool("大文字小文字の違いを無視する"));
        assert_eq!(v.value("PRINTCを並べる数"), "3");
        assert_eq!(v.value("フォント名"), "ＭＳ ゴシック");
        // a yes/no that doesn't parse: Emuera's default, not an earlier file's
        assert!(!v.bool("_Rename.csvを利用する"));
        // the English extension key reaches LOADTEXT/SAVETEXT
        let (mut con, mut ext, mut so) = (Console::new(), Vec::new(), SaveOpts::default());
        load(&v, &mut con, &mut ext, &mut so);
        assert_eq!(ext, ["xml"]);
        // CONFIGファイルの内容を英語で保存する decides the keys written
        std::fs::write(d.join("CSV/_fixed.config"), "CONFIGファイルの内容を英語で保存する:NO\n").unwrap();
        let mut v = Values::read(&d);
        let it = ITEMS.iter().find(|i| i.jp == "PRINTCを並べる数").unwrap();
        v.set(it, Some("4")).unwrap();
        assert!(String::from_utf8_lossy(&std::fs::read(d.join("emuera.config")).unwrap()).contains("PRINTCを並べる数:4"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
