//! Full-screen terminal frontend: raw-mode input with mouse support, a
//! redraw-per-frame renderer for the Emuera console, and inline images via
//! the kitty / sixel / framebuffer backends.
//!
//! Each frame is rebuilt from the [`Console`]: lines are laid out bottom-up
//! into screen rows (`layout_line`, which wraps and aligns), the visible
//! window is chosen from the scroll offset, and the whole screen is written
//! in one go. While drawing, the positions of buttons are recorded
//! (`btn_map`) so mouse clicks and hover can be mapped back to values.
//!
//! Input is read raw from stdin and decoded into [`Key`]s: UTF-8 text,
//! cursor/paging keys, and mouse reports in SGR, urxvt or X10 encoding.
//! On the Linux console the mouse comes from `/dev/input/mice` instead
//! (`crate::mice`). Colours are downgraded to 256 or 16 when the terminal
//! can't do 24-bit.

use crate::ast::Align;
use crate::console::{Console, Line, Rgb, Seg, Style};
use crate::frontend::{Frontend, InputKind, InputReq, InputResult};
use crate::gfx::{self, Backend, Placement};
use crate::resources::Resources;
use crate::vm::value::Value;
use std::io::{self, Write};
use std::time::{Duration, Instant};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

mod envvars;
mod gamecfg;
mod lineedit;
mod saves;

/// How many rows above / below the view are searched for images that
/// reach into it (portraits are about 15 rows tall).
const IMG_REACH: usize = 40;

/// How many colours the terminal can show (picked from TERM / COLORTERM).
#[derive(Clone, Copy, PartialEq)]
enum ColorMode {
    True,
    C256,
    C16,
}

/// One screen cell after layout; `w` is 2 for wide (CJK) characters.
#[derive(Clone)]
struct Cell {
    ch: char,
    w: u8,
    style: Style,
    button: Option<usize>,
    /// combining marks drawn with `ch` (Devanagari vowel signs, accents
    /// written as separate characters, variation selectors, ZWJ)
    marks: Option<Box<str>>,
}

/// An image anchored at a column of a screen row.
struct ImgAt {
    name: String,
    /// the button it belongs to and its `srcb` picture
    button: Option<usize>,
    hover: Option<String>,
    col: usize,
    w: f32,
    h: f32,
    ypos: f32,
}

/// Translate all text (menus too), not only dialog? Auto: unless the
/// game is shown in English.
fn all_text(st: &crate::settings::Settings) -> bool {
    match st.get("translate_what") {
        "all" => true,
        "auto" => st.get("translate") != "en",
        _ => false,
    }
}

/// Where a line was printed ("ERB/x.ERB:12"), for the translation log.
fn place_of(l: &Line, files: &[String]) -> Option<String> {
    (l.src != 0).then(|| format!("{}:{}", files.get((l.src >> 32) as usize - 1).map_or("?", |f| f.as_str()), l.src & 0xFFFF_FFFF))
}

/// Does `--translation-filter` keep this line untranslated: a listed file
/// printed it, or its text is a listed line?
fn line_filtered(l: &Line, files: &[String]) -> bool {
    let file = (l.src != 0).then(|| files.get((l.src >> 32) as usize - 1).map_or("?", |f| f.as_str()));
    let text: String = l.segs.iter().filter_map(|s| if let Seg::Text { text, .. } = s { Some(text.as_str()) } else { None }).collect();
    crate::mt::filtered(file, &text)
}

/// What identifies a line across frames (indices move when the scrollback
/// is trimmed): where it was printed, in which wait, and its text.
fn line_key(l: &Line) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    l.wait.hash(&mut h);
    l.src.hash(&mut h);
    for s in &l.segs {
        if let Seg::Text { text, .. } = s {
            text.hash(&mut h);
        }
    }
    h.finish()
}

/// `t` spread over lines as wide as `widths` say, in proportion: words
/// (characters, for text without spaces) go to the line whose share of
/// the whole they fall in.
fn spread(t: &str, widths: &[usize], width: impl Fn(&str) -> usize) -> Vec<String> {
    let n = widths.len().max(1);
    let total = widths.iter().sum::<usize>().max(1);
    // words; text written without spaces (Japanese, Chinese) by character
    let tokens: Vec<String> =
        if t.contains(' ') || t.is_ascii() { t.split_inclusive(' ').map(str::to_string).collect() } else { t.chars().map(|c| c.to_string()).collect() };
    let all = width(t).max(1);
    let mut out = vec![String::new(); n];
    let (mut k, mut done, mut share) = (0usize, 0usize, widths[0]);
    for tok in tokens {
        let w = width(&tok);
        // past this line's share of the translation (by its middle): next line
        while k + 1 < n && !out[k].trim().is_empty() && (2 * done + w) * total > 2 * share * all {
            k += 1;
            share += widths[k];
        }
        out[k].push_str(&tok);
        done += w;
    }
    out.iter().map(|s| s.trim().to_string()).collect()
}

/// `s` broken into lines of at most `w` columns at spaces (a word longer
/// than that is cut), `most` lines at most, the last ending in … if more
/// was left.
fn wrap_words(s: &str, w: usize, most: usize) -> Vec<String> {
    let cw = |s: &str| s.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum::<usize>();
    let mut lines: Vec<String> = vec![String::new()];
    for word in s.split(' ') {
        let cur = lines.last_mut().unwrap();
        if !cur.is_empty() && cw(cur) + 1 + cw(word) > w {
            lines.push(String::new());
        }
        let cur = lines.last_mut().unwrap();
        if !cur.is_empty() {
            cur.push(' ');
        }
        // a word wider than the line (or text without spaces: Chinese,
        // Japanese) carries on onto the next line
        for c in word.chars() {
            let cur = lines.last_mut().unwrap();
            if !cur.is_empty() && cw(cur) + UnicodeWidthChar::width(c).unwrap_or(0) > w {
                lines.push(String::new());
            }
            lines.last_mut().unwrap().push(c);
        }
    }
    if lines.len() > most {
        lines.truncate(most);
        let last = lines.last_mut().unwrap();
        while cw(last) + 1 > w {
            last.pop();
        }
        last.push('…');
    }
    lines
}

/// One laid-out screen row.
#[derive(Default)]
struct DRow {
    cells: Vec<Cell>,
    imgs: Vec<ImgAt>,
    /// `<div>` content anchored on this row, painted over the rows at its
    /// offset once the screen is laid out
    divs: Vec<DivAt>,
}

/// A laid-out `<div>`: its rows, drawn from column `col`, `dy` rows below
/// (negative: above) the row it is anchored on.
struct DivAt {
    col: usize,
    dy: i32,
    rows: Vec<DRow>,
}

/// A div's rows inside its background, padding and border: `cols` wide,
/// `rows` high (else as tall as the content needs).
fn boxed_div(content: Vec<DRow>, deco: &crate::console::DivBox, cols: usize, rows: Option<usize>) -> Vec<DRow> {
    let ins = deco.insets();
    let blank = |bg: Option<Rgb>| Cell {
        ch: ' ',
        w: 1,
        style: Style { fg: Rgb(0, 0, 0), bg, bold: false, italic: false, underline: false, strike: false },
        button: None,
        marks: None,
    };
    let edge = blank(deco.bcolor);
    let fill = blank(deco.bg);
    let height = rows.unwrap_or(content.len() + ins[0] + ins[2]).max(ins[0] + ins[2] + 1);
    let mut content = content.into_iter();
    let mut out = Vec::with_capacity(height);
    for r in 0..height {
        let mut row = DRow::default();
        let border_row = (r == 0 && deco.border[0]) || (r + 1 == height && deco.border[2]);
        if border_row {
            row.cells = vec![edge.clone(); cols];
            out.push(row);
            continue;
        }
        if deco.border[3] {
            row.cells.push(edge.clone());
        }
        row.cells.extend(std::iter::repeat_n(fill.clone(), deco.pad[3]));
        let start = row.cells.len();
        let in_w = cols.saturating_sub(ins[1] + ins[3]);
        if r >= ins[0] && r + ins[2] < height {
            if let Some(c) = content.next() {
                for mut cell in c.cells {
                    if cell.style.bg.is_none() {
                        cell.style.bg = deco.bg;
                    }
                    row.cells.push(cell);
                }
                for mut im in c.imgs {
                    im.col += start;
                    row.imgs.push(im);
                }
                for mut d in c.divs {
                    d.col += start;
                    row.divs.push(d);
                }
            }
        }
        // pad (or cut) the content to the inner width
        let mut used = 0usize;
        let mut k = start;
        while k < row.cells.len() {
            let w = row.cells[k].w.max(1) as usize;
            if used + w > in_w {
                row.cells.truncate(k);
                break;
            }
            used += w;
            k += 1;
        }
        row.cells.extend(std::iter::repeat_n(fill.clone(), in_w.saturating_sub(used) + deco.pad[1]));
        if deco.border[1] {
            row.cells.push(edge.clone());
        }
        out.push(row);
    }
    // rounded corners: a quarter block in the corner cell's colour
    if height > 1 && cols > 1 {
        for (k, glyph) in ['▗', '▖', '▘', '▝'].into_iter().enumerate() {
            if !deco.round[k] {
                continue;
            }
            let row = if k < 2 { out.first_mut() } else { out.last_mut() };
            let cell = row.and_then(|r| if k == 0 || k == 3 { r.cells.first_mut() } else { r.cells.last_mut() });
            if let Some(c) = cell.filter(|c| c.ch == ' ' && c.style.bg.is_some()) {
                c.ch = glyph;
                c.style.fg = c.style.bg.take().unwrap();
            }
        }
    }
    out
}

/// Paint `cells` over `row` from screen column `col`: covered cells (and
/// wide characters cut in half) give way, the row is padded if short.
fn paint_cells(row: &mut DRow, col: usize, cells: Vec<Cell>) {
    if cells.is_empty() {
        return;
    }
    let width: usize = cells.iter().map(|c| c.w.max(1) as usize).sum();
    let end = col + width;
    let blank = |st: &Style| Cell { ch: ' ', w: 1, style: st.clone(), button: None, marks: None };
    let plain = Style { fg: Rgb(0, 0, 0), bg: None, bold: false, italic: false, underline: false, strike: false };
    let mut before = Vec::new();
    let mut after = Vec::new();
    let mut c = 0usize;
    for cell in std::mem::take(&mut row.cells) {
        let w = cell.w.max(1) as usize;
        if c + w <= col {
            before.push(cell);
        } else if c >= end {
            after.push(cell);
        } else {
            // partly covered: keep the uncovered columns blank
            for k in c..c + w {
                if k < col {
                    before.push(blank(&cell.style));
                } else if k >= end {
                    after.push(blank(&cell.style));
                }
            }
        }
        c += w;
    }
    while c < col {
        before.push(blank(&plain));
        c += 1;
    }
    before.extend(cells);
    before.extend(after);
    row.cells = before;
}

/// A decoded input event. Mouse `btn` uses xterm codes: 0/1/2 buttons,
/// +32 motion, 64/65 wheel; `x`/`y` are 1-based cells.
#[derive(Debug)]
enum Key {
    Char(char),
    Enter,
    Backspace,
    Esc,
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
    Delete,
    /// Ctrl or Alt + Up / Down: scroll the log a line (Up / Down alone go
    /// through the inputs given before, as in Emuera)
    ScrollUp,
    ScrollDown,
    CtrlC,
    CtrlL,
    Left,
    Right,
    /// Ctrl-O (and F3 unless a keyboard macro is stored on it): the settings
    /// overlay
    Settings,
    /// Emuera-Anchor's hotkeys: Ctrl-T title, Ctrl-R restart, Ctrl-E (and
    /// F5 unless a macro is stored on it) reload the ERB files
    Title,
    Restart,
    Reload,
    /// Tab: continue and skip messages until a choice (as a right click)
    Skip,
    /// F1…F12, with Shift or not: keyboard macros (`macros.rs`), when the
    /// game's config has them on; F3 is Settings and F5 Reload where no
    /// macro is stored on them
    F(u8, bool),
    /// Alt+0…9, or Ctrl+0…9 where the terminal reports it: the macro group
    Group(u8),
    Mouse {
        btn: u16,
        x: u16,
        y: u16,
        press: bool,
    },
    /// Insert, or Shift+Enter where the terminal tells it from Enter: a
    /// click with the keyboard pointer
    Click,
    /// a gamepad's d-pad, left stick, A or B: the pointer and clicks while
    /// a game waits for input (`Tty::game_key`), arrows, Enter and Esc in
    /// menus (`Tty::next_key`)
    Pad(crate::gamepad::Ev),
    Other,
}

/// Do a modified arrow's parameters (`1;5` in `ESC [ 1 ; 5 A`) hold Ctrl
/// or Alt?
fn scroll_mods(p: &[u8]) -> bool {
    let m = std::str::from_utf8(p).ok().and_then(|s| s.split(';').nth(1)).and_then(|m| m.parse::<u8>().ok());
    m.is_some_and(|m| m.saturating_sub(1) & 6 != 0)
}

/// Cells for a shape `w` cells wide (fractional) starting at `col`, with
/// `frac` cells carried from the shape before it in the same run (None: it
/// starts one): up to where its exact right edge rounds to; a rectangle
/// (`rect`) starting a run gets at least one cell, and the shapes after it
/// give that back. The difference goes on in `frac`.
pub(crate) fn shape_cells(col: usize, frac: &mut Option<f32>, w: f32, rect: bool) -> usize {
    let start = frac.is_none();
    let exact = col as f32 + frac.unwrap_or(0.0) + w.max(0.0);
    let mut end = exact.round().max(col as f32) as usize;
    if rect && start && w > 0.0 {
        end = end.max(col + 1);
    }
    *frac = Some(exact - end as f32);
    end - col
}

/// A function key from a CSI sequence's parameters and final byte, with
/// whether Shift was held: xterm's `ESC [ 15 ; 2 ~` and `ESC [ 1 ; 2 P`,
/// VT220's `ESC [ 11 ~` (F1), the Linux console's and rxvt's F13–F20,
/// which are Shift+F3…F10 there (their Shift+F1 / F2 are F11 / F12), and
/// the illumos / Solaris console's `ESC [ 224 z` … `ESC [ 235 z`.
fn fkey(p: &[u8], fin: u8) -> Option<(u8, bool)> {
    let s = std::str::from_utf8(p).ok()?;
    let mut it = s.split(';');
    let a: u8 = match it.next()? {
        "" => 1,
        x => x.parse().ok()?,
    };
    let m: u8 = match it.next() {
        None => 1,
        Some(x) => x.parse().ok()?,
    };
    if it.next().is_some() {
        return None;
    }
    let shift = m.saturating_sub(1) & 1 != 0;
    let (n, f13) = match (fin, a) {
        (b'P'..=b'S', 1) => (fin - b'P' + 1, false),
        (b'~', 11..=15) => (a - 10, false),
        (b'~', 17..=21) => (a - 11, false),
        (b'~', 23 | 24) => (a - 12, false),
        (b'~', 25 | 26) => (a - 22, true),
        (b'~', 28 | 29) => (a - 23, true),
        (b'~', 31..=34) => (a - 24, true),
        (b'z', 224..=235) => (a - 223, false),
        _ => return None,
    };
    Some((n, shift || f13))
}

/// Ctrl+digit or Alt+digit as xterm's modifyOtherKeys (`ESC [ 27 ; 5 ; 49 ~`)
/// and CSI-u terminals (`ESC [ 49 ; 5 u`) report it: the digit.
fn digit_group(p: &[u8], fin: u8) -> Option<u8> {
    let n: Vec<u32> = std::str::from_utf8(p).ok()?.split(';').map(|x| x.parse().ok()).collect::<Option<_>>()?;
    let (c, m) = match (fin, n.as_slice()) {
        (b'~', [27, m, c]) => (*c, *m),
        (b'u', [c, m]) => (*c, *m),
        _ => return None,
    };
    // Alt (2) or Ctrl (4), without Shift
    let mods = m.checked_sub(1)?;
    ((48..=57).contains(&c) && mods & 6 != 0 && mods & 1 == 0).then(|| (c - 48) as u8)
}

/// Does the game's font draw `\` (U+005C) as ¥, as Japanese fonts do so
/// that Shift-JIS text shows yen? Emuera never changes the character: games
/// made in Japan show ¥ (eraTW's money unit is a backslash in its
/// _Replace.csv) because its default font, MS Gothic, draws it that way.
/// Its MS Gothic / Mincho family and Meiryo do; Yu Gothic and others don't.
/// No font set means Emuera's default, MS Gothic.
fn backslash_is_yen(font: &str) -> bool {
    let f = font_key(font);
    if f.is_empty() {
        return true;
    }
    let ms = f.starts_with("ms") && ["ゴシック", "明朝", "gothic", "mincho"].iter().any(|k| f.contains(k));
    ms || f.contains("meiryo") || f.contains("メイリオ")
}

/// A font name to compare: full-width letters (ＭＳ) as ASCII, no spaces,
/// any case.
fn font_key(font: &str) -> String {
    font.chars()
        .filter(|c| !c.is_whitespace() && *c != '\u{3000}')
        .map(|c| match c {
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            c => c,
        })
        .collect::<String>()
        .to_lowercase()
}

/// Does the game's font draw Greek and Cyrillic letters two columns wide?
/// They are double-byte in Shift-JIS, and the fixed-pitch Japanese fonts
/// (MS Gothic, Emuera's default, and MS Mincho) draw them full-width;
/// Emuera draws text by its measured glyphs, so with any other font (the
/// proportional MS PGothic and MS UI Gothic included) they are narrow.
fn letters_are_wide(font: &str) -> bool {
    let f = font_key(font);
    f.is_empty()
        || (f.starts_with("ms") && !f.starts_with("msp") && !f.contains("ui") && ["ゴシック", "明朝", "gothic", "mincho"].iter().any(|k| f.contains(k)))
}

/// A Greek or Cyrillic letter (those in Shift-JIS are double-byte there).
fn is_greek_cyrillic(ch: char) -> bool {
    matches!(ch as u32, 0x370..=0x4FF)
}

/// How many columns Greek and Cyrillic letters take on this screen (the
/// "Greek and Cyrillic" setting and the game's font).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Letters {
    /// two each, as MS Gothic draws them
    Wide,
    /// one each
    Narrow,
    /// the font draws them wide, but only a letter on its own keeps two
    /// columns (the `Д` of `(´Д｀)`, a `Σ` in a stat line: Japanese games
    /// line those up as MS Gothic draws them); letters in a run are text
    /// written in those scripts, which spaced out (`П р и в е т`) can't be
    /// read, so they take one
    ByWord,
}

/// The characters of `s`, each with whether a Greek or Cyrillic letter
/// there takes two columns (for [`char_cells`]).
fn letter_widths(s: &str, letters: Letters) -> impl Iterator<Item = (char, bool)> + '_ {
    let mut it = s.chars().peekable();
    let mut prev = false;
    std::iter::from_fn(move || {
        let ch = it.next()?;
        let wide = match letters {
            Letters::Wide => true,
            Letters::Narrow => false,
            Letters::ByWord => !prev && !it.peek().is_some_and(|&c| is_greek_cyrillic(c)),
        };
        prev = is_greek_cyrillic(ch);
        Some((ch, wide))
    })
}

/// The column in a cursor position report (`ESC [ row ; col R`).
fn cpr_column(s: &str) -> Option<u32> {
    let mut rest = s;
    while let Some(i) = rest.find("\x1b[") {
        rest = &rest[i + 2..];
        let end = rest.find(|c: char| !(c.is_ascii_digit() || c == ';'))?;
        if rest[end..].starts_with('R') {
            return rest[..end].split(';').nth(1)?.parse().ok();
        }
    }
    None
}

/// The colour in an OSC 11 reply (`ESC ] 11 ; rgb:RRRR/GGGG/BBBB`, one to
/// four hex digits per channel).
fn parse_osc11(s: &str) -> Option<Rgb> {
    let rest = &s[s.find("]11;rgb:")? + 8..];
    let end = rest.find(['\x07', '\x1b']).unwrap_or(rest.len());
    let ch: Vec<u8> = rest[..end]
        .split('/')
        .map(|h| {
            let v = u32::from_str_radix(h, 16).ok()?;
            let max = (1u32 << (4 * h.len().clamp(1, 4))) - 1;
            Some((v * 255 / max) as u8)
        })
        .collect::<Option<_>>()?;
    (ch.len() == 3).then(|| Rgb(ch[0], ch[1], ch[2]))
}

/// What the settings overlay asks the input loop to do next.
enum OverlayExit {
    Close,
    /// the game's settings (emuera.config) changed
    Config,
    Title,
    Restart,
    /// restart with `--debug` turned on (or off, if it was on)
    RestartDebug,
    Reload,
    SwitchGame,
    Quit,
}

/// Is this run in debug mode (`--debug`: [IF_DEBUG] blocks and ;#; lines)?
fn debug_run() -> bool {
    std::env::args_os().skip(1).any(|a| a == "--debug")
}

pub struct Tty {
    raw_mode: Option<crate::sys::RawMode>,
    gfx: Box<dyn Backend>,
    cell: (u32, u32),
    cols: usize,
    rows: usize,
    color: ColorMode,
    scroll: usize,
    /// the input line: what is typed, its cursor, the inputs given before
    line: lineedit::LineEdit,
    /// the last input timed out (TINPUT): what was typed stays for the
    /// next, as Emuera's input box keeps it (AnonTW's animated map is a
    /// TINPUT loop)
    keep_buf: bool,
    /// GETKEY: keys / mouse buttons held (Windows codes: 1 left, 2 right)
    keys_down: std::collections::HashSet<i64>,
    /// GETKEYTRIGGERED: pressed since asked last
    keys_hit: std::collections::HashSet<i64>,
    pending: Vec<u8>,
    /// keys decoded but not yet handed out (console mouse events)
    queued: std::collections::VecDeque<Key>,
    /// pointer on the Linux console via /dev/input/mice (no terminal reports)
    mice: Option<crate::mice::Mice>,
    mouse: Option<(usize, usize)>,
    /// How characters Emuera draws two columns wide but the terminal draws
    /// one column wide are handled (detected at startup, see `char_cells`).
    ambi: Ambi,
    /// what the Linux console's font can draw (None: any terminal font)
    font_chars: Option<std::collections::HashSet<char>>,
    /// the terminal doesn't take UTF-8: everything else drawn as ASCII
    ascii_only: bool,
    /// Widths this terminal actually draws, for characters where tables
    /// disagree (measured at startup; only entries that differ from
    /// unicode-width are kept).
    widths: std::collections::HashMap<char, u8>,
    /// per screen row: (col_start, col_end, button id)
    btn_map: Vec<Vec<(usize, usize, usize)>>,
    buttons: Vec<Value>,
    hover: Option<usize>,
    active: bool,
    /// the terminal went away (hangup / end of input): quit instead of
    /// waiting forever on a dead stdin
    hangup: bool,
    /// a setup problem the user should see (e.g. no console mouse)
    notice: Option<String>,
    /// the game's "Use keyboard macros": F1…F12 are macro keys
    macros_on: bool,
    /// the game's macro.txt, read when a macro key is first used
    macros: Option<crate::macros::Macros>,
    /// what a macro key did (the group chosen…), shown in the input line
    /// for a moment
    macro_note: Option<(String, Instant)>,
    fb_cell_fixed: bool,
    /// the last frame, for moving the console pointer without a full
    /// redraw (which makes images flicker): per screen row (col, cell,
    /// hover colour), the images, the pointer cell and the input cursor
    frame: Vec<Vec<(usize, Cell, Option<Rgb>)>>,
    frame_imgs: Vec<Placement>,
    /// makes overlapping pictures into one (see `gfx::Merger`)
    merger: crate::gfx::Merger,
    /// the button the pointer rests on and since when (tooltip delay)
    tip_since: Option<(usize, Instant)>,
    /// the last frame showed a tooltip
    tip_drawn: bool,
    drawn_pointer: Option<(usize, usize)>,
    frame_cursor: Option<(usize, usize)>,
    /// the window background for this frame (None: the terminal's own)
    screen_bg: Option<Rgb>,
    /// the background the game assumes where it sets none (its default,
    /// black unless the config says otherwise)
    game_bg: Rgb,
    /// the terminal's own background (OSC 11), shown where the game sets none
    term_bg: Option<Rgb>,
    /// minimum contrast for coloured text (1: off), see `console::readable`
    pub contrast: f32,
    /// colours drawn as others (`recolor` setting: AnonTW's time-stop navy
    /// as a softer blue…); the game still sees its own
    pub recolor: Vec<(Rgb, Rgb)>,
    /// lines per mouse-wheel step
    pub wheel: usize,
    /// "Scroll to end on keyboard input": any key press while scrolled back jumps to the end
    scroll_end_on_key: bool,
    /// the "Backslash as ¥" setting: auto, on, off
    pub yen_mode: String,
    /// draw `\` in game text as ¥ on this screen (see `backslash_is_yen`)
    yen: bool,
    /// the "Greek and Cyrillic" setting: auto, wide, narrow
    pub letters_mode: String,
    /// Greek and Cyrillic letters' columns on this screen
    letters: Letters,
    /// show the hovered button's value, memory and the running function
    /// in the bottom row
    pub status_info: bool,
    /// the keyboard pointer (see `kbd_pointer`)
    pub kbd_ptr: bool,
    /// gamepads are read (the Gamepad setting)
    pad_enabled: bool,
    /// a gamepad has been used: its pointer shows
    pad_on: bool,
    /// a game's input is reading keys: a pad moves the pointer and clicks
    /// (see `game_key`)
    pad_game: bool,
    /// EMUERA_MOUSE_LOG: what the mouse did, for finding out why a pointer
    /// misbehaves where it can't be watched (the Linux console)
    mouse_log: Option<std::fs::File>,
    /// console mouse events received (shown in the bottom bar there)
    mouse_events: u64,
    /// our pointer blinks: shown now, and when that changes
    ptr_on: bool,
    blink_at: Instant,
    /// what applying a setting has to say (shown on the settings page)
    setting_note: String,
    /// live translation of character dialog (see `mt`): the translator, and
    /// into / from which language, and what (dialog, all)
    mt: Option<crate::mt::Mt>,
    tr_to: String,
    tr_from: String,
    tr_all: bool,
    /// the memory in use in the bottom row too (it is always in F3)
    pub show_memory: bool,
    /// the bottom row's right side as last drawn: (columns the prompt may
    /// use, the function asking, the column it starts at); None: not shown
    bar: Option<(usize, String, usize)>,
    /// memory in use, read at most once a second: (when, bytes)
    mem_seen: Option<(Instant, Option<u64>)>,
    /// when the script started its current stretch of computing
    busy_since: Option<Instant>,
    /// the "Copy to clipboard" setting: game (emuera.config), on, off
    pub clip_mode: String,
    clip: crate::clipboard::Clipboard,
    /// the last left click (time, cell), to tell double clicks
    last_click: Option<(Instant, (u16, u16))>,
    /// frames drawn without images (under the settings overlay)
    no_images: bool,
    /// the colour mode detected at startup, for the "auto" setting
    auto_color: ColorMode,
    /// the `--gfx` preference and $TERM, to set the image backend up again
    gfx_pref: String,
    term: String,
    /// fingerprint of the last frame sent (an identical one is skipped)
    last_frame: u64,
    force_frame: bool,
    /// the game's "ボタンの途中で行を折りかえさない" setting (per frame)
    button_wrap: bool,
    /// Emuera pixels per text cell: half the font size, the width of a
    /// half-width character, which shapes are measured against
    text_px: f32,
    /// CBG pictures scaled to the terminal, by (fingerprint, size)
    cbg_scaled: std::collections::HashMap<(u64, u32, u32), std::rc::Rc<image::RgbaImage>>,
    /// the game folder and its save folder, for the save manager
    game: Option<(std::path::PathBuf, std::path::PathBuf)>,
    /// an animated sprite was on the last frame
    animating: bool,
    /// fbterm: 256 colours through its own escapes
    fbterm: bool,
    /// 16 colours: the bright ones as 90-97 (the Hurd's console drops a
    /// whole SGR with them: bold brightens there)
    bright_codes: bool,
    /// hash of each screen row as last written (0: unknown, write it)
    row_hash: Vec<u64>,
    hash_cols: usize,
    /// something drew over the rows outside `render` (overlay boxes)
    redraw_all: std::cell::Cell<bool>,
    /// an overlay box (list, prompt, page) closed or changed size: the
    /// next frame is drawn in full even if the game's screen is the same
    box_closed: std::cell::Cell<bool>,
    /// the folder the save manager's file browser was last used in
    browse_dir: Option<std::path::PathBuf>,
    /// buttons with a `srcb` hover picture (their hover needs a full redraw)
    hover_imgs: std::collections::HashSet<usize>,
    /// translated lines the player clicked back to the original text
    /// (see [`line_key`])
    show_orig: std::collections::HashSet<u64>,
    /// per screen row of the last frame: the key of the translatable line
    /// it belongs to, for clicks
    row_lines: Vec<Option<u64>>,
    /// lines the player middle-clicked to translate whatever the Translate
    /// setting says (see [`line_key`])
    forced: std::collections::HashSet<u64>,
    /// per screen row of the last frame: the key of its line, translated
    /// or not (while translating), for those clicks
    row_all: Vec<Option<u64>>,
}

