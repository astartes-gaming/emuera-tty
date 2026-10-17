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

// ===================================================== editing and XPath
// Emuera-EM's XML_* functions work on .NET XmlDocuments: they select nodes
// with XPath, read or change them, and hand the edited text back. What
// follows is that model on top of `Element`: a document is a wrapper
// element (the document node) whose child is the root element, a node is
// addressed by its child-index path from the wrapper, and the XPath subset
// covers what scripts use: `/a/b`, `//b`, `*`, `text()`, `node()`, `@x`,
// `@*`, `.`, `..`, and predicates (`[2]`, `[last()]`, `[@x='v' and y!=1]`,
// `contains()`, `count()`, `not()`…).

/// Parse XML text into a document (whitespace-only text dropped, as
/// `XmlDocument.LoadXml` does).
pub fn load_doc(src: &str) -> Result<Element, String> {
    let mut root = parse(src)?;
    strip_ws(&mut root);
    Ok(Element { name: "#document".into(), attrs: vec![], children: vec![Node::Elem(root)] })
}

fn strip_ws(e: &mut Element) {
    e.children.retain(|c| !matches!(c, Node::Text(t) if t.trim().is_empty()));
    for c in &mut e.children {
        if let Node::Elem(c) = c {
            strip_ws(c);
        }
    }
}

/// Parse a fragment (`InnerXml = …`): its top-level nodes.
pub fn parse_fragment(src: &str) -> Result<Vec<Node>, String> {
    let mut e = parse(&format!("<x>{}</x>", src))?;
    strip_ws(&mut e);
    Ok(e.children)
}

/// A selected node.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Sel {
    /// the element or text node at this path (empty: the document)
    Node(Vec<usize>),
    /// attribute number `.1` of the element at `.0`
    Attr(Vec<usize>, usize),
}

enum NodeRef<'a> {
    Elem(&'a Element),
    Text(&'a str),
}

fn elem_at<'a>(doc: &'a Element, path: &[usize]) -> Option<&'a Element> {
    let mut e = doc;
    for &i in path {
        match e.children.get(i)? {
            Node::Elem(c) => e = c,
            Node::Text(_) => return None,
        }
    }
    Some(e)
}

fn elem_at_mut<'a>(doc: &'a mut Element, path: &[usize]) -> Option<&'a mut Element> {
    let mut e = doc;
    for &i in path {
        match e.children.get_mut(i)? {
            Node::Elem(c) => e = c,
            Node::Text(_) => return None,
        }
    }
    Some(e)
}

fn node_at<'a>(doc: &'a Element, path: &[usize]) -> Option<NodeRef<'a>> {
    let Some((&last, parent)) = path.split_last() else { return Some(NodeRef::Elem(doc)) };
    match elem_at(doc, parent)?.children.get(last)? {
        Node::Elem(e) => Some(NodeRef::Elem(e)),
        Node::Text(t) => Some(NodeRef::Text(t)),
    }
}

/// Write an element the way .NET's `OuterXml` does (`<a x="1">…</a>`, `<a />`).
pub fn write_elem(e: &Element, out: &mut String) {
    out.push('<');
    out.push_str(&e.name);
    for (k, v) in &e.attrs {
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        out.push_str(&escape_attr(v));
        out.push('"');
    }
    if e.children.is_empty() {
        out.push_str(" />");
        return;
    }
    out.push('>');
    write_children(e, out);
    out.push_str("</");
    out.push_str(&e.name);
    out.push('>');
}

fn write_children(e: &Element, out: &mut String) {
    for c in &e.children {
        match c {
            Node::Elem(c) => write_elem(c, out),
            Node::Text(t) => out.push_str(&escape_text(t)),
        }
    }
}

fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn escape_attr(s: &str) -> String {
    escape_text(s).replace('"', "&quot;")
}

/// The document's text (`XmlDocument.OuterXml`).
pub fn doc_to_string(doc: &Element) -> String {
    let mut s = String::new();
    write_children(doc, &mut s);
    s
}

