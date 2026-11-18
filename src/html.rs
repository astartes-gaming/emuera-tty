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
        // (Emuera's PrintHtml prints nothing for "")
        if src.is_empty() {
            return;
        }
        if !self.con.cur.is_empty() {
            self.con.newline();
        }
        let saved_style = self.con.style.clone();
        let saved_align = self.con.align;
        // its lines start left-aligned whatever ALIGNMENT says; only <p
        // align> changes that (Emuera-EM's HtmlManager state, Emuera.NET's
        // Html2DisplayLine): eraMegatenP centres its title logo itself,
        // inside a centred ALIGNMENT
        self.con.align = Align::Left;
        self.con.cur_align = Some(Align::Left);
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
                            self.html_div(&attrs, tag, &inner);
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
                            self.html_div(&div, "div", &format!("<img{}>", img));
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
                                let (w, h) = if params.len() >= 4 { (params[2], params[3]) } else { (params.first().copied().unwrap_or(fs), fs) };
                                let color = attrs.get("color").and_then(|c| color_from_str(c)).unwrap_or(self.con.style.fg);
                                self.con.push_seg(Seg::Rect { w, h, color, button: if clear { None } else { button.clone() } });
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
        if self.con.cur.is_empty() {
            self.con.cur_align = None;
        } else {
            self.con.newline();
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
        let deco = crate::console::DivBox {
            bg: attrs.get("color").and_then(|c| color_from_str(c)),
            bcolor: attrs.get("bcolor").and_then(|c| color_from_str(c.split(',').next().unwrap_or(""))).or(Some(self.con.style.fg)),
            border: [border[0] > 0.0, border[1] > 0.0, border[2] > 0.0, border[3] > 0.0],
            pad: to_cells(sides("padding")),
            // a corner smaller than half a cell wouldn't show
            round: sides("radius").map(|r| r >= cell_px / 2.0),
        };
        let ins = deco.insets();
        x += margin[3] as f32 * cell_px;
        y += margin[0] as f32 * lh;
        // no width: as wide as its widest line (Emuera.NET's ConsoleDivElement;
        // Emuera-EM requires a width)
        let auto_width = w.is_none();
        let screen_px = self.con.width as f32 * cell_px;
        let w = w.unwrap_or(screen_px).max(cell_px) - (margin[1] + margin[3]) as f32 * cell_px;
        let w = w.max(cell_px * (ins[1] + ins[3] + 1) as f32);
        let h = h.map(|h| (h - (margin[0] + margin[2]) as f32 * lh).max(lh));
        let saved_width = self.con.width;
        // (Emuera.NET doesn't wrap a div's content either)
        let net_fork = self.net_fork();
        let wrap_at = if net_fork { saved_width } else { ((w / cell_px).floor() as usize).saturating_sub(1 + ins[1] + ins[3]).max(1) };
        let lines = self.html_apart(inner, wrap_at);
        let widest = lines.iter().map(|l| line_cells(l, cell_px)).max().unwrap_or(0);
        let fits = ((widest + ins[1] + ins[3]).clamp(1, saved_width.max(1)), lines.len() + ins[0] + ins[2]);
        let mut cols = if auto_width { fits.0 } else { (w / cell_px).floor().max(1.0) as usize };
        let mut rows = h.map(|h| (h / lh).round().max(1.0) as usize);
        // Emuera.NET doesn't clip a div: its content runs past the box
        // (eraMegatenP's popups are sized in pixels for text that spills).
        // The box grows to hold it instead, the border and padding taking
        // whole cells here.
        if net_fork {
            cols = cols.max(fits.0);
            rows = rows.map(|r| r.max(fits.1));
        }
        let (place, mut dy) = div_place(attrs.get("display").map(String::as_str), (y / lh).round() as i32, rows.unwrap_or(fits.1));
        // ...and one placed above the window's bottom edge stays above it
        if net_fork && place == crate::console::DivPlace::Bottom && dy < 0 {
            dy = dy.min(-(rows.unwrap_or(fits.1) as i32));
        }
        let html = format!("<{}>{}</div>", tag, inner);
        self.con.push_seg(Seg::Div { dx: (x / cell_px).round() as i32, dy, cols, rows, lines, html, deco: Box::new(deco), place });
    }

    /// Does the game ship epizo12345's Emuera.NET fork? Looked up once.
    fn net_fork(&mut self) -> bool {
        let dir = &self.game_dir;
        *self.net_fork.get_or_insert_with(|| crate::loader::ships_net_fork(dir))
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
    #[test]
    fn emuera_net_divs_hold_their_content() {
        use crate::console::DivPlace;
        let popup = "<div display='absolute-leftbottom' xpos='0px' ypos='-72px' height='72px' width='117px' padding='60' border_width='30'>Test popup?<br>[0] Yes<br>[1] No</div>";
        let mut vm = vm();
        vm.net_fork = Some(false);
        vm.con.font_size = 18.0;
        vm.con.line_height = 18.0;
        vm.html_print(popup);
        assert_eq!(divs(&vm)[0], (13, 0, -4, DivPlace::Bottom), "Emuera-EM: as declared, clipped");
        vm.net_fork = Some(true);
        vm.html_print(popup);
        // 11 columns of text and 3 lines inside a border and padding of a cell each
        assert_eq!(divs(&vm)[0], (15, 0, -7, DivPlace::Bottom));
        // its lines aren't wrapped at the declared width
        let Some(Seg::Div { lines, .. }) = vm.con.lines.last().unwrap().segs.first() else { panic!("no div") };
        assert!(lines.iter().all(|l| l.cols >= vm.con.width), "{:?}", lines.iter().map(|l| l.cols).collect::<Vec<_>>());
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