/// Terminal size: (cols, rows, pixel width, pixel height); pixels are 0
/// when the terminal doesn't report them.
fn winsize() -> (usize, usize, u32, u32) {
    crate::sys::winsize().unwrap_or((80, 24, 0, 0))
}

/// Decode an X10 / urxvt button byte (already minus 32). Releases come as
/// button 3 without saying which button: report them as a left release.
fn x10_mouse(cb: u16, x: u16, y: u16) -> Key {
    let release = cb & 3 == 3 && cb & (32 | 64) == 0;
    Key::Mouse { btn: if release { cb & !3 } else { cb }, x, y, press: !release }
}

/// A character the Linux console's font has instead of `ch` (which it
/// would show as a diamond): the nearest ASCII or console-font lookalike.
fn console_lookalike(ch: char, has: &std::collections::HashSet<char>) -> char {
    let cands: &[char] = match ch {
        // full-width ASCII
        '\u{FF01}'..='\u{FF5E}' => return char::from_u32(ch as u32 - 0xFEE0).unwrap_or('?'),
        '\u{3000}' => return ' ',
        '【' | '〔' | '「' | '『' | '〈' | '《' | '〖' | '〘' | '〚' => &['[', '['],
        '】' | '〕' | '」' | '』' | '〉' | '》' | '〗' | '〙' | '〛' => &[']', ']'],
        '・' | '･' | '·' => &['·', '.'],
        '、' | '､' => &[','],
        '。' | '｡' => &['.'],
        '…' | '‥' => &['…', '.'],
        'ー' | '―' | '‐' | '—' | '–' | '─' | '━' => &['─', '-'],
        '〜' => &['~'],
        '“' | '”' | '″' | '〝' | '〟' => &['"'],
        '‘' | '’' | '′' => &['\''],
        '★' | '☆' | '※' => &['*'],
        '○' | '◯' | '◎' | '〇' => &['○', 'o'],
        '●' => &['●', '•', 'o'],
        '□' | '◇' => &['□', '#'],
        '■' | '◆' => &['■', '#'],
        '△' | '▲' => &['▲', '^'],
        '▽' | '▼' => &['▼', 'v'],
        '→' | '⇒' => &['→', '>'],
        '←' | '⇐' => &['←', '<'],
        '↑' => &['↑', '^'],
        '↓' => &['↓', 'v'],
        '×' => &['×', 'x'],
        '÷' => &['÷', '/'],
        '±' => &['±', '+'],
        '♪' | '♫' => &['♪', '#'],
        '♡' | '♥' => &['♥', '<'],
        '≠' => &['≠', '#'],
        '≦' | '≤' => &['≤', '<'],
        '≧' | '≥' => &['≥', '>'],
        '┃' | '│' => &['│', '|'],
        '░' => &['░', ':'],
        '▒' => &['▒', '#'],
        '▓' | '█' => &['█', '#'],
        // nothing like it (kana, kanji): the console's diamond says so
        _ => return ch,
    };
    cands.iter().copied().find(|c| c.is_ascii() || has.contains(c)).unwrap_or(ch)
}

/// A character drawn with the one before it, in its cell: zero width and
/// not a control character (combining marks, Devanagari vowel signs and
/// virama, variation selectors, ZWJ). Kept with their base character rather
/// than dropped, so the terminal can compose them.
fn is_mark(ch: char) -> bool {
    !ch.is_control() && UnicodeWidthChar::width(ch) == Some(0)
}

/// Add combining mark `ch` to the cell holding its base character.
fn add_mark(cell: &mut Cell, ch: char) {
    let mut m = cell.marks.take().map(String::from).unwrap_or_default();
    m.push(ch);
    cell.marks = Some(m.into_boxed_str());
}

/// How to draw one character so the screen matches Emuera's layout:
/// (cells the terminal draws it in, filler cells to add, filler char).
///
/// Emuera games are laid out for a Japanese font where every Shift_JIS
/// double-byte character is two columns wide, including box drawing
/// (`━┃┏`), blocks (`■□`) and symbols (`※○→`). Most terminals draw those
/// "East Asian ambiguous" characters one column wide, which skews ASCII
/// maps. The missing column is filled: line and block characters continue
/// themselves (`━━`, `┏━`, `■■`) so walls stay solid; others get a space.
/// With [`Ambi::Osc66`] such a character is instead drawn as one 2-cell
/// glyph (no filler); with [`Ambi::Wide`] the terminal already draws it wide.
/// `measured`: the width this terminal was seen to draw `ch` at (see
/// `Tty::measure_widths`), which beats any table. `wide_letters`: a Greek or
/// Cyrillic letter gets two columns too (the game's font draws it so, see
/// [`letters_are_wide`] and [`letter_widths`]); else one, as the terminal
/// draws it.
fn char_cells(ch: char, ambi: Ambi, measured: Option<u8>, wide_letters: bool) -> (usize, usize, char) {
    let tw = match measured {
        Some(w) => w as usize,
        None => if ambi == Ambi::Wide { UnicodeWidthChar::width_cjk(ch) } else { UnicodeWidthChar::width(ch) }.unwrap_or(0),
    };
    // a terminal with no double-width cells (the Linux console before
    // 6.15) draws every character in one cell
    let tw = if ambi == Ambi::Narrow { tw.min(1) } else { tw };
    if tw == 0 || ch.is_ascii() || (!wide_letters && is_greek_cyrillic(ch)) {
        return (tw, 0, ' ');
    }
    let mut buf = [0u8; 4];
    let (bytes, _, unmappable) = encoding_rs::SHIFT_JIS.encode(ch.encode_utf8(&mut buf));
    let emuera = if unmappable { tw } else { bytes.len() };
    let fill = match ch {
        '━' | '┏' | '┗' | '┣' | '┳' | '┻' | '╋' | '┯' | '┷' | '┿' | '┝' | '┍' | '┕' => '━',
        '─' | '┌' | '└' | '├' | '┬' | '┴' | '┼' | '┠' | '┰' | '┸' | '╂' | '┎' | '┖' => '─',
        '═' | '╔' | '╚' | '╠' | '╦' | '╩' | '╬' => '═',
        '┄' | '┅' | '┈' | '┉' | '╌' | '╍' | '■' | '█' | '▓' | '▒' | '░' | '―' | '‐' | '〓' | '≡' | 'Ⅱ' | 'Ⅲ' => ch,
        _ => ' ',
    };
    let extra = emuera.saturating_sub(tw);
    if ambi == Ambi::Osc66 && extra > 0 {
        return (tw + extra, 0, ' ');
    }
    (tw, extra, fill)
}

/// A tooltip laid out: its lines (text, columns) and its box (1-based
/// top row and left column, width in columns).
struct TipBox {
    lines: Vec<(String, usize)>,
    top: usize,
    left: usize,
    w: usize,
}

/// Text from the game safe to write into a box: a tab becomes a space,
/// other control characters go (CR and LF stay for the caller to split
/// lines at); written raw they would move the cursor.
pub(crate) fn printable(s: &str) -> String {
    s.chars()
        .filter_map(|c| match c {
            '\t' => Some(' '),
            '\r' | '\n' => Some(c),
            c if c.is_control() => None,
            c => Some(c),
        })
        .collect()
}

/// Strategy for East Asian "ambiguous width" characters (`━ ■ ※ …`).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Ambi {
    /// draw one cell, then a filler cell (works everywhere)
    Pad,
    /// the terminal already draws them two cells wide
    Wide,
    /// the terminal draws even CJK in one cell (Linux console): every wide
    /// character is padded to its Emuera width
    Narrow,
    /// kitty's text sizing protocol: `OSC 66 ; w=2 ; ch` draws the glyph
    /// across two cells (opt-in: kitty's rendering of it doesn't match
    /// Emuera's font well)
    Osc66,
}

/// `from:to` pairs of hex colours, comma separated (`01183c:111d3b`,
/// `#` or `0x` allowed); pairs that don't parse are left out.
fn parse_recolor(v: &str) -> Vec<(Rgb, Rgb)> {
    let hex = |s: &str| {
        let s = s.trim().trim_start_matches('#');
        let s = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
        u32::from_str_radix(s, 16).ok().filter(|_| !s.is_empty()).map(|n| Rgb::from_i64(n as i64))
    };
    v.split(',').filter_map(|p| p.split_once(':')).filter_map(|(a, b)| Some((hex(a)?, hex(b)?))).collect()
}

// what tests draw goes here instead of the terminal (see `capture`)
#[cfg(test)]
thread_local! {
    static CAPTURE: std::cell::RefCell<Option<Vec<u8>>> = const { std::cell::RefCell::new(None) };
}

/// What `f` writes to the terminal (tests).
#[cfg(test)]
pub(crate) fn capture(f: impl FnOnce()) -> Vec<u8> {
    CAPTURE.with(|c| *c.borrow_mut() = Some(Vec::new()));
    f();
    CAPTURE.with(|c| c.borrow_mut().take().unwrap_or_default())
}

/// Write to the terminal and flush.
fn write_all(b: &[u8]) {
    #[cfg(test)]
    if CAPTURE.with(|c| c.borrow_mut().as_mut().map(|v| v.extend_from_slice(b)).is_some()) {
        return;
    }
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(b);
    let _ = out.flush();
}

/// Running inside tmux? `TMUX` alone isn't enough: a terminal started from
/// a shell inside tmux inherits it (Ghostty opened from a tmux pane got
/// tmux's passthrough wrappers and showed no pictures).
fn in_tmux() -> bool {
    let var = |k| std::env::var(k).unwrap_or_default();
    in_tmux_env(std::env::var_os("TMUX").is_some(), &var("TERM"), &var("TERM_PROGRAM"))
}

/// `in_tmux` from the environment: tmux sets `TERM` to `tmux-*` /
/// `screen-*` by default and, since 3.2, `TERM_PROGRAM=tmux`; a terminal
/// that sets `TERM_PROGRAM` to its own name (Ghostty, WezTerm, iTerm2…)
/// means tmux's variable is only inherited.
fn in_tmux_env(tmux: bool, term: &str, program: &str) -> bool {
    tmux && (term.starts_with("tmux") || term.starts_with("screen") || program.is_empty() || program == "tmux")
}

/// Does tmux let escape sequences through to the terminal it runs in
/// (`allow-passthrough on` or `all`)? Without it, pictures can't be shown.
fn tmux_passthrough() -> bool {
    std::process::Command::new("tmux")
        .args(["show-options", "-gqv", "allow-passthrough"])
        .output()
        .is_ok_and(|o| matches!(String::from_utf8_lossy(&o.stdout).trim(), "on" | "all"))
}

impl Tty {
    /// Enter raw mode and the alternate screen, pick colour and graphics
    /// support, open the console mouse if needed, and enable mouse reports.
    /// A screen of `cols` × `rows` with nothing detected or set up yet: what
    /// [`Tty::new`] starts from, and what tests draw into (`raw_mode` None:
    /// no terminal to restore).
    fn blank(raw_mode: Option<crate::sys::RawMode>, cols: usize, rows: usize, color: ColorMode) -> Tty {
        Tty {
            raw_mode,
            gfx: Box::new(gfx::NoGfx),
            cell: (8, 16),
            cols,
            rows,
            color,
            scroll: 0,
            line: Default::default(),
            keep_buf: false,
            keys_down: Default::default(),
            keys_hit: Default::default(),
            pending: Vec::new(),
            queued: std::collections::VecDeque::new(),
            mice: None,
            mouse: None,
            ambi: Ambi::Pad,
            font_chars: None,
            ascii_only: false,
            widths: std::collections::HashMap::new(),
            btn_map: Vec::new(),
            buttons: Vec::new(),
            hover: None,
            active: true,
            hangup: false,
            notice: None,
            macros_on: true,
            macros: None,
            macro_note: None,
            fb_cell_fixed: false,
            frame: Vec::new(),
            frame_imgs: Vec::new(),
            merger: Default::default(),
            tip_since: None,
            tip_drawn: false,
            drawn_pointer: None,
            frame_cursor: None,
            screen_bg: None,
            game_bg: Rgb(0, 0, 0),
            term_bg: None,
            contrast: 3.0,
            recolor: Vec::new(),
            wheel: 3,
            scroll_end_on_key: false,
            yen_mode: "auto".into(),
            yen: false,
            letters_mode: "auto".into(),
            letters: Letters::ByWord,
            status_info: true,
            kbd_ptr: false,
            pad_enabled: false,
            pad_on: false,
            pad_game: false,
            mouse_log: std::env::var_os("EMUERA_MOUSE_LOG").and_then(|p| std::fs::OpenOptions::new().create(true).append(true).open(p).ok()),
            mouse_events: 0,
            ptr_on: true,
            blink_at: Instant::now() + Duration::from_millis(530),
            setting_note: String::new(),
            mt: None,
            tr_to: String::new(),
            tr_from: "auto".into(),
            tr_all: false,
            show_memory: false,
            bar: None,
            mem_seen: None,
            busy_since: None,
            clip_mode: "game".into(),
            clip: crate::clipboard::Clipboard::default(),
            last_click: None,
            no_images: false,
            auto_color: color,
            gfx_pref: String::new(),
            term: String::new(),
            last_frame: 0,
            force_frame: false,
            cbg_scaled: std::collections::HashMap::new(),
            game: None,
            animating: false,
            fbterm: false,
            bright_codes: true,
            row_hash: Vec::new(),
            hash_cols: 0,
            redraw_all: std::cell::Cell::new(true),
            box_closed: std::cell::Cell::new(false),
            browse_dir: None,
            hover_imgs: std::collections::HashSet::new(),
            show_orig: std::collections::HashSet::new(),
            row_lines: Vec::new(),
            forced: std::collections::HashSet::new(),
            row_all: Vec::new(),
            button_wrap: false,
            text_px: 9.0,
        }
    }

    pub fn new(gfx_pref: &str) -> Result<Tty, String> {
        let raw_mode = crate::sys::RawMode::enter()?;
        let term = std::env::var("TERM").unwrap_or_default();
        let colorterm = std::env::var("COLORTERM").unwrap_or_default();
        let forced = match std::env::var("EMUERA_COLORS").as_deref() {
            Ok("truecolor") | Ok("24bit") => Some(ColorMode::True),
            Ok("256") => Some(ColorMode::C256),
            Ok("16") => Some(ColorMode::C16),
            _ => None,
        };
        let detected = if term == "linux" || term == "hurd" {
            ColorMode::C16
        } else if term.starts_with("fbterm") {
            // with its own escapes (see `sgr`)
            ColorMode::C256
        } else if colorterm.contains("truecolor") || colorterm.contains("24bit") || term.contains("kitty") || term.contains("foot") || term.contains("direct") {
            ColorMode::True
        } else if term.contains("256") {
            ColorMode::C256
        } else {
            ColorMode::True
        };
        let color = forced.unwrap_or(detected);
        let (cols, rows, xp, yp) = winsize();
        let mut t = Tty::blank(Some(raw_mode), cols, rows, color);
        t.auto_color = detected;
        t.gfx_pref = gfx_pref.to_string();
        t.term = term.clone();
        t.fbterm = term.starts_with("fbterm");
        t.bright_codes = term != "hurd";
        if xp > 0 && yp > 0 {
            t.cell = (xp / cols as u32, yp / rows as u32);
        }
        let gfx_note = t.setup_gfx(gfx_pref, &term);
        eprintln!("{}", gfx_note);
        t.ambi = t.detect_ambiguous();
        // (every width is 1 there: nothing to measure)
        if t.ambi != Ambi::Narrow {
            t.widths = t.measure_widths();
        }
        let is_vt = text_console(&term);
        // the terminal's own background, for readable colours (the Linux
        // console's is black and it doesn't answer)
        if !is_vt {
            let (bg, utf8) = t.query_terminal();
            t.term_bg = bg;
            // an 8-bit terminal: plain ASCII (its widths measured wrong too)
            if !utf8 {
                t.ascii_only = true;
                t.widths.clear();
            }
        }
        // (fbterm draws with fontconfig's fonts)
        if term == "linux" {
            t.font_chars = crate::sys::console_font_chars();
        }
        if crate::mice::Mice::wanted(is_vt) {
            // gpm first (works without extra permissions), else the device
            let gpm = if std::env::var("EMUERA_MOUSE").as_deref() == Ok("mice") { Err(String::new()) } else { crate::mice::Mice::open_gpm() };
            match gpm.or_else(|_| crate::mice::Mice::open()) {
                Ok(m) => {
                    t.mice = Some(m);
                    // the pointer shows from the start, mid-screen, until
                    // the mouse moves it
                    t.mouse = Some((t.cols / 2, t.rows / 2));
                }
                Err(e) => {
                    // shown on the input line (and again on exit): the
                    // startup text is gone once the screen is drawn
                    t.notice = Some(t!("no mouse ({error}): install gpm, or add yourself to the input group", error = e));
                }
            }
        }
        // alt screen, hide cursor; mouse reports: buttons, drag, any motion,
        // in SGR (1006) or urxvt (1015) form where supported, else X10
        write_all(b"\x1b[?1049h\x1b[?25l\x1b[2J");
        // the terminal's mouse reports — but not with a console mouse of our
        // own: on the Linux console they make gpm send the mouse to the
        // console (clicks only, as X10 reports) instead of to us, and the
        // pointer never moved
        if t.mice.is_none() {
            write_all(b"\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1015h\x1b[?1006h");
        }
        // xterm's modifyOtherKeys (Shift+Enter apart from Enter); not for
        // the Linux console, which has no such mode, nor kitty (its own
        // keyboard protocol; it logs this as a parse error)
        if !is_vt && term != "xterm-kitty" {
            write_all(b"\x1b[>4;1m");
        }
        t.mlog(&format!(
            "TERM={} gfx={} cell={:?} size={}x{} mouse={}",
            term,
            t.gfx.name(),
            t.cell,
            t.cols,
            t.rows,
            t.mice.as_ref().map_or_else(|| t.notice.clone().unwrap_or_else(|| "none".into()), |m| m.describe().to_string())
        ));
        // KMSCON (only when its TERM is set to kmscon: its default is
        // xterm-256color): no image protocol, and no hover reports
        if term == "kmscon" || term.starts_with("kmscon-") {
            t.notice = Some("KMSCON: no pictures (it has no image protocol), no mouse hover".into());
        }
        Ok(t)
    }

    /// Wait up to `timeout_ms` for input on stdin (and the console mouse);
    /// returns whether anything arrived.
    fn read_bytes(&mut self, timeout_ms: i32) -> bool {
        let pad = if self.pad_enabled { crate::gamepad::fd() } else { -1 };
        let r = crate::sys::poll2(timeout_ms, self.mice.as_ref().map_or(-1, |m| m.fd()), pad);
        if r.hangup {
            self.hangup = true;
            return false;
        }
        if r.extra2 {
            use crate::gamepad::Ev;
            for ev in crate::gamepad::read() {
                self.queued.push_back(match ev {
                    Ev::Enter => Key::Enter,
                    Ev::Settings => Key::Settings,
                    Ev::Esc => Key::Esc,
                    Ev::PageUp => Key::PageUp,
                    Ev::PageDown => Key::PageDown,
                    Ev::ScrollUp => Key::ScrollUp,
                    Ev::ScrollDown => Key::ScrollDown,
                    Ev::Home => Key::Home,
                    Ev::End => Key::End,
                    ev => Key::Pad(ev),
                });
            }
        }
        if r.extra {
            let (cell, cols, rows) = (self.cell, self.cols, self.rows);
            let evs = self.mice.as_mut().map(|m| m.read(cell, cols, rows)).unwrap_or_default();
            for (btn, x, y, press) in evs {
                self.mouse_events += 1;
                self.mlog(&format!("event btn={} cell=({},{}) press={}", btn, x, y, press));
                self.queued.push_back(Key::Mouse { btn, x, y, press });
            }
        }
        if !r.input {
            return !self.queued.is_empty();
        }
        let mut b = [0u8; 4096];
        match crate::sys::read_stdin(&mut b) {
            Ok(n) if n > 0 => {
                self.pending.extend_from_slice(&b[..n]);
                true
            }
            // an error other than "try again"
            Err(e) if matches!(e.kind(), std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock) => false,
            // 0: end of input
            _ => {
                self.hangup = true;
                false
            }
        }
    }

    /// Is unread input waiting on stdin right now?
    fn input_waiting(&self) -> bool {
        crate::sys::poll(0, -1).input
    }

    /// The terminal's background colour (OSC 11), if it says, and whether
    /// it takes UTF-8: `ñ` (two bytes, never wide) moves the cursor one
    /// column there, two on an 8-bit terminal (NetBSD's console). The
    /// device attributes query after them answers anyway, so a terminal
    /// that ignores the others costs no wait. Haiku's Terminal answers DA
    /// before OSC 11, so the OSC reply gets 60 ms more after the DA one.
    fn query_terminal(&mut self) -> (Option<Rgb>, bool) {
        let da_at = std::cell::Cell::new(None::<Instant>);
        let r = self.query(
            "\r\u{f1}\x1b[6n\r\x1b[K\x1b]11;?\x1b\\\x1b[c".as_bytes(),
            |b| {
                if da_at.get().is_none() && b.windows(3).any(|w| w == b"\x1b[?") && b.contains(&b'c') {
                    da_at.set(Some(Instant::now()));
                }
                let osc = b.windows(4).position(|w| w == b"\x1b]11").is_some_and(|i| b[i..].iter().any(|&c| c == 0x07 || c == b'\\'));
                da_at.get().is_some_and(|t| osc || t.elapsed() >= Duration::from_millis(60))
            },
            300,
        );
        let r = String::from_utf8_lossy(&r);
        (parse_osc11(&r), cpr_column(&r) != Some(3))
    }

    /// Send a query and collect the reply until `done` or timeout.
    fn query(&mut self, q: &[u8], done: impl Fn(&[u8]) -> bool, ms: u64) -> Vec<u8> {
        self.pending.clear();
        write_all(q);
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(ms) {
            self.read_bytes(20);
            if done(&self.pending) {
                break;
            }
        }
        std::mem::take(&mut self.pending)
    }

    /// Ask the terminal how wide it draws each character that is two columns
    /// in Emuera but whose width terminals disagree on (Shift_JIS
    /// double-byte symbols, box drawing, Greek/Cyrillic, kana; ~800). All
    /// probes go out in one write — `\r ch CSI 6n` each — and the replies'
    /// cursor columns give the widths. Terminals that don't answer leave the
    /// table empty. `EMUERA_MEASURE=0` skips this.
    fn measure_widths(&mut self) -> std::collections::HashMap<char, u8> {
        let mut out = std::collections::HashMap::new();
        if std::env::var("EMUERA_MEASURE").as_deref() == Ok("0") {
            return out;
        }
        let chars: Vec<char> = (0xA0u32..0x3400)
            .chain(0xFF00..0xFFF0)
            .filter_map(char::from_u32)
            .filter(|c| {
                let mut b = [0u8; 4];
                let (bytes, _, bad) = encoding_rs::SHIFT_JIS.encode(c.encode_utf8(&mut b));
                !bad && bytes.len() == 2
            })
            .collect();
        let mut q = String::with_capacity(chars.len() * 8);
        for c in &chars {
            q.push('\r');
            q.push(*c);
            q.push_str("\x1b[6n");
        }
        q.push_str("\r\x1b[K");
        self.pending.clear();
        write_all(q.as_bytes());
        // collect replies until all arrived or the terminal goes quiet
        let count = |b: &[u8]| b.iter().filter(|&&x| x == b'R').count();
        let start = Instant::now();
        while count(&self.pending) < chars.len() && start.elapsed() < Duration::from_millis(1500) {
            if !self.read_bytes(150) && count(&self.pending) == 0 && start.elapsed() > Duration::from_millis(400) {
                break; // no replies at all: terminal doesn't answer
            }
        }
        let text = String::from_utf8_lossy(&std::mem::take(&mut self.pending)).to_string();
        let cols: Vec<u32> = text.split('R').filter_map(|r| r.rfind("\x1b[").and_then(|i| r[i + 2..].split(';').nth(1)).and_then(|c| c.parse().ok())).collect();
        if cols.len() == chars.len() {
            for (c, col) in chars.iter().zip(cols) {
                let w = col.saturating_sub(1).min(2) as u8;
                // 0 = drew nothing / unknown: keep the table's answer
                if w > 0 && UnicodeWidthChar::width(*c) != Some(w as usize) {
                    out.insert(*c, w);
                }
            }
        }
        out
    }

    /// Pick the [`Ambi`] strategy: print `■` and ask where the cursor went
    /// (CSI 6n). Column 3 means the terminal already draws it wide; else pad.
    /// `EMUERA_AMBIGUOUS=wide|narrow|osc66` overrides. OSC 66 is opt-in only:
    /// kitty draws box drawing stretched (lines look doubled) and blocks
    /// small with a gap, so padding looks closer to Emuera.
    fn detect_ambiguous(&mut self) -> Ambi {
        match std::env::var("EMUERA_AMBIGUOUS").as_deref() {
            Ok("wide") => return Ambi::Wide,
            Ok("narrow") => return Ambi::Pad,
            Ok("osc66") => return Ambi::Osc66,
            Ok("single") => return Ambi::Narrow,
            _ => {}
        }
        let cursor_col = |t: &mut Tty, probe: &str| -> Option<u32> {
            let r = t.query(format!("\r\x1b[K{}\x1b[6n", probe).as_bytes(), |b| b.contains(&b'R'), 300);
            write_all(b"\r\x1b[K");
            let s = String::from_utf8_lossy(&r).to_string();
            s.rfind("\x1b[").and_then(|i| s[i + 2..].trim_end_matches('R').split(';').nth(1).and_then(|c| c.parse::<u32>().ok()))
        };
        // a kanji that moves the cursor one cell: no double-width at all;
        // so does the illumos / Solaris console, which doesn't answer
        let kanji = cursor_col(self, "漢");
        if kanji == Some(2) || (kanji.is_none() && sun_console(&self.term)) {
            return Ambi::Narrow;
        }
        if cursor_col(self, "■") == Some(3) {
            Ambi::Wide
        } else {
            Ambi::Pad
        }
    }

