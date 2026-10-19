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

/// Is `v` a `<nonbutton>` tooltip area rather than a button? (see
/// [`Console::tip_area`])
pub fn is_tip_area(v: &Value) -> bool {
    matches!(v, Value::Str(s) if s.starts_with('\u{1}'))
}

/// A real (clickable) button value.
fn is_button(b: &Option<Value>) -> bool {
    b.as_ref().is_some_and(|v| !is_tip_area(v))
}

/// One piece of a line.
#[derive(Clone, Debug)]
pub enum Seg {
    /// `auto`: plain printed text that may still contain `[N]` markers to
    /// turn into buttons when the line ends (see `auto_buttons`).
    Text { text: String, style: Style, button: Option<Value>, auto: bool },
    /// Image: sizes in Emuera pixels (font-size based).
    /// `hover`: the `srcb` picture, shown while the button is under the pointer.
    Image { name: String, w: f32, h: f32, ypos: f32, button: Option<Value>, hover: Option<String> },
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
    /// HTML_PRINT `<div>`: its own lines, wrapped at `cols` and drawn
    /// `dx` cells right and `dy` rows down from where it sits (the same
    /// cell and row sizes CLIENTWIDTH / CLIENTHEIGHT count in), at most
    /// `rows` rows. Takes no width in the line. `html` is its source, for
    /// HTML_GETPRINTEDSTR.
    Div { dx: i32, dy: i32, cols: usize, rows: Option<usize>, lines: Vec<Line>, html: String, deco: Box<DivBox> },
}

/// A `<div>`'s background, border and padding, in cells.
#[derive(Clone, Debug, Default)]
pub struct DivBox {
    pub bg: Option<Rgb>,
    pub bcolor: Option<Rgb>,
    /// border on top, right, bottom, left (drawn one cell wide)
    pub border: [bool; 4],
    /// padding top, right, bottom, left
    pub pad: [usize; 4],
    /// rounded corners (`radius`): top left, top right, bottom right,
    /// bottom left, as Emuera orders them
    pub round: [bool; 4],
}