/// How XML_GET reports a node (its fourth argument).
pub fn node_output(doc: &Element, sel: &Sel, style: i64) -> String {
    match sel {
        Sel::Attr(p, k) => {
            let (n, v) = elem_at(doc, p).and_then(|e| e.attrs.get(*k)).cloned().unwrap_or_default();
            match style {
                2 => escape_attr(&v),
                3 => format!("{}=\"{}\"", n, escape_attr(&v)),
                4 => n,
                _ => v,
            }
        }
        Sel::Node(p) => match node_at(doc, p) {
            Some(NodeRef::Text(t)) => match style {
                2 | 3 => escape_text(t),
                4 => "#text".into(),
                _ => t.to_string(),
            },
            Some(NodeRef::Elem(e)) => match style {
                1 => e.text(),
                2 => {
                    let mut s = String::new();
                    write_children(e, &mut s);
                    s
                }
                3 if p.is_empty() => doc_to_string(doc),
                3 => {
                    let mut s = String::new();
                    write_elem(e, &mut s);
                    s
                }
                4 => e.name.clone(),
                // an element has no Value (null: an empty string)
                _ => String::new(),
            },
            None => String::new(),
        },
    }
}

/// The XPath string value of a node.
fn string_value(doc: &Element, sel: &Sel) -> String {
    match sel {
        Sel::Node(p) => match node_at(doc, p) {
            Some(NodeRef::Elem(e)) => e.text(),
            Some(NodeRef::Text(t)) => t.to_string(),
            None => String::new(),
        },
        Sel::Attr(..) => node_output(doc, sel, 0),
    }
}

/// XML_SET: style 0 the node's value (text / attribute), 1 InnerText,
/// 2 InnerXml. False if the node can't take it.
pub fn set_node(doc: &mut Element, sel: &Sel, val: &str, style: i64) -> Result<bool, String> {
    match sel {
        Sel::Attr(p, k) => match elem_at_mut(doc, p).and_then(|e| e.attrs.get_mut(*k)) {
            Some(a) => {
                a.1 = val.to_string();
                Ok(true)
            }
            None => Ok(false),
        },
        Sel::Node(p) => {
            let Some((&last, parent)) = p.split_last() else { return Ok(false) };
            let Some(par) = elem_at_mut(doc, parent) else { return Ok(false) };
            match par.children.get_mut(last) {
                Some(Node::Text(t)) => {
                    *t = val.to_string();
                    Ok(true)
                }
                Some(Node::Elem(e)) => {
                    match style {
                        1 => e.children = if val.is_empty() { vec![] } else { vec![Node::Text(val.to_string())] },
                        2 => e.children = parse_fragment(val)?,
                        // an element's Value can't be set
                        _ => return Ok(false),
                    }
                    Ok(true)
                }
                None => Ok(false),
            }
        }
    }
}

/// XML_ADDNODE: `method` 0 appends `child` to the node, 1 inserts it
/// before, 2 after.
pub fn insert_node(doc: &mut Element, sel: &Sel, child: Node, method: i64) -> bool {
    let Sel::Node(p) = sel else { return false };
    if method == 0 {
        return match elem_at_mut(doc, p) {
            Some(e) => {
                e.children.push(child);
                true
            }
            None => false,
        };
    }
    // the document node has no parent; nor can it get a second root
    let Some((&last, parent)) = p.split_last() else { return false };
    if parent.is_empty() {
        return false;
    }
    match elem_at_mut(doc, parent) {
        Some(par) => {
            par.children.insert(if method == 1 { last } else { last + 1 }, child);
            true
        }
        None => false,
    }
}

/// XML_ADDATTRIBUTE: `method` 0 adds to the element (replacing one of the
/// same name), 1 / 2 insert before / after the selected attribute.
pub fn insert_attr(doc: &mut Element, sel: &Sel, name: &str, val: &str, method: i64) -> bool {
    let attr = (name.to_string(), val.to_string());
    match (sel, method) {
        (Sel::Node(p), 0) if !p.is_empty() => match elem_at_mut(doc, p) {
            Some(e) => {
                e.attrs.retain(|(k, _)| k != name);
                e.attrs.push(attr);
                true
            }
            None => false,
        },
        (Sel::Attr(p, k), 1 | 2) => match elem_at_mut(doc, p) {
            Some(e) => {
                let mut k = *k + (method == 2) as usize;
                if let Some(old) = e.attrs.iter().position(|(n, _)| n == name) {
                    e.attrs.remove(old);
                    if old < k {
                        k -= 1;
                    }
                }
                e.attrs.insert(k.min(e.attrs.len()), attr);
                true
            }
            None => false,
        },
        _ => false,
    }
}