    /// Choose the image backend: framebuffer on a VT, otherwise
    /// probe for kitty graphics, then sixel (DA1 attribute 4); also learn the
    /// cell size in pixels so images can be scaled. Returns what was chosen
    /// (and why the framebuffer wasn't), for stderr at start or the settings
    /// page later: printed while the game is on screen it would land on it.
    fn setup_gfx(&mut self, pref: &str, term: &str) -> String {
        let is_vt = text_console(term);
        let mut why = String::new();
        let mut try_fb = |t: &mut Tty| -> bool {
            match gfx::fb::Fb::open() {
                Ok(fb) => {
                    t.cell = ((fb.xres / t.cols as u32).max(1), (fb.yres / t.rows as u32).max(1));
                    t.fb_cell_fixed = true;
                    t.gfx = Box::new(fb);
                    true
                }
                Err(e) => {
                    let hint = if e.contains("ermission") { t!(" (/dev/fb0 needs the video group)") } else { String::new() };
                    why = t!("; framebuffer unavailable: {error}{hint}", error = e, hint = hint);
                    false
                }
            }
        };
        match pref {
            "none" => return t!("graphics backend: {name}", name = "none"),
            "kitty" if in_tmux() => self.gfx = Box::new(gfx::kitty::Kitty::new_tmux()),
            "kitty" => self.gfx = Box::new(gfx::kitty::Kitty::new()),
            "iterm" => self.gfx = Box::new(gfx::iterm::Iterm::new()),
            "sixel" => self.gfx = Box::new(gfx::sixel::Sixel::new()),
            "fb" => {
                try_fb(self);
            }
            _ => {
                // iTerm2 says who it is (and has no kitty graphics)
                // (WezTerm too: its kitty pictures flash as they are
                // replaced, its iTerm ones don't)
                let iterm = matches!(std::env::var("TERM_PROGRAM").as_deref(), Ok("iTerm.app") | Ok("WezTerm"))
                    || std::env::var("LC_TERMINAL").as_deref() == Ok("iTerm2");
                if is_vt {
                    try_fb(self);
                } else if iterm {
                    self.gfx = Box::new(gfx::iterm::Iterm::new());
                } else {
                    // inside tmux the kitty query goes through its
                    // passthrough (the terminal's answer may not come back:
                    // the terminal tmux runs in also says who it is through
                    // the environment); tmux answers the attributes itself
                    // (sixel if it was built with it, and it then shows
                    // sixel pictures on its own)
                    let tmux = in_tmux();
                    let passthrough = tmux && tmux_passthrough();
                    if std::env::var_os("STY").is_some() && !tmux {
                        why = t!("; inside GNU screen there are no pictures (tmux can show them)");
                    }
                    if tmux && !passthrough {
                        why = t!("; inside tmux, kitty-graphics terminals (Ghostty, kitty) need `set -g allow-passthrough all` for pictures");
                    }
                    let kitty_q: &[u8] = if passthrough {
                        b"\x1bPtmux;\x1b\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\x1b\\\x1b\\\x1b[c"
                    } else {
                        b"\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\\x1b[c"
                    };
                    let r = self.query(
                        kitty_q,
                        |b| {
                            let s = String::from_utf8_lossy(b);
                            s.contains("\x1b[?") && s.trim_end().ends_with('c')
                        },
                        400,
                    );
                    let s = String::from_utf8_lossy(&r).to_string();
                    // (terminals with kitty's Unicode placeholders)
                    let placeholders = std::env::var_os("GHOSTTY_RESOURCES_DIR").is_some() || std::env::var_os("KITTY_WINDOW_ID").is_some();
                    if passthrough && (s.contains("_Gi=31;OK") || placeholders) {
                        self.gfx = Box::new(gfx::kitty::Kitty::new_tmux());
                    } else if s.contains("_Gi=31;OK") && !tmux {
                        self.gfx = Box::new(gfx::kitty::Kitty::new());
                    } else if let Some(i) = s.find("\x1b[?") {
                        let da = &s[i + 3..];
                        let da = &da[..da.find('c').unwrap_or(da.len())];
                        if da.split(';').any(|x| x == "4") {
                            self.gfx = Box::new(gfx::sixel::Sixel::new());
                        }
                    }
                }
            }
        }
        if self.gfx.name() != "fb" && self.gfx.name() != "none" {
            let (_, _, xp, yp) = winsize();
            if xp == 0 || yp == 0 {
                // CSI 16 t -> CSI 6 ; h ; w t
                let r = self.query(b"\x1b[16t", |b| b.ends_with(b"t"), 300);
                let s = String::from_utf8_lossy(&r).to_string();
                if let Some(i) = s.find("\x1b[6;") {
                    let p: Vec<u32> = s[i + 4..].trim_end_matches('t').split(';').filter_map(|x| x.parse().ok()).collect();
                    if p.len() == 2 && p[0] > 0 && p[1] > 0 {
                        self.cell = (p[1], p[0]);
                    }
                }
            }
        }
        t!("graphics backend: {name}", name = self.gfx.name()) + &format!(" ({}x{} px){}", self.cell.0, self.cell.1, why)
    }

    /// The game's macros, read from its macro.txt the first time.
    fn macros(&mut self) -> &mut crate::macros::Macros {
        if self.macros.is_none() {
            let m = self.macro_path().map(|p| crate::macros::Macros::load(&p)).unwrap_or_default();
            self.macros = Some(m);
        }
        self.macros.as_mut().unwrap()
    }

    /// The game folder's macro.txt (Emuera keeps it beside the exe, which
    /// is in the game folder).
    fn macro_path(&self) -> Option<std::path::PathBuf> {
        let dir = &self.game.as_ref()?.0;
        Some(crate::loader::find_file_ci(dir, "macro.txt").unwrap_or_else(|| dir.join("macro.txt")))
    }

    /// F`n`: the macro into the input box; Shift+F`n`: what is typed there
    /// stored as the macro (nothing typed, nothing stored).
    fn macro_key(&mut self, n: u8, shift: bool) {
        let i = n as usize - 1;
        if !shift {
            let text = self.macros().get(i).to_string();
            self.line.set(&text);
        } else if !self.line.is_empty() {
            let text = self.line.text().to_string();
            self.macros().set(i, &text);
            self.note_macro(t!("F{n} stored", n = n));
        }
    }

    /// Is F`n` emuera-tty's own key now (macros off, or no macro stored
    /// on it in the current group)?
    fn f_key_free(&self, n: usize) -> bool {
        !self.macros_on || self.macros.as_ref().is_none_or(|m| m.get(n - 1).is_empty())
    }

    /// Alt+`g` / Ctrl+`g`: F1…F12 use group `g` now; its name shows.
    fn macro_group(&mut self, g: u8) {
        let m = self.macros();
        m.group = g as usize;
        let name = m.group_name().to_string();
        self.note_macro(name);
    }

    /// Show what a macro key did for two seconds, as Emuera's label does.
    fn note_macro(&mut self, s: String) {
        self.macro_note = Some((s, Instant::now() + Duration::from_secs(2)));
    }

    /// The next key or mouse event, waiting up to `timeout_ms`. F1…F12
    /// and Alt+digit stay macro keys while the game's macros are on, except
    /// that F3 and F5 with no macro stored (where Emuera would only empty
    /// the input line) are still the settings and the reload key; with
    /// macros off they always are.
    fn next_key(&mut self, timeout_ms: i32) -> Option<Key> {
        let k = self.next_key_raw(timeout_ms)?;
        Some(match k {
            Key::F(n @ (3 | 5), false) if self.macros_on && self.macros().get(n as usize - 1).is_empty() => {
                if n == 3 {
                    Key::Settings
                } else {
                    Key::Reload
                }
            }
            Key::F(..) | Key::Group(_) if self.macros_on => k,
            Key::F(3, false) => Key::Settings,
            Key::F(5, false) => Key::Reload,
            Key::F(..) => Key::Other,
            Key::Group(d) => {
                self.queued.push_front(Key::Char((b'0' + d) as char));
                Key::Esc
            }
            // a pad in a menu (settings, saves, a dialog): arrows, Enter, Esc
            Key::Pad(ev) if !self.pad_game => {
                use crate::gamepad::Ev;
                match ev {
                    Ev::Dir(0) => Key::Up,
                    Ev::Dir(1) => Key::Down,
                    Ev::Dir(2) => Key::Left,
                    Ev::Dir(_) => Key::Right,
                    Ev::A => Key::Enter,
                    Ev::B => Key::Esc,
                    _ => Key::Other,
                }
            }
            k => k,
        })
    }

    /// [`Tty::next_key`] for a game's input, with the keyboard pointer and
    /// the gamepad's pointer applied (`raw`: a picture menu's input).
    fn game_key(&mut self, timeout_ms: i32, raw: bool) -> Option<Key> {
        self.pad_game = true;
        let k = self.next_key(timeout_ms);
        self.pad_game = false;
        k.map(|k| self.kbd_pointer(k, raw))
    }

    /// Decode the next key or mouse event, waiting up to `timeout_ms`.
    /// Incomplete escape sequences wait briefly for their remaining bytes.
    fn next_key_raw(&mut self, timeout_ms: i32) -> Option<Key> {
        if let Some(k) = self.queued.pop_front() {
            return Some(k);
        }
        if self.pending.is_empty() && !self.read_bytes(timeout_ms) {
            return None;
        }
        if let Some(k) = self.queued.pop_front() {
            return Some(k);
        }
        if self.pending.is_empty() {
            return None;
        }
        let b = &self.pending;
        let (k, n) = match b[0] {
            0x1b => {
                if b.len() == 1 {
                    // lone ESC? wait briefly for the rest of a sequence
                    if !self.read_bytes(30) {
                        self.pending.clear();
                        return Some(Key::Esc);
                    }
                    return self.next_key(0);
                }
                let b = self.pending.clone();
                if b[1] == b'[' {
                    if b.get(2) == Some(&b'[') {
                        // the Linux console's F1–F5: ESC [ [ A … E (the second
                        // [ is no final byte here)
                        if b.len() < 4 {
                            self.read_bytes(30);
                            if self.pending.len() < 4 {
                                self.pending.clear();
                                return Some(Key::Other);
                            }
                            return self.next_key(0);
                        }
                        (
                            match b[3] {
                                c @ b'A'..=b'E' => Key::F(c - b'A' + 1, false),
                                _ => Key::Other,
                            },
                            4,
                        )
                    } else if b.get(2) == Some(&b'M') {
                        // X10 / normal mouse: ESC [ M Cb Cx Cy, each value + 32
                        if b.len() < 6 {
                            self.read_bytes(30);
                            if self.pending.len() < 6 {
                                self.pending.clear();
                                return Some(Key::Other);
                            }
                            return self.next_key(0);
                        }
                        let (cb, cx, cy) = (b[3].wrapping_sub(32) as u16, b[4].wrapping_sub(32) as u16, b[5].wrapping_sub(32) as u16);
                        (x10_mouse(cb, cx, cy), 6)
                    } else if b.get(2) == Some(&b'<') {
                        // SGR mouse: ESC [ < b ; x ; y (M|m)
                        match b.iter().position(|&c| c == b'M' || c == b'm') {
                            Some(end) => {
                                let s = String::from_utf8_lossy(&b[3..end]).to_string();
                                let p: Vec<u16> = s.split(';').filter_map(|x| x.parse().ok()).collect();
                                let k = if p.len() == 3 { Key::Mouse { btn: p[0], x: p[1], y: p[2], press: b[end] == b'M' } } else { Key::Other };
                                (k, end + 1)
                            }
                            None => {
                                self.read_bytes(30);
                                return self.next_key(0);
                            }
                        }
                    } else {
                        let end = b[2..].iter().position(|c| (0x40..=0x7e).contains(c)).map(|i| i + 2);
                        match end {
                            Some(e) => {
                                let k = match (&b[2..e], b[e]) {
                                    // urxvt 1015 mouse: ESC [ Cb ; x ; y M  (Cb + 32, decimal)
                                    (p, b'M') if p.iter().filter(|&&c| c == b';').count() == 2 => {
                                        let v: Vec<u16> = String::from_utf8_lossy(p).split(';').filter_map(|x| x.parse().ok()).collect();
                                        if v.len() == 3 {
                                            x10_mouse(v[0].saturating_sub(32), v[1], v[2])
                                        } else {
                                            Key::Other
                                        }
                                    }
                                    (p, f @ (b'A' | b'B')) if scroll_mods(p) => {
                                        if f == b'A' {
                                            Key::ScrollUp
                                        } else {
                                            Key::ScrollDown
                                        }
                                    }
                                    (p, f) if fkey(p, f).is_some() => fkey(p, f).map_or(Key::Other, |(n, s)| Key::F(n, s)),
                                    (p, f) if digit_group(p, f).is_some() => digit_group(p, f).map_or(Key::Other, Key::Group),
                                    (_, b'A') => Key::Up,
                                    (_, b'B') => Key::Down,
                                    (_, b'C') => Key::Right,
                                    (_, b'D') => Key::Left,
                                    (_, b'H') => Key::Home,
                                    (_, b'F') => Key::End,
                                    // Insert; Shift+Enter as xterm's modifyOtherKeys and
                                    // CSI-u terminals send it
                                    (p, b'~') if p == b"2" || p == b"27;2;13" => Key::Click,
                                    (p, b'u') if p == b"13;2" => Key::Click,
                                    (p, b'~') if p == b"3" => Key::Delete,
                                    (p, b'~') if p == b"5" => Key::PageUp,
                                    (p, b'~') if p == b"6" => Key::PageDown,
                                    (p, b'~') if p == b"1" || p == b"7" => Key::Home,
                                    (p, b'~') if p == b"4" || p == b"8" => Key::End,
                                    // the illumos / Solaris console's (sun-color)
                                    (p, b'z') if p == b"214" => Key::Home,
                                    (p, b'z') if p == b"216" => Key::PageUp,
                                    (p, b'z') if p == b"220" => Key::End,
                                    (p, b'z') if p == b"222" => Key::PageDown,
                                    (p, b'z') if p == b"247" => Key::Click,
                                    _ => Key::Other,
                                };
                                (k, e + 1)
                            }
                            None => (Key::Other, b.len()),
                        }
                    }
                } else if b[1] == b'O' && b.len() >= 3 {
                    (
                        match b[2] {
                            c @ b'P'..=b'S' => Key::F(c - b'P' + 1, false),
                            b'A' => Key::Up,
                            b'B' => Key::Down,
                            b'C' => Key::Right,
                            b'D' => Key::Left,
                            // rxvt's Ctrl+Up / Down
                            b'a' => Key::ScrollUp,
                            b'b' => Key::ScrollDown,
                            // Konsole's Shift+Enter (and the keypad's Enter)
                            b'M' => Key::Click,
                            _ => Key::Other,
                        },
                        3,
                    )
                } else if b[1].is_ascii_digit() {
                    // Alt+digit (sent as ESC and the digit)
                    (Key::Group(b[1] - b'0'), 2)
                } else if matches!(b[1], b']' | b'P' | b'_' | b'^' | b'X') {
                    // a terminal's string reply (OSC, DCS, APC…) that came
                    // after its query gave up: skip it whole, to BEL or ESC \
                    let end = |b: &[u8]| {
                        b[2..].iter().position(|&c| c == 0x07).map(|i| i + 3).or_else(|| b[2..].windows(2).position(|w| w == b"\x1b\\").map(|i| i + 4))
                    };
                    if end(&self.pending).is_none() {
                        self.read_bytes(30);
                        if end(&self.pending).is_none() {
                            self.pending.clear();
                            return Some(Key::Other);
                        }
                        return self.next_key(0);
                    }
                    (Key::Other, end(&b).unwrap())
                } else {
                    (Key::Esc, 1)
                }
            }
            b'\r' | b'\n' => (Key::Enter, 1),
            0x7f | 0x08 => (Key::Backspace, 1),
            0x03 => (Key::CtrlC, 1),
            0x0c => (Key::CtrlL, 1),
            0x0f => (Key::Settings, 1),
            0x14 => (Key::Title, 1),
            0x12 => (Key::Restart, 1),
            0x05 => (Key::Reload, 1),
            0x09 => (Key::Skip, 1),
            // Ctrl-U: clear the line being typed
            0x15 => (Key::Char('\u{15}'), 1),
            c if c < 0x20 => (Key::Other, 1),
            c => {
                let len = if c >= 0xf0 {
                    4
                } else if c >= 0xe0 {
                    3
                } else if c >= 0xc0 {
                    2
                } else {
                    1
                };
                if b.len() < len {
                    self.read_bytes(30);
                    if self.pending.len() < len {
                        self.pending.clear();
                        return Some(Key::Other);
                    }
                }
                let s = String::from_utf8_lossy(&self.pending[..len]).to_string();
                (Key::Char(s.chars().next().unwrap_or('?')), len)
            }
        };
        let n = n.min(self.pending.len());
        self.pending.drain(..n);
        Some(k)
    }

    // ------------------------------------------------------------ layout
    /// Wrap one console line into screen rows at the width it was printed
    /// with (`Line::cols`) and apply its alignment. Rows wider than the
    /// current screen are clipped by `render`. Buttons get ids (indices into
    /// `self.buttons`) only if the line belongs to the current input generation.
    fn layout_line(&mut self, line: &Line, cur_gen: u64, scale: f32) -> Vec<DRow> {
        let screen = self.cols.max(1);
        self.layout_line_in(line, cur_gen, scale, screen)
    }

    /// Lay out a line; `align_w` is the width center / right alignment
    /// works in (the screen, or a `<div>`).
    fn layout_line_in(&mut self, line: &Line, cur_gen: u64, scale: f32, align_w: usize) -> Vec<DRow> {
        let width = if line.cols > 0 { line.cols } else { self.cols.max(1) };
        let mut rows = vec![DRow::default()];
        let cw = self.cell.0.max(1) as f32;
        let mut col = 0usize;
        let active = line.gen == cur_gen;
        let push_btn = |v: &Option<Value>, buttons: &mut Vec<Value>| -> Option<usize> {
            match v {
                Some(v) if active => {
                    buttons.push(v.clone());
                    Some(buttons.len() - 1)
                }
                _ => None,
            }
        };
        // shapes are measured in Emuera's text grid (a cell is half the
        // font size, as games count them: 50% of the font is one half-width
        // character) and placed at their pixel positions: each one ends
        // where its exact right edge rounds to, the rest carried to the
        // next, so shapes in a row (a bar's filled and empty parts) keep
        // their total width and what follows lines up
        let mut frac: Option<f32> = None;
        for (si, seg) in line.segs.iter().enumerate() {
            if !matches!(seg, Seg::Rect { .. } | Seg::Space { .. }) {
                frac = None;
            }
            match seg {
                Seg::Text { text, style, button, .. } => {
                    // Emuera's "ボタンの途中で行を折りかえさない": a button that
                    // doesn't fit the rest of the row starts the next one
                    // whole (its pieces in other colours count with it)
                    let starts_button =
                        button.is_some() && !matches!(si.checked_sub(1).and_then(|p| line.segs.get(p)), Some(Seg::Text { button: b, .. }) if b == button);
                    if self.button_wrap && starts_button && col > 0 {
                        let mut w = 0;
                        for s2 in &line.segs[si..] {
                            match s2 {
                                Seg::Text { text: t2, button: b2, .. } if b2 == button => {
                                    w += letter_widths(t2, self.letters)
                                        .map(|(c, wl)| {
                                            let (tw, extra, _) =
                                                if c == '\t' { (4, 0, ' ') } else { char_cells(c, self.ambi, self.widths.get(&c).copied(), wl) };
                                            tw + extra
                                        })
                                        .sum::<usize>();
                                }
                                _ => break,
                            }
                        }
                        if col + w > width && w <= width {
                            rows.push(DRow::default());
                            col = 0;
                        }
                    }
                    let bid = push_btn(button, &mut self.buttons);
                    // the cell (in the last row) a combining mark belongs to:
                    // to start with, the previous run's last (a mark printed
                    // in another colour than its base)
                    let mut base: Option<usize> = rows.last().and_then(|r| r.cells.len().checked_sub(1));
                    for (ch, wl) in letter_widths(text, self.letters) {
                        let ch = if self.yen && ch == '\\' { '¥' } else { ch };
                        let (tw, extra, fill) = if ch == '\t' { (4, 0, ' ') } else { char_cells(ch, self.ambi, self.widths.get(&ch).copied(), wl) };
                        let (ch, tw, extra, fill) = self.drawable(ch, tw, extra, fill);
                        let w = tw + extra;
                        if tw == 0 {
                            if let (Some(i), true) = (base, is_mark(ch)) {
                                add_mark(&mut rows.last_mut().unwrap().cells[i], ch);
                            }
                            continue;
                        }
                        if col + w > width {
                            rows.push(DRow::default());
                            col = 0;
                        }
                        let r = rows.last_mut().unwrap();
                        base = (ch != '\t').then_some(r.cells.len());
                        if ch == '\t' {
                            for _ in 0..4 {
                                r.cells.push(Cell { ch: ' ', w: 1, style: style.clone(), button: bid, marks: None });
                            }
                        } else {
                            r.cells.push(Cell { ch, w: tw as u8, style: style.clone(), button: bid, marks: None });
                            for _ in 0..extra {
                                r.cells.push(Cell { ch: fill, w: 1, style: style.clone(), button: bid, marks: None });
                            }
                        }
                        col += w;
                    }
                }
                Seg::Image { name, w, h, ypos, button, hover } => {
                    let bid = push_btn(button, &mut self.buttons);
                    if let (Some(b), Some(_)) = (bid, hover) {
                        self.hover_imgs.insert(b);
                    }
                    let cells = ((w * scale) / cw).round().max(1.0) as usize;
                    if col + cells > width && col > 0 {
                        rows.push(DRow::default());
                        col = 0;
                    }
                    let r = rows.last_mut().unwrap();
                    r.imgs.push(ImgAt { name: name.clone(), button: bid, hover: hover.clone(), col, w: *w, h: *h, ypos: *ypos });
                    let st = Style { fg: Rgb(90, 90, 90), bg: None, bold: false, italic: false, underline: false, strike: false };
                    let fill = if self.gfx.name() == "none" { '░' } else { ' ' };
                    for _ in 0..cells.min(width) {
                        r.cells.push(Cell { ch: fill, w: 1, style: st.clone(), button: bid, marks: None });
                    }
                    col += cells.min(width);
                }
                Seg::Space { w } => {
                    let cells = shape_cells(col, &mut frac, w / self.text_px, false);
                    let r = rows.last_mut().unwrap();
                    let st = Style { fg: Rgb(0, 0, 0), bg: None, bold: false, italic: false, underline: false, strike: false };
                    for _ in 0..cells.min(width.saturating_sub(col)) {
                        r.cells.push(Cell { ch: ' ', w: 1, style: st.clone(), button: None, marks: None });
                    }
                    col = (col + cells).min(width);
                }
                Seg::Rect { w, color, button, .. } => {
                    let bid = push_btn(button, &mut self.buttons);
                    let mut cells = shape_cells(col, &mut frac, w / self.text_px, true);
                    if col + cells > width && col > 0 {
                        rows.push(DRow::default());
                        col = 0;
                        frac = None;
                        cells = shape_cells(col, &mut frac, w / self.text_px, true);
                    }
                    let r = rows.last_mut().unwrap();
                    let st = Style { fg: *color, bg: None, bold: false, italic: false, underline: false, strike: false };
                    for _ in 0..cells.min(width) {
                        r.cells.push(Cell { ch: '█', w: 1, style: st.clone(), button: bid, marks: None });
                    }
                    col += cells.min(width);
                }
                Seg::Div { dx, dy, cols, rows: max_rows, lines, deco, .. } => {
                    let ins = deco.insets();
                    let in_w = cols.saturating_sub(ins[1] + ins[3]).max(1);
                    let mut inner = Vec::new();
                    for l in lines {
                        inner.extend(self.layout_line_in(l, cur_gen, scale, in_w));
                    }
                    if let Some(m) = max_rows {
                        inner.truncate(m.saturating_sub(ins[0] + ins[2]).max(1));
                    }
                    if deco.bg.is_some() || deco.border.iter().any(|b| *b) || ins.iter().any(|n| *n > 0) {
                        inner = boxed_div(inner, deco, *cols, *max_rows);
                    }
                    let at = (col as i64 + *dx as i64).max(0) as usize;
                    rows.last_mut().unwrap().divs.push(DivAt { col: at, dy: *dy, rows: inner });
                }
            }
        }
        // alignment, within the screen (or div) as it is now
        if line.align != Align::Left {
            let screen = align_w.max(1);
            for r in rows.iter_mut() {
                let used: usize = r.cells.iter().map(|c| c.w as usize).sum();
                let pad = match line.align {
                    Align::Center => (screen.saturating_sub(used)) / 2,
                    _ => screen.saturating_sub(used),
                };
                if pad > 0 {
                    let st = Style { fg: Rgb(0, 0, 0), bg: None, bold: false, italic: false, underline: false, strike: false };
                    let mut nc: Vec<Cell> = (0..pad).map(|_| Cell { ch: ' ', w: 1, style: st.clone(), button: None, marks: None }).collect();
                    nc.append(&mut r.cells);
                    r.cells = nc;
                    for im in r.imgs.iter_mut() {
                        im.col += pad;
                    }
                    for d in r.divs.iter_mut() {
                        d.col += pad;
                    }
                }
            }
        }
        rows
    }

    /// Emit the SGR sequence for a style in the terminal's colour mode.
    /// The background is the text's own, else the window's (SETBGCOLOR);
    /// the foreground is made readable against it (see `contrast`).
    fn sgr(&self, out: &mut Vec<u8>, st: &Style, fg_override: Option<Rgb>) {
        let bg = st.bg.map(|c| self.recolored(c)).or(self.screen_bg);
        // where the game sets no background, the terminal's shows: the game
        // meant its colours for its own default (Emuera's black)
        let (shown, meant) = match bg {
            Some(b) => (b, b),
            None => (self.term_bg.unwrap_or(Rgb(0, 0, 0)), self.game_bg),
        };
        let fg = crate::console::readable_on(self.recolored(fg_override.unwrap_or(st.fg)), shown, meant, self.contrast);
        out.extend_from_slice(b"\x1b[0");
        if st.bold {
            out.extend_from_slice(b";1");
        }
        if st.italic {
            out.extend_from_slice(b";3");
        }
        if st.underline {
            out.extend_from_slice(b";4");
        }
        if st.strike {
            out.extend_from_slice(b";9");
        }
        match self.color {
            ColorMode::True => {
                let _ = write!(out, ";38;2;{};{};{}", fg.0, fg.1, fg.2);
                if let Some(b) = bg {
                    let _ = write!(out, ";48;2;{};{};{}", b.0, b.1, b.2);
                }
            }
            // fbterm: its own colour escapes after the attributes (terminfo
            // setaf=\E[1;%p1%d}, setab=\E[2;%p1%d})
            ColorMode::C256 if self.fbterm => {
                let _ = write!(out, "m\x1b[1;{}}}", rgb256(fg));
                if let Some(b) = bg {
                    let _ = write!(out, "\x1b[2;{}}}", rgb256(b));
                }
                return;
            }
            ColorMode::C256 => {
                let _ = write!(out, ";38;5;{}", rgb256(fg));
                if let Some(b) = bg {
                    let _ = write!(out, ";48;5;{}", rgb256(b));
                }
            }
            ColorMode::C16 => {
                let (c, bright) = rgb16(fg);
                let _ = write!(out, ";{}", if bright && self.bright_codes { 90 + c } else { 30 + c });
                // the Linux console and fbterm brighten via bold, not 90-97
                if bright {
                    out.extend_from_slice(b";1");
                }
                if let Some(b) = bg {
                    let _ = write!(out, ";{}", 40 + rgb16_bg(b));
                }
            }
        }
        out.push(b'm');
    }

    /// Reset attributes to the window background and clear to the end of
    /// the line with it.
    fn clear_eol(&self, out: &mut Vec<u8>) {
        match self.screen_bg {
            None => out.extend_from_slice(b"\x1b[0m\x1b[K"),
            Some(b) => {
                let _ = match self.color {
                    ColorMode::True => write!(out, "\x1b[0;48;2;{};{};{}m\x1b[K", b.0, b.1, b.2),
                    ColorMode::C256 if self.fbterm => write!(out, "\x1b[0m\x1b[2;{}}}\x1b[K", rgb256(b)),
                    ColorMode::C256 => write!(out, "\x1b[0;48;5;{}m\x1b[K", rgb256(b)),
                    ColorMode::C16 => write!(out, "\x1b[0;{}m\x1b[K", 40 + rgb16_bg(b)),
                };
            }
        }
    }

