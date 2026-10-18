//! HTML_PRINT subset: p, font, b, i, u, s, button, nonbutton, img, shape, br.
//!
//! Emuera's HTML is a small markup for one console line, not real HTML:
//! tags are interpreted left to right straight into console segments,
//! with a stack so `</font>` / `</b>` restore the previous style. Sizes
//! (`<img height>`, `<shape param>`) are percentages of the font size, or
//! pixels with a `px` suffix (Emuera-EM). Unknown tags are ignored.

use crate::ast::Align;
use crate::console::{Seg, Style};
use crate::vm::builtins::color_from_str;
use crate::vm::value::Value;
use crate::vm::Vm;
use crate::fxhash::HashMap;

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
        if !self.con.cur.is_empty() {
            self.con.newline();
        }
        let saved_style = self.con.style.clone();
        let saved_align = self.con.align;
        let mut style_stack: Vec<Style> = Vec::new();
        let mut button: Option<Value> = None;
        // inside <clearbutton>: buttons are shown but not clickable
        let mut clear = false;
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
                    let end = rest.find('>').unwrap_or(rest.len() - 1);
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
                        }
                        ("button", true) | ("nonbutton", _) => button = None,
                        ("clearbutton", c) => clear = !c,
                        ("div", false) => {
                            // the content up to </div> (divs don't nest)
                            let lower = rest.to_ascii_lowercase();
                            let end = lower.find("</div").unwrap_or(rest.len());
                            let inner = rest[..end].to_string();
                            rest = match rest[end..].find('>') {
                                Some(j) => &rest[end + j + 1..],
                                None => "",
                            };
                            self.html_div(&attrs, tag, &inner);
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
                                .map(|p| p.split(',').filter_map(|x| mixed_num(x)).map(|(v, px)| if px { v } else { fs * v / 100.0 }).collect())
                                .unwrap_or_default();
                            if ty == "space" {
                                let w = params.first().copied().unwrap_or(fs);
                                self.con.push_seg(Seg::Space { w });
                            } else if ty == "rect" {
                                let (w, h) = if params.len() >= 4 { (params[2], params[3]) } else { (params.first().copied().unwrap_or(fs), fs) };
                                let color = attrs.get("color").and_then(|c| color_from_str(c)).unwrap_or(self.con.style.fg);
                                self.con.push_seg(Seg::Rect { w, h, color, button: if clear { None } else { button.clone() } });
                            }
                            first_text = false;
                        }
                        ("space", _) => {
                            let w = attrs.get("param").or(attrs.get("value")).and_then(|v| mixed_num(v)).map_or(fs, |(v, px)| if px { v } else { fs * v / 100.0 });
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
        self.con.newline();
        self.con.style = saved_style;
        self.con.align = saved_align;
    }

    /// `<div>`: print `inner` as lines of its own, wrapped at the div's
    /// width, and put them in a zero-width segment drawn at its offset.
    fn html_div(&mut self, attrs: &HashMap<String, String>, tag: &str, inner: &str) {
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
        let w = w.unwrap_or(self.con.width as f32 * cell_px).max(cell_px);
        // lay the content out as lines of its own
        let saved_cur = std::mem::take(&mut self.con.cur);
        let saved_cur_align = self.con.cur_align.take();
        let saved_width = self.con.width;
        let (n0, dropped0) = (self.con.lines.len(), self.con.dropped_front);
        self.con.width = ((w / cell_px).floor() as usize).saturating_sub(1).max(1);
        self.html_print(inner);
        let start = n0.saturating_sub(self.con.dropped_front - dropped0).min(self.con.lines.len());
        let lines = self.con.lines.split_off(start);
        self.con.low_water = self.con.low_water.min(start);
        self.con.width = saved_width;
        self.con.cur = saved_cur;
        self.con.cur_align = saved_cur_align;
        let html = format!("<{}>{}</div>", tag, inner);
        let lh = self.con.line_height.max(1.0);
        self.con.push_seg(Seg::Div {
            dx: (x / cell_px).round() as i32,
            dy: (y / lh).round() as i32,
            cols: (w / cell_px).floor().max(1.0) as usize,
            rows: h.map(|h| (h / lh).round().max(1.0) as usize),
            lines,
            html,
        });
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
