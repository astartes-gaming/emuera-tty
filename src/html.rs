//! HTML_PRINT subset: p, font, b, i, u, s, button, nonbutton, img, shape, br.
//!
//! Emuera's HTML is a small markup for one console line, not real HTML:
//! tags are interpreted left to right straight into console segments,
//! with a stack so `</font>` / `</b>` restore the previous style. Sizes
//! (`<img height>`, `<shape param>`) are percentages of the font size, or
//! pixels with a `px` suffix (Emuera-EM). Unknown tags are ignored.

use crate::ast::Align;
use crate::console::{Seg, Style};
use crate::fxhash::HashMap;
use crate::vm::builtins::color_from_str;
use crate::vm::value::Value;
use crate::vm::Vm;

/// Replace `&lt;` `&amp;` `&nbsp;` `&#…;` etc. Unknown entities stay as-is.
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let tail = &rest[i..];
        match tail.find(';') {
            Some(j) if j <= 10 => {
                let ent = &tail[1..j];
                let rep = match ent {
                    "lt" => Some("<".to_string()),
                    "gt" => Some(">".to_string()),
                    "amp" => Some("&".to_string()),
                    "quot" => Some("\"".to_string()),
                    "apos" => Some("'".to_string()),
                    "nbsp" => Some(" ".to_string()),
                    e if e.starts_with("#x") => u32::from_str_radix(&e[2..], 16).ok().and_then(char::from_u32).map(|c| c.to_string()),
                    e if e.starts_with('#') => e[1..].parse().ok().and_then(char::from_u32).map(|c| c.to_string()),
                    _ => None,
                };
                match rep {
                    Some(r) => {
                        out.push_str(&r);
                        rest = &tail[j + 1..];
                    }
                    None => {
                        out.push('&');
                        rest = &tail[1..];
                    }
                }
            }
            _ => {
                out.push('&');
                rest = &tail[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Where the tag starting at `tag[0]` (`<`) ends: the first `>` outside a
/// quoted attribute value. Emuera reads a start tag with its lexer, single-
/// and double-quoted strings included (HtmlManager.tagAnalyze), and the
/// Emuera.NET fork with a full HTML parser, so `title = '<浴室>'` doesn't
/// end the tag at the `>` inside it (eraKATANA's map tooltips). A quote
/// left open ends nothing: then the first `>` ends the tag, as before.
fn tag_end(tag: &str) -> Option<usize> {
    let mut open_quote: Option<char> = None;
    for (i, c) in tag.char_indices().skip(1) {
        match (open_quote, c) {
            (Some(q), c) if c == q => open_quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => open_quote = Some(c),
            (None, '>') => return Some(i),
            (None, _) => {}
        }
    }
    // an unclosed quote: fall back to the first `>` anywhere
    open_quote.and_then(|_| tag.find('>'))
}

/// `key='v' key="v" key=v flag` → map with lower-cased keys.
fn parse_attrs(s: &str) -> HashMap<String, String> {
    let mut m = HashMap::default();
    let c: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < c.len() {
        while i < c.len() && c[i].is_whitespace() {
            i += 1;
        }
        let ks = i;
        while i < c.len() && !c[i].is_whitespace() && c[i] != '=' {
            i += 1;
        }
        let key: String = c[ks..i].iter().collect::<String>().to_lowercase();
        while i < c.len() && c[i].is_whitespace() {
            i += 1;
        }
        if i < c.len() && c[i] == '=' {
            i += 1;
            while i < c.len() && c[i].is_whitespace() {
                i += 1;
            }
            let val: String = if i < c.len() && (c[i] == '\'' || c[i] == '"') {
                let q = c[i];
                i += 1;
                let vs = i;
                while i < c.len() && c[i] != q {
                    i += 1;
                }
                let v = c[vs..i].iter().collect();
                i += 1;
                v
            } else {
                let vs = i;
                while i < c.len() && !c[i].is_whitespace() {
                    i += 1;
                }
                c[vs..i].iter().collect()
            };
            if !key.is_empty() {
                m.insert(key, decode_entities(&val));
            }
        } else if !key.is_empty() {
            m.insert(key, String::new());
        }
    }
    m
}

/// HTML_TOPLAINTEXT: drop tags (`<br>` becomes a newline), decode entities.
pub fn to_plain(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    let mut buf = String::new();
    for ch in s.chars() {
        if in_tag {
            if ch == '>' {
                in_tag = false;
                if buf.trim().eq_ignore_ascii_case("br") {
                    out.push('\n');
                }
                buf.clear();
            } else {
                buf.push(ch);
            }
        } else if ch == '<' {
            in_tag = true;
        } else {
            out.push(ch);
        }
    }
    decode_entities(&out)
}

impl Vm {
    /// HTML_PRINT: render `src` as its own line(s). Style and alignment
    /// changes don't leak out: both are restored afterwards.
    pub fn html_print(&mut self, src: &str) {
        self.html_print_ex(src, true);
    }

    /// HTML_PRINT with its second argument: non-zero leaves the line open,
    /// so what is printed next goes on the same line (Emuera-EM puts the
    /// HTML into the print buffer; Emuera.NET marks the line not ended and
    /// merges the next one into it). eraMegatenP prints each dungeon map
    /// row as `HTML_PRINT row, -1` and a PRINTL: ending the line here made
    /// that PRINTL a blank line under every row.
    pub fn html_print_ex(&mut self, src: &str, end_line: bool) {
        // (Emuera's PrintHtml prints nothing for "")
        if src.is_empty() {
            return;
        }
        // an open line keeps what was printed before it on the same line
        if end_line && !self.con.cur.is_empty() {
            self.con.newline();
        }
        let saved_style = self.con.style.clone();
        let saved_align = self.con.align;
        // its lines start left-aligned whatever ALIGNMENT says; only <p
        // align> changes that (Emuera-EM's HtmlManager state, Emuera.NET's
        // Html2DisplayLine): eraMegatenP centres its title logo itself,
        // inside a centred ALIGNMENT
        // (an open line gets its alignment when it ends)
        if end_line {
            self.con.align = Align::Left;
            self.con.cur_align = Some(Align::Left);
        }
        let mut style_stack: Vec<Style> = Vec::new();
        // inside a <div> that a <button> holds: the button goes on
        let (mut button, mut clear): (Option<Value>, bool) = self.html_outer_button.take().unwrap_or_default();
        // (clear: inside <clearbutton>, buttons are shown but not clickable)
        let fs = self.con.font_size;
        let mut rest = src;
        let mut first_text = true;
        // strip comments
        let cleaned;
        if src.contains("<!--") {
            let mut s = String::new();
            let mut r = src;
            while let Some(i) = r.find("<!--") {
                s.push_str(&r[..i]);
                match r[i..].find("-->") {
                    Some(j) => r = &r[i + j + 3..],
                    None => {
                        r = "";
                    }
                }
            }
            s.push_str(r);
            cleaned = s;
            rest = &cleaned;
        }
        while !rest.is_empty() {
            match rest.find('<') {
                Some(0) => {
                    let end = tag_end(rest).unwrap_or(rest.len() - 1);
                    let tag = &rest[1..end];
                    rest = &rest[(end + 1).min(rest.len())..];
                    let closing = tag.starts_with('/');
                    let tag = tag.trim_start_matches('/').trim_end_matches('/');
                    let (name, attrs) = match tag.find(char::is_whitespace) {
                        Some(i) => (tag[..i].to_lowercase(), parse_attrs(&tag[i..])),
                        None => (tag.to_lowercase(), HashMap::default()),
                    };
                    match (name.as_str(), closing) {
                        ("p", false) => {
                            // alignment applies to the line, so only before any content
                            if first_text {
                                self.con.align = match attrs.get("align").map(|s| s.to_lowercase()).as_deref() {
                                    Some("center") => Align::Center,
                                    Some("right") => Align::Right,
                                    _ => Align::Left,
                                };
                                self.con.cur_align = Some(self.con.align);
                            }
                        }
                        ("p", true) => {}
                        ("br", _) => {
                            self.con.newline();
                        }
                        ("font", false) => {
                            style_stack.push(self.con.style.clone());
                            if let Some(c) = attrs.get("color").and_then(|c| color_from_str(c)) {
                                self.con.style.fg = c;
                            }
                        }
                        ("b", false) | ("i", false) | ("u", false) | ("s", false) => {
                            style_stack.push(self.con.style.clone());
                            match name.as_str() {
                                "b" => self.con.style.bold = true,
                                "i" => self.con.style.italic = true,
                                "u" => self.con.style.underline = true,
                                _ => self.con.style.strike = true,
                            }
                        }
                        ("font", true) | ("b", true) | ("i", true) | ("u", true) | ("s", true) => {
                            if let Some(st) = style_stack.pop() {
                                self.con.style = st;
                            }
                        }
                        ("button", false) => {
                            button = attrs.get("value").map(|v| match v.trim().parse::<i64>() {
                                Ok(i) => Value::Int(i),
                                Err(_) => Value::Str(v.clone()),
                            });
                            if let Some(b) = &button {
                                let t = attrs.get("title").map(|t| decode_entities(t)).unwrap_or_default();
                                self.con.set_tooltip(b, t.trim());
                            }
                        }
                        // Emuera-EM: text with a tooltip that is no button
                        ("nonbutton", false) => {
                            let t = attrs.get("title").map(|t| decode_entities(t)).unwrap_or_default();
                            button = (!t.trim().is_empty()).then(|| self.con.tip_area(t.trim()));
                        }
                        ("button", true) | ("nonbutton", true) => button = None,
                        ("clearbutton", c) => clear = !c,
                        ("div", false) => {
                            // the content up to its own </div> (Emuera.NET's
                            // divs nest: eraMegatenP's message log)
                            let (end, after) = div_content_end(rest);
                            let inner = rest[..end].to_string();
                            rest = &rest[after..];
                            // a button around a div holds what the div shows
                            // (eraMegatenP's battle slots: `<button value='9'>`
                            // around the enemy's divs, clicked to pick it)
                            self.html_outer_button = Some((button.clone(), clear));
                            self.html_div(&attrs, tag, &inner);
                            self.html_outer_button = None;
                            first_text = false;
                        }
                        // Emuera.NET: a picture placed against the window
                        // (`xpos` counts only there) is a div holding it
                        ("img", _) if attrs.get("display").is_some_and(|d| d.trim().to_ascii_lowercase().starts_with("absolute-")) => {
                            let mut div = HashMap::default();
                            for k in ["display", "xpos", "ypos"] {
                                if let Some(v) = attrs.get(k) {
                                    div.insert(k.to_string(), v.clone());
                                }
                            }
                            let img: String = ["src", "srcb", "width", "height"]
                                .iter()
                                .filter_map(|k| attrs.get(*k).map(|v| format!(" {}='{}'", k, v.replace('\'', "&#39;"))))
                                .collect();
                            self.html_outer_button = Some((button.clone(), clear));
                            self.html_div(&div, "div", &format!("<img{}>", img));
                            self.html_outer_button = None;
                            first_text = false;
                        }
                        ("img", _) => {
                            let src = attrs.get("src").cloned().unwrap_or_default();
                            let h = attrs.get("height").and_then(|v| mixed_num(v));
                            let w = attrs.get("width").and_then(|v| mixed_num(v));
                            let y = attrs.get("ypos").and_then(|v| mixed_num(v));
                            let srcb = attrs.get("srcb").cloned().filter(|s| !s.is_empty());
                            self.print_img(&src, w, h, y, if clear { None } else { button.clone() }, srcb);
                            first_text = false;
                        }
                        ("shape", _) => {
                            let ty = attrs.get("type").map(|s| s.to_lowercase()).unwrap_or_default();
                            // each value in pixels (`px`) or percent of the font size
                            let params: Vec<f32> = attrs
                                .get("param")
                                .map(|p| p.split(',').filter_map(mixed_num).map(|(v, px)| if px { v } else { fs * v / 100.0 }).collect())
                                .unwrap_or_default();
                            if ty == "space" {
                                let w = params.first().copied().unwrap_or(fs);
                                self.con.push_seg(Seg::Space { w });
                            } else if ty == "rect" {
                                // `x,y,w,h` or just the width (a full-height square-ish block)
                                let (w, h, y) =
                                    if params.len() >= 4 { (params[2], params[3], params[1]) } else { (params.first().copied().unwrap_or(fs), fs, 0.0) };
                                let color = attrs.get("color").and_then(|c| color_from_str(c)).unwrap_or(self.con.style.fg);
                                self.con.push_seg(Seg::Rect { w, h, y, color, button: if clear { None } else { button.clone() } });
                            }
                            first_text = false;
                        }
                        ("space", _) => {
                            let w =
                                attrs.get("param").or(attrs.get("value")).and_then(|v| mixed_num(v)).map_or(fs, |(v, px)| if px { v } else { fs * v / 100.0 });
                            self.con.push_seg(Seg::Space { w });
                        }
                        _ => {}
                    }
                }
                Some(i) => {
                    let text = decode_entities(&rest[..i]);
                    self.html_lines(&text, if clear { &None } else { &button });
                    first_text = false;
                    rest = &rest[i..];
                }
                None => {
                    let text = decode_entities(rest);
                    self.html_lines(&text, if clear { &None } else { &button });
                    rest = "";
                }
            }
        }
        // As in Emuera's Html2DisplayLine, `<br>` ends a line but the end of
        // the text adds one only if something follows the last `<br>`:
        // `HTML_PRINT "<br>"` is one line, not two. (AnonTW's animations
        // print a picture and nine of those, then CLEARLINE 10.)
        if end_line {
            if self.con.cur.is_empty() {
                self.con.cur_align = None;
            } else {
                self.con.newline();
            }
        }
        self.con.style = saved_style;
        self.con.align = saved_align;
    }

    /// `<div>`: print `inner` as lines of its own, wrapped at the div's
    /// width, and put them in a zero-width segment drawn at its offset.
    fn html_div(&mut self, attrs: &HashMap<String, String>, tag: &str, inner: &str) {
        let attrs = &with_emuera_net_names(attrs);
        let fs = self.con.font_size;
        let px = |v: &str| mixed_num(v.trim()).map(|(v, is_px)| if is_px { v } else { fs * v / 100.0 });
        let get = |k: &str| attrs.get(k).and_then(|v| px(v));
        let (mut x, mut y, mut w, mut h) = (get("xpos").unwrap_or(0.0), get("ypos").unwrap_or(0.0), get("width"), get("height"));
        if let Some(sz) = attrs.get("size") {
            let v: Vec<Option<f32>> = sz.split(',').map(px).collect();
            w = v.first().copied().flatten().or(w);
            h = v.get(1).copied().flatten().or(h);
        }
        if let Some(r) = attrs.get("rect") {
            let v: Vec<Option<f32>> = r.split(',').map(px).collect();
            x = v.first().copied().flatten().unwrap_or(x);
            y = v.get(1).copied().flatten().unwrap_or(y);
            w = v.get(2).copied().flatten().or(w);
            h = v.get(3).copied().flatten().or(h);
        }
        let cell_px = (fs / 2.0).max(1.0);
        let lh = self.con.line_height.max(1.0);
        // margin / padding / border: one value for all sides, or CSS's
        // "top,right,bottom,left" (2: top-bottom,left-right; 3: top,left-right,bottom)
        let sides = |k: &str| -> [f32; 4] {
            let Some(v) = attrs.get(k) else { return [0.0; 4] };
            let n: Vec<f32> = v.split(',').map(|x| px(x).unwrap_or(0.0)).collect();
            match n.len() {
                1 => [n[0]; 4],
                2 => [n[0], n[1], n[0], n[1]],
                3 => [n[0], n[1], n[2], n[1]],
                _ if n.len() >= 4 => [n[0], n[1], n[2], n[3]],
                _ => [0.0; 4],
            }
        };
        // vertical sides in lines, horizontal ones in half-width cells
        let to_cells = |v: [f32; 4]| -> [usize; 4] {
            [(v[0] / lh).round() as usize, (v[1] / cell_px).round() as usize, (v[2] / lh).round() as usize, (v[3] / cell_px).round() as usize]
        };
        let margin = to_cells(sides("margin"));
        let border = sides("border");
        let padding = sides("padding");
        let has_border = [border[0] > 0.0, border[1] > 0.0, border[2] > 0.0, border[3] > 0.0];
        x += margin[3] as f32 * cell_px;
        y += margin[0] as f32 * lh;
        // no width: as wide as its widest line (Emuera.NET's ConsoleDivElement;
        // Emuera-EM requires a width)
        let auto_width = w.is_none();
        let screen_px = self.con.width as f32 * cell_px;
        let w = w.unwrap_or(screen_px).max(cell_px) - (margin[1] + margin[3]) as f32 * cell_px;
        let h = h.map(|h| (h - (margin[0] + margin[2]) as f32 * lh).max(lh));
        // border plus padding on each side, in lines / cells
        let inset = |k: usize, unit: f32| (border[k] + padding[k]) / unit;
        // a div inside another starts from where Emuera puts the outer
        // one's content, not from the cell that was rounded to
        let display = attrs.get("display").map(String::as_str);
        let frac = if div_place(display, 0, 0).0 == crate::console::DivPlace::Relative { self.con.div_frac } else { (0.0, 0.0) };
        let (left, top) = (x / cell_px + frac.0, y / lh + frac.1);
        let room = if div_place(display, 0, 0).0 == crate::console::DivPlace::Relative { self.con.div_room } else { (0, 0) };
        let across = div_span(left, (!auto_width).then_some(w / cell_px), [inset(3, cell_px), inset(1, cell_px)], [has_border[3], has_border[1]], room.0);
        let down = div_span(top, h.map(|h| h / lh), [inset(0, lh), inset(2, lh)], [has_border[0], has_border[2]], room.1);
        // (only the rounding: content pushed past its border keeps what is
        // inside it there too, not back on the border)
        let rounding = |v: f32| v.clamp(-0.49, 0.49);
        let content_frac = (
            rounding(left + inset(3, cell_px) - (across.first + has_border[3] as i32 + across.pad[0] as i32) as f32),
            rounding(top + inset(0, lh) - (down.first + has_border[0] as i32 + down.pad[0] as i32) as f32),
        );
        let deco = crate::console::DivBox {
            bg: attrs.get("color").and_then(|c| color_from_str(c)),
            bcolor: attrs.get("bcolor").and_then(|c| color_from_str(c.split(',').next().unwrap_or(""))).or(Some(self.con.style.fg)),
            border: has_border,
            pad: [down.pad[0], across.pad[1], down.pad[1], across.pad[0]],
            // a corner smaller than half a cell wouldn't show
            round: sides("radius").map(|r| r >= cell_px / 2.0),
            thin: border.iter().all(|b| *b < cell_px / 2.0),
            inline_bottom: down.inline_after && border[2] < cell_px / 2.0,
        };
        let ins = deco.insets();
        let w = w.max(cell_px * (ins[1] + ins[3] + 1) as f32);
        let saved_width = self.con.width;
        // (Emuera.NET doesn't wrap a div's content either)
        let net_fork = self.net_fork();
        let wrap_at = if net_fork { saved_width } else { ((w / cell_px).floor() as usize).saturating_sub(1 + ins[1] + ins[3]).max(1) };
        let saved_frac = std::mem::replace(&mut self.con.div_frac, content_frac);
        let saved_room = std::mem::replace(&mut self.con.div_room, (ins[3] as i32, ins[0] as i32));
        let lines = self.html_apart(inner, wrap_at);
        self.con.div_frac = saved_frac;
        self.con.div_room = saved_room;
        let widest = lines.iter().map(|l| line_cells(l, cell_px)).max().unwrap_or(0);
        let fits = ((widest + ins[1] + ins[3]).clamp(1, saved_width.max(1)), if net_fork { content_rows(&lines) } else { lines.len() } + ins[0] + ins[2]);
        let mut cols = if auto_width { fits.0 } else { across.count.unwrap_or(fits.0).max(1) };
        let (top_row, mut rows) = (down.first, down.count);
        // Emuera.NET doesn't clip a div: its content runs past the box
        // (eraMegatenP's popups are sized in pixels for text that spills).
        // The box grows to hold it instead, the border and padding taking
        // whole cells here.
        if net_fork {
            cols = cols.max(fits.0);
            rows = rows.map(|r| r.max(fits.1));
        }
        let (place, mut dy) = div_place(display, top_row, rows.unwrap_or(fits.1));
        // ...and one placed above the window's bottom edge stays above it
        if net_fork && place == crate::console::DivPlace::Bottom && dy < 0 {
            dy = dy.min(-(rows.unwrap_or(fits.1) as i32));
        }
        let html = format!("<{}>{}</div>", tag, inner);
        self.con.push_seg(Seg::Div { dx: across.first, dy, cols, rows, lines, html, deco: Box::new(deco), place });
    }

    /// Does the game ship epizo12345's Emuera.NET fork? Looked up once.
    fn net_fork(&mut self) -> bool {
        self.con.net_fork()
    }

    /// `src` printed as lines of their own, `width` cells wide, and taken
    /// back out of the log (a div's content, the HTML island's lines).
    pub fn html_apart(&mut self, src: &str, width: usize) -> Vec<crate::console::Line> {
        let saved_cur = std::mem::take(&mut self.con.cur);
        let saved_cur_align = self.con.cur_align.take();
        let saved_width = self.con.width;
        let (n0, dropped0) = (self.con.lines.len(), self.con.dropped_front);
        self.con.width = width;
        self.html_print(src);
        let start = n0.saturating_sub(self.con.dropped_front - dropped0).min(self.con.lines.len());
        let lines = self.con.lines.split_off(start);
        self.con.low_water = self.con.low_water.min(start);
        self.con.width = saved_width;
        self.con.cur = saved_cur;
        self.con.cur_align = saved_cur_align;
        lines
    }

    /// Text between tags; a newline in it breaks the line like `<br>`
    /// (Emuera's HTML_PRINT does the same).
    fn html_lines(&mut self, text: &str, button: &Option<Value>) {
        let mut parts = text.split('\n');
        if let Some(first) = parts.next() {
            self.html_text(first.trim_end_matches('\r'), button);
        }
        for p in parts {
            self.con.newline();
            self.html_text(p.trim_end_matches('\r'), button);
        }
    }

    /// Text between tags: a button segment inside `<button>`, else plain
    /// styled text (no `[N]` auto-buttons in HTML).
    fn html_text(&mut self, text: &str, button: &Option<Value>) {
        if text.is_empty() {
            return;
        }
        match button {
            Some(v) => self.con.print_button(text, v.clone()),
            None => {
                let style = self.con.style.clone();
                self.con.push_seg(Seg::Text { text: text.to_string(), style, button: None, auto: false });
            }
        }
    }
}

/// Where a `<div>`'s content ends in `rest` (the text after its opening
/// tag), counting the divs opened inside it: (end of the content, start of
/// what follows its `</div>`). Without a closing tag the div runs to the end.
fn div_content_end(rest: &str) -> (usize, usize) {
    let lower = rest.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    // `<div` followed by a space, `>` or `/` (not `<divider>`)
    let opens_at = |i: usize| lower[i..].starts_with("<div") && matches!(bytes.get(i + 4), Some(b' ' | b'\t' | b'\r' | b'\n' | b'>' | b'/'));
    let mut depth = 0usize;
    let mut i = 0usize;
    while let Some(j) = lower[i..].find('<') {
        let at = i + j;
        if lower[at..].starts_with("</div") {
            if depth == 0 {
                let after = lower[at..].find('>').map_or(rest.len(), |k| at + k + 1);
                return (at, after);
            }
            depth -= 1;
        } else if opens_at(at) {
            depth += 1;
        }
        i = at + 1;
    }
    (rest.len(), rest.len())
}

/// A `<div>`'s `display` and its `ypos` in rows (`rows` high) as the
/// placement it is drawn with and the row offset from that place.
/// Emuera-EM's `absolute` measures `ypos` up from the window's bottom to
/// the div's bottom (ConsoleDivPart.DrawTo); Emuera.NET's
/// `absolute-leftbottom` measures it down from the bottom to the div's top,
/// `absolute-lefttop` from the top. A value neither knows is `relative`.
/// Where a `<div>` lands along one axis, in whole rows (or cells).
#[derive(Debug, PartialEq)]
struct DivSpan {
    /// the first row, borders included, from the div's anchor
    first: i32,
    /// rows from the first to the last (`None`: as many as the content needs)
    count: Option<usize>,
    /// padding rows before and after the content
    pad: [usize; 2],
    /// no length given, and the border after the content is less than half
    /// a row from it: drawn on the content's last row, not on a row of its own
    inline_after: bool,
}

/// A `<div>`'s rows (or cells) along one axis. `start` and `len` are its
/// edges in lines (or cells), `inset` the border plus padding before and
/// after it, `border` whether each side has one.
///
/// A border takes a whole row here but only a few pixels in Emuera. The
/// content goes to the row nearest where Emuera draws it, and a border to
/// the row its edge falls in, or the one outside the content when that row
/// is the content's own. The padding is what is left between them, so a
/// border and a padding of half a line together take one row, not two:
/// each nested frame used to push its content a row lower and a cell to
/// the right (eraMegatenP's training screen). A border only moves before
/// the div's place into rows the parent has there: its content's (`start`
/// at 1 or more), or `room`, the parent's own border and padding rows on
/// that side. One at `xpos='0'` at the top level keeps its border on
/// screen; one at the corner of a framed parent shares the parent's border
/// row instead of pushing its content a row down (the training screen's
/// portraits).
fn div_span(start: f32, len: Option<f32>, inset: [f32; 2], border: [bool; 2], room: i32) -> DivSpan {
    const EPS: f32 = 0.01;
    let mut content = (start + inset[0]).round() as i32;
    let first = if !border[0] {
        (start.round() as i32).min(content)
    } else {
        let edge = (start + EPS).floor() as i32;
        match () {
            _ if content > edge => edge,
            _ if edge >= 1 - room => edge - 1,
            _ => edge,
        }
    };
    content = content.max(first + border[0] as i32);
    let pad_before = (content - first - border[0] as i32).max(0) as usize;
    let Some(len) = len else {
        let pad_after = ((inset[1]).round() as i32 - border[1] as i32).max(0) as usize;
        let inline_after = border[1] && inset[1] < 0.5;
        return DivSpan { first, count: None, pad: [pad_before, pad_after], inline_after };
    };
    let end_edge = start + len;
    // one past the last row
    let mut end = if border[1] { (end_edge - EPS).floor() as i32 + 1 } else { end_edge.round() as i32 };
    end = end.max(content + 1 + border[1] as i32);
    let content_end = ((end_edge - inset[1]).round() as i32).max(content + 1);
    let pad_after = (end - border[1] as i32 - content_end).max(0) as usize;
    DivSpan { first, count: Some((end - first) as usize), pad: [pad_before, pad_after], inline_after: false }
}

/// Rows a div's content takes, counting the divs placed inside it: an
/// Emuera.NET frame grows to hold them too (eraMegatenP's PALAM gauges,
/// a div placed under the text, lost their last row).
fn content_rows(lines: &[crate::console::Line]) -> usize {
    let mut rows = lines.len() as i64;
    for (k, line) in lines.iter().enumerate() {
        for seg in &line.segs {
            if let Seg::Div { dy, rows: r, lines: inner, deco, place: crate::console::DivPlace::Relative, .. } = seg {
                let ins = deco.insets();
                let h = r.unwrap_or_else(|| content_rows(inner) + ins[0] + ins[2]);
                rows = rows.max(k as i64 + *dy as i64 + h as i64);
            }
        }
    }
    rows.max(0) as usize
}

fn div_place(display: Option<&str>, ypos_rows: i32, rows: usize) -> (crate::console::DivPlace, i32) {
    use crate::console::DivPlace;
    match display.map(|d| d.trim().to_ascii_lowercase()).as_deref() {
        Some("absolute") => (DivPlace::Bottom, -ypos_rows - rows as i32),
        Some("absolute-leftbottom") => (DivPlace::Bottom, ypos_rows),
        Some("absolute-lefttop") => (DivPlace::Top, ypos_rows),
        _ => (DivPlace::Relative, ypos_rows),
    }
}

/// A laid-out line's width in half-width cells of `cell_px` pixels, as an
/// auto-sized `<div>` measures its content: text by its Shift-JIS width,
/// pictures and shapes by their pixel width; a nested div takes none.
fn line_cells(line: &crate::console::Line, cell_px: f32) -> usize {
    let px_cells = |w: f32| (w.abs() / cell_px).round() as usize;
    line.segs
        .iter()
        .map(|s| match s {
            Seg::Text { text, .. } => crate::loader::sjis_len(text),
            Seg::Image { w, .. } | Seg::Space { w } | Seg::Rect { w, .. } => px_cells(*w),
            Seg::Div { .. } => 0,
        })
        .sum()
}

/// Emuera-EM's `MixedNum`: `300px` is pixels, a bare number is percent of
/// the font size. Returns (value, is_px).
fn mixed_num(v: &str) -> Option<(f32, bool)> {
    let t = v.trim();
    let lower = t.to_ascii_lowercase();
    match lower.strip_suffix("px") {
        Some(n) => n.trim().parse().ok().map(|x| (x, true)),
        None => t.parse().ok().map(|x| (x, false)),
    }
}

/// Emuera's `HtmlSubString`: split HTML text at `cols` half-width columns
/// (or at the first `<br>`). The first part closes the tags still open,
/// the rest reopens them. Images and shapes count two columns.
pub fn html_substring(src: &str, cols: usize) -> (String, String) {
    let mut open: Vec<(String, String)> = Vec::new(); // (opening tag, name)
    let mut used = 0usize;
    let mut i = 0usize;
    let mut cut = src.len();
    let mut skip_br = 0;
    let mut content = false;
    while i < src.len() {
        let rest = &src[i..];
        if rest.starts_with('<') {
            let Some(end) = rest.find('>') else { break };
            let tag = &rest[1..end];
            let name = tag.trim_start_matches('/').split(|c: char| c.is_whitespace()).next().unwrap_or("").to_lowercase();
            if name == "br" {
                cut = i;
                skip_br = end + 1;
                break;
            }
            if name == "img" || name == "shape" {
                if used + 2 > cols && content {
                    cut = i;
                    break;
                }
                used += 2;
                content = true;
            } else if tag.starts_with('/') {
                open.pop();
            } else {
                open.push((rest[..=end].to_string(), name));
            }
            i += end + 1;
            continue;
        }
        let c = rest.chars().next().unwrap();
        // entities count as one character
        let (w, len) = if c == '&' {
            match rest.find(';') {
                Some(e) if e < 10 => (1, e + 1),
                _ => (1, 1),
            }
        } else {
            (crate::loader::sjis_len(&c.to_string()), c.len_utf8())
        };
        if used + w > cols && content {
            cut = i;
            break;
        }
        used += w;
        content = true;
        i += len;
    }
    let mut first = src[..cut].to_string();
    for (_, n) in open.iter().rev() {
        first.push_str(&format!("</{}>", n));
    }
    let mut second: String = open.iter().map(|(t, _)| t.as_str()).collect();
    second.push_str(&src[(cut + skip_br).min(src.len())..]);
    if second.chars().all(|c| c.is_whitespace()) || cut + skip_br >= src.len() {
        second = if cut + skip_br >= src.len() { String::new() } else { second };
    }
    (first, second)
}

/// A `<div>`'s attributes with Emuera.NET's names (`border_width`,
/// `border_color`, `background_color`, its HtmlManager's) made Emuera-EM's
/// (`border`, `bcolor`, `color`); EM's own names win where both are given.
/// eraMegatenP is written for Emuera.NET; EM rejects these names, so no EM
/// game has them.
fn with_emuera_net_names(attrs: &HashMap<String, String>) -> HashMap<String, String> {
    let mut out = attrs.clone();
    for (net, em) in [("border_width", "border"), ("border_color", "bcolor"), ("background_color", "color")] {
        if let Some(v) = attrs.get(net) {
            out.entry(em.to_string()).or_insert_with(|| v.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use crate::ast::Align;
    use crate::console::Seg;
    use std::sync::Arc;

    fn vm() -> crate::vm::Vm {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let cfg = Arc::new(crate::emuconfig::Values::emuera());
        let csv = crate::csvdata::GameCsv::load(&root.join("sample/CSV"), None, &cfg);
        let fe = Box::new(crate::frontend::Headless::new(false));
        crate::vm::Vm::new(crate::parser::Program::default(), csv, crate::resources::Resources::default(), fe, root.join("sample"), cfg)
    }

    /// A `>` (or `<`) inside a quoted attribute value doesn't end the tag:
    /// eraKATANA's map buttons have `title = '…<入浴可能=1>…'`, and their
    /// `value` and text leaked out as `' value = '10115'>15`.
    #[test]
    fn quoted_angle_brackets_stay_in_the_tag() {
        assert_eq!(super::tag_end("<b>x"), Some(2));
        assert_eq!(super::tag_end("<a t='>' v=\"<>\">x"), Some(15));
        // an unclosed quote: the first `>` still ends the tag
        assert_eq!(super::tag_end("<a t='x>y"), Some(7));
        assert_eq!(super::tag_end("<a"), None);
        let mut vm = vm();
        vm.html_print("A<button title = '\r\n■浴室\r\n<入浴可能=1>\r\n' value = '10115'>15</button>B");
        let line = vm.con.lines.last().expect("a line");
        let text = crate::console::Console::line_text(line);
        assert_eq!(text, "A15B");
        assert!(line.segs.iter().any(|s| matches!(s, Seg::Text { text, button: Some(_), .. } if text == "15")), "{:?}", line.segs);
        assert!(vm.con.button_values().contains(&crate::vm::value::Value::Int(10115)));
    }

    /// HTML_PRINT's lines start left-aligned whatever ALIGNMENT says (only
    /// `<p align>` changes them), and the script's ALIGNMENT is kept.
    #[test]
    fn html_lines_ignore_the_scripts_alignment() {
        let mut vm = vm();
        vm.con.align = Align::Center;
        vm.html_print("logo");
        assert_eq!(vm.con.lines.last().map(|l| l.align), Some(Align::Left));
        vm.html_print("<p align='right'>r</p>");
        assert_eq!(vm.con.lines.last().map(|l| l.align), Some(Align::Right));
        assert_eq!(vm.con.align, Align::Center);
    }

    /// The last line's div segments, in order (nested ones after their parent).
    fn divs(vm: &crate::vm::Vm) -> Vec<(usize, i32, i32, crate::console::DivPlace)> {
        fn walk(segs: &[Seg], out: &mut Vec<(usize, i32, i32, crate::console::DivPlace)>) {
            for s in segs {
                if let Seg::Div { cols, dx, dy, place, lines, .. } = s {
                    out.push((*cols, *dx, *dy, *place));
                    for l in lines {
                        walk(&l.segs, out);
                    }
                }
            }
        }
        let mut out = Vec::new();
        walk(&vm.con.lines.last().unwrap().segs, &mut out);
        out
    }

    /// A div's content ends at its own `</div>`, past the ones nested in it.
    #[test]
    fn div_content_end_skips_nested_divs() {
        let rest = "a<div x='1'>b<DIV>c</div></div>d</div>tail";
        let (end, after) = super::div_content_end(rest);
        assert_eq!(&rest[..end], "a<div x='1'>b<DIV>c</div></div>d");
        assert_eq!(&rest[after..], "tail");
        // <divider> opens no div
        let rest = "x<divider>y</div>t";
        let (end, after) = super::div_content_end(rest);
        assert_eq!((&rest[..end], &rest[after..]), ("x<divider>y", "t"));
        assert_eq!(super::div_content_end("open"), (4, 4));
    }

    /// Emuera.NET: a div without a width is as wide as its widest line
    /// (plus its border); nested divs keep their own place.
    #[test]
    fn div_sizes_to_its_content_and_nests() {
        let mut vm = vm();
        vm.html_print("<div>abc<br>abcdef</div>");
        assert_eq!(divs(&vm)[0].0, 6);
        vm.html_print("<div border_width='1px'>ab</div>");
        assert_eq!(divs(&vm)[0].0, 4);
        vm.html_print("<div width='200px'>ab</div>");
        assert_eq!(divs(&vm)[0].0, (200.0 / (vm.con.font_size / 2.0)).floor() as usize);
        vm.html_print("<div xpos='0'>a<div xpos='90px'>b</div>c</div>");
        let d = divs(&vm);
        assert_eq!(d.len(), 2, "{d:?}");
        assert_eq!(d[1].1, (90.0 / (vm.con.font_size / 2.0)).round() as i32);
    }

    /// `display`: Emuera.NET's absolute-lefttop / absolute-leftbottom count
    /// `ypos` from the window's top / bottom to the div's top; Emuera-EM's
    /// absolute counts it up from the bottom to the div's bottom.
    #[test]
    fn div_display_modes() {
        use crate::console::DivPlace;
        let mut vm = vm();
        let lh = vm.con.line_height;
        let rows = |px: f32| (px / lh).round() as i32;
        vm.html_print(&format!("<div display='absolute-leftbottom' ypos='{}px' xpos='36px'>x</div>", -10.0 * lh));
        assert_eq!(divs(&vm)[0], (1, 4, rows(-10.0 * lh), DivPlace::Bottom));
        vm.html_print("<div display='absolute-lefttop' ypos='0'>x</div>");
        assert_eq!(divs(&vm)[0].3, DivPlace::Top);
        vm.html_print(&format!("<div display='absolute' ypos='{}px' width='90px' height='{}px'>x</div>", 2.0 * lh, 3.0 * lh));
        assert_eq!((divs(&vm)[0].2, divs(&vm)[0].3), (-5, DivPlace::Bottom));
        vm.html_print("<div display='relative' ypos='0'>x</div>");
        assert_eq!(divs(&vm)[0].3, DivPlace::Relative);
        // a picture placed against the window: a div holding it
        vm.html_print("<img src='none' display='absolute-lefttop' xpos='18px'>");
        assert_eq!((divs(&vm)[0].1, divs(&vm)[0].3), (2, DivPlace::Top));
    }

    /// An Emuera.NET game's div shows all its content: the box grows to
    /// hold it, and one placed against the window's bottom stays above it
    /// (eraMegatenP's INPUT_YN_POPUP, sized for text that spills out).
    /// Emuera.NET removes tabs from printed text, HTML included, so a
    /// frame line holding them (eraMegatenP's message window footer,
    /// `AUTO&lt;<tab>OFF&gt;`) stays as wide as the lines above it.
    /// Emuera-EM keeps them.
    #[test]
    fn emuera_net_prints_no_tabs() {
        let texts = |vm: &crate::vm::Vm| -> Vec<String> { vm.con.lines.iter().map(crate::console::Console::line_text).collect() };
        let mut vm = vm();
        vm.con.net_fork = Some(true);
        vm.html_print("AUTO&lt;\tOFF&gt;");
        vm.con.print("a\tb");
        vm.con.newline();
        vm.con.print_button("c\td", crate::vm::value::Value::Int(1));
        vm.con.newline();
        assert_eq!(texts(&vm), ["AUTO<OFF>", "ab", "cd"]);
        let mut vm = self::vm();
        vm.con.net_fork = Some(false);
        vm.con.print("a\tb");
        vm.con.newline();
        assert_eq!(texts(&vm), ["a\tb"]);
    }

    #[test]
    fn emuera_net_divs_hold_their_content() {
        use crate::console::DivPlace;
        let popup = "<div display='absolute-leftbottom' xpos='0px' ypos='-72px' height='72px' width='117px' padding='60' border_width='30'>Test popup?<br>[0] Yes<br>[1] No</div>";
        let mut vm = vm();
        vm.con.net_fork = Some(false);
        vm.con.font_size = 18.0;
        vm.con.line_height = 18.0;
        vm.html_print(popup);
        assert_eq!(divs(&vm)[0], (13, 0, -4, DivPlace::Bottom), "Emuera-EM: as declared, clipped");
        vm.con.net_fork = Some(true);
        vm.html_print(popup);
        // 11 columns of text inside a border and padding of a cell each; 3
        // lines inside 0.9 of a line of border and padding (one row) each
        assert_eq!(divs(&vm)[0], (15, 0, -5, DivPlace::Bottom));
        // its lines aren't wrapped at the declared width
        let Some(Seg::Div { lines, .. }) = vm.con.lines.last().unwrap().segs.first() else { panic!("no div") };
        assert!(lines.iter().all(|l| l.cols >= vm.con.width), "{:?}", lines.iter().map(|l| l.cols).collect::<Vec<_>>());
    }

    /// A bordered edge between two rows goes to the outer row; other edges
    /// round (eraMegatenP's battle navigator: a frame from half a line
    /// above its text to half a line below).
    #[test]
    fn a_border_between_rows_goes_outward() {
        use super::{div_span, DivSpan};
        let span = |first, count, pad| DivSpan { first, count, pad, inline_after: false };
        let (both, none) = ([true; 2], [false; 2]);
        assert_eq!(div_span(0.5, Some(3.0), [0.0; 2], both, 0), span(0, Some(4), [0, 0]));
        assert_eq!(div_span(0.5, Some(3.0), [0.0; 2], none, 0), span(1, Some(3), [0, 0]));
        // an edge on a row's top: the border goes to the row above, the
        // content stays where Emuera draws it
        assert_eq!(div_span(1.0, Some(3.0), [0.0; 2], both, 0), span(0, Some(4), [0, 0]));
        assert_eq!(div_span(-0.25, None, [0.0; 2], both, 0), DivSpan { inline_after: true, ..span(-1, None, [0, 0]) });
        assert_eq!(div_span(-0.25, None, [0.0; 2], none, 0), span(0, None, [0, 0]));
        // ...but not before the parent's content (a border at xpos 0 stays on screen)
        assert_eq!(div_span(0.0, Some(4.0), [0.1; 2], both, 0), span(0, Some(4), [0, 0]));
        // ...unless the parent has a border or padding row there: a box at
        // the corner of a frame shares its border row, its content (a
        // portrait) staying on the frame's first row
        assert_eq!(div_span(0.2, Some(6.0), [0.1; 2], both, 1), span(-1, Some(8), [0, 0]));
        // eraMegatenP's training screen: a border plus half a line of
        // padding is one row, not two (the frame's text on the row after
        // its top border); the TURN box (1.2 lines from line 1) keeps its
        // text on line 1
        assert_eq!(div_span(2.7, Some(10.2), [0.53; 2], both, 0), span(2, Some(11), [0, 0]));
        assert_eq!(div_span(1.0, Some(1.2), [0.25; 2], both, 0), span(0, Some(3), [0, 0]));
        // eraMegatenP's "Dangerous Day" badge: its bottom border, a fifth of
        // a line under the text, is drawn on the text's row
        assert_eq!(div_span(-1.07, None, [0.21; 2], both, 0), DivSpan { inline_after: true, ..span(-2, None, [0, 0]) });
        assert!(!div_span(0.0, None, [0.6; 2], both, 0).inline_after);
        // thick padding still takes its rows
        assert_eq!(div_span(0.0, Some(6.0), [2.0; 2], both, 0), span(0, Some(6), [1, 1]));
        // the frame now encloses the text div beside it
        let mut vm = vm();
        let lh = vm.con.line_height;
        vm.html_print(&format!(
            "<div ypos='{}px' height='{}px' width='300' border='2px'></div><div xpos='50' ypos='{}px'>a<br>b</div>",
            0.5 * lh,
            3.0 * lh,
            lh
        ));
        let d = divs(&vm);
        assert_eq!((d[0].2, d[1].2), (0, 1));
    }

    /// A button around a div holds the div's content (eraMegatenP's battle
    /// slots, clicked to pick an enemy); a div after it doesn't.
    #[test]
    fn a_button_around_a_div_holds_its_content() {
        use crate::vm::value::Value;
        fn buttons(segs: &[Seg], out: &mut Vec<Option<Value>>) {
            for s in segs {
                match s {
                    Seg::Text { button, .. } => out.push(button.clone()),
                    Seg::Div { lines, .. } => lines.iter().for_each(|l| buttons(&l.segs, out)),
                    _ => {}
                }
            }
        }
        let mut vm = vm();
        let found = |vm: &crate::vm::Vm| {
            let mut out = Vec::new();
            buttons(&vm.con.lines.last().unwrap().segs, &mut out);
            out
        };
        vm.html_print("<button value='9'><div xpos='0' ypos='0'>[ 9] Rat</div></button>");
        assert_eq!(found(&vm), [Some(Value::Int(9))]);
        vm.html_print("<div xpos='200'><button value='7'><div>[ 7] Pix</div></button></div>");
        assert_eq!(found(&vm), [Some(Value::Int(7))]);
        vm.html_print("<clearbutton><button value='3'><div>[ 3]</div></button></clearbutton>");
        assert_eq!(found(&vm), [None]);
        vm.html_print("<div>after</div>");
        assert_eq!(found(&vm), [None]);
    }

    /// Emuera.NET's div attribute names stand for Emuera-EM's.
    #[test]
    fn emuera_net_div_names() {
        let mut a = crate::fxhash::HashMap::default();
        a.insert("border_width".to_string(), "2px".to_string());
        a.insert("background_color".to_string(), "#102030".to_string());
        a.insert("bcolor".to_string(), "#FFFFFF".to_string());
        a.insert("border_color".to_string(), "#000000".to_string());
        let b = super::with_emuera_net_names(&a);
        assert_eq!((b["border"].as_str(), b["color"].as_str(), b["bcolor"].as_str()), ("2px", "#102030", "#FFFFFF"));
    }
}