    /// Draw a full frame: text rows, images, the prompt / status line.
    fn render(&mut self, con: &mut Console, res: &mut Resources, req: Option<&InputReq>) {
        // the window background, unless it is the game's default (then the
        // terminal's own background shows)
        self.screen_bg = (con.screen_bg != con.default_bg).then_some(self.recolored(con.screen_bg));
        self.game_bg = self.recolored(con.default_bg);
        let (cols, rows, xp, yp) = winsize();
        self.cols = cols;
        self.rows = rows.max(2);
        if !self.fb_cell_fixed && xp > 0 && yp > 0 {
            self.cell = ((xp / cols as u32).max(1), (yp / rows as u32).max(1));
        }
        con.width = cols.saturating_sub(1).max(20);
        self.button_wrap = con.button_wrap;
        self.text_px = (con.font_size / 2.0).max(1.0);
        self.yen = match self.yen_mode.as_str() {
            "on" => true,
            "off" => false,
            _ => backslash_is_yen(&con.default_font),
        };
        self.letters = match self.letters_mode.as_str() {
            "wide" => Letters::Wide,
            "narrow" => Letters::Narrow,
            _ if letters_are_wide(&con.default_font) => Letters::ByWord,
            _ => Letters::Narrow,
        };
        let scale = self.cell.1 as f32 / con.line_height.max(1.0);
        // Reserve 2 rows: top bar (row 1) and bottom bar (row self.rows)
        let view_h = self.rows.saturating_sub(2).max(1);
        self.buttons.clear();
        self.hover_imgs.clear();
        // layout from the bottom up
        let mut all: Vec<DRow> = Vec::new();
        // (plus some rows above the view: images there can reach into it)
        let need = view_h + self.scroll + IMG_REACH;
        let cur_line = if con.cur.is_empty() {
            None
        } else {
            Some(Line {
                segs: con.cur.clone(),
                align: con.cur_align.unwrap_or(con.align),
                gen: con.gen,
                temporary: false,
                cols: self.cols,
                kojo: false,
                src: 0,
                wait: con.waits,
            })
        };
        let mut chunks: Vec<Vec<DRow>> = Vec::new();
        // for each chunk: the key of its line if it is translated (or could
        // be shown so), for clicking between the original and the translation
        let mut chunk_keys: Vec<Option<u64>> = Vec::new();
        // and the key of every line while translating, to force one
        let mut chunk_all: Vec<Option<u64>> = Vec::new();
        let mut count = 0;
        if let Some(l) = &cur_line {
            let r = self.layout_line(l, con.valid_gen, scale);
            count += r.len();
            chunks.push(r);
            chunk_keys.push(None);
            chunk_all.push(None);
        }
        // dialog translated in batches (the lines that may show)
        let blocks = if self.mt.is_some() { self.translate_blocks(con, con.lines.len().saturating_sub(need)) } else { crate::fxhash::HashMap::default() };
        for (i, l) in con.lines.iter().enumerate().rev() {
            if count >= need {
                break;
            }
            let all_key = self.mt.is_some().then(|| line_key(l));
            let tr = match blocks.get(&i) {
                Some(t) => Some(t.clone()),
                None if self.mt.is_some() => {
                    let force = all_key.is_some_and(|k| self.forced.contains(&k));
                    self.translated(l, &con.src_files, force)
                }
                None => None,
            };
            // a translated line the player clicked shows its original again
            let key = tr.as_ref().map(|_| line_key(l));
            let tr = match key {
                Some(k) if self.show_orig.contains(&k) => None,
                _ => tr,
            };
            let r = self.layout_line(tr.as_ref().unwrap_or(l), con.valid_gen, scale);
            count += r.len();
            chunks.push(r);
            chunk_keys.push(key);
            chunk_all.push(all_key);
        }
        let mut row_keys: Vec<Option<u64>> = Vec::new();
        let mut row_all: Vec<Option<u64>> = Vec::new();
        for ((c, k), a) in chunks.into_iter().zip(chunk_keys).zip(chunk_all).rev() {
            row_keys.extend(std::iter::repeat_n(k, c.len()));
            row_all.extend(std::iter::repeat_n(a, c.len()));
            all.extend(c);
        }
        // <div> content: painted over the rows it reaches (Emuera draws a
        // div on top of whatever lines are there)
        for idx in 0..all.len() {
            if all[idx].divs.is_empty() {
                continue;
            }
            for d in std::mem::take(&mut all[idx].divs) {
                for (k, r) in d.rows.into_iter().enumerate() {
                    let t = idx as i64 + d.dy as i64 + k as i64;
                    if t < 0 || t as usize >= all.len() {
                        continue;
                    }
                    let t = t as usize;
                    for mut im in r.imgs {
                        im.col += d.col;
                        all[t].imgs.push(im);
                    }
                    paint_cells(&mut all[t], d.col, r.cells);
                }
            }
        }
        // a picture menu (CBG layers) may draw anywhere on the screen:
        // give it rows to draw text into below short content
        if !res.cbg.is_empty() && self.scroll == 0 {
            while all.len() < view_h {
                all.push(DRow::default());
            }
        }
        let max_scroll = all.len().saturating_sub(view_h);
        if self.scroll > max_scroll {
            self.scroll = max_scroll;
        }
        let end = all.len() - self.scroll.min(all.len());
        let start = end.saturating_sub(view_h);
        let cbg_places = if res.cbg.is_empty() { Vec::new() } else { self.cbg_layers(con, res, &mut all[start..end]) };
        let visible = &all[start..end];
        self.row_lines = (start..end).map(|i| row_keys.get(i).copied().flatten()).collect();
        self.row_all = (start..end).map(|i| row_all.get(i).copied().flatten()).collect();

        let mut out: Vec<u8> = Vec::with_capacity(64 * 1024);
        out.extend_from_slice(b"\x1b[?25l");
        self.gfx.begin(&mut out);
        // hover button id (from the previous frame's map; ids are stable)
        let hover = self.mouse.and_then(|(x, y)| self.button_at(x, y));
        self.hover = hover;
        self.btn_map = vec![Vec::new(); self.rows];
        self.frame = vec![Vec::new(); self.rows];
        let mut placements = cbg_places;
        // the console mouse (gpm or the raw device) has no pointer of its
        // own on screen: draw one
        // (blinking: off half the time)
        let pointer = if self.own_pointer() && self.ptr_on { self.mouse } else { None };
        let mut pointer_drawn = false;
        // Draw the top bar at row 1
        self.draw_top_bar(&mut out);
        // each row's bytes, for damage tracking (see `skip_unchanged_rows`)
        let mut row_bytes: Vec<(usize, usize)> = Vec::with_capacity(view_h);
        let rows_from = out.len();
        for (sr, row) in visible.iter().enumerate() {
            let row_start = out.len();
            // sr + 2 because row 1 is the top bar (terminal row), screen row is sr+1
            let _ = write!(out, "\x1b[{};1H", sr + 2);
            let mut col = 0usize;
            let mut last_style: Option<(Style, Option<Rgb>)> = None;
            let mut btn_start: Option<(usize, usize)> = None;
            for cell in &row.cells {
                // clip rows printed when the screen was wider
                if col + cell.w as usize > cols {
                    break;
                }
                if let Some((s, b)) = btn_start {
                    if cell.button != Some(b) {
                        self.btn_map[sr + 1].push((s, col, b));
                        btn_start = None;
                    }
                }
                if let Some(b) = cell.button {
                    if btn_start.is_none() {
                        btn_start = Some((col, b));
                    }
                }
                let ov = match (cell.button, hover) {
                    (Some(b), Some(h)) if b == h && self.is_button(b) => Some(con.focus_fg),
                    _ => None,
                };
                let key = (cell.style.clone(), ov);
                if last_style.as_ref() != Some(&key) {
                    self.sgr(&mut out, &cell.style, ov);
                    last_style = Some(key);
                }
                self.frame[sr + 1].push((col, cell.clone(), ov));
                let mut tmp = [0u8; 4];
                let at_pointer = pointer.is_some_and(|(px, py)| py == sr + 1 && px >= col && px < col + cell.w.max(1) as usize);
                if at_pointer {
                    if cell.ch.is_whitespace() {
                        let blank = Style { fg: self.pointer_fg(cell.style.bg), ..cell.style.clone() };
                        self.sgr(&mut out, &blank, None);
                        last_style = None;
                    }
                    out.extend_from_slice(b"\x1b[7m");
                    pointer_drawn = true;
                }
                let marks = cell.marks.as_deref().unwrap_or("");
                if self.ambi == Ambi::Osc66 && cell.w == 2 && UnicodeWidthChar::width(cell.ch) == Some(1) {
                    // a one-column glyph drawn across two cells (see char_cells)
                    let _ = write!(out, "\x1b]66;w=2;{}{}\x1b\\", cell.ch, marks);
                } else if self.ascii_only && !cell.ch.is_ascii() {
                    self.put_ascii(&mut out, cell.ch, cell.w as usize);
                } else {
                    out.extend_from_slice(cell.ch.encode_utf8(&mut tmp).as_bytes());
                    out.extend_from_slice(marks.as_bytes());
                }
                if at_pointer {
                    out.extend_from_slice(b"\x1b[27m");
                }
                col += cell.w as usize;
            }
            if let Some((s, b)) = btn_start {
                self.btn_map[sr + 1].push((s, col, b));
            }
            self.clear_eol(&mut out);
            row_bytes.push((row_start, out.len()));
        }
        // images: placed by pixel, so a picture can start rows above or
        // below its own line (negative `ypos` stacks portrait layers, each
        // printed on the next line, back onto the first one) and is cropped
        // to the view. The top bar occupies cell row 0, so visible content
        // starts at cell row 1 (pixel `ch`).
        let (ch, view_px) = (self.cell.1.max(1) as i32, (view_h as u32 * self.cell.1.max(1)) as i32);
        let view_top_px = ch; // pixel coordinate of first visible cell row (row 1)
        let view_bottom_px = ch + view_px; // pixel coordinate just past last visible cell row
        let mut pending: Vec<(String, u32, u32, i32, i32, u32, u32)> = Vec::new();
        for idx in start.saturating_sub(IMG_REACH)..all.len().min(end + IMG_REACH) {
            let sr = idx as i32 - start as i32;
            for im in all[idx].imgs.iter().filter(|im| im.col < cols) {
                let pw = (im.w * scale).round().max(1.0) as u32;
                let ph = (im.h * scale).round().max(1.0) as u32;
                // sr=0 corresponds to cell row 1 (pixel `ch`), so add `ch` to top
                let top = sr * ch + (im.ypos * scale).round() as i32 + ch;
                if top + ph as i32 <= view_top_px || top >= view_bottom_px {
                    continue;
                }
                let crop_top = (view_top_px - top).max(0) as u32;
                let top = top.max(view_top_px);
                let crop_h = (ph - crop_top).min((view_bottom_px - top) as u32);
                if crop_h == 0 {
                    continue;
                }
                // a picture in a button: all of it is the button (hover,
                // tooltip, click), as in Emuera, not just its text row
                if let Some(b) = im.button {
                    let cw = self.cell.0.max(1) as usize;
                    let (c0, c1) = (im.col, (im.col + (pw as usize).div_ceil(cw)).min(cols));
                    // visible rows in btn_map are 1..=view_h (0 is top bar)
                    let (r0, r1) = (top as usize / ch as usize, (top as usize + crop_h as usize).div_ceil(ch as usize).min(view_h + 1));
                    for r in r0..r1 {
                        if let Some(m) = self.btn_map.get_mut(r) {
                            m.push((c0, c1, b));
                        }
                    }
                }
                // images off (or hidden under the overlay): nothing is decoded
                // or scaled, so no picture is read until a backend is chosen
                if self.no_images || self.gfx.name() == "none" {
                    continue;
                }
                // srcb: the hover picture while the pointer is on its button
                let name = match &im.hover {
                    Some(h) if im.button.is_some() && im.button == self.hover => h,
                    _ => &im.name,
                };
                pending.push((res.frame_name(name), pw, ph, im.col as i32, top, crop_top, crop_h));
            }
        }
        // pictures not scaled yet are scaled on all cores at once (a status
        // screen shows several large portrait layers)
        res.prescale(pending.iter().map(|p| (p.0.as_str(), p.1, p.2)).collect());
        // an animated sprite on screen: the input wait redraws it
        self.animating = res.take_anim_used();
        for (name, pw, ph, col, top, crop_top, crop_h) in pending {
            if let Some(img) = res.scaled(&name, pw, ph) {
                placements.push(Placement {
                    key: format!("{}@{}x{}#{}", name, pw, ph, res.version(&name)),
                    img,
                    col,
                    row: top / ch,
                    y_off: top % ch,
                    crop_top,
                    crop_h,
                    below: false,
                });
            }
        }
        for sr in visible.len()..view_h {
            let row_start = out.len();
            // sr + 2 because row 1 is the top bar
            let _ = write!(out, "\x1b[{};1H", sr + 2);
            self.clear_eol(&mut out);
            row_bytes.push((row_start, out.len()));
        }
        let rows_to = out.len();
        if let (Some((px, py)), false) = (pointer, pointer_drawn) {
            // pointer over blank space (visible content is at screen rows 1..view_h, terminal rows 2..view_h+1)
            if py >= 1 && py <= view_h {
                let _ = write!(out, "\x1b[{};{}H", py + 1, px + 1);
                let blank = Style { fg: self.pointer_fg(None), bg: None, bold: false, italic: false, underline: false, strike: false };
                self.sgr(&mut out, &blank, None);
                out.extend_from_slice(b"\x1b[7m \x1b[0m");
            }
        }
        // status / input line
        let _ = write!(out, "\x1b[{};1H", self.rows);
        self.clear_eol(&mut out);
        let prompt = match req {
            Some(r) if r.kind == InputKind::Wait => "\x1b[2m▼ (Enter)\x1b[0m".to_string(),
            Some(_) if self.line.is_empty() && self.notice.is_some() => format!("\x1b[1m>\x1b[0m \x1b[2m({})\x1b[0m", self.notice.as_deref().unwrap_or("")),
            // a recalled input is selected: typing replaces it
            Some(_) if self.line.selected() => format!("\x1b[1m>\x1b[0m \x1b[7m{}\x1b[0m", self.line.text()),
            Some(_) => format!("\x1b[1m>\x1b[0m {}", self.line.text()),
            None => String::new(),
        };
        out.extend_from_slice(prompt.as_bytes());
        let note = self.macro_note.as_ref().filter(|_| req.is_some()).map(|(s, _)| format!("  [{}]", s)).unwrap_or_default();
        if !note.is_empty() {
            let _ = write!(out, "\x1b[2m{}\x1b[0m", note);
        }
        if req.is_some() {
            self.busy_since = None;
        }
        self.bar = None;
        if self.scroll == 0 && cols > 40 && req.is_some() && !self.no_images {
            let used = 2 + self.line.cols() + note.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum::<usize>() + 20;
            self.bar = Some((used, con.running.clone(), 0));
            self.draw_bar(&mut out, hover);
        }
        if self.scroll > 0 {
            let tag = format!(" [{}] ", t!("scroll -{n}", n = self.scroll));
            let _ = write!(out, "\x1b[{};{}H\x1b[7m{}\x1b[0m", self.rows, cols.saturating_sub(crate::i18n::width(&tag)), tag);
        }
        // overlapping layers (portraits) go to the terminal as one picture
        let mut placements = self.merger.merge(placements, self.cell, self.gfx.name() == "kitty-tmux");
        // buttons with tooltips need a full redraw when hovered, and the
        // hovered one shows its tooltip by the pointer
        for (i, v) in self.buttons.iter().enumerate() {
            if con.tooltip_for(v).is_some() {
                self.hover_imgs.insert(i);
            }
        }
        // the tooltip shows once the pointer has rested on its button for
        // TOOLTIP_SETDELAY, until TOOLTIP_SETDURATION has gone by
        let has_tip = |b: usize| self.buttons.get(b).and_then(|v| con.tooltip_for(v)).is_some();
        match (hover.filter(|&b| has_tip(b)), self.tip_since) {
            (Some(b), Some((t, _))) if t == b => {}
            (Some(b), _) => self.tip_since = Some((b, Instant::now())),
            (None, _) => self.tip_since = None,
        }
        let tip = match (hover, self.mouse) {
            (Some(b), Some((mx, my))) if self.tip_due(con).0 => self.buttons.get(b).and_then(|v| con.tooltip_for(v)).map(|t| self.tooltip_box(t, mx, my)),
            _ => None,
        };
        self.tip_drawn = tip.is_some();
        // pictures sit above text: the tooltip's place is cut out of them
        // (not out of tmux's placeholders, which are text the tooltip
        // overwrites anyway: a cut picture would be a new one, sent whole
        // through tmux, and the portrait would vanish until it arrived)
        if let (Some(t), false) = (&tip, self.gfx.name() == "kitty-tmux") {
            let cells = (t.left as i64 - 1, t.top as i64 - 1, (t.left + t.w) as i64 - 1, (t.top + t.lines.len()) as i64 - 1);
            placements = placements.into_iter().map(|p| self.merger.cut(p, cells, self.cell)).collect();
        }
        if self.force_frame {
            self.gfx.reset(&mut out);
        }
        // pictures not yet in the terminal go before the text (see `prepare`)
        let mut pre = Vec::new();
        self.gfx.prepare(&mut pre, &placements, self.cell);
        self.gfx.draw(&mut out, &placements, self.cell);
        let mut tip_rows = 0..0;
        if let Some(t) = &tip {
            tip_rows = self.draw_tooltip(&mut out, t, con.tip_colors);
        }
        self.drawn_pointer = pointer.filter(|p| p.1 < view_h);
        if self.mouse_log.is_some() {
            let (m, d, own) = (self.mouse, self.drawn_pointer, self.own_pointer());
            self.mlog(&format!("frame: mouse={:?} pointer drawn at {:?} (ours={} on={})", m, d, own, self.ptr_on));
        }
        self.frame_cursor = None;
        // the cursor where the input line's is
        if req.map(|r| r.kind != InputKind::Wait).unwrap_or(false) {
            let w = self.line.cursor_cols();
            let _ = write!(out, "\x1b[{};{}H\x1b[?25h", self.rows, 3 + w);
            self.frame_cursor = Some((self.rows, 3 + w));
        }
        // a frame exactly like the last one (a script redrawing a picture
        // menu every few milliseconds) isn't sent again
        let fp = {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            out.hash(&mut h);
            for p in &placements {
                p.key.hash(&mut h);
            }
            // (a pointer drawn as an arrow isn't in the text)
            self.drawn_pointer.hash(&mut h);
            h.finish()
        };
        // (unless a box drawn over it has closed: what it covered must
        // come back)
        if fp == self.last_frame && !self.force_frame && !self.box_closed.get() {
            if !pre.is_empty() {
                write_all(&pre);
            }
            self.frame_imgs = placements;
            con.dirty = false;
            return;
        }
        let out = self.skip_unchanged_rows(out, (rows_from, rows_to), &row_bytes, &placements, cols, tip_rows);
        if std::env::var_os("EMUERA_DEBUG_FRAMES").is_some() {
            eprintln!("frame {:x} len {} keys {:?}", fp, out.len(), placements.iter().map(|p| p.key.clone()).collect::<Vec<_>>());
        }
        self.last_frame = fp;
        self.force_frame = false;
        // as one synchronized update (DEC mode 2026: kitty, WezTerm, foot,
        // Ghostty, Konsole, Windows Terminal…; others ignore it), so a
        // picture replaced by another is never shown half done or gone
        let console = text_console(&self.term);
        if !console {
            write_all(b"\x1b[?2026h");
        }
        if !pre.is_empty() {
            write_all(&pre);
        }
        write_all(&out);
        if !console {
            write_all(b"\x1b[?2026l");
        }
        crate::sys::drain_stdout();
        self.gfx.after_flush(&placements, self.cell);
        self.pointer_over_images(&placements);
        self.frame_imgs = placements;
        con.dirty = false;
    }

    /// The client background layers (CBG*): as images where the terminal
    /// shows them, else the text drawn into them (GDRAWTEXT) painted into
    /// the screen `rows`, so a menu drawn as a picture still works.
    /// Returns their image placements, back to front.
    fn cbg_layers(&mut self, con: &Console, res: &mut Resources, rows: &mut [DRow]) -> Vec<Placement> {
        use crate::resources::CbgSrc;
        let cw = (con.font_size / 2.0).max(1.0);
        let lh = con.line_height.max(1.0);
        let client_h = self.rows as f32 * lh;
        // the button under the mouse (button map colour)
        let hovered = self.mouse.and_then(|(c, r)| {
            let x = (c as f32 * cw + cw / 2.0) as i64;
            let y = (r as f32 * lh + lh / 2.0 - client_h) as i64;
            res.cbg_button_at(x, y)
        });
        let images = self.gfx.name() != "none" && !self.no_images;
        let (sx, sy) = (self.cell.0 as f32 / cw, self.cell.1 as f32 / lh);
        let screen_px = (self.rows as u32 * self.cell.1.max(1)) as i32;
        let mut out = Vec::new();
        for l in res.cbg.clone() {
            let src = match &l.button {
                Some((v, hover)) if hovered == Some(*v) => match hover {
                    Some(h) => h.clone(),
                    None => continue,
                },
                _ => l.src.clone(),
            };
            if let CbgSrc::Sprite(n) = &src {
                if n.is_empty() {
                    continue;
                }
            }
            if !images {
                // (only the size is needed: no pixel fingerprint)
                let CbgSrc::G(gid) = src else { continue };
                let Some(g) = res.graphics.get(&gid) else { continue };
                let top_e = client_h + l.y as f32 - g.height() as f32;
                // text fallback: what GDRAWTEXT wrote into the buffer
                for t in &g.texts {
                    // text drawn larger than the console font takes more
                    // pixels per character than a cell: keep its middle
                    // where it was (menus are centred), not its left edge
                    let text = t.text.trim();
                    let lead = t.text.len() - t.text.trim_start().len();
                    let half = t.size as f32 / 2.0; // px per half-width char
                    let w_px = crate::loader::sjis_len(text) as f32 * half;
                    let mid = l.x as f32 + t.x as f32 + lead as f32 * half + w_px / 2.0;
                    let st = Style {
                        fg: Rgb(t.color[0], t.color[1], t.color[2]),
                        bg: None,
                        bold: t.size as f32 > con.font_size,
                        italic: false,
                        underline: false,
                        strike: false,
                    };
                    let cells = self.text_cells(text, &st);
                    let width: usize = cells.iter().map(|c| c.w as usize).sum();
                    let col = (mid / cw - width as f32 / 2.0).round().max(0.0);
                    let row = ((top_e + t.y as f32 + t.size as f32 / 2.0) / lh).floor();
                    if row < 0.0 || row as usize >= rows.len() || text.is_empty() {
                        continue;
                    }
                    paint_cells(&mut rows[row as usize], col as usize, cells);
                }
                continue;
            }
            let Some((img, fp)) = res.cbg_pixels(&src) else { continue };
            let (w, h) = (img.width() as f32, img.height() as f32);
            // a sprite is offset by its base position (CSV columns 7-8)
            let (bx, by) = match &src {
                CbgSrc::Sprite(n) => res.base_pos(n),
                CbgSrc::G(_) => (0, 0),
            };
            let l = crate::resources::CbgLayer { x: l.x + bx as i32, y: l.y + by as i32, ..l.clone() };
            let top_e = client_h + l.y as f32 - h;
            let (pw, ph) = ((w * sx).round().max(1.0) as u32, (h * sy).round().max(1.0) as u32);
            let (left, top) = ((l.x as f32 * sx).round() as i32, (top_e * sy).round() as i32);
            if left < 0 || top + ph as i32 <= 0 || top >= screen_px {
                continue;
            }
            let crop_top = (-top).max(0) as u32;
            let top_c = top.max(0);
            let crop_h = (ph - crop_top.min(ph)).min((screen_px - top_c) as u32);
            if crop_h == 0 {
                continue;
            }
            let key = (fp, pw, ph);
            let scaled = match self.cbg_scaled.get(&key) {
                Some(i) => i.clone(),
                None => {
                    // (bounded by size: a canvas redrawn every frame is a
                    // new picture each time, never asked for again)
                    let held: usize = self.cbg_scaled.values().map(|i| i.as_raw().len()).sum();
                    if self.cbg_scaled.len() > 8 || held > 16 << 20 {
                        self.cbg_scaled.clear();
                    }
                    let i = if (pw, ph) == img.dimensions() { img.clone() } else { std::rc::Rc::new(crate::resources::resize(&img, pw, ph)) };
                    self.cbg_scaled.insert(key, i.clone());
                    i
                }
            };
            let ch = self.cell.1.max(1) as i32;
            out.push(Placement {
                key: format!("cbg{:x}@{}x{}", fp, pw, ph),
                img: scaled,
                col: left / self.cell.0.max(1) as i32,
                row: top_c / ch,
                y_off: top_c % ch,
                crop_top,
                crop_h,
                below: l.z > 0 && self.screen_bg.is_none(),
            });
        }
        out
    }

    /// What an 8-bit terminal gets for a character drawn by other paths
    /// than [`Self::drawable`] (placeholders, the status line): an ASCII
    /// lookalike or `?`, padded to the cells it takes.
    fn put_ascii(&self, out: &mut Vec<u8>, ch: char, w: usize) {
        let c = console_lookalike(ch, &std::collections::HashSet::new());
        out.push(if c.is_ascii() { c as u8 } else { b'?' });
        out.extend(std::iter::repeat_n(b' ', w.saturating_sub(1)));
    }

    /// A character as the screen can draw it (`char_cells`' cells): on the
    /// Linux console one its font lacks becomes a lookalike, in the same
    /// cells (the lookalike in the first, filler after).
    fn drawable(&self, ch: char, tw: usize, extra: usize, fill: char) -> (char, usize, usize, char) {
        if self.ascii_only && tw > 0 && !ch.is_ascii() {
            let c = console_lookalike(ch, &std::collections::HashSet::new());
            let c = if c.is_ascii() { c } else { '?' };
            return (c, 1, tw + extra - 1, if fill == ch { c } else { ' ' });
        }
        match &self.font_chars {
            Some(has) if tw > 0 && !ch.is_ascii() && !has.contains(&ch) => {
                let c = console_lookalike(ch, has);
                if c == ch {
                    return (ch, tw, extra, fill);
                }
                (c, 1, tw + extra - 1, if fill == ch { c } else { ' ' })
            }
            _ => (ch, tw, extra, fill),
        }
    }

    /// Cells for a run of text in one style (for text painted over rows).
    fn text_cells(&self, text: &str, st: &Style) -> Vec<Cell> {
        let mut v = Vec::new();
        let mut base: Option<usize> = None;
        for (ch, wl) in letter_widths(text, self.letters) {
            let ch = if self.yen && ch == '\\' { '¥' } else { ch };
            let (tw, extra, fill) = char_cells(ch, self.ambi, self.widths.get(&ch).copied(), wl);
            let (ch, tw, extra, fill) = self.drawable(ch, tw, extra, fill);
            if tw == 0 {
                if let (Some(i), true) = (base, is_mark(ch)) {
                    add_mark(&mut v[i], ch);
                }
                continue;
            }
            base = Some(v.len());
            v.push(Cell { ch, w: tw as u8, style: st.clone(), button: None, marks: None });
            for _ in 0..extra {
                v.push(Cell { ch: fill, w: 1, style: st.clone(), button: None, marks: None });
            }
        }
        v
    }

    /// A tooltip box next to the pointer at 0-based cell (`mx`, `my`):
    /// below it, or above when there is no room.
    /// Where a tooltip goes and its lines (see [`Tty::draw_tooltip`]).
    fn tooltip_box(&self, tip: &str, mx: usize, my: usize) -> TipBox {
        let maxw = self.cols.saturating_sub(4).clamp(10, 80);
        let mut lines: Vec<(String, usize)> = Vec::new();
        // games break tooltip lines with CR (`UNICODE(0x000D)`, AnonTW's
        // map) as well as LF; written raw, a CR sends the cursor to the
        // left edge and the rest lands over the screen
        let mut tip = tip.replace("\r\n", "\n");
        // Emuera turns `<br>` into a line break too
        for br in ["<br>", "<BR>", "<br/>", "<br />"] {
            tip = tip.replace(br, "\n");
        }
        let tip = printable(&tip);
        for para in tip.split(['\r', '\n']) {
            let (mut cur, mut w) = (String::new(), 0usize);
            for word in para.split(' ') {
                let ww: usize = word.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum();
                if w > 0 && w + 1 + ww > maxw {
                    lines.push((std::mem::take(&mut cur), w));
                    w = 0;
                }
                if w > 0 {
                    cur.push(' ');
                    w += 1;
                }
                for c in word.chars() {
                    let cw = UnicodeWidthChar::width(c).unwrap_or(0);
                    if w + cw > maxw {
                        lines.push((std::mem::take(&mut cur), w));
                        w = 0;
                    }
                    cur.push(c);
                    w += cw;
                }
            }
            lines.push((cur, w));
        }
        // as tall as the screen allows (Emuera shows all of it: AnonTW's
        // ability descriptions run to a dozen lines); a cut one ends in …
        let most = self.rows.saturating_sub(2).max(1);
        if lines.len() > most {
            lines.truncate(most);
            if let Some((l, w)) = lines.last_mut() {
                if *w + 1 >= maxw {
                    l.pop();
                } else {
                    *w += 1;
                }
                l.push('…');
            }
        }
        let w = lines.iter().map(|l| l.1).max().unwrap_or(0) + 2;
        let h = lines.len();
        let view = self.rows.saturating_sub(1);
        let top = if my + 2 + h <= view { my + 2 } else { (my + 1).saturating_sub(h).max(1) };
        let left = (mx + 2).min(self.cols.saturating_sub(w)).max(1);
        TipBox { lines, top, left, w }
    }

    /// Should the hovered button's tooltip show now, and when that next
    /// changes (its delay or duration running out).
    fn tip_due(&self, con: &Console) -> (bool, Option<Instant>) {
        let Some((_, t0)) = self.tip_since else { return (false, None) };
        let (delay, dur) = (Duration::from_millis(con.tip_delay), Duration::from_millis(con.tip_duration));
        let e = t0.elapsed();
        if e < delay {
            (false, Some(t0 + delay))
        } else if con.tip_duration == 0 {
            (true, None)
        } else if e < delay + dur {
            (true, Some(t0 + delay + dur))
        } else {
            (false, None)
        }
    }

    /// Write a tooltip box; returns the screen rows (0-based) it covers.
    fn draw_tooltip(&self, out: &mut Vec<u8>, t: &TipBox, colors: Option<(Rgb, Rgb)>) -> std::ops::Range<usize> {
        let (fg, bg) = colors.map_or(((0, 0, 0), (255, 245, 200)), |(f, b)| ((f.0, f.1, f.2), (b.0, b.1, b.2)));
        let st = self.ov_style(fg, bg, false);
        for (i, (s, sw)) in t.lines.iter().enumerate() {
            let _ = write!(out, "\x1b[{};{}H{} {}{} \x1b[0m", t.top + i, t.left, st, s, " ".repeat(t.w - 2 - sw));
        }
        t.top - 2..t.top - 2 + t.lines.len()
    }