impl DivBox {
    /// Rows / columns the border and padding take: (top, right, bottom, left).
    pub fn insets(&self) -> [usize; 4] {
        let mut n = self.pad;
        for (k, b) in self.border.iter().enumerate() {
            n[k] += *b as usize;
        }
        n
    }
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
    /// printed by a character's dialogue (kojo) code (see `mt::is_dialog`)
    pub kojo: bool,
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
    /// The window background (SETBGCOLOR): Emuera paints the whole
    /// console with it, old lines included.
    pub screen_bg: Rgb,
    /// Colour of a hovered / selected button.
    pub focus_fg: Rgb,
    pub log_fg: Rgb,
    /// TOOLTIP_SETCOLOR: (text, background); None: the frontend's own
    pub tip_colors: Option<(Rgb, Rgb)>,
    /// TOOLTIP_SETDELAY / TOOLTIP_SETDURATION (ms): a tooltip shows after
    /// resting on a button that long, and goes after `tip_duration` (0:
    /// stays). Emuera's defaults are a Windows tooltip's: 500 and 5000.
    pub tip_delay: u64,
    pub tip_duration: u64,
    /// ALIGNMENT setting for new lines.
    pub align: Align,
    pub font_size: f32,
    pub line_height: f32,
    /// console width in half-width characters (for DRAWLINE, alignment)
    pub width: usize,
    /// Column width of PRINTC / PRINTLC.
    pub printc_len: usize,
    /// PRINTCPERLINE (config "PRINTCを並べる数")
    pub printc_per_line: usize,
    /// emuera.config's clipboard options (Emuera-EE / Emuera-Anchor)
    pub clip: crate::clipboard::ClipConfig,
    /// "ボタンの途中で行を折りかえさない": a button that doesn't fit the
    /// rest of the line moves to the next one whole
    pub button_wrap: bool,
    /// Generation stamped on new lines (Emuera's `newButtonGeneration`).
    pub gen: u64,
    /// `<button title=…>` tooltips: button value → (generation, text)
    pub tooltips: std::collections::HashMap<String, (u64, String)>,
    /// `<nonbutton>` areas made so far (each gets its own value)
    tip_areas: u64,
    /// dialogue (kojo) code is running (set by the VM), and it printed on
    /// the line being made
    pub in_kojo: bool,
    cur_kojo: bool,
    /// the Sound setting (the settings page turns it on and off)
    pub sound_on: bool,
    /// the function running (or asking for input), for the bottom bar;
    /// the VM updates it now and then, not on every call
    pub running: String,
    /// Buttons on lines of this generation are clickable
    /// (`lastButtonGeneration`); see `begin_input`.
    pub valid_gen: u64,
    /// buttons were printed since the last input request
    buttons_since_input: bool,
    /// where the last input request came from (function, instruction)
    last_input_site: Option<(usize, usize)>,
    /// the last input request was for a number (INPUT) rather than text
    last_input_int: bool,
    pub max_lines: usize,
    /// DRAWLINE unit (repeated across the width).
    pub drawline_str: String,
    /// REDRAW setting (whether the frontend may draw between inputs).
    pub redraw: bool,
    /// Something changed since the frontend last drew.
    pub dirty: bool,
    /// SKIPDISP: output is discarded.
    pub skip_disp: bool,
    /// skip_disp came from the script's SKIPDISP (an INPUT under it is an
    /// error), not from the engine (CALLTRAIN's screens, where it is skipped)
    pub skip_user: bool,
    pub font_name: String,
    /// the game's configured font ("フォント名"), what SETFONT "" restores
    pub default_font: String,
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
            clip: crate::clipboard::ClipConfig::default(),
            lines: Vec::new(),
            cur: Vec::new(),
            cur_align: None,
            style: Style { fg, bg: None, bold: false, italic: false, underline: false, strike: false },
            default_fg: fg,
            default_bg: Rgb(0, 0, 0),
            screen_bg: Rgb(0, 0, 0),
            focus_fg: Rgb(255, 255, 0),
            tip_colors: None,
            tip_delay: 500,
            tip_duration: 5000,
            log_fg: Rgb(192, 192, 192),
            align: Align::Left,
            font_size: 18.0,
            line_height: 19.0,
            width: 80,
            printc_len: 25,
            printc_per_line: 3,
            button_wrap: false,
            gen: 0,
            tooltips: std::collections::HashMap::new(),
            tip_areas: 0,
            running: String::new(),
            in_kojo: false,
            cur_kojo: false,
            sound_on: false,
            valid_gen: 0,
            buttons_since_input: false,
            last_input_site: None,
            last_input_int: true,
            max_lines: 5000,
            drawline_str: "-".into(),
            redraw: true,
            dirty: true,
            skip_disp: false,
            skip_user: false,
            font_name: String::new(),
            default_font: String::new(),
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

