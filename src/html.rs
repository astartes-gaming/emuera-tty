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
                        ("img", _) => {
                            let src = attrs.get("src").cloned().unwrap_or_default();
                            let h = attrs.get("height").and_then(|v| mixed_num(v));
                            let w = attrs.get("width").and_then(|v| mixed_num(v));
                            let y = attrs.get("ypos").and_then(|v| mixed_num(v));
                            self.print_img(&src, w, h, y, button.clone());
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
                                self.con.push_seg(Seg::Rect { w, h, color, button: button.clone() });
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
                    self.html_text(&text, &button);
                    first_text = false;
                    rest = &rest[i..];
                }
                None => {
                    let text = decode_entities(rest);
                    self.html_text(&text, &button);
                    rest = "";
                }
            }
        }
        self.con.newline();
        self.con.style = saved_style;
        self.con.align = saved_align;
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
