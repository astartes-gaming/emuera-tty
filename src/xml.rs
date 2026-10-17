//! Minimal XML reader / writer helpers: enough for Emuera-EM's DT_FROMXML /
//! DT_TOXML (ADO.NET DataTable XML) and the XML_* functions. Handles
//! elements, attributes, text, CDATA, comments, processing instructions and
//! the predefined + numeric character entities. No DTDs, no namespaces
//! (prefixes are kept as part of the name).

#[derive(Debug, Clone, Default)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Node>,
}

#[derive(Debug, Clone)]
pub enum Node {
    Elem(Element),
    Text(String),
}

impl Element {
    pub fn attr(&self, k: &str) -> Option<&str> {
        self.attrs.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str())
    }
    pub fn elems(&self) -> impl Iterator<Item = &Element> {
        self.children.iter().filter_map(|n| match n {
            Node::Elem(e) => Some(e),
            _ => None,
        })
    }
    /// Concatenated text of this element and its descendants.
    pub fn text(&self) -> String {
        let mut s = String::new();
        for c in &self.children {
            match c {
                Node::Text(t) => s.push_str(t),
                Node::Elem(e) => s.push_str(&e.text()),
            }
        }
        s
    }
    /// Local name (after any `prefix:`).
    pub fn local(&self) -> &str {
        self.name.rsplit(':').next().unwrap_or(&self.name)
    }
    /// Depth-first search for elements with the given local name.
    pub fn find_all<'a>(&'a self, local: &str, out: &mut Vec<&'a Element>) {
        for e in self.elems() {
            if e.local() == local {
                out.push(e);
            }
            e.find_all(local, out);
        }
    }
}

pub fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&apos;"),
            c => o.push(c),
        }
    }
    o
}

pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut o = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(p) = rest.find('&') {
        o.push_str(&rest[..p]);
        rest = &rest[p..];
        let Some(semi) = rest.find(';') else { break };
        let ent = &rest[1..semi];
        let rep = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ if ent.starts_with("#x") || ent.starts_with("#X") => u32::from_str_radix(&ent[2..], 16).ok().and_then(char::from_u32),
            _ if ent.starts_with('#') => ent[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match rep {
            Some(c) => {
                o.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                o.push('&');
                rest = &rest[1..];
            }
        }
    }
    o.push_str(rest);
    o
}

/// Parse a document and return its root element.
pub fn parse(src: &str) -> Result<Element, String> {
    let mut p = Parser { s: src.trim_start_matches('\u{feff}'), pos: 0 };
    p.skip_misc()?;
    let root = p.element()?;
    Ok(root)
}

struct Parser<'a> {
    s: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn rest(&self) -> &'a str {
        &self.s[self.pos..]
    }
    fn err<T>(&self, m: &str) -> Result<T, String> {
        let line = self.s[..self.pos].matches('\n').count() + 1;
        Err(format!("XML line {}: {}", line, m))
    }
    fn skip_ws(&mut self) {
        let t = self.rest();
        self.pos += t.len() - t.trim_start().len();
    }
    fn skip_until(&mut self, end: &str) -> Result<(), String> {
        match self.rest().find(end) {
            Some(i) => {
                self.pos += i + end.len();
                Ok(())
            }
            None => self.err(&format!("missing '{}'", end)),
        }
    }
    /// Skip whitespace, comments, `<?…?>` and `<!DOCTYPE…>` before the root.
    fn skip_misc(&mut self) -> Result<(), String> {
        loop {
            self.skip_ws();
            let r = self.rest();
            if r.starts_with("<?") {
                self.skip_until("?>")?;
            } else if r.starts_with("<!--") {
                self.skip_until("-->")?;
            } else if r.starts_with("<!") {
                self.skip_until(">")?;
            } else {
                return Ok(());
            }
        }
    }
    fn name(&mut self) -> Result<String, String> {
        let r = self.rest();
        let n = r.find(|c: char| c.is_whitespace() || "/>=".contains(c)).unwrap_or(r.len());
        if n == 0 {
            return self.err("expected a name");
        }
        self.pos += n;
        Ok(r[..n].to_string())
    }
    fn element(&mut self) -> Result<Element, String> {
        if !self.rest().starts_with('<') {
            return self.err("expected '<'");
        }
        self.pos += 1;
        let mut el = Element { name: self.name()?, ..Default::default() };
        loop {
            self.skip_ws();
            let r = self.rest();
            if r.starts_with("/>") {
                self.pos += 2;
                return Ok(el);
            }
            if r.starts_with('>') {
                self.pos += 1;
                break;
            }
            let k = self.name()?;
            self.skip_ws();
            if !self.rest().starts_with('=') {
                return self.err("expected '=' in attribute");
            }
            self.pos += 1;
            self.skip_ws();
            let q = self.rest().chars().next();
            let Some(q @ ('"' | '\'')) = q else { return self.err("expected quoted attribute value") };
            self.pos += 1;
            let Some(end) = self.rest().find(q) else { return self.err("unterminated attribute value") };
            let v = unescape(&self.rest()[..end]);
            self.pos += end + 1;
            el.attrs.push((k, v));
        }
        // content
        let mut text = String::new();
        loop {
            let r = self.rest();
            if r.is_empty() {
                return self.err(&format!("unclosed <{}>", el.name));
            }
            if r.starts_with("</") {
                if !text.is_empty() {
                    el.children.push(Node::Text(std::mem::take(&mut text)));
                }
                self.pos += 2;
                let n = self.name()?;
                if n != el.name {
                    return self.err(&format!("</{}> closes <{}>", n, el.name));
                }
                self.skip_ws();
                if !self.rest().starts_with('>') {
                    return self.err("expected '>'");
                }
                self.pos += 1;
                return Ok(el);
            }
            if r.starts_with("<!--") {
                self.skip_until("-->")?;
            } else if r.starts_with("<![CDATA[") {
                let Some(end) = r.find("]]>") else { return self.err("unterminated CDATA") };
                text.push_str(&r[9..end]);
                self.pos += end + 3;
            } else if r.starts_with("<?") {
                self.skip_until("?>")?;
            } else if r.starts_with('<') {
                if !text.is_empty() {
                    el.children.push(Node::Text(std::mem::take(&mut text)));
                }
                let child = self.element()?;
                el.children.push(Node::Elem(child));
            } else {
                let end = r.find('<').unwrap_or(r.len());
                text.push_str(&unescape(&r[..end]));
                self.pos += end;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_with_entities() {
        let d = parse("<?xml version='1.0'?>\n<!-- c --><a x=\"1&amp;2\"><b>t&lt;x</b><c/><![CDATA[<raw>]]></a>").unwrap();
        assert_eq!(d.name, "a");
        assert_eq!(d.attr("x"), Some("1&2"));
        assert_eq!(d.elems().count(), 2);
        assert_eq!(d.elems().next().unwrap().text(), "t<x");
        assert_eq!(d.text(), "t<x<raw>");
    }

    #[test]
    fn rejects_mismatched_close() {
        assert!(parse("<a><b></a>").is_err());
    }
}
