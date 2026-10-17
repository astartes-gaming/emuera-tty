//! Emuera console model: a scrollback of display lines made of styled
//! segments (text, buttons, images, spaces). Frontends lay it out.

use crate::ast::Align;
use crate::loader::sjis_len;
use crate::vm::value::Value;

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

#[derive(Clone, Debug)]
pub enum Seg {
    Text { text: String, style: Style, button: Option<Value>, auto: bool },
    /// Image: sizes in Emuera pixels (font-size based).
    Image { name: String, w: f32, h: f32, ypos: f32, button: Option<Value> },
    Space { w: f32 },
    Rect { w: f32, h: f32, color: Rgb, button: Option<Value> },
}

#[derive(Clone, Debug)]
pub struct Line {
    pub segs: Vec<Seg>,
    pub align: Align,
    /// input generation this line was printed in (buttons active only for current)
    pub gen: u64,
    /// number of display sub-lines forced by <br> (counted as 1 logical line)
    pub temporary: bool,
}

pub struct Console {
    pub lines: Vec<Line>,
    pub cur: Vec<Seg>,
    pub cur_align: Option<Align>,
    pub style: Style,
    pub default_fg: Rgb,
    pub default_bg: Rgb,
    pub focus_fg: Rgb,
    pub log_fg: Rgb,
    pub align: Align,
    pub font_size: f32,
    pub line_height: f32,
    /// console width in half-width characters (for DRAWLINE, alignment)
    pub width: usize,
    pub printc_len: usize,
    pub gen: u64,
    pub max_lines: usize,
    pub drawline_str: String,
    pub redraw: bool,
    pub dirty: bool,
    pub skip_disp: bool,
    pub font_name: String,
    pub title: String,
    pub last_temp: bool,
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
        }
    }

    fn drop_temp(&mut self) {
        if self.last_temp {
            self.last_temp = false;
            if self.lines.last().map(|l| l.temporary).unwrap_or(false) {
                self.lines.pop();
            }
        }
    }

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

    pub fn print_button(&mut self, text: &str, value: Value) {
        if self.skip_disp {
            return;
        }
        self.drop_temp();
        self.push_text(text, Some(value), false);
    }

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

    pub fn current_style(&self) -> Style {
        self.style.clone()
    }

    pub fn newline(&mut self) {
        if self.skip_disp {
            return;
        }
        self.drop_temp();
        let segs = std::mem::take(&mut self.cur);
        let segs = auto_buttons(segs);
        let align = self.cur_align.take().unwrap_or(self.align);
        self.lines.push(Line { segs, align, gen: self.gen, temporary: false });
        if self.lines.len() > self.max_lines {
            let n = self.lines.len() - self.max_lines;
            self.lines.drain(0..n);
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
        });
        self.last_temp = true;
        self.dirty = true;
    }

    pub fn line_is_empty(&self) -> bool {
        self.cur.is_empty()
    }

    pub fn clear_lines(&mut self, n: usize) {
        let n = n.min(self.lines.len());
        let len = self.lines.len();
        self.lines.truncate(len - n);
        self.dirty = true;
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Current line's text width in half-width units.
    pub fn cur_width(&self) -> usize {
        self.cur
            .iter()
            .map(|s| match s {
                Seg::Text { text, .. } => sjis_len(text),
                Seg::Space { w } | Seg::Image { w, .. } | Seg::Rect { w, .. } => (*w / (self.font_size / 2.0)).ceil() as usize,
            })
            .sum()
    }

    pub fn drawline(&mut self) {
        let unit = if self.drawline_str.is_empty() { "-".to_string() } else { self.drawline_str.clone() };
        let s = repeat_to_width(&unit, self.width);
        if !self.cur.is_empty() {
            self.newline();
        }
        self.push_text(&s, None, false);
        self.newline();
    }

    pub fn custom_drawline(&mut self, unit: &str) {
        let s = repeat_to_width(if unit.is_empty() { "-" } else { unit }, self.width);
        if !self.cur.is_empty() {
            self.newline();
        }
        self.push_text(&s, None, false);
        self.newline();
    }

    /// Called when the program asks for input: older buttons become inactive.
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

fn marker_len(s: &str) -> (usize, i64) {
    let end = s.find(']').map(|i| i + 1).unwrap_or(s.len());
    let v = s[1..end - 1].trim().parse().unwrap_or(0);
    (end, v)
}

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