/// Remove a node or attribute; false for the document itself.
pub fn remove(doc: &mut Element, sel: &Sel, attrs: bool) -> bool {
    match (sel, attrs) {
        (Sel::Attr(p, k), true) => match elem_at_mut(doc, p) {
            Some(e) if *k < e.attrs.len() => {
                e.attrs.remove(*k);
                true
            }
            _ => false,
        },
        (Sel::Node(p), false) => {
            let Some((&last, parent)) = p.split_last() else { return false };
            match elem_at_mut(doc, parent) {
                Some(par) if last < par.children.len() => {
                    par.children.remove(last);
                    true
                }
                _ => false,
            }
        }
        _ => false,
    }
}

/// XML_REPLACE: put `new` where the node is.
pub fn replace_node(doc: &mut Element, sel: &Sel, new: Node) -> bool {
    let Sel::Node(p) = sel else { return false };
    let Some((&last, parent)) = p.split_last() else { return false };
    match elem_at_mut(doc, parent).and_then(|par| par.children.get_mut(last)) {
        Some(slot) => {
            *slot = new;
            true
        }
        None => false,
    }
}

/// Select nodes with an XPath expression, in document order.
pub fn select(doc: &Element, xpath: &str) -> Result<Vec<Sel>, String> {
    let p = xpath.trim();
    if p.is_empty() {
        return Err("empty XPath".into());
    }
    // absolute or not, a document-level select starts at the document node
    let rel = p.strip_prefix('/').unwrap_or(p);
    if rel.is_empty() {
        return Ok(vec![Sel::Node(vec![])]);
    }
    eval_path(doc, vec![Sel::Node(vec![])], rel)
}