    /// Damage tracking: drop from `out` the rows (byte ranges `rows` within
    /// `out`'s `span`) that are the same as on the terminal already. A row
    /// is written anyway after anything else drew over it (an overlay, a
    /// tooltip, a hover repaint), on a forced or resized frame, and where
    /// the pictures changed for backends that draw them into the cells
    /// (framebuffer, sixel, iTerm2), so old pixels go.
    #[allow(clippy::too_many_arguments)]
    fn skip_unchanged_rows(
        &mut self,
        out: Vec<u8>,
        span: (usize, usize),
        rows: &[(usize, usize)],
        placements: &[Placement],
        cols: usize,
        tip_rows: std::ops::Range<usize>,
    ) -> Vec<u8> {
        use std::hash::{Hash, Hasher};
        let full =
            self.force_frame || self.redraw_all.replace(false) | self.box_closed.replace(false) || self.row_hash.len() != rows.len() || self.hash_cols != cols;
        let ch = self.cell.1.max(1) as i32;
        // rows under pictures, before and now, when the pictures changed
        let mut img_rows = vec![false; rows.len()];
        let cell_images = !matches!(self.gfx.name(), "kitty" | "none");
        let changed = placements.len() != self.frame_imgs.len()
            || placements
                .iter()
                .zip(&self.frame_imgs)
                .any(|(a, b)| a.key != b.key || (a.col, a.row, a.y_off, a.crop_top, a.crop_h) != (b.col, b.row, b.y_off, b.crop_top, b.crop_h));
        if cell_images && changed {
            for p in placements.iter().chain(&self.frame_imgs) {
                let last = p.row + (p.y_off + p.crop_h as i32 + ch - 1) / ch;
                for r in p.row.max(0)..last.max(0) {
                    if let Some(f) = img_rows.get_mut(r as usize) {
                        *f = true;
                    }
                }
            }
        }
        let mut hashes = Vec::with_capacity(rows.len());
        let mut kept = Vec::with_capacity(out.len());
        kept.extend_from_slice(&out[..span.0]);
        for (r, &(a, b)) in rows.iter().enumerate() {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            out[a..b].hash(&mut h);
            let h = h.finish() | 1;
            if full || img_rows[r] || self.row_hash.get(r) != Some(&h) {
                kept.extend_from_slice(&out[a..b]);
            }
            hashes.push(h);
        }
        kept.extend_from_slice(&out[span.1..]);
        // the tooltip covers these rows now: write them again next time
        for r in tip_rows {
            if let Some(h) = hashes.get_mut(r) {
                *h = 0;
            }
        }
        self.row_hash = hashes;
        self.hash_cols = cols;
        kept
    }

    /// The clipboard options in force: emuera.config's, unless the
    /// settings force the feature on or off.
    fn clip_config(&self, con: &Console) -> crate::clipboard::ClipConfig {
        let mut c = con.clip.clone();
        match self.clip_mode.as_str() {
            "on" => c.enabled = true,
            "off" => c.enabled = false,
            _ => {}
        }
        c
    }

    /// Copy the printed text to the clipboard if `t` is a trigger for it.
    fn copy_text(&mut self, con: &Console, t: crate::clipboard::Trigger) {
        // (the Hurd's and illumos' consoles would print the OSC 52 string)
        if self.term == "hurd" || sun_console(&self.term) {
            return;
        }
        let cfg = self.clip_config(con);
        if let Some(seq) = self.clip.check(con, &cfg, t) {
            write_all(&seq);
        }
    }

    /// A mouse press may trigger a copy (left, middle or double click).
    /// Click on screen row `y` (0-based): if it belongs to a translated
    /// line, show that line's original text instead (or the translation
    /// again); any other line, while translating, is translated now
    /// whatever the Translate setting says. Whether anything changed.
    fn toggle_translation(&mut self, y: usize) -> bool {
        if let Some(Some(k)) = self.row_lines.get(y).copied() {
            if !self.show_orig.remove(&k) {
                self.show_orig.insert(k);
            }
            self.force_frame = true;
            return true;
        }
        let Some(Some(k)) = self.row_all.get(y).copied() else { return false };
        if self.mt.is_none() || !self.forced.insert(k) {
            return false;
        }
        self.force_frame = true;
        true
    }

    fn click_copies(&mut self, con: &Console, btn: u16, x: u16, y: u16) {
        use crate::clipboard::Trigger;
        match btn & !(4 | 8 | 16) {
            0 => {
                let double = self.last_click.is_some_and(|(t, p)| t.elapsed() < Duration::from_millis(400) && p == (x, y));
                self.last_click = Some((Instant::now(), (x, y)));
                self.copy_text(con, if double { Trigger::DoubleClick } else { Trigger::LeftClick });
            }
            1 => self.copy_text(con, Trigger::MiddleClick),
            _ => {}
        }
    }

    /// Emuera-Anchor's hotkeys (asking first, as Emuera does, for the two
    /// that lose the game's progress). None: cancelled.
    fn anchor_hotkey(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq, k: &Key) -> Option<InputResult> {
        match k {
            Key::Title => {
                if !self.confirm(con, res, req, &t!("Back to the title screen? Unsaved progress is lost.")) {
                    return None;
                }
                let cfg = self.clip_config(con);
                self.clip.reset(con, &cfg);
                Some(InputResult::Hotkey(crate::frontend::Hotkey::Title))
            }
            Key::Restart => self.restart(con, res, req, false),
            _ => Some(InputResult::Hotkey(crate::frontend::Hotkey::Reload)),
        }
    }

    /// Restart emuera-tty on the same game, after asking (Ctrl-R, the
    /// settings); `toggle_debug` turns `--debug` on or off for the new run.
    fn restart(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq, toggle_debug: bool) -> Option<InputResult> {
        let q = match (toggle_debug, debug_run()) {
            (false, _) => t!("Restart the game? Unsaved progress is lost."),
            (true, false) => t!("Restart the game in debug mode? Unsaved progress is lost."),
            (true, true) => t!("Restart the game without debug mode? Unsaved progress is lost."),
        };
        if !self.confirm(con, res, req, &q) {
            return None;
        }
        let mut args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
        if toggle_debug {
            if debug_run() {
                args.retain(|a| a != "--game-debug");
            } else {
                args.push("--game-debug".into());
            }
        }
        self.shutdown();
        let e = crate::sys::exec_self(&args);
        eprintln!("emuera-tty: can't restart: {}", e);
        Some(InputResult::Quit)
    }

    /// Does the pointer's move change which hover picture (`srcb`) shows?
    fn hover_swaps_image(&self) -> bool {
        let new = self.mouse.and_then(|(x, y)| self.button_at(x, y));
        new != self.hover && [new, self.hover].iter().flatten().any(|b| self.hover_imgs.contains(b))
    }

    /// After a mouse move, repaint only what changed from the last frame:
    /// the old and new console pointer cells and the cells of the button
    /// that lost or gained the hover highlight (re-blitting framebuffer
    /// images under them). A full redraw makes the images flicker.
    fn repaint_hover(&mut self, focus: Rgb) {
        let new_ptr = if self.own_pointer() && self.ptr_on { self.mouse.filter(|p| p.1 < self.rows.saturating_sub(1)) } else { None };
        let new_hover = self.mouse.and_then(|(x, y)| self.button_at(x, y));
        let old_hover = self.hover;
        if new_ptr == self.drawn_pointer && new_hover == old_hover {
            return;
        }
        let hits = |pos: Option<(usize, usize)>, r: usize, c: usize, w: usize| pos.is_some_and(|(px, py)| py == r && px >= c && px < c + w);
        let mut out: Vec<u8> = Vec::with_capacity(1024);
        out.extend_from_slice(b"\x1b[?25l");
        let mut touched = Vec::new();
        let mut covered = [false, false];
        for r in 0..self.frame.len() {
            for i in 0..self.frame[r].len() {
                let (c, ref cell, _) = self.frame[r][i];
                let w = cell.w.max(1) as usize;
                let on_old = hits(self.drawn_pointer, r, c, w);
                let on_new = hits(new_ptr, r, c, w);
                covered[0] |= on_old;
                covered[1] |= on_new;
                let btn_changed = new_hover != old_hover && cell.button.is_some() && (cell.button == old_hover || cell.button == new_hover);
                if !(on_old || on_new || btn_changed) {
                    continue;
                }
                let ov = match (cell.button, new_hover) {
                    (Some(b), Some(h)) if b == h && self.is_button(b) => Some(focus),
                    _ => None,
                };
                let _ = write!(out, "\x1b[{};{}H\x1b[0m", r + 1, c + 1);
                if on_new && cell.ch.is_whitespace() {
                    let blank = Style { fg: self.pointer_fg(cell.style.bg), ..cell.style.clone() };
                    self.sgr(&mut out, &blank, None);
                } else {
                    self.sgr(&mut out, &cell.style, ov);
                }
                if on_new {
                    out.extend_from_slice(b"\x1b[7m");
                }
                let mut tmp = [0u8; 4];
                let marks = cell.marks.as_deref().unwrap_or("");
                if self.ambi == Ambi::Osc66 && cell.w == 2 && UnicodeWidthChar::width(cell.ch) == Some(1) {
                    let _ = write!(out, "\x1b]66;w=2;{}{}\x1b\\", cell.ch, marks);
                } else if self.ascii_only && !cell.ch.is_ascii() {
                    self.put_ascii(&mut out, cell.ch, cell.w as usize);
                } else {
                    out.extend_from_slice(cell.ch.encode_utf8(&mut tmp).as_bytes());
                    out.extend_from_slice(marks.as_bytes());
                }
                out.extend_from_slice(b"\x1b[0m");
                touched.push((c, r, w));
                self.frame[r][i].2 = ov;
            }
        }
        // pointer over blank space past the end of a row
        for (k, (pos, inverse)) in [(self.drawn_pointer, false), (new_ptr, true)].into_iter().enumerate() {
            if let (Some((px, py)), false) = (pos, covered[k]) {
                let _ = write!(out, "\x1b[{};{}H", py + 1, px + 1);
                // a blank in the window background, inverted for the pointer
                let blank = Style { fg: self.pointer_fg(None), bg: None, bold: false, italic: false, underline: false, strike: false };
                self.sgr(&mut out, &blank, None);
                out.extend_from_slice(if inverse { b"\x1b[7m \x1b[0m" } else { b" \x1b[0m" });
                touched.push((px, py, 1));
            }
        }
        // (on the Linux console every move, for the mouse counter there)
        let console = text_console(&self.term);
        if self.status_info && (new_hover != old_hover || console) {
            self.draw_bar(&mut out, new_hover);
        }
        if let Some((r, c)) = self.frame_cursor {
            let _ = write!(out, "\x1b[{};{}H\x1b[?25h", r, c);
        }
        write_all(&out);
        crate::sys::drain_stdout();
        for &(_, r, _) in &touched {
            if let Some(h) = self.row_hash.get_mut(r) {
                *h = 0;
            }
        }
        if self.mouse_log.is_some() {
            let d = self.drawn_pointer;
            self.mlog(&format!("repaint: pointer {:?} -> {:?}, hover {:?} -> {:?}", d, new_ptr, old_hover, new_hover));
        }
        self.drawn_pointer = new_ptr;
        self.hover = new_hover;
        // images the repainted cells cut into: the framebuffer paints them
        // again over the text; where text erases a picture's cells (sixel,
        // iTerm in WezTerm and the like, kitty's placeholders in tmux) the
        // picture is sent again, or a hovered picture button would be left
        // with black bands
        let (cw, ch) = (self.cell.0.max(1) as i32, self.cell.1.max(1) as i32);
        let hit: Vec<Placement> = self
            .frame_imgs
            .iter()
            .filter(|p| {
                let (x0, y0) = (p.col * cw, p.row * ch + p.y_off);
                let (x1, y1) = (x0 + p.img.width() as i32, y0 + p.crop_h as i32);
                touched.iter().any(|&(c, r, w)| {
                    let (cx0, cy0) = (c as i32 * cw, r as i32 * ch);
                    cx0 < x1 && cx0 + w as i32 * cw > x0 && cy0 < y1 && cy0 + ch > y0
                })
            })
            .cloned()
            .collect();
        if !hit.is_empty() {
            if self.gfx.text_erases() {
                let mut again = b"\x1b[?25l".to_vec();
                // (tmux's placeholders: every picture, so none counts as
                // gone from the screen; they are only text)
                let all = self.gfx.name() == "kitty-tmux";
                self.gfx.draw(&mut again, if all { &self.frame_imgs } else { &hit }, self.cell);
                if let Some((r, c)) = self.frame_cursor {
                    let _ = write!(again, "\x1b[{};{}H\x1b[?25h", r, c);
                }
                write_all(&again);
                crate::sys::drain_stdout();
            }
            self.gfx.after_flush(&hit, self.cell);
            self.pointer_over_images(&hit);
        }
    }

    /// The colour of the pointer over blank space (it shows inverted, as a
    /// block): light on a dark background `bg` (else the window's), dark on
    /// a light one. Not the blank's own text colour: that may be the
    /// background's (black on the console's black, so it vanished).
    fn pointer_fg(&self, bg: Option<Rgb>) -> Rgb {
        match bg.or(self.screen_bg) {
            Some(Rgb(r, g, b)) if r as u32 * 299 + g as u32 * 587 + b as u32 * 114 > 128_000 => Rgb(0, 0, 0),
            _ => Rgb(204, 204, 204),
        }
    }

    /// The framebuffer paints pictures over the text, the pointer's
    /// inverted cell included: invert it again in pixels when it is under
    /// one of the pictures `ps` just painted.
    fn pointer_over_images(&mut self, ps: &[Placement]) {
        let Some((px, py)) = self.drawn_pointer else { return };
        let (cw, ch) = (self.cell.0.max(1) as i32, self.cell.1.max(1) as i32);
        let (cx, cy) = (px as i32 * cw, py as i32 * ch);
        // (where they are see-through the text's inverted cell shows)
        let mut opaque = 0;
        for y in cy..cy + ch {
            for x in cx..cx + cw {
                opaque += ps.iter().any(|p| {
                    let (ix, iy) = (x - p.col * cw, y - (p.row.max(0) * ch + p.y_off) + p.crop_top as i32);
                    ix >= 0
                        && iy >= p.crop_top as i32
                        && iy < (p.crop_top + p.crop_h) as i32
                        && (ix as u32) < p.img.width()
                        && (iy as u32) < p.img.height()
                        && p.img.get_pixel(ix as u32, iy as u32).0[3] >= 128
                }) as i32;
            }
        }
        if opaque * 2 > cw * ch {
            self.gfx.invert_cell(px, py, self.cell);
        }
    }

    /// The settings overlay (Ctrl-O, or F3 with no macro on it): a box over the game listing
    /// `settings::ITEMS`, changed with ← / → / Enter and saved at once;
    /// most apply to the running game. Images are hidden while it is up
    /// (kitty would draw them over it).
    fn settings_overlay(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq) -> OverlayExit {
        use crate::settings::{Settings, ENV_VARS, ITEMS};
        let mut st = Settings::load();
        let seed_row = format!(
            "{} {}  {}",
            crate::i18n::pad(&t!("Random seed"), 18),
            crate::vm::CURRENT_SEED.load(std::sync::atomic::Ordering::Relaxed) as i64,
            t!("(read-only)")
        );
        // the variables page is only offered when one of its variables is set
        let env_any = ENV_VARS.iter().any(|(name, _)| std::env::var_os(name).is_some());
        let mut sel = 0usize;
        let mut note = String::new();
        // a translator test running: (its answer, when it began, what is tested)
        let mut test: Option<(std::sync::mpsc::Receiver<Result<String, String>>, Instant, String)> = None;
        // an action row's id in the `match` below (the seed row has none)
        const SEED_ROW: usize = usize::MAX;
        let exit = loop {
            // the settings shown (the translation ones hide while the
            // Translator is off) and the rows after them: (text, action id)
            let vis: Vec<usize> = ITEMS.iter().enumerate().filter(|(_, it)| st.shown(it)).map(|(i, _)| i).collect();
            let mut rows: Vec<(String, usize)> = vec![(seed_row.clone(), SEED_ROW)];
            if env_any {
                rows.push((t!("Environment variables…  (the EMUERA_* ones emuera-tty reads)"), 0));
            }
            rows.push((t!("Game settings…  (Emuera's settings window: emuera.config)"), 1));
            rows.push((t!("Manage saves…  (copy, export, back up, restore)"), 2));
            if !matches!(st.get("translator"), "off" | "file") {
                rows.push((t!("Test the translator…  (translates a sample line)"), 3));
            }
            let keys = |k: &str| format!("  ({})", k);
            rows.extend([
                (t!("Clear the translation cache  (translate the screen again)"), 4),
                (t!("Export translation cache…  (save translations to a file)"), 14),
                (t!("Clear the picture cache  (frees decoded sprites; they load again when shown)"), 5),
                (t!("Recolor…  (draw a colour of the game's as another)"), 12),
                (t!("Back to the title") + &keys("Ctrl-T"), 6),
                (t!("Restart the game") + &keys("Ctrl-R"), 7),
                (if debug_run() { t!("Restart without debug mode") } else { t!("Restart in debug mode  ([IF_DEBUG] and ;#; lines run)") }, 13),
                (t!("Reload ERB files") + &keys(if self.f_key_free(5) { "F5 / Ctrl-E" } else { "Ctrl-E" }), 8),
                (t!("Switch game…  (unsaved progress is lost)"), 9),
                (t!("Quit emuera-tty"), 10),
                (t!("Close") + &keys("Esc"), 11),
            ]);
            let extra: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
            let n = vis.len() + extra.len();
            sel = sel.min(n - 1);
            if let Some((rx, began, what)) = &test {
                let secs = format!("{:.1}", began.elapsed().as_secs_f32());
                let mut running = false;
                note = match rx.try_recv() {
                    Ok(Ok(r)) => t!("works ({secs} s, {what}): {result}", secs = secs, what = what, result = r),
                    Ok(Err(e)) => t!("failed ({secs} s, {what}): {error}", secs = secs, what = what, error = e),
                    Err(std::sync::mpsc::TryRecvError::Empty) => {
                        running = true;
                        t!("testing {what}… {secs} s", what = what, secs = format!("{:.0}", began.elapsed().as_secs_f32()))
                    }
                    Err(_) => t!("{what} stopped", what = what),
                };
                if !running {
                    test = None;
                }
            }
            self.no_images = true;
            self.render(con, res, Some(req));
            let geo = self.draw_overlay(&st, &vis, &extra, sel, &note);
            let Some(k) = self.next_key(500) else {
                if self.hangup {
                    break OverlayExit::Quit;
                }
                continue;
            };
            note.clear();
            // a click picks a row (and acts on it)
            let mut act = 0i32; // 1 next / Enter, -1 previous
            match k {
                Key::Up | Key::Char('k') => sel = (sel + n - 1) % n,
                Key::Down | Key::Char('j') => sel = (sel + 1) % n,
                Key::Esc | Key::Settings | Key::Char('q') => break OverlayExit::Close,
                Key::CtrlC => break OverlayExit::Quit,
                Key::Enter | Key::Right | Key::Char(' ') | Key::Char('l') => act = 1,
                Key::Left | Key::Char('h') | Key::Backspace => act = -1,
                Key::Mouse { btn, y, press: true, .. } => match btn & !(4 | 8 | 16) {
                    64 => sel = (sel + n - 1) % n,
                    65 => sel = (sel + 1) % n,
                    0 => {
                        let row = y as usize;
                        if row > geo.0 && row <= geo.0 + n {
                            sel = row - geo.0 - 1;
                            act = 1;
                        }
                    }
                    _ => {}
                },
                _ => {}
            }
            if act == 0 {
                continue;
            }
            if sel < vis.len() {
                let it = &ITEMS[vis[sel]];
                let v = st.cycle(it.key, act).to_string();
                st.save();
                if it.live {
                    self.apply_setting(it.key, &v, &st);
                    if it.key == "images" {
                        // off: let go of the pictures held; on again: the next
                        // frame decodes what is on screen
                        if self.gfx.name() == "none" {
                            res.release_images();
                        }
                        self.redraw_all.set(true);
                    }
                    if it.key == "sound" {
                        con.sound_on = crate::sound::wanted(&v);
                        self.setting_note = if con.sound_on { t!("sound on") } else { t!("sound off") };
                    }
                    note = std::mem::take(&mut self.setting_note);
                } else {
                    note = t!("{setting} takes effect the next time emuera-tty starts", setting = crate::i18n::tr(it.label));
                }
            } else if rows[sel - vis.len()].1 == SEED_ROW {
                // the seed row: selectable for its description, never changed
                note = t!("the seed can't be changed here: set EMUERA_SEED=N before starting emuera-tty");
            } else if act == 1 {
                match rows[sel - vis.len()].1 {
                    0 => {
                        let quit = self.env_panel(con, res, req);
                        self.box_closed.set(true);
                        if quit {
                            break OverlayExit::Quit;
                        }
                    }
                    1 => {
                        if self.game.is_none() {
                            note = t!("no game folder known here");
                        } else {
                            let r = self.game_settings(con, res, req);
                            self.box_closed.set(true);
                            match r {
                                (_, true) => break OverlayExit::Quit,
                                (true, false) => break OverlayExit::Config,
                                _ => {}
                            }
                        }
                    }
                    2 => {
                        if self.game.is_none() {
                            note = t!("no game folder known here");
                        } else {
                            let quit = self.save_manager(con, res, req);
                            self.box_closed.set(true);
                            if quit {
                                break OverlayExit::Quit;
                            }
                        }
                    }
                    3 if st.get("translator") == "off" => note = t!("the Translator is off: choose one to test it"),
                    3 if st.get("translator") == "file" => note = t!("the Translator is file: no translator is used, so there is nothing to test"),
                    3 => match crate::mt::Engine::choose(st.get("translator")) {
                        Err(e) => note = e,
                        Ok(e) => {
                            // a sample in the game's language, into the one chosen
                            let from = if st.get("translate_from") == "en" { "en" } else { "ja" };
                            let sample = if from == "en" {
                                "Good morning! What would you like to do today?"
                            } else {
                                "「おはようございます。今日は何をしましょうか？」"
                            };
                            let to = st.get("translate").to_string();
                            let to = if to == from {
                                if from == "en" {
                                    "es".into()
                                } else {
                                    "en".into()
                                }
                            } else {
                                to
                            };
                            let (tx, rx) = std::sync::mpsc::channel();
                            let what = e.describe();
                            let _ = std::thread::Builder::new().name("translate-test".into()).spawn(move || {
                                let _ = tx.send(e.test(sample, from, &to));
                            });
                            note = t!("testing {what}…", what = what);
                            test = Some((rx, Instant::now(), what));
                        }
                    },
                    4 => {
                        note = match self.mt.as_mut() {
                            Some(m) => t!("translation cache cleared ({lines} lines): what is on screen is translated again", lines = m.clear()),
                            None => t!("translation is off: no cache to clear"),
                        };
                        self.force_frame = true;
                    }
                    14 => {
                        // Export translation cache: only offered when translation is on and no --translate-log
                        let note_ = match self.mt.as_ref().map(|m| m.logging()) {
                            Some(true) => "a translation log is already being written; no need to export".into(),
                            Some(false) => {
                                if let Some(v) =
                                    self.prompt(con, res, req, "Export translation cache", "File path to write the translation cache (empty: cancel):", "")
                                {
                                    let path = std::path::PathBuf::from(v.trim());
                                    if path.as_os_str().is_empty() {
                                        "export cancelled".into()
                                    } else {
                                        match self.mt.as_ref().map(|m| m.export_cache(&path)) {
                                            Some(Ok(n)) => format!("translation cache exported ({} lines) to {}", n, path.display()),
                                            Some(Err(e)) => format!("export failed: {}", e),
                                            None => "translation is off: no cache to export".into(),
                                        }
                                    }
                                } else {
                                    "export cancelled".into()
                                }
                            }
                            None => "translation is off: no cache to export".into(),
                        };
                        note = note_;
                    }
                    5 => {
                        // whatever the Images setting: drop the decoded
                        // sprites, scaled copies and the like (the game's own
                        // GCREATE… buffers stay)
                        let held = |r: &Resources| r.mem_stats().1.iter().map(|g| g.2).sum::<usize>();
                        let before = held(res);
                        res.release_images();
                        let freed = format!("{:.1}", before.saturating_sub(held(res)) as f64 / (1 << 20) as f64);
                        note = t!("picture cache cleared ({mb} MB freed)", mb = freed);
                        self.force_frame = true;
                    }
                    12 => {
                        let cur = st.get("recolor").to_string();
                        let what = t!(
                            "from:to pairs of hex colours, comma separated (e.g. 01183c:111d3b turns AnonTW's time-stop navy into a softer blue; empty: none)"
                        );
                        if let Some(v) = self.prompt(con, res, req, &t!("Recolor"), &what, &cur) {
                            let v = v.trim().to_string();
                            self.set_recolor(&v);
                            let n = self.recolor.len();
                            st.set("recolor", &v);
                            st.save();
                            note = if v.is_empty() {
                                t!("recolouring off")
                            } else if n == 0 {
                                t!("no colour pairs understood in \"{value}\" (write from:to, e.g. 01183c:111d3b)", value = v)
                            } else if n == 1 {
                                t!("1 colour recoloured (the game still sees its own colours)")
                            } else {
                                t!("{n} colours recoloured (the game still sees its own colours)", n = n)
                            };
                            self.force_frame = true;
                        }
                    }
                    6 => break OverlayExit::Title,
                    7 => break OverlayExit::Restart,
                    13 => break OverlayExit::RestartDebug,
                    8 => break OverlayExit::Reload,
                    9 => break OverlayExit::SwitchGame,
                    10 => break OverlayExit::Quit,
                    _ => break OverlayExit::Close,
                }
            }
        };
        self.no_images = false;
        exit
    }

    /// Apply a changed setting to the running terminal.
    fn apply_setting(&mut self, key: &str, v: &str, st: &crate::settings::Settings) {
        match key {
            "language" => {
                crate::i18n::select(v);
                self.redraw_all.set(true);
            }
            "contrast" => self.contrast = st.contrast_ratio(),
            "wheel" => self.wheel = v.parse().unwrap_or(3),
            "scroll_end_on_key" => self.scroll_end_on_key = v == "on",
            "yen" => {
                self.yen_mode = v.to_string();
                self.redraw_all.set(true);
            }
            "letters" => {
                self.letters_mode = v.to_string();
                self.redraw_all.set(true);
            }
            "statusbar" => self.status_info = v == "on",
            "kbdpointer" => self.set_kbd_pointer(v),
            "gamepad" => self.set_gamepad(v),
            "translate" | "translator" => self.setting_note = self.set_translate(st),
            "translate_from" | "translate_what" => {
                self.tr_from = st.get("translate_from").to_string();
                self.tr_all = all_text(st);
                self.redraw_all.set(true);
            }
            "memory" => self.show_memory = v == "on",
            "clipboard" => self.clip_mode = v.to_string(),
            "colors" => {
                self.color = match v {
                    "truecolor" => ColorMode::True,
                    "256" => ColorMode::C256,
                    "16" => ColorMode::C16,
                    _ => self.auto_color,
                }
            }
            "wide" => {
                if v == "auto" {
                    std::env::remove_var("EMUERA_AMBIGUOUS");
                } else {
                    std::env::set_var("EMUERA_AMBIGUOUS", v);
                }
                self.ambi = self.detect_ambiguous();
            }
            "images" => {
                let mut out = Vec::new();
                self.gfx.shutdown(&mut out);
                write_all(&out);
                self.gfx = Box::new(gfx::NoGfx);
                self.fb_cell_fixed = false;
                let pref = if v == "auto" && self.gfx_pref != "auto" { self.gfx_pref.clone() } else { v.to_string() };
                let term = self.term.clone();
                self.setting_note = self.setup_gfx(&pref, &term);
            }
            _ => {}
        }
    }