    /// One PRINTC item: Emuera makes it a unit of its own (`force_button`),
    /// so its `[N]` turns the whole padded item into one button, whatever
    /// the items around it hold.
    pub fn print_unit(&mut self, text: &str) {
        if self.skip_disp {
            return;
        }
        self.drop_temp();
        if self.cur_align.is_none() {
            self.cur_align = Some(self.align);
        }
        let segs = auto_buttons(vec![Seg::Text { text: text.to_string(), style: self.style.clone(), button: None, auto: true }]);
        for s in segs {
            match s {
                // (marked done: the line's own pass must not regroup it)
                Seg::Text { text, style, button, .. } => self.cur.push(Seg::Text { text, style, button, auto: false }),
                other => self.cur.push(other),
            }
        }
        self.dirty = true;
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
        self.cur_kojo |= self.in_kojo;
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
        if segs.iter().any(|s| matches!(s, Seg::Text { button, .. } | Seg::Image { button, .. } | Seg::Rect { button, .. } if is_button(button))) {
            // new buttons are the clickable ones (UpdateGeneration)
            self.valid_gen = self.gen;
            self.buttons_since_input = true;
        }
        let kojo = std::mem::take(&mut self.cur_kojo) || self.in_kojo;
        self.lines.push(Line { segs, align, gen: self.gen, temporary: false, cols: self.width + 1, kojo });
        // trim the history in batches: dropping one line at a time shifts
        // all the others down for every line printed, which made long
        // listings crawl (seconds per screen) once the history was full
        if self.lines.len() > self.max_lines + (self.max_lines / 8).max(64) {
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
            kojo: false,
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

    /// LINECOUNT: the lines printed so far, those already dropped from the
    /// history too (as Emuera counts; a game waiting for it to reach a
    /// number never got there once the history was full).
    pub fn line_count(&self) -> usize {
        self.dropped_front + self.lines.len()
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

    /// BREAKBUTTON: every button on screen stops being clickable
    /// (Emuera's forceUpdateGeneration).
    pub fn break_buttons(&mut self) {
        self.gen += 1;
        self.valid_gen = self.gen;
        self.buttons_since_input = true;
    }

    /// Values of the buttons an input can be answered with now: those on
    /// lines printed since the last input or still valid from before
    /// (BINPUT's check).
    pub fn button_values(&self) -> Vec<Value> {
        let mut out = Vec::new();
        for l in self.lines.iter().rev() {
            if l.gen != self.gen && l.gen != self.valid_gen {
                break;
            }
            Self::collect_buttons(&l.segs, &mut out);
        }
        out
    }

    fn collect_buttons(segs: &[Seg], out: &mut Vec<Value>) {
        for s in segs {
            match s {
                Seg::Text { button: Some(v), .. } | Seg::Image { button: Some(v), .. } | Seg::Rect { button: Some(v), .. } if !is_tip_area(v) => {
                    if !out.contains(v) {
                        out.push(v.clone());
                    }
                }
                Seg::Div { lines, .. } => {
                    for l in lines {
                        Self::collect_buttons(&l.segs, out);
                    }
                }
                _ => {}
            }
        }
    }

    fn tooltip_key(v: &Value) -> String {
        match v {
            Value::Int(i) => format!("i{}", i),
            Value::Str(s) => format!("s{}", s),
        }
    }

    /// A new `<nonbutton title=…>` area: text that shows a tooltip under the
    /// pointer but is no button. It is marked like a button with a value of
    /// its own (U+0001 and a number, which no game prints) that
    /// [`is_tip_area`] tells apart: never highlighted, clicked or accepted.
    pub fn tip_area(&mut self, text: &str) -> Value {
        self.tip_areas += 1;
        let v = Value::Str(format!("\u{1}{}", self.tip_areas));
        self.set_tooltip(&v, text);
        v
    }

    /// A button printed now has tooltip `text` (empty: none).
    pub fn set_tooltip(&mut self, v: &Value, text: &str) {
        if self.tooltips.len() > 4096 {
            let g = self.valid_gen;
            self.tooltips.retain(|_, t| t.0 >= g);
        }
        let k = Self::tooltip_key(v);
        if text.is_empty() {
            self.tooltips.remove(&k);
        } else {
            self.tooltips.insert(k, (self.gen, text.to_string()));
        }
    }

    /// The tooltip of a clickable button.
    pub fn tooltip_for(&self, v: &Value) -> Option<&str> {
        self.tooltips.get(&Self::tooltip_key(v)).filter(|t| t.0 >= self.valid_gen).map(|t| t.1.as_str())
    }

    /// The script asks for a value (INPUT…, not WAIT) at `site`. Emuera's
    /// `newGeneration`: the buttons on screen stay clickable while the same
    /// input is asked again (a rejected or empty answer, a timeout) and
    /// until another input is reached with no new buttons printed; text and
    /// number inputs don't share buttons.
    pub fn begin_input(&mut self, site: (usize, usize), int: bool) {
        if !self.buttons_since_input && self.last_input_site != Some(site) {
            self.valid_gen = self.gen;
        } else {
            self.buttons_since_input = false;
        }
        self.last_input_site = Some(site);
        if self.valid_gen == self.gen {
            // buttons printed from now on belong to the next input
            self.gen += 1;
        } else if self.last_input_int != int {
            self.valid_gen = self.gen;
        }
        self.last_input_int = int;
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
                Seg::Div { lines, .. } => {
                    let inner: Vec<String> = lines.iter().map(Self::line_text).filter(|t| !t.trim().is_empty()).collect();
                    if !s.is_empty() && !s.ends_with(' ') && !inner.is_empty() {
                        s.push(' ');
                    }
                    s.push_str(&inner.join(" "));
                }
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

/// Emuera turns `[N]` markers in normally printed text into buttons
/// (`PrintStringBuffer.createButtons` + `ButtonStringCreator`). Each run of
/// plain printed text between explicit buttons (PRINTBUTTON…) is split on
/// its own: with one marker the whole run is the button, with several the
/// descriptions attach to their markers as Emuera decides.
pub fn auto_buttons(segs: Vec<Seg>) -> Vec<Seg> {
    if !segs.iter().any(|s| matches!(s, Seg::Text { auto: true, text, .. } if text.contains('['))) {
        return segs;
    }
    let mut out = Vec::with_capacity(segs.len() + 4);
    let mut run: Vec<Seg> = Vec::new();
    for seg in segs {
        match seg {
            Seg::Text { auto: true, .. } => run.push(seg),
            other => {
                flush_run(&mut run, &mut out);
                out.push(other);
            }
        }
    }
    flush_run(&mut run, &mut out);
    out
}

/// Turn one run of auto text segments into button / plain segments.
fn flush_run(run: &mut Vec<Seg>, out: &mut Vec<Seg>) {
    if run.is_empty() {
        return;
    }
    let text: String = run.iter().map(|s| if let Seg::Text { text, .. } = s { text.as_str() } else { "" }).collect();
    let parts = split_buttons(&text);
    // cut the segments at the parts' byte boundaries
    let mut bounds = Vec::with_capacity(parts.len());
    let mut end = 0;
    for (len, v) in &parts {
        end += len;
        bounds.push((end, v.map(Value::Int)));
    }
    let mut pos = 0; // byte offset of the current segment's start in `text`
    let mut b = 0;
    for seg in run.drain(..) {
        let Seg::Text { text: t, style, .. } = seg else { continue };
        let mut rest: &str = &t;
        while !rest.is_empty() {
            let (bend, ref val) = bounds[b.min(bounds.len() - 1)];
            let take = bend.saturating_sub(pos).min(rest.len());
            let take = if take == 0 { rest.len() } else { take };
            out.push(Seg::Text { text: rest[..take].to_string(), style: style.clone(), button: val.clone(), auto: false });
            rest = &rest[take..];
            pos += take;
            if pos >= bend && b + 1 < bounds.len() {
                b += 1;
            }
        }
    }
}

fn is_ws(c: char) -> bool {
    c == ' ' || c == '\t' || c == '\u{3000}'
}

/// `ButtonStringCreator.lex`: `[…]` groups, whitespace runs and other text;
/// None (no buttons at all) for a nested `[` or a stray `]`.
fn lex_buttons(s: &str) -> Option<Vec<&str>> {
    let mut toks = Vec::new();
    let (mut start, mut inside) = (0usize, false);
    let mut it = s.char_indices().peekable();
    while let Some(&(i, c)) = it.peek() {
        if c == '[' {
            if inside {
                return None;
            }
            if i > start {
                toks.push(&s[start..i]);
            }
            start = i;
            inside = true;
            it.next();
        } else if c == ']' {
            if !inside {
                return None;
            }
            it.next();
            let e = it.peek().map_or(s.len(), |&(j, _)| j);
            toks.push(&s[start..e]);
            start = e;
            inside = false;
        } else if !inside && is_ws(c) {
            if i > start {
                toks.push(&s[start..i]);
            }
            start = i;
            while it.peek().is_some_and(|&(_, c)| is_ws(c)) {
                it.next();
            }
            let e = it.peek().map_or(s.len(), |&(j, _)| j);
            toks.push(&s[start..e]);
            start = e;
        } else {
            it.next();
        }
    }
    if start < s.len() {
        toks.push(&s[start..]);
    }
    Some(toks)
}

/// `isButtonCore`: `[ 12 ]`, `[-3]`, `[0x1F]`, `[0b101]`, `[1e3]` → the number.
fn button_core(t: &str) -> Option<i64> {
    let inner = t.strip_prefix('[')?.strip_suffix(']')?.trim_matches(is_ws);
    if inner.is_empty() {
        return None;
    }
    let (neg, body) = match inner.as_bytes()[0] {
        b'-' => (true, &inner[1..]),
        b'+' => (false, &inner[1..]),
        _ => (false, inner),
    };
    let lower = body.to_ascii_lowercase();
    let (radix, digits) = if let Some(h) = lower.strip_prefix("0x") {
        (16, h.to_string())
    } else if let Some(b) = lower.strip_prefix("0b") {
        (2, b.to_string())
    } else {
        (10, lower.clone())
    };
    // optional exponent: e (×10^n) for decimals, p (×2^n)
    let (mant, exp) = match digits.find(|c| c == 'p' || (c == 'e' && radix == 10)) {
        Some(k) => (&digits[..k], Some((digits.as_bytes()[k], digits[k + 1..].parse::<u32>().ok()?))),
        None => (digits.as_str(), None),
    };
    if mant.is_empty() || !mant.chars().all(|c| c.is_digit(radix)) {
        return None;
    }
    let mut v = i64::from_str_radix(mant, radix).ok()?;
    if let Some((k, n)) = exp {
        v = v.checked_mul(if k == b'p' { 2i64.checked_pow(n)? } else { 10i64.checked_pow(n)? })?;
    }
    Some(if neg { -v } else { v })
}

/// `ButtonStringCreator.syn`: split `s` into consecutive parts, each
/// (byte length, Some(value) if it is a button).
fn split_buttons(s: &str) -> Vec<(usize, Option<i64>)> {
    let whole = |v: Option<i64>| vec![(s.len(), v)];
    if s.is_empty() || !s.contains('[') || !s.contains(']') {
        return whole(None);
    }
    let Some(toks) = lex_buttons(s) else { return whole(None) };
    let ws = |t: &str| t.chars().next().is_some_and(is_ws);
    let (mut before, mut after, mut count, mut last) = (false, false, 0, 0i64);
    for t in &toks {
        if ws(t) {
        } else if let Some(v) = button_core(t) {
            count += 1;
            last = v;
            after = false;
        } else {
            after = true;
            if count == 0 {
                before = true;
            }
        }
    }
    if count <= 1 {
        return whole((count == 1).then_some(last));
    }
    let right = !before && after; // descriptions follow their markers
    let left = before && !after; // descriptions precede them
    let etc = !right && !left;
    let mut out = Vec::new();
    let (mut buf, mut sel, mut state) = (0usize, None::<i64>, 0u8);
    let reduce = |out: &mut Vec<(usize, Option<i64>)>, buf: &mut usize, sel: &mut Option<i64>| {
        if *buf > 0 {
            out.push((*buf, *sel));
        }
        *buf = 0;
        *sel = None;
    };
    for t in toks {
        if ws(t) {
            // a finished "[N] description" ends at a run of 2+ spaces
            if state & 3 == 3 && etc && t.chars().count() >= 2 {
                reduce(&mut out, &mut buf, &mut sel);
                buf += t.len();
                state = 0;
            } else {
                buf += t.len();
            }
            continue;
        }
        if let Some(v) = button_core(t) {
            if state & 1 == 1 || right {
                reduce(&mut out, &mut buf, &mut sel);
                buf += t.len();
                sel = Some(v);
                state = 1;
            } else if left {
                buf += t.len();
                sel = Some(v);
                reduce(&mut out, &mut buf, &mut sel);
                state = 0;
            } else {
                buf += t.len();
                sel = Some(v);
                state = 1;
            }
            continue;
        }
        buf += t.len();
        state |= 2;
    }
    reduce(&mut out, &mut buf, &mut sel);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (text, value) of each part `split_buttons` makes of `s`.
    fn parts(s: &str) -> Vec<(&str, Option<i64>)> {
        let mut at = 0;
        split_buttons(s)
            .into_iter()
            .map(|(n, v)| {
                let p = &s[at..at + n];
                at += n;
                (p, v)
            })
            .collect()
    }

    #[test]
    fn buttons_split_like_emuera() {
        // one marker: the whole text is the button
        assert_eq!(parts("  [1] Continue"), vec![("  [1] Continue", Some(1))]);
        // descriptions after their markers (AnonTW's main menu)
        assert_eq!(
            parts("[100] - Wake Up   [102] - Home Settings"),
            vec![("[100] - Wake Up   ", Some(100)), ("[102] - Home Settings", Some(102))]
        );
        // descriptions before their markers (the command list)
        assert_eq!(parts("Rest[403]   Diary[406]"), vec![("Rest[403]", Some(403)), ("   Diary[406]", Some(406))]);
        // text on both sides: a run of 2+ spaces ends each button
        assert_eq!(
            parts("Pick: [1] Yes  [2] No  now"),
            vec![("Pick: [1] Yes", Some(1)), ("  [2] No", Some(2)), ("  now", None)]
        );
        assert_eq!(parts("[0x10] hex"), vec![("[0x10] hex", Some(16))]);
        // not numbers / unbalanced brackets: no buttons
        assert_eq!(parts("[Status] [x]"), vec![("[Status] [x]", None)]);
        assert_eq!(parts("a] [1] b"), vec![("a] [1] b", None)]);
    }

    #[test]
    fn buttons_stay_clickable_like_emuera() {
        let mut c = Console::new();
        c.print("[1] Go  [2] Stay");
        c.newline();
        let menu = c.lines[0].gen;
        c.begin_input((7, 3), true);
        assert_eq!(c.valid_gen, menu, "the menu is clickable at its INPUT");
        // a rejected / empty answer asks the same INPUT again: still valid
        c.begin_input((7, 3), true);
        assert_eq!(c.valid_gen, menu);
        // another input with no new buttons printed: the old ones retire
        c.begin_input((9, 1), true);
        assert_ne!(c.valid_gen, menu);
        // new buttons become the valid ones
        c.print("[3] Next");
        c.newline();
        let next = c.lines[1].gen;
        c.begin_input((9, 1), true);
        assert_eq!(c.valid_gen, next);
        // a text input doesn't take number buttons
        c.begin_input((11, 0), false);
        assert_ne!(c.valid_gen, next);
    }

    #[test]
    fn auto_buttons_around_explicit_ones() {
        let mut c = Console::new();
        c.print("[1] Yes  ");
        c.print_button("[Raise it]", Value::Int(7));
        c.print("  [2] No");
        c.newline();
        let got: Vec<(String, Option<Value>)> = c.lines[0]
            .segs
            .iter()
            .filter_map(|s| match s {
                Seg::Text { text, button, .. } => Some((text.clone(), button.clone())),
                _ => None,
            })
            .collect();
        assert_eq!(
            got,
            vec![
                ("[1] Yes  ".to_string(), Some(Value::Int(1))),
                ("[Raise it]".to_string(), Some(Value::Int(7))),
                ("  [2] No".to_string(), Some(Value::Int(2))),
            ]
        );
    }
}

/// Relative luminance (WCAG) of an sRGB colour.
fn luminance(c: Rgb) -> f32 {
    let lin = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(c.0) + 0.7152 * lin(c.1) + 0.0722 * lin(c.2)
}

/// WCAG contrast ratio of two colours (1 to 21).
pub fn contrast(a: Rgb, b: Rgb) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `fg`, lightened (on a dark `bg`) or darkened (on a light one) until it
/// reaches contrast `min` against `bg`, keeping its hue. Greys are left
/// alone: games use dark greys on purpose (shading, faded map edges);
/// it is saturated colours like pure blue on black that are unreadable.
pub fn readable(fg: Rgb, bg: Rgb, min: f32) -> Rgb {
    if min <= 1.0 {
        return fg;
    }
    let (mx, mn) = (fg.0.max(fg.1).max(fg.2) as i32, fg.0.min(fg.1).min(fg.2) as i32);
    if (mx - mn) * 4 < mx.max(1) || contrast(fg, bg) >= min {
        return fg;
    }
    let target = if luminance(bg) < 0.4 { Rgb(255, 255, 255) } else { Rgb(0, 0, 0) };
    let mix = |t: f32| {
        let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
        Rgb(m(fg.0, target.0), m(fg.1, target.1), m(fg.2, target.2))
    };
    for k in 1..=20 {
        let c = mix(k as f32 / 20.0);
        if contrast(c, bg) >= min {
            return c;
        }
    }
    target
}

#[cfg(test)]
mod contrast_tests {
    use super::*;

    #[test]
    fn dark_blue_gets_readable() {
        let black = Rgb(0, 0, 0);
        let navy = Rgb(1, 24, 60); // AnonTW's time-stop background
        let blue = Rgb(0, 0, 255);
        assert!(contrast(blue, black) < 3.0);
        let b = readable(blue, black, 3.0);
        assert!(contrast(b, black) >= 3.0 && b.2 == 255 && b.0 < 128, "{:?}", b);
        let b = readable(Rgb(64, 64, 200), navy, 3.0);
        assert!(contrast(b, navy) >= 3.0, "{:?}", b);
        // greys and already readable colours stay as they are
        assert_eq!(readable(Rgb(0x15, 0x15, 0x15), black, 3.0), Rgb(0x15, 0x15, 0x15));
        assert_eq!(readable(Rgb(255, 255, 0), black, 3.0), Rgb(255, 255, 0));
        assert_eq!(readable(blue, black, 1.0), blue);
    }
}
