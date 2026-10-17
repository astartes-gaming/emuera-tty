//! Emuera console model: a scrollback of display lines made of styled
//! segments (text, buttons, images, spaces). Frontends lay it out.
//!
//! The VM only ever appends: PRINT adds segments to `cur` (the line being
//! built), a newline moves `cur` into `lines`. Wrapping to the screen width
//! is the frontend's job, so one [`Line`] may cover several screen rows.
//! Buttons carry the value that clicking them "types"; they are only live
//! for lines printed since the last input (`gen`), like in Emuera.

use crate::ast::Align;
use crate::loader::sjis_len;
use crate::vm::value::Value;

/// A 24-bit colour; Emuera stores colours as 0xRRGGBB integers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    pub fn from_i64(v: i64) -> Rgb {
        Rgb(((v >> 16) & 0xff) as u8, ((v >> 8) & 0xff) as u8, (v & 0xff) as u8)
    }
    pub fn to_i64(self) -> i64 {
        ((self.0 as i64) << 16) | ((self.1 as i64) << 8) | self.2 as i64
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Style {
    pub fg: Rgb,
    pub bg: Option<Rgb>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
}

/// One piece of a line.
#[derive(Clone, Debug)]
pub enum Seg {
    /// `auto`: plain printed text that may still contain `[N]` markers to
    /// turn into buttons when the line ends (see `auto_buttons`).
    Text { text: String, style: Style, button: Option<Value>, auto: bool },
    /// Image: sizes in Emuera pixels (font-size based).
    Image { name: String, w: f32, h: f32, ypos: f32, button: Option<Value> },
    Space { w: f32 },
    /// PRINT_RECT / `<shape type='rect'>`. The terminal UI draws one cell
    /// row, so `h` is kept for frontends that can do better.
    Rect {
        w: f32,
        #[allow(dead_code)]
        h: f32,
        color: Rgb,
        button: Option<Value>,
    },
}

/// A finished logical line.
#[derive(Clone, Debug)]
pub struct Line {
    pub segs: Vec<Seg>,
    pub align: Align,
    /// input generation this line was printed in (buttons active only for current)
    pub gen: u64,
    /// REUSELASTLINE line: replaced by whatever is printed next
    pub temporary: bool,
    /// Screen width (cells) when the line was printed. Like Emuera, a line
    /// is wrapped at this width for good, so resizing the window later
    /// clips it instead of re-wrapping (which would break up ASCII maps).
    pub cols: usize,
}

pub struct Console {
    /// Finished lines (scrollback, capped at `max_lines`).
    pub lines: Vec<Line>,
    /// The line being printed (no newline yet).
    pub cur: Vec<Seg>,
    /// Alignment captured for `cur` when its first segment was printed.
    pub cur_align: Option<Align>,
    pub style: Style,
    pub default_fg: Rgb,
    pub default_bg: Rgb,
    /// Colour of a hovered / selected button.
    pub focus_fg: Rgb,
    pub log_fg: Rgb,
    /// ALIGNMENT setting for new lines.
    pub align: Align,
    pub font_size: f32,
    pub line_height: f32,
    /// console width in half-width characters (for DRAWLINE, alignment)
    pub width: usize,
    /// Column width of PRINTC / PRINTLC.
    pub printc_len: usize,
    /// Input generation: bumped at each input, to retire old buttons.
    pub gen: u64,
    pub max_lines: usize,
    /// DRAWLINE unit (repeated across the width).
    pub drawline_str: String,
    /// REDRAW setting (whether the frontend may draw between inputs).
    pub redraw: bool,
    /// Something changed since the frontend last drew.
    pub dirty: bool,
    /// SKIPDISP: output is discarded.
    pub skip_disp: bool,
    pub font_name: String,
    /// WINDOW_TITLE
    pub title: String,
    /// The last line is a REUSELASTLINE line.
    pub last_temp: bool,
    /// lowest line count since the frontend last looked (tail removals)
    pub low_water: usize,
    /// lines dropped from the front so far (history limit)
    pub dropped_front: usize,
}

impl Console {
    pub fn new() -> Console {
        let fg = Rgb(192, 192, 192);
        Console {
            lines: Vec::new(),
            cur: Vec::new(),
            cur_align: None,
            style: Style { fg, bg: None, bold: false, italic: false, underline: false, strike: false },
            default_fg: fg,
            default_bg: Rgb(0, 0, 0),
            focus_fg: Rgb(255, 255, 0),
            log_fg: Rgb(192, 192, 192),
            align: Align::Left,
            font_size: 18.0,
            line_height: 19.0,
            width: 80,
            printc_len: 25,
            gen: 0,
            max_lines: 5000,
            drawline_str: "-".into(),
            redraw: true,
            dirty: true,
            skip_disp: false,
            font_name: String::new(),
            title: String::new(),
            last_temp: false,
            low_water: usize::MAX,
            dropped_front: 0,
        }
    }

    /// Remove a pending REUSELASTLINE line before new output replaces it.
    fn drop_temp(&mut self) {
        if self.last_temp {
            self.last_temp = false;
            if self.lines.last().map(|l| l.temporary).unwrap_or(false) {
                self.lines.pop();
                self.low_water = self.low_water.min(self.lines.len());
            }
        }
    }

    /// Print text with `[N]` auto-buttons enabled.
    pub fn print(&mut self, text: &str) {
        self.print_ex(text, true);
    }

    /// Print text; embedded newlines end lines. `auto` enables `[N]` auto-buttons.
    pub fn print_ex(&mut self, text: &str, auto: bool) {
        if self.skip_disp {
            return;
        }
        self.drop_temp();
        let mut first = true;
        for part in text.split('\n') {
            if !first {
                self.newline();
            }
            first = false;
            if !part.is_empty() {
                self.push_text(part, None, auto);
            }
        }
    }

    fn push_text(&mut self, s: &str, button: Option<Value>, auto: bool) {
        if self.cur_align.is_none() {
            self.cur_align = Some(self.align);
        }
        if let Some(Seg::Text { text, style, button: b, auto: a }) = self.cur.last_mut() {
            if *style == self.style && b.is_none() && button.is_none() && *a == auto {
                text.push_str(s);
                self.dirty = true;
                return;
            }
        }
        self.cur.push(Seg::Text { text: s.to_string(), style: self.style.clone(), button, auto });
        self.dirty = true;
    }

    /// PRINTBUTTON: text that inputs `value` when clicked.
    pub fn print_button(&mut self, text: &str, value: Value) {
        if self.skip_disp {
            return;
        }
        self.drop_temp();
        self.push_text(text, Some(value), false);
    }

    /// Append a non-text segment (image, space, rectangle).
    pub fn push_seg(&mut self, seg: Seg) {
        if self.skip_disp {
            return;
        }
        self.drop_temp();
        if self.cur_align.is_none() {
            self.cur_align = Some(self.align);
        }
        self.cur.push(seg);
        self.dirty = true;
    }

    /// End the current line: resolve auto-buttons, move it to `lines`,
    /// trim the scrollback.
    pub fn newline(&mut self) {
        if self.skip_disp {
            return;
        }
        self.drop_temp();
        let segs = std::mem::take(&mut self.cur);
        let segs = auto_buttons(segs);
        let align = self.cur_align.take().unwrap_or(self.align);
        self.lines.push(Line { segs, align, gen: self.gen, temporary: false, cols: self.width + 1 });
        if self.lines.len() > self.max_lines {
            let n = self.lines.len() - self.max_lines;
            self.lines.drain(0..n);
            self.dropped_front += n;
        }
        self.dirty = true;
    }

    /// REUSELASTLINE: a line that is replaced by the next output.
    pub fn temp_line(&mut self, text: &str) {
        self.drop_temp();
        if !self.cur.is_empty() {
            self.newline();
        }
        self.lines.push(Line {
            segs: vec![Seg::Text { text: text.to_string(), style: self.style.clone(), button: None, auto: false }],
            align: self.align,
            gen: self.gen,
            temporary: true,
            cols: self.width + 1,
        });
        self.last_temp = true;
        self.dirty = true;
    }

    pub fn line_is_empty(&self) -> bool {
        self.cur.is_empty()
    }

    /// CLEARLINE n: drop the last `n` finished lines.
    pub fn clear_lines(&mut self, n: usize) {
        let n = n.min(self.lines.len());
        let len = self.lines.len();
        self.lines.truncate(len - n);
        self.low_water = self.low_water.min(self.lines.len());
        self.dirty = true;
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// DRAWLINE: a full-width rule on a line of its own.
    pub fn drawline(&mut self) {
        let unit = if self.drawline_str.is_empty() { "-".to_string() } else { self.drawline_str.clone() };
        let s = repeat_to_width(&unit, self.width);
        if !self.cur.is_empty() {
            self.newline();
        }
        self.push_text(&s, None, false);
        self.newline();
    }

    /// CUSTOMDRAWLINE / DRAWLINEFORM: a rule made of `unit`.
    pub fn custom_drawline(&mut self, unit: &str) {
        let s = repeat_to_width(if unit.is_empty() { "-" } else { unit }, self.width);
        if !self.cur.is_empty() {
            self.newline();
        }
        self.push_text(&s, None, false);
        self.newline();
    }

    /// Called when the program asks for input: older buttons become inactive.
    /// Start a new input generation (older buttons stop being clickable).
    pub fn next_gen(&mut self) {
        self.gen += 1;
    }

    pub fn reset_style(&mut self) {
        self.style = Style { fg: self.default_fg, bg: None, bold: false, italic: false, underline: false, strike: false };
    }

    /// Plain text of a line (for logs / headless mode).
    pub fn line_text(line: &Line) -> String {
        let mut s = String::new();
        for seg in &line.segs {
            match seg {
                Seg::Text { text, .. } => s.push_str(text),
                Seg::Image { name, .. } => s.push_str(&format!("[img:{}]", name)),
                Seg::Space { w } => s.push_str(&" ".repeat((*w / 9.0).round().max(0.0) as usize)),
                Seg::Rect { .. } => s.push('■'),
            }
        }
        s
    }
}

/// `unit` repeated as many times as fits in `width` half-width cells.
pub fn repeat_to_width(unit: &str, width: usize) -> String {
    let uw = sjis_len(unit).max(1);
    let mut s = String::new();
    let mut w = 0;
    while w + uw <= width {
        s.push_str(unit);
        w += uw;
    }
    s
}

/// Emuera turns `[N] text` in normally printed text into buttons; the button
/// spans until the next `[N]` or end of line.
fn auto_buttons(segs: Vec<Seg>) -> Vec<Seg> {
    if segs.iter().any(|s| matches!(s, Seg::Text { button: Some(_), .. } | Seg::Image { button: Some(_), .. })) {
        return segs;
    }
    let has = segs.iter().any(|s| matches!(s, Seg::Text { auto: true, text, .. } if text.contains('[')));
    if !has {
        return segs;
    }
    let mut out = Vec::new();
    let mut current: Option<Value> = None;
    for seg in segs {
        match seg {
            Seg::Text { text, style, auto: true, .. } => {
                let mut rest = text.as_str();
                while !rest.is_empty() {
                    match find_button_marker(rest) {
                        Some((start, val)) => {
                            if start > 0 {
                                out.push(Seg::Text { text: rest[..start].to_string(), style: style.clone(), button: current.clone(), auto: false });
                            }
                            current = Some(Value::Int(val));
                            // include the marker itself in the new button
                            let (marker_end, _) = marker_len(&rest[start..]);
                            out.push(Seg::Text {
                                text: rest[start..start + marker_end].to_string(),
                                style: style.clone(),
                                button: current.clone(),
                                auto: false,
                            });
                            rest = &rest[start + marker_end..];
                        }
                        None => {
                            out.push(Seg::Text { text: rest.to_string(), style: style.clone(), button: current.clone(), auto: false });
                            rest = "";
                        }
                    }
                }
            }
            other => out.push(other),
        }
    }
    out
}

/// Length of the `[N]` marker at the start of `s`, and N.
fn marker_len(s: &str) -> (usize, i64) {
    let end = s.find(']').map(|i| i + 1).unwrap_or(s.len());
    let v = s[1..end - 1].trim().parse().unwrap_or(0);
    (end, v)
}

/// First `[N]` (an integer in brackets) in `s`: (byte offset, N).
fn find_button_marker(s: &str) -> Option<(usize, i64)> {
    let mut from = 0;
    while let Some(i) = s[from..].find('[') {
        let start = from + i;
        if let Some(j) = s[start..].find(']') {
            let inner = s[start + 1..start + j].trim();
            if !inner.is_empty() && inner.len() <= 18 {
                if let Ok(v) = inner.parse::<i64>() {
                    return Some((start, v));
                }
            }
            from = start + 1;
        } else {
            return None;
        }
    }
    None
}