    /// Translate as the settings say (Translate to / from / what,
    /// Translator). What happened, in words.
    pub fn set_translate(&mut self, st: &crate::settings::Settings) -> String {
        self.mt = None;
        self.redraw_all.set(true);
        let lang = st.get("translate");
        self.tr_to = lang.to_string();
        self.tr_from = st.get("translate_from").to_string();
        self.tr_all = all_text(st);
        if st.get("translator") == "off" {
            return t!("the game shows as it prints itself");
        }
        // the translator, if one is wanted and there is one; the files of
        // --translate-load work with or without it
        let which = st.get("translator");
        let engine = if which == "file" { Err(t!("the Translator is file: no translator is used")) } else { crate::mt::Engine::choose(which) };
        let mt = crate::mt::Mt::start(engine.as_ref().ok().cloned(), lang);
        let what = if self.tr_all { tc!("translating", "the game") } else { tc!("translating", "dialog") };
        // what reading the --translate-load files came to (empty: none given)
        let files = mt.memory_note.clone();
        let loaded = mt.memory_len() > 0;
        let note = match (&engine, loaded) {
            (Ok(e), true) => {
                t!("translating {what} into {lang} from {files}, else with {engine}", what = what, lang = lang, files = files, engine = e.describe())
            }
            (Ok(e), false) if files.is_empty() => t!("translating {what} into {lang} with {engine}", what = what, lang = lang, engine = e.describe()),
            (Ok(e), false) => {
                t!("translating {what} into {lang} with {engine} (--translate-load: {files})", what = what, lang = lang, engine = e.describe(), files = files)
            }
            (Err(_), true) => t!("translating {what} into {lang} from {files} (no translator)", what = what, lang = lang, files = files),
            (Err(_), false) if which == "file" && files.is_empty() => {
                t!("the Translator is file, but no translation file was given: start with --translate-load (or --translate-storage) and a translation log")
            }
            (Err(_), false) if which == "file" => t!("the Translator is file, but --translate-load gave nothing: {files}", files = files),
            (Err(e), false) if files.is_empty() => e.clone(),
            (Err(e), false) => format!("{} (--translate-load: {})", e, files),
        };
        if engine.is_ok() || mt.memory_len() > 0 {
            self.mt = Some(mt);
        }
        note
    }

    /// The line's text, language and first colour if it is dialog to
    /// translate whole: only text (no buttons, pictures), not a drawing,
    /// in a language to translate, and dialog (see `mt::is_dialog`).
    fn dialog_of(&self, l: &Line) -> Option<(&'static str, String, Style)> {
        let mut text = String::new();
        let mut style = None;
        for s in &l.segs {
            match s {
                Seg::Text { text: t, style: st, button: None, .. } => {
                    text.push_str(t);
                    style.get_or_insert_with(|| st.clone());
                }
                _ => return None,
            }
        }
        if crate::mt::is_drawing(&text) || text.trim().is_empty() {
            return None;
        }
        let src = crate::mt::source_of(&text, &self.tr_from, &self.tr_to)?;
        crate::mt::is_dialog(&text, l.kojo, src).then(|| (src, text, style.unwrap()))
    }

    /// Display columns of `s`, as the screen lays it out.
    fn text_width(&self, s: &str) -> usize {
        letter_widths(s, self.letters)
            .map(|(c, wl)| {
                let (tw, extra, _) = if c == '\t' { (4, 0, ' ') } else { char_cells(c, self.ambi, self.widths.get(&c).copied(), wl) };
                tw + extra
            })
            .sum()
    }

    /// Dialog translated in batches: the lines printed between two waits or
    /// inputs (one click: a speech over several lines) that are dialog in one
    /// language go to the translator together, so sentences split over
    /// lines are translated whole. Returns the lines from `start` on that
    /// have their translation in, by index. A translation with as many
    /// lines goes back line by line; one without is spread over the lines
    /// in proportion to their lengths.
    fn translate_blocks(&mut self, con: &Console, start: usize) -> crate::fxhash::HashMap<usize, Line> {
        let mut out = crate::fxhash::HashMap::default();
        let lines = &con.lines;
        // a filtered line stays as it is (a middle-click translates it)
        let dialog: Vec<Option<(&'static str, String, Style)>> = (start..lines.len())
            .map(|i| self.dialog_of(&lines[i]).filter(|_| self.forced.contains(&line_key(&lines[i])) || !line_filtered(&lines[i], &con.src_files)))
            .collect();
        // a speech that began above `start` starts there (unchecked: rare)
        let mut i = 0;
        while i < dialog.len() {
            let Some((src, _, _)) = &dialog[i] else {
                i += 1;
                continue;
            };
            let (src, wait) = (*src, lines[start + i].wait);
            let mut j = i + 1;
            while j < dialog.len() && j - i < 40 && lines[start + j].wait == wait && dialog[j].as_ref().is_some_and(|d| d.0 == src) {
                j += 1;
            }
            let texts: Vec<&str> = (i..j).map(|k| dialog[k].as_ref().unwrap().1.trim()).collect();
            let joined = texts.join("\n");
            // where each line of the batch was printed, for the log (one row each)
            let places: Vec<String> = if self.mt.as_ref().is_some_and(|m| m.logging()) {
                (i..j).map(|k| place_of(&lines[start + k], &con.src_files).unwrap_or_else(|| "-".into())).collect()
            } else {
                Vec::new()
            };
            let tr = self.mt.as_mut().and_then(|m| m.get_lines(&joined, src, &places).map(str::to_string));
            if let Some(tr) = tr {
                let n = j - i;
                let got: Vec<&str> = tr.split('\n').collect();
                let parts: Vec<String> = if got.len() == n {
                    got.iter().map(|s| s.to_string()).collect()
                } else {
                    let widths: Vec<usize> = texts.iter().map(|t| self.text_width(t)).collect();
                    spread(&got.join(" "), &widths, |s| self.text_width(s))
                };
                for (k, part) in (i..j).zip(parts) {
                    let (_, text, style) = dialog[k].as_ref().unwrap();
                    let lead: String = text.chars().take_while(|c| c.is_whitespace()).collect();
                    let mut l = lines[start + k].clone();
                    l.segs = vec![Seg::Text { text: format!("{}{}", lead, part), style: style.clone(), button: None, auto: false }];
                    out.insert(start + k, l);
                }
            }
            i = j;
        }
        out
    }

    /// All-text mode: `l` with each piece of text translated on its own (as
    /// far as translations are in), so buttons and colours stay, and only
    /// the words, not the numbers and brackets around them. Dialog lines
    /// are left to `translate_blocks`. `force`: the player middle-clicked
    /// the line, so it is translated whatever the Translate setting says;
    /// the log then says so where it can't name the file that printed it.
    fn translated(&mut self, l: &Line, files: &[String], force: bool) -> Option<Line> {
        use crate::mt::{is_drawing, source_of, split_words};
        if !(self.tr_all || force) || self.dialog_of(l).is_some() {
            return None;
        }
        if !force && line_filtered(l, files) {
            return None;
        }
        let (from, to) = (self.tr_from.clone(), self.tr_to.clone());
        let logging = self.mt.as_ref().is_some_and(|m| m.logging());
        let mut place = logging.then(|| place_of(l, files)).flatten();
        if logging && force && place.is_none() {
            place = Some("(force-translated, cannot trace original file)".to_string());
        }
        let place = place.as_deref();
        let mt = self.mt.as_mut()?;
        let whole: String = l.segs.iter().filter_map(|s| if let Seg::Text { text, .. } = s { Some(text.as_str()) } else { None }).collect();
        if is_drawing(&whole) {
            return None;
        }
        {
            // menus keep their columns: where a piece is followed by a gap
            // (two spaces or more, then more on the line) its translation
            // keeps its width — padded if shorter, taking the gap's spaces
            // (one stays) if longer, cut with … past that. At the end of a
            // line, or before ordinary text, it just takes the room it needs.
            let (ambi, widths, letters) = (self.ambi, self.widths.clone(), self.letters);
            let width = |s: &str| -> usize {
                letter_widths(s, letters)
                    .map(|(c, wl)| {
                        let (tw, extra, _) = if c == '\t' { (4, 0, ' ') } else { char_cells(c, ambi, widths.get(&c).copied(), wl) };
                        tw + extra
                    })
                    .sum()
            };
            let lead_spaces = |s: &str| s.len() - s.trim_start_matches(' ').len();
            let mut out = l.clone();
            let mut changed = false;
            // spaces a piece took from the ones after it
            let mut eat = 0usize;
            for k in 0..out.segs.len() {
                // the spaces at the start of the pieces after this one, and
                // whether anything comes after them
                let (mut next_lead, mut more) = (0usize, false);
                for s in &out.segs[k + 1..] {
                    match s {
                        Seg::Text { text, .. } if text.trim_start_matches(' ').is_empty() => next_lead += text.len(),
                        Seg::Text { text, .. } => {
                            next_lead += lead_spaces(text);
                            more = true;
                            break;
                        }
                        _ => {
                            more = true;
                            break;
                        }
                    }
                }
                let Seg::Text { text, .. } = &mut out.segs[k] else {
                    eat = 0;
                    continue;
                };
                if eat > 0 {
                    let n = eat.min(lead_spaces(text));
                    text.drain(..n);
                    changed = true;
                    eat -= n;
                    if !text.is_empty() {
                        eat = 0;
                    }
                }
                let (pre, words, post) = split_words(text);
                let Some(src) = source_of(words, &from, &to) else { continue };
                let Some(tr) = mt.get(words, src, place) else { continue };
                let (want, got) = (width(words), width(tr));
                let mut post = post.to_string();
                let tr = if got <= want {
                    post.push_str(&" ".repeat(want - got));
                    tr.to_string()
                } else {
                    // blank space after it: its own trailing spaces, then the
                    // next piece's leading ones, keeping one
                    let own = post.len() - post.trim_end_matches(' ').len();
                    let gap = own + next_lead;
                    let slack = gap.saturating_sub(1);
                    let over = got - want;
                    // not a column: nothing after it, or ordinary text
                    let in_column = more && gap >= 2;
                    if !in_column || over <= slack {
                        let over = if in_column { over } else { over.min(slack) };
                        let from_own = over.min(own);
                        post.truncate(post.len() - from_own);
                        eat = over - from_own;
                        tr.to_string()
                    } else {
                        post.truncate(post.len() - own.min(slack));
                        eat = slack - own.min(slack);
                        let room = want + slack;
                        let mut cut = String::new();
                        for c in tr.chars() {
                            if width(&cut) + width(&c.to_string()) + 1 > room {
                                break;
                            }
                            cut.push(c);
                        }
                        cut.push('…');
                        cut
                    }
                };
                *text = format!("{}{}{}", pre, tr, post);
                changed = true;
            }
            changed.then_some(out)
        }
    }

    /// The SGR sequence for an overlay colour pair, in the terminal's depth.
    fn ov_style(&self, fg: (u8, u8, u8), bg: (u8, u8, u8), bold: bool) -> String {
        let b = if bold { "1;" } else { "" };
        match self.color {
            ColorMode::True => format!("\x1b[0;{}38;2;{};{};{};48;2;{};{};{}m", b, fg.0, fg.1, fg.2, bg.0, bg.1, bg.2),
            ColorMode::C256 if self.fbterm => {
                format!("\x1b[0;{}m\x1b[1;{}}}\x1b[2;{}}}", b.trim_end_matches(';'), rgb256(Rgb(fg.0, fg.1, fg.2)), rgb256(Rgb(bg.0, bg.1, bg.2)))
            }
            ColorMode::C256 => format!("\x1b[0;{}38;5;{};48;5;{}m", b, rgb256(Rgb(fg.0, fg.1, fg.2)), rgb256(Rgb(bg.0, bg.1, bg.2))),
            // 16 colours: white / yellow on black, black on white
            ColorMode::C16 => {
                let f = if fg == (0, 0, 0) {
                    "30"
                } else if fg.2 < 100 {
                    if self.bright_codes {
                        "93"
                    } else {
                        "33;1"
                    }
                } else if self.bright_codes {
                    "97"
                } else {
                    "37;1"
                };
                let k = if bg == (0, 0, 0) || bg.0 < 128 { 40 } else { 47 };
                format!("\x1b[0;{}{};{}m", b, f, k)
            }
        }
    }

    /// The foreground-only SGR for a memory band, in the terminal's depth
    /// (the background and the other attributes stay as they are).
    fn mem_fg(&self, lvl: crate::sys::MemoryLevel) -> String {
        use crate::sys::MemoryLevel::{Green, Orange, Red, Yellow};
        let c = match lvl {
            Green => Rgb(90, 200, 90),
            Yellow => Rgb(240, 220, 70),
            Orange => Rgb(255, 150, 40),
            Red => Rgb(240, 70, 70),
        };
        match self.color {
            ColorMode::True => format!("\x1b[38;2;{};{};{}m", c.0, c.1, c.2),
            ColorMode::C256 if self.fbterm => format!("\x1b[1;{}}}", rgb256(c)),
            ColorMode::C256 => format!("\x1b[38;5;{}m", rgb256(c)),
            // 16 colours: orange is the plain (brownish) yellow and yellow the
            // bright one; the Linux console and fbterm brighten via bold
            ColorMode::C16 => {
                let code = match lvl {
                    Green => "\x1b[32m",
                    Yellow => "\x1b[33;1m",
                    Orange => "\x1b[33m",
                    Red => "\x1b[31m",
                };
                code.to_string()
            }
        }
    }

    /// Draw the overlay box; returns its top row (1-based, the title row)
    /// for mapping clicks.
    fn draw_overlay(&self, st: &crate::settings::Settings, vis: &[usize], extra: &[&str], sel: usize, note: &str) -> (usize,) {
        self.redraw_all.set(true);
        use crate::settings::ITEMS;
        let n = vis.len() + extra.len();
        let w = self.cols.saturating_sub(4).clamp(30, 72);
        let help = if !note.is_empty() {
            note.to_string()
        } else if let (true, Some(e)) = (
            sel < vis.len() && matches!(ITEMS[vis[sel]].key, "translate" | "translator" | "translate_from" | "translate_what"),
            self.mt.as_ref().and_then(|m| m.last_error.as_ref()),
        ) {
            t!("last translation failed: {error}", error = e)
        } else if sel < vis.len() && ITEMS[vis[sel]].key == "gamepad" {
            format!("{}; found: {}", ITEMS[vis[sel]].help, crate::gamepad::describe())
        } else if sel < vis.len() {
            crate::i18n::tr(ITEMS[vis[sel]].help)
        } else if sel == vis.len() {
            t!("the number the random generator started from (read-only); set EMUERA_SEED=N before starting for repeatable runs; RANDOMIZE / INITRAND in the game change it")
        } else {
            t!("↑↓ choose  ←→ / Enter change  Esc close")
        };
        // the help wraps (three lines at most) rather than being cut
        // (always three lines high, so the box doesn't move between rows)
        let mut help = wrap_words(&help, w.saturating_sub(3), 3);
        help.resize(3, String::new());
        let h = n + 3 + help.len(); // title, rows, help, borders
        let left = (self.cols.saturating_sub(w)) / 2 + 1;
        let top = (self.rows.saturating_sub(h)) / 2 + 1;
        let mut out: Vec<u8> = Vec::new();
        let inner = w - 2;
        // fixed colours where the terminal has them: palette colours vary
        // by theme (Konsole's blue is bright, and bright cyan on it vanishes)
        let rgb = |fg: (u8, u8, u8), bg: (u8, u8, u8), bold: bool| -> String { self.ov_style(fg, bg, bold) };
        let box_bg = (22, 32, 58);
        let frame = rgb((150, 170, 210), box_bg, false);
        let title = rgb((255, 255, 255), box_bg, true);
        let normal = rgb((235, 235, 235), box_bg, false);
        let action = rgb((140, 220, 255), box_bg, false);
        let hilite = rgb((0, 0, 0), (230, 230, 230), false);
        let helpc = rgb((255, 220, 90), box_bg, false);
        let line = |out: &mut Vec<u8>, row: usize, text: &str, style: &str| {
            let mut t: String = String::new();
            let mut used = 0;
            for c in text.chars() {
                let cw = UnicodeWidthChar::width(c).unwrap_or(0);
                if used + cw > inner {
                    break;
                }
                t.push(c);
                used += cw;
            }
            let _ = write!(out, "\x1b[{};{}H{}│{}{}{}{}│", row, left, frame, style, t, " ".repeat(inner - used), frame);
        };
        let _ = write!(out, "\x1b[?25l\x1b[{};{}H{}┌{}┐", top, left, frame, "─".repeat(inner));
        // the memory emuera-tty uses, on the right of the title
        let head = format!(" {}  ({})", t!("emuera-tty settings"), if self.f_key_free(3) { "F3 / Ctrl-O" } else { "Ctrl-O" });
        let reading = crate::sys::memory_in_use();
        let mem_label = t!("memory");
        let mem = reading.map(|b| format!("{} {} ", mem_label, crate::sys::memory_text(b))).unwrap_or_default();
        let head_cols = UnicodeWidthStr::width(head.as_str());
        let gap = inner.saturating_sub(head_cols + UnicodeWidthStr::width(mem.as_str()));
        let head = if gap > 0 { format!("{}{}{}", head, " ".repeat(gap), mem) } else { head.to_string() };
        line(&mut out, top + 1, &head, &title);
        // only the live counter is coloured (not the peak after it): drawn
        // over its place in the title row, after the box's left border
        if let (true, Some(b)) = (gap > 0, reading) {
            let (live, _) = crate::sys::memory_parts(b);
            let col = left + 1 + head_cols + gap + UnicodeWidthStr::width(mem_label.as_str()) + 1;
            let _ = write!(out, "\x1b[{};{}H{}{}{}", top + 1, col, title, self.mem_fg(crate::sys::memory_level(b)), live);
        }
        for (i, &ix) in vis.iter().enumerate() {
            let it = &ITEMS[ix];
            let later = if it.live { String::new() } else { format!("  {}", t!("(next start)")) };
            let text = format!("  {} ‹ {} ›{}", crate::i18n::pad(&crate::i18n::tr(it.label), 18), st.shown_value(it.key), later);
            line(&mut out, top + 2 + i, &text, if i == sel { &hilite } else { &normal });
        }
        for (k, e) in extra.iter().enumerate() {
            let i = vis.len() + k;
            line(&mut out, top + 2 + i, &format!("  {}", e), if i == sel { &hilite } else { &action });
        }
        for (k, l) in help.iter().enumerate() {
            line(&mut out, top + 2 + n + k, &format!(" {}", l), &helpc);
        }
        let _ = write!(out, "\x1b[{};{}H{}└{}┘\x1b[0m", top + 2 + n + help.len(), left, frame, "─".repeat(inner));
        write_all(&out);
        (top + 1,)
    }

    /// The top row: dark gray background, [ Settings ] button at left,
    /// memory at right (with colored live counter like the old bottom bar),
    /// and translation status (queued/failed/cached).
    fn draw_top_bar(&mut self, out: &mut Vec<u8>) {
        let cols = self.cols;
        // Dark gray background (237 in 256-color palette), light gray text (250)
        let _ = write!(out, "\x1b[1;1H\x1b[48;5;237m\x1b[38;5;250m\x1b[K");
        // Left: [ Settings ] button (keep background/foreground)
        let settings_label = " [ Settings ] ";
        let settings_width = settings_label.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum::<usize>();
        let _ = write!(out, "\x1b[1;1H{}", settings_label);
        // Register the button in btn_map for click handling
        if self.btn_map.is_empty() {
            self.btn_map.push(Vec::new());
        }
        self.btn_map[0] = vec![(0, settings_width, usize::MAX)]; // MAX = special ID for Settings
                                                                 // Right: memory + translation stats
        let mut right_plain: Vec<String> = Vec::new();
        let mut right_styled: Vec<String> = Vec::new();
        if let Some(mem) = self.memory().filter(|_| self.show_memory) {
            // Color the live memory counter based on memory level (like old bottom bar)
            if let Some(lvl) = self.mem_level() {
                // memory_text format: "123 MB (peak 150 MB)" - color only the live part
                if let Some(peak_idx) = mem.find(" (peak ") {
                    let live_part = &mem[..peak_idx];
                    let peak_part = &mem[peak_idx..];
                    right_plain.push(mem.clone());
                    let colored_mem = format!("{}{}\x1b[38;5;250m{}", self.mem_fg(lvl), live_part, peak_part);
                    right_styled.push(colored_mem);
                } else {
                    right_plain.push(mem.clone());
                    let colored_mem = format!("{}{}\x1b[38;5;250m", self.mem_fg(lvl), mem);
                    right_styled.push(colored_mem);
                }
            } else {
                right_plain.push(mem.clone());
                right_styled.push(mem);
            }
        }
        if let Some(mt) = &self.mt {
            let (queued, failed, cached) = mt.stats();
            if queued > 0 || failed > 0 || cached > 0 {
                let tr_text = format!("TR: {}/{}/{}", queued, failed, cached);
                right_plain.push(tr_text.clone());
                right_styled.push(tr_text);
            }
        }
        if !right_plain.is_empty() {
            let right_text_plain = right_plain.join("  ");
            let right_text_styled = right_styled.join("  ");
            let right_width = right_text_plain.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum::<usize>();
            let start_col = cols.saturating_sub(right_width);
            if start_col > settings_width {
                let _ = write!(out, "\x1b[1;{}H{}", start_col + 1, right_text_styled);
            }
        }
        // Reset colors at the end
        let _ = write!(out, "\x1b[0m");
    }

    /// The right side of the bottom row: the value the hovered button
    /// sends, the function asking for input, the settings
    /// key. What doesn't fit goes: the function, the key, the value last.
    /// (Redrawn on its own when the hovered button changes.)
    fn draw_bar(&mut self, out: &mut Vec<u8>, hover: Option<usize>) {
        let Some((used, running, drawn_at)) = self.bar.clone() else { return };
        let cols = self.cols;
        let mut parts: Vec<(String, &str)> = Vec::new();
        if self.status_info {
            if let Some(v) = hover.filter(|&b| self.is_button(b)).and_then(|b| self.buttons.get(b)) {
                parts.push((format!("→ {}", printable(&v.to_string())), "\x1b[0;1m"));
            }
            if !running.is_empty() {
                parts.push((format!("@{}", running), "\x1b[0;2m"));
            }
            // on the Linux console: is the mouse there, and are its events
            // coming in? (no console mouse elsewhere)
            if text_console(&self.term) && cfg!(target_os = "linux") {
                let m = match &self.mice {
                    Some(m) => format!("mouse:{} {}", m.describe().split(' ').next().unwrap_or(""), self.mouse_events),
                    None => "mouse:none".to_string(),
                };
                parts.insert(0, (m, "\x1b[0;1m"));
            }
        }
        // (F3 is a macro key once a macro is stored on it; Redox's console
        // sends nothing for F1…F12, and its TERM says xterm-256color)
        let f3 = self.f_key_free(3) && !cfg!(target_os = "redox");
        let settings_part = format!("{} {}", if f3 { "F3" } else { "Ctrl-O" }, t!("settings"));
        parts.push((settings_part.clone(), "\x1b[0;2m"));
        let width = |p: &[(String, &str)]| p.iter().map(|(s, _)| s.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum::<usize>() + 2).sum::<usize>();
        while !parts.is_empty() && used + width(&parts) >= cols {
            let k = parts.iter().position(|(s, _)| s.starts_with('@')).or_else(|| parts.iter().position(|(s, _)| *s == settings_part)).unwrap_or(0);
            parts.remove(k);
        }
        // what was there before goes first
        if drawn_at > 0 {
            let _ = write!(out, "\x1b[{};{}H\x1b[0m\x1b[K", self.rows, drawn_at);
        }
        let mut at = 0;
        if !parts.is_empty() {
            // (ending a column short of the edge: no wrap on the last row)
            at = cols + 2 - width(&parts);
            let _ = write!(out, "\x1b[{};{}H", self.rows, at);
            for (k, (s, st)) in parts.iter().enumerate() {
                let sep = if k > 0 { "  " } else { "" };
                let _ = write!(out, "{}{}{}\x1b[0m", sep, st, s);
            }
        }
        if let Some(b) = self.bar.as_mut() {
            b.2 = at;
        }
    }

    /// "123 MB": memory in use (read at most once a second).
    fn memory(&mut self) -> Option<String> {
        let fresh = self.mem_seen.is_some_and(|(t, _)| t.elapsed() < Duration::from_secs(1));
        if !fresh {
            self.mem_seen = Some((Instant::now(), crate::sys::memory_in_use()));
        }
        self.mem_seen.and_then(|(_, m)| m).map(crate::sys::memory_text)
    }

    /// The colour band of the reading [`Self::memory`] shows.
    fn mem_level(&self) -> Option<crate::sys::MemoryLevel> {
        self.mem_seen.and_then(|(_, m)| m).map(crate::sys::memory_level)
    }

    /// The button id at a 0-based screen cell, from the last frame
    /// (`<nonbutton>` tooltip areas included).
    fn button_at(&self, x: usize, y: usize) -> Option<usize> {
        let row = self.btn_map.get(y)?;
        row.iter().find(|(s, e, _)| x >= *s && x < *e).map(|(_, _, b)| *b)
    }

    /// The Keyboard pointer setting: on, off, or auto (on on the Linux
    /// console when there is no mouse there).
    /// A colour as drawn (see `recolor`).
    fn recolored(&self, c: Rgb) -> Rgb {
        self.recolor.iter().find(|(from, _)| *from == c).map_or(c, |(_, to)| *to)
    }

    /// The `recolor` setting (or EMUERA_RECOLOR).
    pub fn set_recolor(&mut self, v: &str) {
        self.recolor = parse_recolor(v);
    }

    pub fn set_kbd_pointer(&mut self, v: &str) {
        let console = text_console(&self.term);
        self.kbd_ptr = v == "on" || (v == "auto" && console && self.mice.is_none());
        if self.kbd_ptr && self.mouse.is_none() {
            self.mouse = Some((self.cols / 2, self.rows / 2));
        }
        self.redraw_all.set(true);
    }

    /// The Gamepad setting: auto (read the pads there are) or off.
    pub fn set_gamepad(&mut self, v: &str) {
        self.pad_enabled = v != "off";
        if self.pad_enabled {
            crate::gamepad::start();
        } else if self.pad_on {
            self.pad_on = false;
            self.redraw_all.set(true);
        }
    }

    /// A gamepad's pointer event in a game's input, as mouse events: the
    /// d-pad jumps between buttons, the left stick moves freely, A clicks
    /// and B right-clicks (Emuera's skip) where the pointer is.
    fn pad_pointer(&mut self, ev: crate::gamepad::Ev) -> Key {
        use crate::gamepad::Ev;
        if !self.pad_on {
            // the first touch only shows the pointer: on the first button
            // (where there is no mouse yet), else in the middle
            self.pad_on = true;
            self.redraw_all.set(true);
            if self.mouse.is_none() {
                let first = self.btn_map.iter().enumerate().find_map(|(r, row)| row.iter().find(|s| self.is_button(s.2)).map(|s| (s.0, r)));
                let (x, y) = first.unwrap_or((self.cols / 2, self.rows / 2));
                self.mouse = Some((x, y));
                return Key::Mouse { btn: 35, x: x as u16 + 1, y: y as u16 + 1, press: true };
            }
        }
        let (x, y) = self.mouse.unwrap_or((self.cols / 2, self.rows / 2));
        let (nx, ny) = match ev {
            Ev::A => return Key::Mouse { btn: 0, x: x as u16 + 1, y: y as u16 + 1, press: true },
            Ev::B => return Key::Mouse { btn: 2, x: x as u16 + 1, y: y as u16 + 1, press: true },
            Ev::Dir(d) => self.button_jump((x, y), d).unwrap_or((x, y)),
            Ev::Move(dx, dy) => (
                (x as isize + dx as isize).clamp(0, self.cols.saturating_sub(1) as isize) as usize,
                (y as isize + dy as isize).clamp(0, self.rows.saturating_sub(2) as isize) as usize,
            ),
            _ => return Key::Other,
        };
        Key::Mouse { btn: 35, x: nx as u16 + 1, y: ny as u16 + 1, press: true }
    }

    /// The button the d-pad goes to from cell `(x, y)` in direction `d`
    /// (0 up, 1 down, 2 left, 3 right): the next one along the row, or on
    /// the nearest row above / below the closest one across; where it
    /// starts. None: there is no button that way.
    fn button_jump(&self, (x, y): (usize, usize), d: u8) -> Option<(usize, usize)> {
        let here = self.button_at(x, y);
        // live buttons: (row, start, end, id), the one under the pointer left out
        let spans: Vec<(usize, usize, usize, usize)> = self
            .btn_map
            .iter()
            .enumerate()
            .flat_map(|(r, row)| row.iter().map(move |&(s, e, b)| (r, s, e, b)))
            .filter(|&(_, _, _, b)| self.is_button(b) && Some(b) != here)
            .collect();
        // where the pointer's button starts (or the pointer), to compare with
        let from = here.and_then(|h| self.btn_map.get(y)?.iter().find(|s| s.2 == h).map(|s| s.0)).unwrap_or(x);
        let centre = |s: usize, e: usize| (s + e) / 2;
        let pick = match d {
            // along the row, else the next row's first / the last row's last
            3 => spans.iter().filter(|t| t.0 == y && t.1 > from).min_by_key(|t| t.1).or_else(|| spans.iter().filter(|t| t.0 > y).min_by_key(|t| (t.0, t.1))),
            2 => spans.iter().filter(|t| t.0 == y && t.1 < from).max_by_key(|t| t.1).or_else(|| spans.iter().filter(|t| t.0 < y).max_by_key(|t| (t.0, t.1))),
            // the nearest row that has one, the closest one across there
            1 => {
                let row = spans.iter().filter(|t| t.0 > y).map(|t| t.0).min();
                row.and_then(|r| spans.iter().filter(|t| t.0 == r).min_by_key(|t| centre(t.1, t.2).abs_diff(x)))
            }
            _ => {
                let row = spans.iter().filter(|t| t.0 < y).map(|t| t.0).max();
                row.and_then(|r| spans.iter().filter(|t| t.0 == r).min_by_key(|t| centre(t.1, t.2).abs_diff(x)))
            }
        };
        pick.map(|t| (t.1, t.0))
    }

    /// Is the mouse pointer ours to draw? (the console mouse, gpm's or the
    /// raw device's, or the keyboard pointer: no terminal shows one)
    fn own_pointer(&self) -> bool {
        self.mice.is_some() || self.kbd_ptr || self.pad_on
    }

    /// Blink the pointer: how long until it changes (None: no pointer of
    /// ours), turning it over when the time has come. It shows at once when
    /// it moves (`show_pointer`).
    fn blink(&mut self) -> Option<i32> {
        if !self.own_pointer() || self.mouse.is_none() {
            return None;
        }
        let now = Instant::now();
        if now >= self.blink_at {
            self.ptr_on = !self.ptr_on;
            self.blink_at = now + Duration::from_millis(530);
        }
        Some(self.blink_at.saturating_duration_since(now).as_millis() as i32 + 1)
    }

    /// Blink the pointer when it is time (repainting just its cell); how
    /// long to wait for the next time.
    fn blink_wait(&mut self, focus: Rgb) -> Option<i32> {
        let was = self.ptr_on;
        let w = self.blink();
        if self.ptr_on != was {
            self.repaint_hover(focus);
        }
        w
    }

    /// A line in the mouse log (EMUERA_MOUSE_LOG), with the time.
    fn mlog(&mut self, what: &str) {
        if let Some(f) = self.mouse_log.as_mut() {
            use std::io::Write;
            let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64() % 1000.0);
            let _ = writeln!(f, "{:8.3} {}", t, what);
        }
    }