/// Split a location path on `/` outside brackets and quotes; `a//b` gives an
/// empty step (descendant-or-self) between `a` and `b`.
fn split_steps(path: &str) -> Result<Vec<&str>, String> {
    let mut out = Vec::new();
    let (mut depth, mut quote, mut start) = (0i32, None, 0);
    for (i, c) in path.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(c),
            (None, '[' | '(') => depth += 1,
            (None, ']' | ')') => depth -= 1,
            (None, '/') if depth == 0 => {
                out.push(&path[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    if quote.is_some() || depth != 0 {
        return Err(format!("unbalanced XPath '{}'", path));
    }
    out.push(&path[start..]);
    Ok(out)
}

fn descendants_or_self(doc: &Element, p: &[usize], out: &mut Vec<Vec<usize>>) {
    out.push(p.to_vec());
    if let Some(e) = elem_at(doc, p) {
        for (i, c) in e.children.iter().enumerate() {
            if matches!(c, Node::Elem(_)) {
                let mut q = p.to_vec();
                q.push(i);
                descendants_or_self(doc, &q, out);
            }
        }
    }
}

fn eval_path(doc: &Element, start: Vec<Sel>, path: &str) -> Result<Vec<Sel>, String> {
    let mut cur = start;
    let mut desc = false;
    for st in split_steps(path)? {
        let st = st.trim();
        if st.is_empty() {
            desc = true;
            continue;
        }
        let mut next = Vec::new();
        for s in &cur {
            let Sel::Node(p) = s else { continue };
            let mut bases = Vec::new();
            if desc {
                descendants_or_self(doc, p, &mut bases);
            } else {
                bases.push(p.clone());
            }
            for b in bases {
                next.extend(step(doc, &b, st)?);
            }
        }
        next.sort();
        next.dedup();
        cur = next;
        desc = false;
    }
    Ok(cur)
}

/// One location step (`name[pred]…`) from the node at `p`.
fn step(doc: &Element, p: &[usize], st: &str) -> Result<Vec<Sel>, String> {
    let (test, preds) = match st.find('[') {
        Some(i) => (st[..i].trim(), &st[i..]),
        None => (st, ""),
    };
    let test = test.strip_prefix("child::").unwrap_or(test);
    let test = test.strip_prefix("attribute::").map(|t| format!("@{}", t));
    let test = test.as_deref().unwrap_or(st.split('[').next().unwrap().trim());
    let test = test.strip_prefix("child::").unwrap_or(test);
    let mut cand: Vec<Sel> = Vec::new();
    let el = elem_at(doc, p);
    match test {
        "." | "self::node()" => cand.push(Sel::Node(p.to_vec())),
        ".." | "parent::node()" => {
            if let Some((_, parent)) = p.split_last() {
                cand.push(Sel::Node(parent.to_vec()));
            }
        }
        _ if test.starts_with('@') => {
            if let Some(e) = el {
                let n = &test[1..];
                for (k, (an, _)) in e.attrs.iter().enumerate() {
                    if n == "*" || an == n {
                        cand.push(Sel::Attr(p.to_vec(), k));
                    }
                }
            }
        }
        _ => {
            if let Some(e) = el {
                for (i, c) in e.children.iter().enumerate() {
                    let ok = match (test, c) {
                        ("node()", _) => true,
                        ("text()", Node::Text(_)) => true,
                        ("*", Node::Elem(_)) => true,
                        (n, Node::Elem(c)) => c.name == n,
                        _ => false,
                    };
                    if ok {
                        let mut q = p.to_vec();
                        q.push(i);
                        cand.push(Sel::Node(q));
                    }
                }
            }
        }
    }
    // predicates, each over the survivors of the previous one
    let mut rest = preds.trim();
    while !rest.is_empty() {
        if !rest.starts_with('[') {
            return Err(format!("bad XPath step '{}'", st));
        }
        let end = matching_bracket(rest).ok_or_else(|| format!("unclosed '[' in '{}'", st))?;
        let expr = &rest[1..end];
        let toks = tokenize(expr)?;
        let size = cand.len();
        let mut kept = Vec::new();
        for (i, c) in cand.into_iter().enumerate() {
            let mut ev = Eval { doc, ctx: &c, pos: i + 1, size, toks: &toks, at: 0 };
            let v = ev.or()?;
            if ev.at != toks.len() {
                return Err(format!("bad XPath predicate '{}'", expr));
            }
            let keep = match v {
                XVal::Num(n) => n == (i + 1) as f64,
                v => v.truthy(),
            };
            if keep {
                kept.push(c);
            }
        }
        cand = kept;
        rest = rest[end + 1..].trim_start();
    }
    Ok(cand)
}

fn matching_bracket(s: &str) -> Option<usize> {
    let (mut depth, mut quote) = (0i32, None);
    for (i, c) in s.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '\'' | '"') => quote = Some(c),
            (None, '[') => depth += 1,
            (None, ']') => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Num(f64),
    Str(String),
    Path(String),
    Func(String),
    Op(&'static str),
    LParen,
    RParen,
    Comma,
}

fn tokenize(s: &str) -> Result<Vec<Tok>, String> {
    let mut out = Vec::new();
    let cs: Vec<char> = s.chars().collect();
    let mut i = 0;
    // after an operand, `-`, `*`, `and`, `or` are operators
    let operand_before = |out: &Vec<Tok>| matches!(out.last(), Some(Tok::Num(_) | Tok::Str(_) | Tok::Path(_) | Tok::RParen));
    while i < cs.len() {
        let c = cs[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let two: String = cs[i..(i + 2).min(cs.len())].iter().collect();
        if let Some(op) = ["!=", "<=", ">="].into_iter().find(|o| *o == two) {
            out.push(Tok::Op(op));
            i += 2;
            continue;
        }
        match c {
            '=' => out.push(Tok::Op("=")),
            '<' => out.push(Tok::Op("<")),
            '>' => out.push(Tok::Op(">")),
            '+' => out.push(Tok::Op("+")),
            '-' if operand_before(&out) => out.push(Tok::Op("-")),
            '-' => out.push(Tok::Op("neg")),
            '(' => out.push(Tok::LParen),
            ')' => out.push(Tok::RParen),
            ',' => out.push(Tok::Comma),
            '\'' | '"' => {
                let end = cs[i + 1..].iter().position(|&x| x == c).ok_or("unterminated XPath string")?;
                out.push(Tok::Str(cs[i + 1..i + 1 + end].iter().collect()));
                i += end + 2;
                continue;
            }
            _ if c.is_ascii_digit() || (c == '.' && cs.get(i + 1).is_some_and(|d| d.is_ascii_digit())) => {
                let st = i;
                while i < cs.len() && (cs[i].is_ascii_digit() || cs[i] == '.') {
                    i += 1;
                }
                let t: String = cs[st..i].iter().collect();
                out.push(Tok::Num(t.parse().map_err(|_| format!("bad number '{}'", t))?));
                continue;
            }
            _ => {
                // a path (`@a`, `b/c`, `text()`, `..`) or a function name
                let st = i;
                let mut depth = 0;
                while i < cs.len() {
                    let x = cs[i];
                    if x == '[' {
                        depth += 1;
                    } else if x == ']' {
                        depth -= 1;
                    } else if depth == 0 && (x.is_whitespace() || "=!<>(),+'\"".contains(x)) {
                        break;
                    }
                    i += 1;
                }
                let mut t: String = cs[st..i].iter().collect();
                let last_seg = t.rsplit('/').next().unwrap_or("").to_string();
                if cs.get(i) == Some(&'(') {
                    if matches!(last_seg.as_str(), "text" | "node") && cs.get(i + 1) == Some(&')') {
                        t.push_str("()");
                        i += 2;
                        // the path may go on (`text()[1]` is rare; stop here)
                        out.push(Tok::Path(t));
                        continue;
                    }
                    out.push(Tok::Func(t));
                    continue;
                }
                if operand_before(&out) && (t == "and" || t == "or" || t == "div" || t == "mod") {
                    out.push(Tok::Op(match t.as_str() {
                        "and" => "and",
                        "or" => "or",
                        "div" => "div",
                        _ => "mod",
                    }));
                } else if operand_before(&out) && t == "*" {
                    out.push(Tok::Op("*"));
                } else if t.is_empty() {
                    return Err(format!("unexpected '{}' in XPath", c));
                } else {
                    out.push(Tok::Path(t));
                }
                continue;
            }
        }
        i += 1;
    }
    Ok(out)
}

#[derive(Debug, Clone)]
enum XVal {
    Num(f64),
    Str(String),
    Bool(bool),
    Set(Vec<String>),
}

impl XVal {
    fn truthy(&self) -> bool {
        match self {
            XVal::Num(n) => *n != 0.0 && !n.is_nan(),
            XVal::Str(s) => !s.is_empty(),
            XVal::Bool(b) => *b,
            XVal::Set(v) => !v.is_empty(),
        }
    }
    fn num(&self) -> f64 {
        match self {
            XVal::Num(n) => *n,
            XVal::Bool(b) => *b as i32 as f64,
            XVal::Str(s) => s.trim().parse().unwrap_or(f64::NAN),
            XVal::Set(v) => v.first().and_then(|s| s.trim().parse().ok()).unwrap_or(f64::NAN),
        }
    }
    fn string(&self) -> String {
        match self {
            XVal::Num(n) if n.fract() == 0.0 && n.is_finite() => format!("{}", *n as i64),
            XVal::Num(n) => n.to_string(),
            XVal::Str(s) => s.clone(),
            XVal::Bool(b) => b.to_string(),
            XVal::Set(v) => v.first().cloned().unwrap_or_default(),
        }
    }
}

fn compare(op: &str, a: &XVal, b: &XVal) -> bool {
    // node sets compare true if any member does
    match (a, b) {
        (XVal::Set(xs), XVal::Set(ys)) => return xs.iter().any(|x| ys.iter().any(|y| compare(op, &XVal::Str(x.clone()), &XVal::Str(y.clone())))),
        (XVal::Set(xs), XVal::Bool(_)) | (XVal::Bool(_), XVal::Set(xs)) => {
            let s = XVal::Bool(!xs.is_empty());
            return if matches!(a, XVal::Set(_)) { compare(op, &s, b) } else { compare(op, a, &s) };
        }
        (XVal::Set(xs), o) => return xs.iter().any(|x| compare(op, &XVal::Str(x.clone()), o)),
        (o, XVal::Set(ys)) => return ys.iter().any(|y| compare(op, o, &XVal::Str(y.clone()))),
        _ => {}
    }
    if op == "=" || op == "!=" {
        let eq = match (a, b) {
            (XVal::Bool(_), _) | (_, XVal::Bool(_)) => a.truthy() == b.truthy(),
            (XVal::Num(_), _) | (_, XVal::Num(_)) => a.num() == b.num(),
            _ => a.string() == b.string(),
        };
        return eq == (op == "=");
    }
    let (x, y) = (a.num(), b.num());
    match op {
        "<" => x < y,
        "<=" => x <= y,
        ">" => x > y,
        _ => x >= y,
    }
}

struct Eval<'a> {
    doc: &'a Element,
    ctx: &'a Sel,
    pos: usize,
    size: usize,
    toks: &'a [Tok],
    at: usize,
}

impl Eval<'_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.at)
    }
    fn eat_op(&mut self, ops: &[&str]) -> Option<&'static str> {
        match self.peek() {
            Some(Tok::Op(o)) if ops.contains(o) => {
                let o = *o;
                self.at += 1;
                Some(o)
            }
            _ => None,
        }
    }
    fn or(&mut self) -> Result<XVal, String> {
        let mut v = self.and()?;
        while self.eat_op(&["or"]).is_some() {
            let r = self.and()?;
            v = XVal::Bool(v.truthy() || r.truthy());
        }
        Ok(v)
    }
    fn and(&mut self) -> Result<XVal, String> {
        let mut v = self.cmp()?;
        while self.eat_op(&["and"]).is_some() {
            let r = self.cmp()?;
            v = XVal::Bool(v.truthy() && r.truthy());
        }
        Ok(v)
    }
    fn cmp(&mut self) -> Result<XVal, String> {
        let mut v = self.add()?;
        while let Some(op) = self.eat_op(&["=", "!=", "<", "<=", ">", ">="]) {
            let r = self.add()?;
            v = XVal::Bool(compare(op, &v, &r));
        }
        Ok(v)
    }
    fn add(&mut self) -> Result<XVal, String> {
        let mut v = self.mul()?;
        while let Some(op) = self.eat_op(&["+", "-"]) {
            let r = self.mul()?;
            v = XVal::Num(if op == "+" { v.num() + r.num() } else { v.num() - r.num() });
        }
        Ok(v)
    }
    fn mul(&mut self) -> Result<XVal, String> {
        let mut v = self.unary()?;
        while let Some(op) = self.eat_op(&["*", "div", "mod"]) {
            let r = self.unary()?;
            v = XVal::Num(match op {
                "*" => v.num() * r.num(),
                "div" => v.num() / r.num(),
                _ => v.num() % r.num(),
            });
        }
        Ok(v)
    }
    fn unary(&mut self) -> Result<XVal, String> {
        if self.eat_op(&["neg"]).is_some() {
            return Ok(XVal::Num(-self.unary()?.num()));
        }
        self.primary()
    }
    fn primary(&mut self) -> Result<XVal, String> {
        let t = self.peek().cloned().ok_or("XPath predicate ends early")?;
        self.at += 1;
        match t {
            Tok::Num(n) => Ok(XVal::Num(n)),
            Tok::Str(s) => Ok(XVal::Str(s)),
            Tok::LParen => {
                let v = self.or()?;
                match self.peek() {
                    Some(Tok::RParen) => {
                        self.at += 1;
                        Ok(v)
                    }
                    _ => Err("missing ')' in XPath".into()),
                }
            }
            Tok::Path(p) => {
                let sels = if let Some(abs) = p.strip_prefix('/') {
                    eval_path(self.doc, vec![Sel::Node(vec![])], abs)?
                } else {
                    eval_path(self.doc, vec![self.ctx.clone()], &p)?
                };
                Ok(XVal::Set(sels.iter().map(|s| string_value(self.doc, s)).collect()))
            }
            Tok::Func(f) => {
                // '(' args ')'
                if !matches!(self.peek(), Some(Tok::LParen)) {
                    return Err(format!("expected '(' after {}", f));
                }
                self.at += 1;
                let mut args = Vec::new();
                if matches!(self.peek(), Some(Tok::RParen)) {
                    self.at += 1;
                } else {
                    loop {
                        args.push(self.or()?);
                        match self.peek() {
                            Some(Tok::Comma) => self.at += 1,
                            Some(Tok::RParen) => {
                                self.at += 1;
                                break;
                            }
                            _ => return Err(format!("bad arguments to {}()", f)),
                        }
                    }
                }
                let s = |k: usize| args.get(k).map(|v| v.string()).unwrap_or_default();
                let ctx_str = || string_value(self.doc, self.ctx);
                Ok(match f.as_str() {
                    "last" => XVal::Num(self.size as f64),
                    "position" => XVal::Num(self.pos as f64),
                    "count" => XVal::Num(match args.first() {
                        Some(XVal::Set(v)) => v.len() as f64,
                        _ => 0.0,
                    }),
                    "not" => XVal::Bool(!args.first().is_some_and(|v| v.truthy())),
                    "true" => XVal::Bool(true),
                    "false" => XVal::Bool(false),
                    "boolean" => XVal::Bool(args.first().is_some_and(|v| v.truthy())),
                    "contains" => XVal::Bool(s(0).contains(&s(1))),
                    "starts-with" => XVal::Bool(s(0).starts_with(&s(1))),
                    "ends-with" => XVal::Bool(s(0).ends_with(&s(1))),
                    "string-length" => XVal::Num(if args.is_empty() { ctx_str() } else { s(0) }.chars().count() as f64),
                    "normalize-space" => {
                        let t = if args.is_empty() { ctx_str() } else { s(0) };
                        XVal::Str(t.split_whitespace().collect::<Vec<_>>().join(" "))
                    }
                    "string" => XVal::Str(if args.is_empty() { ctx_str() } else { s(0) }),
                    "number" => XVal::Num(args.first().map_or_else(|| XVal::Str(ctx_str()).num(), |v| v.num())),
                    "concat" => XVal::Str(args.iter().map(|v| v.string()).collect()),
                    "name" | "local-name" => XVal::Str(match self.ctx {
                        Sel::Node(p) => match node_at(self.doc, p) {
                            Some(NodeRef::Elem(e)) => e.name.clone(),
                            _ => String::new(),
                        },
                        a @ Sel::Attr(..) => node_output(self.doc, a, 4),
                    }),
                    "floor" => XVal::Num(args.first().map_or(f64::NAN, |v| v.num()).floor()),
                    "ceiling" => XVal::Num(args.first().map_or(f64::NAN, |v| v.num()).ceil()),
                    "round" => XVal::Num(args.first().map_or(f64::NAN, |v| v.num()).round()),
                    _ => return Err(format!("unsupported XPath function {}()", f)),
                })
            }
            t => Err(format!("unexpected {:?} in XPath", t)),
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

    #[test]
    fn xpath_select_edit_serialize() {
        let mut d = load_doc("<root>\n  <e o='a' f='x'>s1<g>5</g></e>\n  <e o='b' f='y'>s2<g>6</g></e>\n</root>").unwrap();
        let get = |d: &Element, p: &str| select(d, p).unwrap().iter().map(|s| node_output(d, s, 0)).collect::<Vec<_>>();
        assert_eq!(get(&d, "/root/e[@o='b'][@f='y']/text()"), vec!["s2"]);
        assert_eq!(get(&d, "/root/e[@o='b' and @f='x']/text()"), Vec::<String>::new());
        assert_eq!(get(&d, "/root/*[1]/g/text()"), vec!["5"]);
        assert_eq!(get(&d, "/root/e[last()]/g/text()"), vec!["6"]);
        assert_eq!(get(&d, "//g[. > 5]/text()"), vec!["6"]);
        assert_eq!(get(&d, "/root/e/@o"), vec!["a", "b"]);
        assert_eq!(select(&d, "/root/*").unwrap().len(), 2);
        assert_eq!(get(&d, "/root/e[g=6]/@f"), vec!["y"]);
        assert_eq!(get(&d, "/root/e[contains(@f,'x') or position()=2]/@o"), vec!["a", "b"]);
        // add, then remove the first element
        let r = select(&d, "/root").unwrap();
        let n = parse_fragment("<e o='c'>s3</e>").unwrap().remove(0);
        assert!(insert_node(&mut d, &r[0], n, 0));
        let last = select(&d, "/root/e[last()]").unwrap();
        assert!(insert_node(&mut d, &last[0], parse_fragment("<g>7</g>").unwrap().remove(0), 0));
        let first = select(&d, "/root/*[1]").unwrap();
        assert!(remove(&mut d, &first[0], false));
        assert_eq!(
            doc_to_string(&d),
            r#"<root><e o="b" f="y">s2<g>6</g></e><e o="c">s3<g>7</g></e></root>"#
        );
        let t = select(&d, "/root/e[@o='c']/text()").unwrap();
        assert!(set_node(&mut d, &t[0], "z&", 0).unwrap());
        assert!(doc_to_string(&d).contains(">z&amp;<g>"));
        assert_eq!(doc_to_string(&load_doc("<a />").unwrap()), "<a />");
    }
}