    /// The pointer moved: show it, and keep it on for a moment.
    fn show_pointer(&mut self) {
        self.ptr_on = true;
        self.blink_at = Instant::now() + Duration::from_millis(530);
    }

    /// The keyboard pointer: with it on, the cursor keys move the pointer a
    /// cell and Insert or Shift+Enter click under it, as mouse events (Enter
    /// keeps its own uses). `raw`: a picture menu's input.
    fn kbd_pointer(&mut self, k: Key, _raw: bool) -> Key {
        if let Key::Pad(ev) = k {
            return self.pad_pointer(ev);
        }
        if !self.kbd_ptr {
            return k;
        }
        let (x, y) = self.mouse.unwrap_or((self.cols / 2, self.rows / 2));
        let (dx, dy): (isize, isize) = match k {
            Key::Up => (0, -1),
            Key::Down => (0, 1),
            Key::Left => (-1, 0),
            Key::Right => (1, 0),
            Key::Click => return Key::Mouse { btn: 0, x: x as u16 + 1, y: y as u16 + 1, press: true },
            k => return k,
        };
        let nx = (x as isize + dx).clamp(0, self.cols.saturating_sub(1) as isize) as usize;
        let ny = (y as isize + dy).clamp(0, self.rows.saturating_sub(2) as isize) as usize;
        // a move with no button held (SGR motion)
        Key::Mouse { btn: 35, x: nx as u16 + 1, y: ny as u16 + 1, press: true }
    }

    /// The clickable button at a cell (not a `<nonbutton>` area).
    fn click_at(&self, x: usize, y: usize) -> Option<usize> {
        self.button_at(x, y).filter(|&b| self.is_button(b))
    }

    /// Is button id `b` a real button (not a `<nonbutton>` tooltip area)?
    fn is_button(&self, b: usize) -> bool {
        self.buttons.get(b).is_some_and(|v| !crate::console::is_tip_area(v))
    }
}

/// Nearest xterm-256 colour: the 6×6×6 cube or, for greys, the 24-step
/// grey ramp (the cube has only black below 95, so dark map tiles and
/// shading would all turn black).
fn rgb256(c: Rgb) -> u8 {
    let q = |v: u8| -> u8 {
        if v < 48 {
            0
        } else if v < 115 {
            1
        } else {
            (v - 35) / 40
        }
    };
    let lv = |k: u8| -> i32 {
        if k == 0 {
            0
        } else {
            55 + 40 * k as i32
        }
    };
    let (r, g, b) = (c.0 as i32, c.1 as i32, c.2 as i32);
    let cube = (q(c.0), q(c.1), q(c.2));
    let d_cube = (lv(cube.0) - r).pow(2) + (lv(cube.1) - g).pow(2) + (lv(cube.2) - b).pow(2);
    // grey ramp 232..=255 is 8, 18, ..., 238
    let avg = (r + g + b) / 3;
    let gi = ((avg - 3) / 10).clamp(0, 23);
    let gv = 8 + 10 * gi;
    let d_grey = (gv - r).pow(2) + (gv - g).pow(2) + (gv - b).pow(2);
    if d_grey < d_cube && avg >= 4 {
        232 + gi as u8
    } else {
        16 + 36 * cube.0 + 6 * cube.1 + cube.2
    }
}

/// A text console: no answers to terminal queries, no pictures beyond its
/// own framebuffer, a pointer of our own for the mouse. The Linux console,
/// fbterm, the Hurd's console (which prints OSC strings as text) and the
/// illumos / Solaris one.
pub(crate) fn text_console(term: &str) -> bool {
    term == "linux" || term.starts_with("fbterm") || term == "hurd" || sun_console(term)
}

/// The illumos / Solaris console (`tem`; its logins get `sun-color`): no
/// alternate screen, every character one cell, no answer to any query
/// (and its kernel can crash replying to `CSI 14 t` / `18 t`).
fn sun_console(term: &str) -> bool {
    term == "sun" || term.starts_with("sun-")
}

/// One of the 8 ANSI colours for an RGB colour, and whether it should be
/// the bright one. Hues are kept even when dark (Emuera games draw dim
/// teal walls or brown floors on black, which nearest-distance would turn
/// into black); greys step through black, dark grey, light grey, white.
pub(crate) fn rgb16(c: Rgb) -> (u8, bool) {
    let (r, g, b) = (c.0 as u32, c.1 as u32, c.2 as u32);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    if max < 24 {
        return (0, false);
    }
    // greyish: little colour compared with the brightness
    if (max - min) * 4 < max {
        return match max {
            0..=39 => (0, false),
            40..=127 => (0, true),
            128..=210 => (7, false),
            _ => (7, true),
        };
    }
    // which channels stand out within the colour's own range
    let t = min + (max - min) / 3;
    let code = ((r > t) as u8) | ((g > t) as u8) << 1 | ((b > t) as u8) << 2;
    (code, max > 200)
}

/// An ANSI background colour for a window background: only the strongest
/// channels count, so a dark navy stays blue (not cyan) and very dark
/// colours stay black.
fn rgb16_bg(c: Rgb) -> u8 {
    let (r, g, b) = (c.0 as u32, c.1 as u32, c.2 as u32);
    let max = r.max(g).max(b);
    if max < 40 {
        return 0;
    }
    let t = max * 2 / 3;
    ((r > t) as u8) | ((g > t) as u8) << 1 | ((b > t) as u8) << 2
}

#[cfg(test)]
mod colour_tests {
    use super::*;

    fn row(text: &str) -> DRow {
        let st = Style { fg: Rgb(0, 0, 0), bg: None, bold: false, italic: false, underline: false, strike: false };
        DRow {
            cells: text.chars().map(|ch| Cell { ch, w: if ch.is_ascii() { 1 } else { 2 }, style: st.clone(), button: None, marks: None }).collect(),
            ..Default::default()
        }
    }

    fn text(r: &DRow) -> String {
        r.cells.iter().map(|c| c.ch).collect()
    }

    #[test]
    fn div_cells_paint_over_rows() {
        let mut r = row("abcdef");
        paint_cells(&mut r, 2, row("XY").cells);
        assert_eq!(text(&r), "abXYef");
        // past the end: padded
        let mut r = row("ab");
        paint_cells(&mut r, 4, row("Z").cells);
        assert_eq!(text(&r), "ab  Z");
        // a wide character cut in half leaves a blank
        let mut r = row("a漢b");
        paint_cells(&mut r, 2, row("Q").cells);
        assert_eq!(text(&r), "a Qb");
    }

    #[test]
    fn dark_tints_stay_visible() {
        // dim teal / brown map tiles keep their hue
        assert_eq!(rgb16(Rgb(0x01, 0x3d, 0x3d)), (6, false));
        assert_eq!(rgb16(Rgb(0x47, 0x29, 0x1f)), (1, false));
        assert_eq!(rgb16(Rgb(0xc9, 0xa1, 0x79)), (3, true));
        assert_eq!(rgb16(Rgb(0xff, 0x66, 0xcc)), (5, true));
        assert_eq!(rgb16(Rgb(0x50, 0xa0, 0xf0)), (6, true));
        // the grey vignette fades out through dark grey to black
        assert_eq!(rgb16(Rgb(0x79, 0x79, 0x79)), (0, true));
        assert_eq!(rgb16(Rgb(0x29, 0x29, 0x29)), (0, true));
        assert_eq!(rgb16(Rgb(0x0b, 0x0b, 0x0b)), (0, false));
        assert_eq!(rgb16(Rgb(0xc0, 0xc0, 0xc0)), (7, false));
        assert_eq!(rgb16(Rgb(0xff, 0xff, 0xff)), (7, true));
        assert_eq!(rgb16(Rgb(0xff, 0xff, 0x00)), (3, true));
        // 256 colours: dark greys use the grey ramp, not cube black
        assert_eq!(rgb256(Rgb(0x15, 0x15, 0x15)), 233);
        assert_eq!(rgb256(Rgb(0xff, 0x00, 0x00)), 196);
        assert_eq!(rgb256(Rgb(0, 0, 0)), 16);
    }
}

impl Frontend for Tty {
    fn flush(&mut self, con: &mut Console, res: &mut Resources) {
        if con.redraw {
            self.render(con, res, None);
        }
    }

    fn refresh(&mut self, con: &mut Console, res: &mut Resources) {
        self.render(con, res, None);
    }

    /// Show the console and wait for input: typed text + Enter, a button
    /// click, any key/click for WAIT, or a timeout. Scrolling keys and the
    /// wheel move through the log meanwhile.
    fn input(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq) -> InputResult {
        if !std::mem::take(&mut self.keep_buf) || req.kind == InputKind::Wait {
            self.line.clear();
        }
        self.macros_on = con.key_macro;
        if self.macros_on {
            // (loaded now, so the hints know which F keys hold macros)
            self.macros();
        }
        self.scroll = 0;
        self.render(con, res, Some(req));
        self.copy_text(con, if req.kind == InputKind::Wait { crate::clipboard::Trigger::Wait } else { crate::clipboard::Trigger::Input });
        let deadline = req.timeout_ms.map(|t| Instant::now() + Duration::from_millis(t));
        let (mut last_cols, mut last_rows) = (self.cols, self.rows);
        // a redraw owed for input already handled: done once the burst of
        // input (mouse motion, a held key) has been read, not per event
        let mut owed = false;
        // only the console pointer moved: repaint just its cells
        let mut pointer_owed = false;
        loop {
            if self.hangup {
                return InputResult::Quit;
            }
            if (owed || pointer_owed) && self.queued.is_empty() && self.pending.is_empty() && !self.input_waiting() {
                if owed || self.hover_swaps_image() {
                    self.render(con, res, Some(req));
                } else {
                    self.repaint_hover(con.focus_fg);
                }
                owed = false;
                pointer_owed = false;
            }
            let wait = match deadline {
                Some(d) => {
                    let now = Instant::now();
                    if now >= d {
                        self.keep_buf = true;
                        return InputResult::Timeout;
                    }
                    ((d - now).as_millis() as i32).min(200)
                }
                None => 200,
            };
            // SETANIMETIMER: redraw animated sprites that often (at most
            // 10 times a second, terminals are slower than a window)
            let anim = self.animating && res.anim_timer > 0;
            let wait = if anim { wait.min(res.anim_timer.max(100) as i32) } else { wait };
            // a tooltip's delay or duration running out
            let tip_next = self.tip_due(con).1;
            let wait = match tip_next {
                Some(t) => wait.min(t.saturating_duration_since(Instant::now()).as_millis() as i32 + 1),
                None => wait,
            };
            // dialog being translated: look for the results now and then
            let wait = if self.mt.as_ref().is_some_and(|m| m.busy()) { wait.min(250) } else { wait };
            // the pointer blinking
            let wait = self.blink_wait(con.focus_fg).map_or(wait, |b| wait.min(b));
            // a macro key's note going away
            let wait = match &self.macro_note {
                Some((_, t)) => wait.min(t.saturating_duration_since(Instant::now()).as_millis() as i32 + 1),
                None => wait,
            };
            let Some(k) = self.game_key(wait, false) else {
                let (c, r, _, _) = winsize();
                let translated = self.mt.as_mut().is_some_and(|m| m.poll());
                let note_gone = self.macro_note.as_ref().is_some_and(|(_, t)| Instant::now() >= *t);
                if note_gone {
                    self.macro_note = None;
                }
                if (c, r) != (last_cols, last_rows) || anim || translated || note_gone || self.tip_due(con).0 != self.tip_drawn {
                    last_cols = c;
                    last_rows = r;
                    self.render(con, res, Some(req));
                }
                continue;
            };
            let mut redraw = false;
            match k {
                Key::CtrlC => return InputResult::Quit,
                Key::CtrlL => {
                    self.force_frame = true;
                    redraw = true;
                }
                Key::Title | Key::Restart | Key::Reload => {
                    if let Some(r) = self.anchor_hotkey(con, res, req, &k) {
                        return r;
                    }
                    redraw = true;
                }
                Key::Settings => match self.settings_overlay(con, res, req) {
                    OverlayExit::Close => redraw = true,
                    OverlayExit::Title => {
                        if let Some(r) = self.anchor_hotkey(con, res, req, &Key::Title) {
                            return r;
                        }
                        redraw = true;
                    }
                    OverlayExit::Restart => {
                        if let Some(r) = self.anchor_hotkey(con, res, req, &Key::Restart) {
                            return r;
                        }
                        redraw = true;
                    }
                    OverlayExit::RestartDebug => {
                        if let Some(r) = self.restart(con, res, req, true) {
                            return r;
                        }
                        redraw = true;
                    }
                    OverlayExit::Reload => return InputResult::Hotkey(crate::frontend::Hotkey::Reload),
                    OverlayExit::Config => return InputResult::Hotkey(crate::frontend::Hotkey::Config),
                    OverlayExit::Quit => return InputResult::Quit,
                    OverlayExit::SwitchGame => {
                        // start over on the launcher (this game's unsaved
                        // progress goes, as the overlay warns)
                        self.shutdown();
                        let e = crate::sys::exec_self(&["--choose".into()]);
                        eprintln!("emuera-tty: can't restart: {}", e);
                        return InputResult::Quit;
                    }
                },
                // Up / Down: the inputs given before, as in Emuera (at a
                // WAIT, which shows no input line, they scroll)
                Key::Up | Key::Down if req.kind != InputKind::Wait => {
                    self.line.recall(if matches!(k, Key::Up) { -1 } else { 1 });
                    redraw = true;
                }
                Key::PageUp | Key::Up | Key::ScrollUp => {
                    self.scroll += if matches!(k, Key::PageUp) { self.rows / 2 } else { 1 };
                    redraw = true;
                }
                Key::PageDown | Key::Down | Key::ScrollDown => {
                    self.scroll = self.scroll.saturating_sub(if matches!(k, Key::PageDown) { self.rows / 2 } else { 1 });
                    redraw = true;
                }
                // Home / End: in the text typed, or through the log when
                // there is none
                Key::Home | Key::End if req.kind != InputKind::Wait && !self.line.is_empty() => {
                    if matches!(k, Key::Home) {
                        self.line.home();
                    } else {
                        self.line.end();
                    }
                    redraw = true;
                }
                Key::Home => {
                    self.scroll = usize::MAX / 2;
                    redraw = true;
                }
                Key::End => {
                    self.scroll = 0;
                    redraw = true;
                }
                Key::Mouse { btn, x, y, press } => {
                    // Only left (0) and right (2) click jump to end; middle (1) does not
                    let button = btn & !(4 | 8 | 16);
                    if press && self.scroll > 0 && (button == 0 || button == 2) {
                        self.scroll = 0;
                        redraw = true;
                    }
                    if press {
                        self.click_copies(con, btn, x, y);
                    }
                    let (x, y) = (x.saturating_sub(1) as usize, y.saturating_sub(1) as usize);
                    // ignore Shift / Alt / Ctrl so modified clicks still work
                    match btn & !(4 | 8 | 16) {
                        64 => {
                            self.scroll += self.wheel;
                            redraw = true;
                        }
                        65 => {
                            self.scroll = self.scroll.saturating_sub(self.wheel);
                            redraw = true;
                        }
                        b if b & 32 != 0 => {
                            // motion: update hover highlight
                            self.mouse = Some((x, y));
                            self.show_pointer();
                            // the console pointer is drawn by us: move it, but
                            // only once a burst of motion has been read
                            if self.own_pointer() || self.button_at(x, y) != self.hover {
                                pointer_owed = true;
                            }
                        }
                        0 if press => {
                            if req.kind == InputKind::Wait {
                                return InputResult::Text(String::new());
                            }
                            // Check for Settings button click (top bar, row 0)
                            if y == 0 {
                                if let Some(row) = self.btn_map.first() {
                                    for (s, e, b) in row {
                                        if *b == usize::MAX && x >= *s && x < *e {
                                            let _ = self.settings_overlay(con, res, req);
                                            redraw = true;
                                            break;
                                        }
                                    }
                                }
                            }
                            if let Some(b) = self.click_at(x, y) {
                                // a click goes through Emuera's input box,
                                // so it is in the history too
                                let v = self.buttons[b].to_string();
                                self.line.remember(&v);
                                if !v.is_empty() {
                                    con.reset_style();
                                    con.print_ex(&v, false);
                                    con.newline();
                                }
                                return InputResult::Text(v);
                            }
                        }
                        // a middle click on a translated line flips it
                        // between the original and the translation (also at
                        // a WAIT)
                        1 if press => redraw |= self.toggle_translation(y),
                        // a right click: as Emuera, at a WAIT continue and
                        // skip the following waits up to the next choice;
                        // it never chooses a button (Emuera's MainWindow
                        // selects on a left or middle click only)
                        2 if press && req.kind == InputKind::Wait => {
                            self.scroll = 0;
                            con.reset_style();
                            con.print_ex("[skip]", false);
                            con.newline();
                            return InputResult::Skip(String::new());
                        }
                        _ => {}
                    }
                }
                Key::Skip if req.kind == InputKind::Wait => {
                    self.scroll = 0;
                    con.reset_style();
                    con.print_ex("[skip]", false);
                    con.newline();
                    return InputResult::Skip(String::new());
                }
                // Emuera's macro keys (MainWindow): Fn puts its macro in
                // the input box, Shift+Fn stores what is typed there; at a
                // WAIT, which has no box here, Fn gives it at once
                Key::F(n, false) if req.kind == InputKind::Wait => {
                    let text = self.macros().get(n as usize - 1).to_string();
                    if !text.is_empty() {
                        self.scroll = 0;
                        con.reset_style();
                        con.print_ex(&text, false);
                        con.newline();
                        return InputResult::Line(text);
                    }
                }
                Key::F(n, shift) => {
                    self.macro_key(n, shift);
                    redraw = true;
                }
                Key::Group(g) => {
                    self.macro_group(g);
                    redraw = true;
                }
                // "Scroll to end on keyboard input": any key while scrolled back jumps to end
                _ if self.scroll > 0
                    && self.scroll_end_on_key
                    && !matches!(
                        k,
                        Key::CtrlC
                            | Key::CtrlL
                            | Key::Title
                            | Key::Restart
                            | Key::Reload
                            | Key::Settings
                            | Key::F(..)
                            | Key::Group(_)
                            | Key::Pad(_)
                            | Key::Other
                    ) =>
                {
                    self.scroll = 0;
                    redraw = true;
                }
                Key::Enter if self.scroll > 0 => {
                    self.scroll = 0;
                    redraw = true;
                }
                Key::Enter => {
                    let s = self.line.take();
                    self.line.remember(&s);
                    if !s.is_empty() {
                        con.reset_style();
                        con.print_ex(&s, false);
                        con.newline();
                    }
                    return InputResult::Line(s);
                }
                Key::Backspace => {
                    self.line.backspace();
                    redraw = true;
                }
                Key::Delete => {
                    self.line.delete();
                    redraw = true;
                }
                Key::Left => {
                    self.line.left();
                    redraw = true;
                }
                Key::Right => {
                    self.line.right();
                    redraw = true;
                }
                Key::Esc => {
                    self.line.clear();
                    redraw = true;
                }
                Key::Char('\u{15}') => {
                    self.line.clear();
                    redraw = true;
                }
                Key::Char(c) => {
                    if req.one {
                        let s = c.to_string();
                        self.line.remember(&s);
                        con.reset_style();
                        con.print_ex(&s, false);
                        con.newline();
                        return InputResult::Text(s);
                    }
                    self.line.insert(c);
                    redraw = true;
                }
                _ => {}
            }
            owed |= redraw;
        }
    }

    fn mouse_cell(&self) -> Option<(usize, usize)> {
        self.mouse
    }

    fn await_input(&mut self, con: &mut Console, res: &mut Resources, ms: u64) -> bool {
        use crate::frontend::char_keycode;
        // the input box shows what is typed, as at an INPUTS
        let req = InputReq { kind: InputKind::Str, one: false, timeout_ms: None, stop_skip: false };
        self.render(con, res, Some(&req));
        let end = Instant::now() + Duration::from_millis(ms);
        let mut changed = false;
        loop {
            if self.hangup {
                return true;
            }
            let left = end.saturating_duration_since(Instant::now()).as_millis() as i32;
            let Some(k) = self.game_key(left, false) else {
                if Instant::now() >= end {
                    break;
                }
                continue;
            };
            let hit = |t: &mut Tty, code: i64| {
                t.keys_hit.insert(code);
            };
            match k {
                Key::CtrlC => return true,
                Key::Char(c) => {
                    self.line.insert(c);
                    hit(self, char_keycode(c).0);
                    changed = true;
                }
                Key::Backspace => {
                    self.line.backspace();
                    hit(self, 8);
                    changed = true;
                }
                Key::Delete => {
                    self.line.delete();
                    hit(self, 46);
                    changed = true;
                }
                Key::Enter => hit(self, 13),
                Key::Esc => hit(self, 27),
                Key::Left => {
                    self.line.left();
                    hit(self, 37);
                    changed = true;
                }
                Key::Up => hit(self, 38),
                Key::Right => {
                    self.line.right();
                    hit(self, 39);
                    changed = true;
                }
                Key::Down => hit(self, 40),
                Key::Mouse { btn, x, y, press } => {
                    let (x, y) = (x.saturating_sub(1) as usize, y.saturating_sub(1) as usize);
                    self.mouse = Some((x, y));
                    // Windows codes: 1 left, 2 right, 4 middle
                    let code = match btn & !(4 | 8 | 16 | 32) {
                        0 => 1,
                        2 => 2,
                        1 => 4,
                        _ => 0,
                    };
                    if btn & 32 != 0 {
                        self.show_pointer();
                        self.repaint_hover(con.focus_fg);
                    } else if code != 0 && press {
                        self.keys_down.insert(code);
                        hit(self, code);
                    } else if code != 0 {
                        self.keys_down.remove(&code);
                    }
                }
                _ => {}
            }
        }
        if changed {
            self.render(con, res, Some(&req));
        }
        false
    }

    fn textbox(&self) -> String {
        self.line.text().to_string()
    }

    fn set_textbox(&mut self, s: &str) {
        self.line.set(s);
    }

    fn hover_value(&self) -> String {
        match self.hover {
            Some(b) if self.is_button(b) => self.buttons[b].as_string(),
            _ => String::new(),
        }
    }

    fn key_state(&mut self, code: i64, triggered: bool) -> bool {
        if triggered {
            self.keys_hit.remove(&code)
        } else {
            // a terminal reports key presses, not releases: a key counts as
            // held until it is read once
            self.keys_down.contains(&code) || self.keys_hit.contains(&code)
        }
    }

    /// INPUTMOUSEKEY: the next click, wheel step or key (mouse moves only
    /// update the pointer, which MOUSEX / MOUSEY read), or the timeout.
    fn input_raw(&mut self, con: &mut Console, res: &mut Resources, timeout_ms: Option<u64>) -> crate::frontend::RawInput {
        use crate::frontend::{char_keycode, RawInput};
        self.scroll = 0;
        self.line.clear();
        self.macros_on = con.key_macro;
        if self.macros_on {
            // (loaded now, so the hints know which F keys hold macros)
            self.macros();
        }
        self.render(con, res, None);
        let deadline = timeout_ms.map(|t| Instant::now() + Duration::from_millis(t));
        let mut moved = false;
        let cbg_buttons = res.cbg.iter().any(|l| l.button.is_some());
        loop {
            if self.hangup {
                return RawInput::Quit;
            }
            if moved && self.queued.is_empty() && self.pending.is_empty() && !self.input_waiting() {
                // a picture button may light up under the pointer
                if cbg_buttons || self.hover_swaps_image() {
                    self.render(con, res, None);
                } else {
                    self.repaint_hover(con.focus_fg);
                }
                moved = false;
            }
            let wait = match deadline {
                Some(d) => {
                    let now = Instant::now();
                    if now >= d {
                        return RawInput::Timeout;
                    }
                    ((d - now).as_millis() as i32).clamp(1, 200)
                }
                None => 200,
            };
            let wait = self.blink_wait(con.focus_fg).map_or(wait, |b| wait.min(b));
            let Some(k) = self.game_key(wait, true) else { continue };
            let key = |code: i64| RawInput::Key { code, data: code };
            match k {
                Key::CtrlC => return RawInput::Quit,
                Key::Title | Key::Restart | Key::Reload => {
                    let req = InputReq { kind: InputKind::Wait, one: false, timeout_ms: None, stop_skip: false };
                    let r = self.anchor_hotkey(con, res, &req, &k);
                    self.force_frame = true;
                    self.render(con, res, None);
                    match r {
                        Some(InputResult::Hotkey(h)) => return RawInput::Hotkey(h),
                        Some(_) => return RawInput::Quit,
                        None => {}
                    }
                }
                Key::Settings => match self.settings_overlay(con, res, &InputReq { kind: InputKind::Wait, one: false, timeout_ms: None, stop_skip: false }) {
                    OverlayExit::Quit => return RawInput::Quit,
                    OverlayExit::Config => return RawInput::Hotkey(crate::frontend::Hotkey::Config),
                    // (INPUTMOUSEKEY menus, pops-tw's title: restarts too)
                    e @ (OverlayExit::Restart | OverlayExit::RestartDebug) => {
                        let req = InputReq { kind: InputKind::Wait, one: false, timeout_ms: None, stop_skip: false };
                        if self.restart(con, res, &req, matches!(e, OverlayExit::RestartDebug)).is_some() {
                            return RawInput::Quit;
                        }
                        self.force_frame = true;
                        self.render(con, res, None);
                    }
                    _ => {
                        self.force_frame = true;
                        self.render(con, res, None);
                    }
                },
                Key::CtrlL => {
                    self.force_frame = true;
                    self.render(con, res, None);
                }
                Key::Mouse { btn, x, y, press } => {
                    if press {
                        self.click_copies(con, btn, x, y);
                    }
                    let cell = (x.saturating_sub(1) as usize, y.saturating_sub(1) as usize);
                    match btn & !(4 | 8 | 16) {
                        64 => return RawInput::Wheel { up: true, cell },
                        65 => return RawInput::Wheel { up: false, cell },
                        b if b & 32 != 0 => {
                            self.mouse = Some(cell);
                            self.show_pointer();
                            moved = true;
                        }
                        b @ 0..=2 if press => {
                            self.mouse = Some(cell);
                            let button = self.click_at(cell.0, cell.1).map(|i| self.buttons[i].clone());
                            return RawInput::Click { btn: b as u8, cell, button };
                        }
                        _ => {}
                    }
                }
                // Windows key codes (System.Windows.Forms.Keys)
                Key::Enter => return key(13),
                Key::Backspace => return key(8),
                Key::Esc => return key(27),
                Key::PageUp => return key(33),
                Key::PageDown => return key(34),
                Key::End => return key(35),
                Key::Home => return key(36),
                Key::Left => return key(37),
                Key::Up => return key(38),
                Key::Right => return key(39),
                Key::Down => return key(40),
                // (as Ctrl, Keys.Control: Alt can't be told apart here)
                Key::ScrollUp => return RawInput::Key { code: 38, data: 38 | 0x20000 },
                Key::ScrollDown => return RawInput::Key { code: 40, data: 40 | 0x20000 },
                Key::Click => return key(45),
                Key::Delete => return key(46),
                Key::Skip => return key(9),
                Key::Char(c) => {
                    let (code, data) = char_keycode(c);
                    return RawInput::Key { code, data };
                }
                // macro keys work on the input box here too (Emuera takes
                // them before the game sees keys)
                Key::F(n, shift) => self.macro_key(n, shift),
                Key::Group(g) => self.macro_group(g),
                // (game_key has made the pad's moves and clicks mouse events)
                Key::Other | Key::Pad(_) => {}
            }
        }
    }

    fn cols(&self) -> usize {
        self.cols
    }

    fn rows(&self) -> usize {
        self.rows
    }

    fn is_fullscreen(&self) -> bool {
        true
    }

    /// Ctrl+C typed (or the terminal gone) while a script runs. Other keys
    /// read meanwhile stay pending for the next input.
    /// Computing for more than a moment: say so in the bottom row, with
    /// the function running and the memory in use.
    fn busy(&mut self, con: &Console) {
        let since = *self.busy_since.get_or_insert_with(Instant::now);
        if !self.status_info || self.cols < 30 || since.elapsed() < Duration::from_millis(400) {
            return;
        }
        let mut s = format!("… {}", t!("working"));
        if !con.running.is_empty() {
            s += &format!("  @{}", con.running);
        }
        // where the live counter sits in the text (chars from the start, its
        // length) and its band, to colour it (the peak after it stays plain)
        let mut mem_at: Option<(usize, usize, crate::sys::MemoryLevel)> = None;
        if let Some(m) = self.memory().filter(|_| self.show_memory) {
            s += "  ";
            if let Some(l) = self.mem_level() {
                mem_at = Some((printable(&s).chars().count(), m.find(" MB").map_or(m.len(), |i| i + 3), l));
            }
            s += &m;
        }
        let mut line = String::new();
        let mut w = 0;
        for c in printable(&s).chars() {
            let cw = UnicodeWidthChar::width(c).unwrap_or(0);
            if w + cw >= self.cols {
                break;
            }
            line.push(c);
            w += cw;
        }
        let shown = line.chars().count();
        let text = match mem_at.filter(|&(at, len, _)| at + len <= shown) {
            Some((at, len, lvl)) => {
                let pre: String = line.chars().take(at).collect();
                let live: String = line.chars().skip(at).take(len).collect();
                let post: String = line.chars().skip(at + len).collect();
                format!("{}\x1b[0m{}{}\x1b[0;2m{}", pre, self.mem_fg(lvl), live, post)
            }
            None => line,
        };
        write_all(format!("\x1b[{};1H\x1b[0;2m{}\x1b[0m\x1b[K", self.rows, text).as_bytes());
        // the next frame must repaint the row, even if nothing else changed
        self.last_frame = 0;
    }

    fn macro_stop(&mut self) -> bool {
        if self.input_waiting() {
            self.read_bytes(0);
        }
        if let Some(i) = self.queued.iter().position(|k| matches!(k, Key::Esc)) {
            self.queued.drain(..=i);
            return true;
        }
        // a lone ESC: one not starting a sequence
        let p = &self.pending;
        let lone = (0..p.len()).find(|&i| p[i] == 0x1b && p.get(i + 1).is_none_or(|&c| c == 0x1b));
        if let Some(i) = lone {
            self.pending.drain(..=i);
            return true;
        }
        false
    }

    fn poll_interrupt(&mut self) -> bool {
        if self.input_waiting() {
            self.read_bytes(0);
        }
        if let Some(i) = self.pending.iter().position(|&b| b == 0x03) {
            self.pending.drain(..=i);
            return true;
        }
        self.hangup
    }

    /// Restore the terminal (mouse off, main screen, cooked mode).
    fn set_game(&mut self, game: &std::path::Path, save_dir: &std::path::Path) {
        self.game = Some((game.to_path_buf(), save_dir.to_path_buf()));
    }
    fn shutdown(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        // stored macros go to macro.txt, as Emuera writes it on closing
        if let (Some(m), Some(p)) = (&self.macros, self.macro_path()) {
            if let Err(e) = m.save(&p) {
                self.notice = Some(t!("can't write {file}: {error}", file = p.display(), error = e));
            }
        }
        let mut out = Vec::new();
        self.gfx.shutdown(&mut out);
        if !text_console(&self.term) {
            out.extend_from_slice(b"\x1b[>4m");
        } else {
            // no alternate screen there: the last frame (and the keyboard
            // pointer) would stay under the shell's prompt
            out.extend_from_slice(b"\x1b[0m\x1b[2J\x1b[H");
        }
        out.extend_from_slice(b"\x1b[?1006l\x1b[?1015l\x1b[?1003l\x1b[?1002l\x1b[?1000l\x1b[0m\x1b[?25h\x1b[?1049l");
        write_all(&out);
        if let Some(n) = &self.notice {
            eprintln!("emuera-tty: {}", n);
        }
        if let Some(r) = &self.raw_mode {
            r.restore();
        }
    }

    /// Show a Yes/No confirmation dialog (for infinite loop warning, etc.).
    /// Returns true for Yes, false for No.
    fn confirm(&mut self, con: &mut Console, res: &mut Resources, msg: &str) -> bool {
        use crate::tty::saves::Pick;
        let mut sel = 1; // default to "No"
        self.no_images = true;
        let r = self.list_box(
            con,
            res,
            &InputReq { kind: InputKind::Str, one: false, timeout_ms: None, stop_skip: false },
            "Infinite loop detected",
            &["Yes".to_string(), "No".to_string()],
            0,
            &mut sel,
            msg,
        );
        matches!(r, Pick::Row(0))
    }

    /// Show a second confirmation dialog with custom button labels.
    /// Returns true for "Exit anyway", false for "Wait for the line to finish".
    fn confirm2(&mut self, con: &mut Console, res: &mut Resources, msg: &str) -> bool {
        use crate::tty::saves::Pick;
        let mut sel = 1; // default to "Wait for the line to finish"
        self.no_images = true;
        let r = self.list_box(
            con,
            res,
            &InputReq { kind: InputKind::Str, one: false, timeout_ms: None, stop_skip: false },
            "Confirm exit",
            &["Exit anyway".to_string(), "Wait for the line to finish".to_string()],
            0,
            &mut sel,
            msg,
        );
        matches!(r, Pick::Row(0))
    }

    fn debugger_input(&mut self, _con: &mut Console, _res: &mut Resources, prompt: &str) -> String {
        // Disable mouse reporting (including UTF-8 mode ?1005 for scroll wheel)
        write_all(b"\x1b[?1006l\x1b[?1015l\x1b[?1005l\x1b[?1003l\x1b[?1002l\x1b[?1000l");
        let _ = io::stdout().flush();
        // Temporarily restore cooked mode
        if let Some(r) = &self.raw_mode {
            r.restore();
        }
        eprint!("{}", prompt);
        let _ = io::stderr().flush();
        // Read line while filtering out mouse escape sequences
        let line = read_line_filtered();
        // Re-enter raw mode using the original termios
        if let Some(r) = &self.raw_mode {
            let _ = r.re_enter();
        }
        // Re-enable mouse reporting
        write_all(b"\x1b[?1000h\x1b[?1002h\x1b[?1003h\x1b[?1005h\x1b[?1015h\x1b[?1006h");
        line
    }
}

/// Read a line from stdin, filtering out mouse escape sequences.
/// Mouse sequences: VT200 (\x1b[M + 3 bytes), SGR (\x1b[<...M/m), urxvt (\x1b[...M), UTF-8 (\x1b[<...M)
#[cfg(unix)]
fn read_line_filtered() -> String {
    use libc;
    use std::os::fd::AsRawFd;
    let stdin_fd = io::stdin().as_raw_fd();
    let mut line = String::new();
    let mut buf = [0u8; 1024];
    let mut escape_state = 0; // 0=normal, 1=ESC, 2=CSI, 3=mouse params
    let mut mouse_bytes_remaining = 0;

    loop {
        // Use poll to wait for input with a small timeout
        let mut fds = [libc::pollfd { fd: stdin_fd, events: libc::POLLIN, revents: 0 }];
        let ret = unsafe { libc::poll(fds.as_mut_ptr(), 1, 50) };
        if ret <= 0 {
            continue; // timeout or error, keep waiting
        }

        // Read available data
        let n = unsafe { libc::read(stdin_fd, buf.as_mut_ptr() as *mut libc::c_void, buf.len()) };
        if n <= 0 {
            break; // EOF or error
        }

        for &b in &buf[..n as usize] {
            match escape_state {
                0 => {
                    if b == 0x1b {
                        // ESC
                        escape_state = 1;
                    } else if b == b'\n' || b == b'\r' {
                        return line.trim_end_matches(['\r', '\n']).to_string();
                    } else {
                        line.push(b as char);
                    }
                }
                1 => {
                    if b == b'[' {
                        escape_state = 2;
                    } else if b == b'M' || b == b'm' {
                        // ESC M or ESC m - could be start of mouse sequence, but not CSI
                        // Treat as normal chars
                        line.push(0x1b as char);
                        line.push(b as char);
                        escape_state = 0;
                    } else {
                        // Not a CSI sequence, treat ESC as literal
                        line.push(0x1b as char);
                        line.push(b as char);
                        escape_state = 0;
                    }
                }
                2 => {
                    // CSI sequence - check for mouse sequences and cursor keys
                    if b == b'<' || b == b'M' {
                        // CSI < ... or CSI M - likely mouse sequence
                        escape_state = 3;
                        if b == b'M' {
                            // VT200 mouse: ESC [ M + 3 bytes
                            mouse_bytes_remaining = 3;
                        }
                    } else if b.is_ascii_digit() || b == b';' || b == b':' || b == b'?' {
                        // Parameter bytes, stay in CSI state
                    } else if (0x40..=0x7e).contains(&b) {
                        // Final byte of CSI sequence
                        if mouse_bytes_remaining > 0 {
                            // This was a mouse sequence, discard it all
                            mouse_bytes_remaining = 0;
                            escape_state = 0;
                        } else if matches!(b, b'A' | b'B' | b'C' | b'D') {
                            // Cursor key (Up/Down/Right/Left) - likely from scroll wheel
                            // when mouse reporting is disabled. Discard silently.
                            escape_state = 0;
                        } else {
                            // Not a mouse sequence or cursor key, treat as literal
                            line.push(0x1b as char);
                            line.push('[');
                            // We've lost the intermediate bytes, but this is rare for non-mouse
                            line.push(b as char);
                            escape_state = 0;
                        }
                    } else {
                        // Unexpected byte, abort escape sequence
                        escape_state = 0;
                    }
                }
                3 => {
                    // In mouse parameter/data bytes
                    if mouse_bytes_remaining > 0 {
                        mouse_bytes_remaining -= 1;
                        if mouse_bytes_remaining == 0 {
                            escape_state = 0;
                        }
                    } else if b == b'M' || b == b'm' {
                        // End of SGR/urxvt mouse sequence
                        escape_state = 0;
                    }
                    // Otherwise keep consuming mouse bytes
                }
                _ => {}
            }
        }
    }
    line.trim_end_matches(['\r', '\n']).to_string()
}

/// Windows implementation: in cooked mode (after RawMode::restore), just use standard read_line
/// since mouse reporting is disabled via escape sequences.
#[cfg(windows)]
fn read_line_filtered() -> String {
    use std::io::{stdin, BufRead};
    let mut line = String::new();
    stdin().lock().read_line(&mut line).unwrap_or_default();
    line.trim_end_matches(['\r', '\n']).to_string()
}

impl Drop for Tty {
    fn drop(&mut self) {
        self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::{char_cells, console_lookalike, Ambi};

    /// tmux is recognised by its own `TERM` / `TERM_PROGRAM`, not by an
    /// inherited `TMUX` in a terminal opened from a tmux pane
    #[test]
    fn tmux_detection() {
        use super::in_tmux_env as t;
        assert!(t(true, "tmux-256color", "tmux"));
        assert!(t(true, "screen-256color", ""));
        assert!(t(true, "xterm-256color", "tmux"));
        assert!(t(true, "tmux-256color", "ghostty"));
        assert!(!t(true, "xterm-ghostty", "ghostty"));
        assert!(!t(true, "xterm-256color", "WezTerm"));
        assert!(!t(false, "tmux-256color", "tmux"));
    }

    /// A bar drawn as two rectangles (filled, empty) is as wide whatever
    /// the split, so a column after it lines up (eratohoReverse's gauges:
    /// 800% of the font in all, 50% a cell)
    #[test]
    fn shape_runs_keep_their_width() {
        for filled in [0, 1, 5, 37, 49, 50, 51, 99, 400, 763, 799, 800] {
            let mut frac = None;
            let mut col = 3;
            for w in [filled, 800 - filled] {
                if w > 0 {
                    col += super::shape_cells(col, &mut frac, w as f32 / 50.0, true);
                }
            }
            assert_eq!(col, 3 + 16, "split {} + {}", filled, 800 - filled);
        }
    }

    /// Greek and Cyrillic: two columns in MS Gothic / MS Mincho (Emuera's
    /// default), one in other fonts, the proportional MS ones included
    #[test]
    fn letters_follow_the_font() {
        for f in ["", "ＭＳ ゴシック", "MS Gothic", "ＭＳ 明朝", "MS Mincho"] {
            assert!(super::letters_are_wide(f), "{:?}", f);
        }
        for f in ["ＭＳ Ｐゴシック", "MS PGothic", "MS UI Gothic", "メイリオ", "Yu Gothic", "Consolas", "Noto Sans CJK JP"] {
            assert!(!super::letters_are_wide(f), "{:?}", f);
        }
        use super::{char_cells, Ambi};
        assert_eq!(char_cells('Ж', Ambi::Pad, None, true), (1, 1, ' '));
        assert_eq!(char_cells('Ж', Ambi::Pad, None, false), (1, 0, ' '));
        assert_eq!(char_cells('Ω', Ambi::Pad, None, false), (1, 0, ' '));
        // the rest of Shift-JIS keeps Emuera's two columns
        assert_eq!(char_cells('※', Ambi::Pad, None, false), (1, 1, ' '));
        assert_eq!(char_cells('林', Ambi::Pad, None, false), (2, 0, ' '));
    }

    #[test]
    fn letters_in_words_are_narrow() {
        use super::{letter_widths, Letters};
        let wide = |s: &str, l| letter_widths(s, l).filter(|&(c, _)| super::is_greek_cyrillic(c)).map(|(_, w)| w).collect::<Vec<_>>();
        // a letter on its own keeps MS Gothic's two columns
        assert_eq!(wide("(´Д｀)", Letters::ByWord), [true]);
        assert_eq!(wide("(・ω・)ノ", Letters::ByWord), [true]);
        assert_eq!(wide("Σ：12 α", Letters::ByWord), [true, true]);
        // a word written in those scripts takes one each
        assert_eq!(wide("「Привет, Ω」", Letters::ByWord), [false, false, false, false, false, false, true]);
        assert_eq!(wide("αβ", Letters::ByWord), [false, false]);
        assert_eq!(wide("Привет", Letters::Wide), [true; 6]);
        assert_eq!(wide("Д", Letters::Narrow), [false]);
    }

    /// The fonts that draw a backslash as ¥ (Emuera's default among them)
    #[test]
    fn yen_fonts() {
        for f in ["", "ＭＳ ゴシック", "ＭＳ Ｐゴシック", "MS Gothic", "ＭＳ 明朝", "MS PMincho", "MS UI Gothic", "メイリオ", "Meiryo UI"]
        {
            assert!(super::backslash_is_yen(f), "{:?}", f);
        }
        for f in ["游ゴシック", "Yu Gothic", "Consolas", "DFKai-SB", "Courier New", "ＤＦ平成ゴシック体"] {
            assert!(!super::backslash_is_yen(f), "{:?}", f);
        }
    }

    /// The column in a cursor position report, among other replies
    #[test]
    fn cursor_reports() {
        assert_eq!(super::cpr_column("\x1b[12;2R\x1b]11;rgb:0/0/0\x07\x1b[?62c"), Some(2));
        assert_eq!(super::cpr_column("\x1b[?6c\x1b[1;3R"), Some(3));
        assert_eq!(super::cpr_column("\x1b[?62;4c"), None);
    }

    /// The terminal's background from its OSC 11 reply
    #[test]
    fn osc11_replies() {
        let r = crate::console::Rgb;
        assert_eq!(super::parse_osc11("\x1b]11;rgb:2828/2c2c/3434\x1b\\\x1b[?62;4c"), Some(r(0x28, 0x2c, 0x34)));
        assert_eq!(super::parse_osc11("\x1b]11;rgb:ff/80/00\x07"), Some(r(255, 128, 0)));
        assert_eq!(super::parse_osc11("\x1b[?62;4c"), None);
    }

    /// Ctrl / Alt + arrow scroll; plain and Shift+arrow don't
    #[test]
    fn scroll_modifiers() {
        for (p, want) in [("1;5", true), ("1;3", true), ("1;7", true), ("1;6", true), ("1;2", false), ("", false), ("1", false)] {
            assert_eq!(super::scroll_mods(p.as_bytes()), want, "{}", p);
        }
    }

    /// F-keys and Ctrl/Alt+digit from the CSI sequences terminals send
    #[test]
    fn function_keys() {
        let f = |p: &str, fin: u8| super::fkey(p.as_bytes(), fin);
        assert_eq!(f("", b'P'), Some((1, false)));
        assert_eq!(f("1;2", b'S'), Some((4, true)));
        assert_eq!(f("11", b'~'), Some((1, false)));
        assert_eq!(f("15", b'~'), Some((5, false)));
        assert_eq!(f("17;2", b'~'), Some((6, true)));
        assert_eq!(f("24", b'~'), Some((12, false)));
        // the Linux console's F13 / F20: Shift+F3 / F10
        assert_eq!(f("25", b'~'), Some((3, true)));
        assert_eq!(f("34", b'~'), Some((10, true)));
        // the illumos console's F1 / F12
        assert_eq!(f("224", b'z'), Some((1, false)));
        assert_eq!(f("235", b'z'), Some((12, false)));
        assert_eq!(f("214", b'z'), None);
        // not function keys: Insert, Home, arrows, Shift+Enter
        for (p, fin) in [("2", b'~'), ("1", b'~'), ("1;2", b'A'), ("27;2;13", b'~'), ("16", b'~')] {
            assert_eq!(f(p, fin), None, "{} {}", p, fin as char);
        }
        let g = |p: &str, fin: u8| super::digit_group(p.as_bytes(), fin);
        assert_eq!(g("27;5;49", b'~'), Some(1));
        assert_eq!(g("48;5", b'u'), Some(0));
        assert_eq!(g("57;3", b'u'), Some(9));
        // Shift+digit is a symbol, Ctrl+Enter no digit
        assert_eq!(g("50;2", b'u'), None);
        assert_eq!(g("27;5;13", b'~'), None);
    }

    #[test]
    fn recolor_pairs() {
        let r = crate::console::Rgb;
        assert_eq!(
            super::parse_recolor("01183c:111d3b, #000000:0x202020, bad, 12:zz"),
            vec![(r(1, 24, 60), r(0x11, 0x1d, 0x3b)), (r(0, 0, 0), r(0x20, 0x20, 0x20))]
        );
        assert!(super::parse_recolor("").is_empty());
    }

    #[test]
    fn osc66_draws_one_two_cell_glyph() {
        assert_eq!(char_cells('━', Ambi::Osc66, None, true), (2, 0, ' '));
        assert_eq!(char_cells('林', Ambi::Osc66, None, true), (2, 0, ' '));
        assert_eq!(char_cells('a', Ambi::Osc66, None, true), (1, 0, ' '));
    }

    #[test]
    fn emuera_widths_on_a_narrow_terminal() {
        assert_eq!(char_cells('a', Ambi::Pad, None, true), (1, 0, ' '));
        assert_eq!(char_cells('林', Ambi::Pad, None, true), (2, 0, ' ')); // already wide
        assert_eq!(char_cells('━', Ambi::Pad, None, true), (1, 1, '━')); // line continues
        assert_eq!(char_cells('┏', Ambi::Pad, None, true), (1, 1, '━'));
        assert_eq!(char_cells('┃', Ambi::Pad, None, true), (1, 1, ' ')); // no rightward stroke
        assert_eq!(char_cells('■', Ambi::Pad, None, true), (1, 1, '■'));
        assert_eq!(char_cells('※', Ambi::Pad, None, true), (1, 1, ' '));
        assert_eq!(char_cells('―', Ambi::Pad, None, true), (1, 1, '―')); // horizontal bar continues
        assert_eq!(char_cells('ｱ', Ambi::Pad, None, true), (1, 0, ' ')); // half-width kana: 1 in Emuera too
    }

    #[test]
    fn measured_width_wins() {
        // a terminal seen drawing ━ two cells wide needs no filler
        assert_eq!(char_cells('━', Ambi::Pad, Some(2), true), (2, 0, '━'));
        // one that draws ￤ narrow (tables say 2) gets a filler instead
        assert_eq!(char_cells('￤', Ambi::Pad, Some(1), true), (1, 1, ' '));
    }

    #[test]
    fn linux_console_draws_everything_in_one_cell() {
        // kanji, full-width space and letters: one cell plus a filler
        // the Linux console's font: missing glyphs drawn as lookalikes
        let has: std::collections::HashSet<char> = ['─', '■', '·'].into_iter().collect();
        assert_eq!(console_lookalike('【', &has), '[');
        assert_eq!(console_lookalike('）', &has), ')');
        assert_eq!(console_lookalike('Ａ', &has), 'A');
        assert_eq!(console_lookalike('ー', &has), '─');
        assert_eq!(console_lookalike('・', &has), '·');
        assert_eq!(console_lookalike('●', &has), 'o');
        assert_eq!(console_lookalike('漢', &has), '漢');
        assert_eq!(char_cells('漢', Ambi::Narrow, None, true), (1, 1, ' '));
        assert_eq!(char_cells('　', Ambi::Narrow, None, true), (1, 1, ' '));
        assert_eq!(char_cells('Ａ', Ambi::Narrow, None, true), (1, 1, ' '));
        assert_eq!(char_cells('━', Ambi::Narrow, None, true), (1, 1, '━'));
        assert_eq!(char_cells('a', Ambi::Narrow, None, true), (1, 0, ' '));
    }

    #[test]
    fn no_filler_when_terminal_draws_ambiguous_wide() {
        assert_eq!(char_cells('━', Ambi::Wide, None, true), (2, 0, '━'));
        assert_eq!(char_cells('■', Ambi::Wide, None, true), (2, 0, '■'));
    }
}

/// The interface drawn in every pseudo-language (and English) at two
/// screen sizes keeps its boxes in line, stays on the screen and keeps
/// every combining mark.
#[cfg(test)]
pub(crate) mod render_tests {
    use super::*;
    use crate::i18n::screen::Screen;

    /// The languages and screen sizes the drawing tests run in.
    pub(crate) fn cases() -> Vec<(&'static str, usize, usize)> {
        let langs = std::iter::once("en").chain(crate::i18n::PSEUDO.iter().copied());
        langs.flat_map(|l| [(l, 80, 30), (l, 132, 44)]).collect()
    }

    /// `bytes` drawn on a `cols` × `rows` screen, checked: the box in line,
    /// nothing off the screen, no dropped mark.
    pub(crate) fn check(what: &str, lang: &str, cols: usize, rows: usize, bytes: &[u8]) -> Screen {
        let mut scr = Screen::new(cols, rows);
        scr.feed(bytes);
        let at = format!("{} in {} at {}x{}", what, lang, cols, rows);
        assert!(!scr.overflow, "{}: text off the screen\n{}", at, scr.text());
        if let Err(e) = scr.check_box() {
            panic!("{}: {}\n{}", at, e, scr.text());
        }
        assert!(scr.dropped_mark().is_none(), "{}: a mark dropped in {:?}", at, scr.dropped_mark());
        scr
    }

    /// Choosing a language in the overlay translates the next frame (a
    /// one-message catalogue stands in for a real translation)
    #[test]
    fn language_setting_applies_at_once() {
        crate::i18n::install("ja", "msgid \"emuera-tty settings\"\nmsgstr \"emuera-tty の設定\"\n");
        let mut t = Tty::blank(None, 100, 30, ColorMode::True);
        let mut st = crate::settings::Settings::default();
        st.set("language", "ja");
        if std::env::var_os("EMUERA_LANG").is_none() {
            t.apply_setting("language", "ja", &st);
        }
        let out = capture(|| {
            t.draw_overlay(&st, &[0], &[], 0, "");
        });
        let scr = check("the overlay", "ja", 100, 30, &out);
        assert!(scr.text().contains("emuera-tty の設定"), "{}", scr.text());
        crate::i18n::set_language("en");
    }

    #[test]
    fn overlay_in_every_script() {
        use crate::settings::{Settings, ITEMS};
        for (lang, cols, rows) in cases() {
            crate::i18n::set_language(lang);
            let t = Tty::blank(None, cols, rows, ColorMode::True);
            let st = Settings::default();
            let vis: Vec<usize> = ITEMS.iter().enumerate().filter(|(_, it)| st.shown(it)).map(|(i, _)| i).collect();
            let rows_text = [
                t!("Manage saves…  (copy, export, back up, restore)"),
                t!("Clear the picture cache  (frees decoded sprites; they load again when shown)"),
                t!("Back to the title") + "  (Ctrl-T)",
                t!("Quit emuera-tty"),
            ];
            let extra: Vec<&str> = rows_text.iter().map(String::as_str).collect();
            let note = t!("{setting} takes effect the next time emuera-tty starts", setting = crate::i18n::tr(ITEMS[0].label));
            let out = capture(|| {
                t.draw_overlay(&st, &vis, &extra, vis.len() + 1, &note);
            });
            let scr = check("the overlay", lang, cols, rows, &out);
            // the title's text made it (in its script)
            let title = t!("emuera-tty settings");
            let head: String = title.chars().take(3).collect();
            assert!(scr.text().contains(&head), "{}: {:?} not shown", lang, head);
        }
        crate::i18n::set_language("en");
    }
}

#[cfg(test)]
mod mark_tests {
    use super::*;

    /// Help text wraps at spaces, and text without spaces (Japanese) wraps
    /// anywhere instead of being cut
    #[test]
    fn wrapping_without_spaces() {
        assert_eq!(wrap_words("one two three", 7, 4), ["one two", "three"]);
        assert_eq!(wrap_words("設定は次回の起動から有効になります", 10, 4), ["設定は次回", "の起動から", "有効になり", "ます"]);
        let w = |s: &String| s.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum::<usize>();
        assert!(wrap_words("आ\u{901}आ\u{901}आ\u{901}", 2, 4).iter().all(|l| w(l) <= 2));
    }

    /// Combining marks, vowel signs, variation selectors and ZWJ are marks;
    /// controls and spacing characters aren't
    #[test]
    fn what_is_a_mark() {
        for c in ['\u{301}', '\u{94D}', '\u{941}', '\u{FE0F}', '\u{200D}'] {
            assert!(is_mark(c), "{:?}", c);
        }
        for c in ['a', 'あ', '\n', '\t', '\u{7}', '\u{10B00}'] {
            assert!(!is_mark(c), "{:?}", c);
        }
    }

    /// Marks pile up on their base cell in order
    #[test]
    fn marks_kept_in_order() {
        let st = Style { fg: Rgb(0, 0, 0), bg: None, bold: false, italic: false, underline: false, strike: false };
        let mut c = Cell { ch: 'क', w: 1, style: st, button: None, marks: None };
        add_mark(&mut c, '\u{94D}');
        add_mark(&mut c, '\u{200D}');
        assert_eq!(c.marks.as_deref(), Some("\u{94D}\u{200D}"));
    }
}

#[cfg(test)]
mod translate_tests {
    /// A translation without the speech's line breaks is spread over its
    /// lines by their lengths.
    #[test]
    fn spread_by_length() {
        let w = |s: &str| s.chars().count();
        assert_eq!(super::spread("one two three four", &[10, 10], w), ["one two", "three four"]);
        assert_eq!(super::spread("a b c d e f", &[2, 10], w), ["a", "b c d e f"]);
        assert_eq!(super::spread("あいうえおか", &[3, 3], w), ["あいう", "えおか"]);
        assert_eq!(super::spread("solo", &[5, 5, 5], w), ["solo", "", ""]);
    }
}
