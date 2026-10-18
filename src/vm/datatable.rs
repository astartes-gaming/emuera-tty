//! Emuera-EM `DT_*` data tables: a small stand-in for the ADO.NET DataTable
//! that emuera.em wraps. Column 0 is always the int64 primary key `id`.
//! Covers what scripts use: typed columns with defaults, row add/set/remove,
//! `Select(filter, sort)` with the common DataColumn expression subset, and
//! the DataTable XML schema / data format (WriteXmlSchema / WriteXml).

use super::value::Value;
use crate::xml::{self, Element};
use std::cell::Cell;
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColType {
    I8,
    I16,
    I32,
    I64,
    Str,
}

impl ColType {
    /// DT_COLUMN_ADD type names
    pub fn from_name(n: &str) -> Option<ColType> {
        Some(match n {
            "int8" => ColType::I8,
            "int16" => ColType::I16,
            "int32" => ColType::I32,
            "int64" => ColType::I64,
            "string" => ColType::Str,
            _ => return None,
        })
    }
    /// DT_COLUMN_ADD type codes / DT_COLUMN_EXIST result: 1..=5
    pub fn from_int(i: i64) -> Option<ColType> {
        [ColType::I8, ColType::I16, ColType::I32, ColType::I64, ColType::Str].get((i - 1) as usize).copied().filter(|_| i >= 1)
    }
    pub fn to_int(self) -> i64 {
        self as i64 + 1
    }
    fn xsd(self) -> &'static str {
        match self {
            ColType::I8 => "xs:byte",
            ColType::I16 => "xs:short",
            ColType::I32 => "xs:int",
            ColType::I64 => "xs:long",
            ColType::Str => "xs:string",
        }
    }
    fn from_xsd(t: &str) -> ColType {
        match t.rsplit(':').next().unwrap_or(t) {
            "byte" => ColType::I8,
            "short" => ColType::I16,
            "int" => ColType::I32,
            "long" => ColType::I64,
            _ => ColType::Str,
        }
    }
    pub fn is_str(self) -> bool {
        self == ColType::Str
    }
    /// Utils.DataTable.ConvertInt: saturate into the column's range.
    pub fn clamp(self, v: i64) -> i64 {
        match self {
            ColType::I8 => v.clamp(i8::MIN as i64, i8::MAX as i64),
            ColType::I16 => v.clamp(i16::MIN as i64, i16::MAX as i64),
            ColType::I32 => v.clamp(i32::MIN as i64, i32::MAX as i64),
            _ => v,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Column {
    pub name: String,
    pub ty: ColType,
    pub nullable: bool,
    pub default: Option<Value>,
}

pub type Row = Vec<Option<Value>>;

#[derive(Clone, Debug)]
pub struct DataTable {
    pub name: String,
    pub columns: Vec<Column>,
    pub rows: Vec<Row>,
    pub case_sensitive: bool,
}

thread_local! {
    static LAST_ID: Cell<i64> = const { Cell::new(0) };
}

/// Utils.TimePoint(): a strictly increasing tick count used for row ids.
pub fn next_id() -> i64 {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| (d.as_nanos() / 100) as i64)
        .unwrap_or(0);
    LAST_ID.with(|l| {
        let id = now.max(l.get() + 1);
        l.set(id);
        id
    })
}

impl DataTable {
    pub fn new(name: &str) -> DataTable {
        DataTable {
            name: name.to_string(),
            columns: vec![Column { name: "id".into(), ty: ColType::I64, nullable: false, default: None }],
            rows: Vec::new(),
            case_sensitive: true,
        }
    }

    /// Column lookup is case-insensitive, like DataColumnCollection.
    pub fn col(&self, name: &str) -> Option<usize> {
        self.columns.iter().position(|c| c.name == name).or_else(|| self.columns.iter().position(|c| c.name.eq_ignore_ascii_case(name)))
    }

    pub fn add_column(&mut self, name: &str, ty: ColType, nullable: bool) {
        self.columns.push(Column { name: name.to_string(), ty, nullable, default: None });
        for r in &mut self.rows {
            r.push(None);
        }
    }

    pub fn remove_column(&mut self, i: usize) {
        self.columns.remove(i);
        for r in &mut self.rows {
            r.remove(i);
        }
    }

    pub fn find_row(&self, id: i64) -> Option<usize> {
        self.rows.iter().position(|r| matches!(r[0], Some(Value::Int(x)) if x == id))
    }

    pub fn row_id(&self, r: usize) -> i64 {
        self.rows[r][0].as_ref().map(|v| v.as_int()).unwrap_or(0)
    }

    /// A fresh row with column defaults and a new id (not yet added).
    pub fn new_row(&self) -> Row {
        let mut r: Row = self.columns.iter().map(|c| c.default.clone()).collect();
        r[0] = Some(Value::Int(next_id()));
        r
    }

    /// Store `v` (None = DBNull) into a cell, checking the column type.
    pub fn check_value(&self, c: usize, v: Option<Value>) -> Result<Option<Value>, ()> {
        let col = &self.columns[c];
        match v {
            None => Ok(None),
            Some(Value::Str(s)) if col.ty.is_str() => Ok(Some(Value::Str(s))),
            Some(Value::Int(i)) if !col.ty.is_str() => Ok(Some(Value::Int(col.ty.clamp(i)))),
            _ => Err(()),
        }
    }

    // ------------------------------------------------------------- Select

    /// DataTable.Select(filter, sort): ids of matching rows.
    pub fn select(&self, filter: Option<&str>, sort: Option<&str>) -> Result<Vec<i64>, String> {
        let pred = match filter.map(str::trim).filter(|f| !f.is_empty()) {
            Some(f) => Some(FilterParser::new(f, self).parse()?),
            None => None,
        };
        let mut hits: Vec<usize> = Vec::new();
        for (i, r) in self.rows.iter().enumerate() {
            let keep = match &pred {
                Some(p) => self.eval(p, r).truthy(),
                None => true,
            };
            if keep {
                hits.push(i);
            }
        }
        if let Some(sort) = sort.map(str::trim).filter(|s| !s.is_empty()) {
            let mut keys = Vec::new();
            for part in sort.split(',') {
                let mut w = part.split_whitespace();
                let Some(name) = w.next() else { continue };
                let name = name.trim_start_matches('[').trim_end_matches(']');
                let desc = matches!(w.next().map(|s| s.to_ascii_uppercase()).as_deref(), Some("DESC"));
                let c = self.col(name).ok_or_else(|| format!("unknown sort column '{}'", name))?;
                keys.push((c, desc));
            }
            hits.sort_by(|&a, &b| {
                for &(c, desc) in &keys {
                    let o = self.cmp_cells(&self.rows[a][c], &self.rows[b][c]);
                    let o = if desc { o.reverse() } else { o };
                    if o != Ordering::Equal {
                        return o;
                    }
                }
                Ordering::Equal
            });
        }
        Ok(hits.into_iter().map(|i| self.row_id(i)).collect())
    }

    /// Null sorts first; numbers numerically; strings ordinally.
    fn cmp_cells(&self, a: &Option<Value>, b: &Option<Value>) -> Ordering {
        match (a, b) {
            (None, None) => Ordering::Equal,
            (None, _) => Ordering::Less,
            (_, None) => Ordering::Greater,
            (Some(a), Some(b)) => self.cmp_vals(a, b),
        }
    }

    fn cmp_vals(&self, a: &Value, b: &Value) -> Ordering {
        self.cmp_v(V::of(a), V::of(b))
    }

    /// Compare two values (case-insensitively unless the table says
    /// otherwise), without allocating.
    fn cmp_v(&self, a: V, b: V) -> Ordering {
        match (a, b) {
            (V::Int(x), V::Int(y)) => x.cmp(&y),
            (V::Str(x), V::Str(y)) => {
                if self.case_sensitive {
                    x.cmp(y)
                } else if x.is_ascii() && y.is_ascii() {
                    x.bytes().map(|c| c.to_ascii_lowercase()).cmp(y.bytes().map(|c| c.to_ascii_lowercase()))
                } else {
                    x.chars().flat_map(char::to_lowercase).cmp(y.chars().flat_map(char::to_lowercase))
                }
            }
            // mixed: compare as numbers when the string is numeric
            (V::Int(x), V::Str(s)) => s.trim().parse::<i64>().map(|y| x.cmp(&y)).unwrap_or_else(|_| x.to_string().as_str().cmp(s)),
            (V::Str(_), V::Int(_)) => self.cmp_v(b, a).reverse(),
            // (nulls are handled by the callers)
            (V::Null, V::Null) => Ordering::Equal,
            (V::Null, _) => Ordering::Less,
            (_, V::Null) => Ordering::Greater,
        }
    }

    /// A filter's value for row `r`, borrowing cells and literals.
    fn eval<'a>(&'a self, f: &'a Filter, r: &'a Row) -> V<'a> {
        let b = |x: bool| V::Int(x as i64);
        match f {
            Filter::Lit(v) => v.as_ref().map_or(V::Null, V::of),
            Filter::Col(c) => r[*c].as_ref().map_or(V::Null, V::of),
            Filter::Not(x) => b(!self.eval(x, r).truthy()),
            Filter::And(x, y) => b(self.eval(x, r).truthy() && self.eval(y, r).truthy()),
            Filter::Or(x, y) => b(self.eval(x, r).truthy() || self.eval(y, r).truthy()),
            Filter::Cmp(op, x, y) => {
                let (x, y) = (self.eval(x, r), self.eval(y, r));
                if matches!(x, V::Null) || matches!(y, V::Null) {
                    return b(false);
                }
                let o = self.cmp_v(x, y);
                b(match op {
                    CmpOp::Eq => o == Ordering::Equal,
                    CmpOp::Ne => o != Ordering::Equal,
                    CmpOp::Lt => o == Ordering::Less,
                    CmpOp::Le => o != Ordering::Greater,
                    CmpOp::Gt => o == Ordering::Greater,
                    CmpOp::Ge => o != Ordering::Less,
                })
            }
            Filter::Like(x, pat, neg) => {
                let v = self.eval(x, r);
                if matches!(v, V::Null) {
                    return b(false);
                }
                let s = v.to_string();
                let (s, p) = if self.case_sensitive { (s, pat.clone()) } else { (s.to_lowercase(), pat.to_lowercase()) };
                b(like(&s, &p) != *neg)
            }
            Filter::In(x, list, neg) => {
                let v = self.eval(x, r);
                if matches!(v, V::Null) {
                    return b(false);
                }
                let hit = list.iter().any(|e| {
                    let w = self.eval(e, r);
                    !matches!(w, V::Null) && self.cmp_v(v, w) == Ordering::Equal
                });
                b(hit != *neg)
            }
            Filter::IsNull(x, neg) => b(matches!(self.eval(x, r), V::Null) != *neg),
        }
    }

    // ---------------------------------------------------------------- XML

    /// WriteXmlSchema equivalent, byte for byte as .NET writes it (checked
    /// against a real Emuera save): CRLF line ends, the table's own
    /// CaseSensitive flag.
    pub fn to_xml_schema(&self) -> String {
        let n = xml::escape(&self.name);
        let cs = if self.case_sensitive { "True" } else { "False" };
        let mut s = String::from("<?xml version=\"1.0\" encoding=\"utf-16\"?>\n");
        s += "<xs:schema id=\"NewDataSet\" xmlns=\"\" xmlns:xs=\"http://www.w3.org/2001/XMLSchema\" xmlns:msdata=\"urn:schemas-microsoft-com:xml-msdata\">\n";
        s += &format!(
            "  <xs:element name=\"NewDataSet\" msdata:IsDataSet=\"true\" msdata:MainDataTable=\"{}\" msdata:CaseSensitive=\"{}\" msdata:UseCurrentLocale=\"true\">\n",
            n, self.case_sensitive
        );
        s += "    <xs:complexType>\n      <xs:choice minOccurs=\"0\" maxOccurs=\"unbounded\">\n";
        s += &format!("        <xs:element name=\"{}\" msdata:CaseSensitive=\"{}\">\n          <xs:complexType>\n            <xs:sequence>\n", n, cs);
        for c in &self.columns {
            let mut e = format!("              <xs:element name=\"{}\" type=\"{}\"", xml::escape(&c.name), c.ty.xsd());
            if let Some(d) = &c.default {
                e += &format!(" default=\"{}\"", xml::escape(&d.as_string()));
            }
            if c.nullable {
                e += " minOccurs=\"0\"";
            }
            s += &e;
            s += " />\n";
        }
        s += "            </xs:sequence>\n          </xs:complexType>\n        </xs:element>\n";
        s += "      </xs:choice>\n    </xs:complexType>\n";
        s += &format!(
            "    <xs:unique name=\"Constraint1\" msdata:PrimaryKey=\"true\">\n      <xs:selector xpath=\".//{}\" />\n      <xs:field xpath=\"id\" />\n    </xs:unique>\n",
            n
        );
        s += "  </xs:element>\n</xs:schema>";
        s.replace('\n', "\r\n")
    }

    /// WriteXml equivalent (null cells are omitted; no XML declaration and
    /// CRLF line ends, as .NET writes it to a string).
    pub fn to_xml(&self) -> String {
        let n = xml::escape(&self.name);
        let mut s = String::from("<DocumentElement>\r\n");
        for r in &self.rows {
            s += &format!("  <{}>\r\n", n);
            for (c, v) in self.columns.iter().zip(r) {
                if let Some(v) = v {
                    let cn = xml::escape(&c.name);
                    s += &format!("    <{}>{}</{}>\r\n", cn, xml::escape(&v.as_string()), cn);
                }
            }
            s += &format!("  </{}>\r\n", n);
        }
        s += "</DocumentElement>";
        s
    }

    /// ReadXmlSchema + ReadXml into a new table called `key`.
    pub fn from_xml(key: &str, schema: &str, data: &str) -> Result<DataTable, String> {
        let sch = xml::parse(schema)?;
        // the table is an xs:element whose complexType holds a sequence of columns
        let mut els = Vec::new();
        sch.find_all("element", &mut els);
        let is_table = |e: &&Element| e.elems().any(|ct| ct.local() == "complexType" && ct.elems().any(|q| q.local() == "sequence"));
        let tables: Vec<&Element> = els.into_iter().filter(is_table).collect();
        let tbl = tables
            .iter()
            .find(|e| e.attr("name") == Some(key))
            .or_else(|| tables.iter().find(|e| e.attr("msdata:IsDataSet") != Some("true")))
            .ok_or("schema has no table")?;
        let tname = tbl.attr("name").unwrap_or(key).to_string();
        let mut dt = DataTable { name: key.to_string(), columns: Vec::new(), rows: Vec::new(), case_sensitive: true };
        if let Some(cs) = tbl.attr("msdata:CaseSensitive") {
            dt.case_sensitive = cs.eq_ignore_ascii_case("true");
        }
        let seq = tbl.elems().find(|e| e.local() == "complexType").and_then(|ct| ct.elems().find(|e| e.local() == "sequence")).ok_or("schema table has no columns")?;
        for c in seq.elems().filter(|e| e.local() == "element") {
            let name = c.attr("name").ok_or("schema column without a name")?.to_string();
            let ty = ColType::from_xsd(c.attr("type").unwrap_or("xs:string"));
            let nullable = c.attr("minOccurs") == Some("0");
            let default = c.attr("default").map(|d| if ty.is_str() { Value::Str(d.to_string()) } else { Value::Int(d.trim().parse().unwrap_or(0)) });
            dt.columns.push(Column { name, ty, nullable, default });
        }
        match dt.col("id") {
            Some(0) => {}
            Some(i) => {
                let c = dt.columns.remove(i);
                dt.columns.insert(0, c);
            }
            None => dt.columns.insert(0, Column { name: "id".into(), ty: ColType::I64, nullable: false, default: None }),
        }
        if data.trim().is_empty() {
            return Ok(dt);
        }
        let doc = xml::parse(data)?;
        for re in doc.elems().filter(|e| e.local() == tname) {
            let mut row: Row = dt.columns.iter().map(|c| c.default.clone()).collect();
            for ce in re.elems() {
                let Some(c) = dt.col(ce.local()) else { continue };
                let t = ce.text();
                row[c] = Some(if dt.columns[c].ty.is_str() { Value::Str(t) } else { Value::Int(dt.columns[c].ty.clamp(t.trim().parse().unwrap_or(0))) });
            }
            if row[0].is_none() {
                row[0] = Some(Value::Int(next_id()));
            }
            dt.rows.push(row);
        }
        Ok(dt)
    }
}

/// A value seen by a filter: borrowed from the table or the filter.
#[derive(Clone, Copy)]
enum V<'a> {
    Null,
    Int(i64),
    Str(&'a str),
}

impl<'a> V<'a> {
    fn of(v: &'a Value) -> V<'a> {
        match v {
            Value::Int(i) => V::Int(*i),
            Value::Str(s) => V::Str(s),
        }
    }
    fn truthy(self) -> bool {
        match self {
            V::Null => false,
            V::Int(i) => i != 0,
            V::Str(s) => !s.is_empty(),
        }
    }
    fn to_string(self) -> String {
        match self {
            V::Null => String::new(),
            V::Int(i) => i.to_string(),
            V::Str(s) => s.to_string(),
        }
    }
}

/// LIKE with `%` / `*` (any run) and `_` (one char) wildcards.
fn like(s: &str, p: &str) -> bool {
    let s: Vec<char> = s.chars().collect();
    let p: Vec<char> = p.chars().collect();
    fn go(s: &[char], p: &[char]) -> bool {
        match p.first() {
            None => s.is_empty(),
            Some('%' | '*') => (0..=s.len()).any(|k| go(&s[k..], &p[1..])),
            Some('_') => !s.is_empty() && go(&s[1..], &p[1..]),
            Some(c) => s.first() == Some(c) && go(&s[1..], &p[1..]),
        }
    }
    go(&s, &p)
}

// ------------------------------------------------------------ filter parser

#[derive(Debug, Clone, Copy)]
enum CmpOp {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

#[derive(Debug)]
enum Filter {
    Lit(Option<Value>),
    Col(usize),
    Not(Box<Filter>),
    And(Box<Filter>, Box<Filter>),
    Or(Box<Filter>, Box<Filter>),
    Cmp(CmpOp, Box<Filter>, Box<Filter>),
    Like(Box<Filter>, String, bool),
    In(Box<Filter>, Vec<Filter>, bool),
    IsNull(Box<Filter>, bool),
}

struct FilterParser<'a> {
    s: Vec<char>,
    pos: usize,
    dt: &'a DataTable,
}

impl<'a> FilterParser<'a> {
    fn new(src: &str, dt: &'a DataTable) -> Self {
        FilterParser { s: src.chars().collect(), pos: 0, dt }
    }
    fn err<T>(&self, m: &str) -> Result<T, String> {
        Err(format!("DT filter: {} near '{}'", m, self.s[self.pos.min(self.s.len())..].iter().collect::<String>()))
    }
    fn ws(&mut self) {
        while self.s.get(self.pos).is_some_and(|c| c.is_whitespace()) {
            self.pos += 1;
        }
    }
    /// Case-insensitive keyword followed by a non-word char.
    fn kw(&mut self, k: &str) -> bool {
        self.ws();
        let n = k.chars().count();
        let w: String = self.s[self.pos..(self.pos + n).min(self.s.len())].iter().collect();
        let after = self.s.get(self.pos + n).copied();
        if w.eq_ignore_ascii_case(k) && !after.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            self.pos += n;
            true
        } else {
            false
        }
    }
    fn sym(&mut self, t: &str) -> bool {
        self.ws();
        let n = t.chars().count();
        if self.s[self.pos..].iter().take(n).copied().eq(t.chars()) {
            self.pos += n;
            true
        } else {
            false
        }
    }
    fn parse(mut self) -> Result<Filter, String> {
        let f = self.or()?;
        self.ws();
        if self.pos < self.s.len() {
            return self.err("unexpected text");
        }
        Ok(f)
    }
    fn or(&mut self) -> Result<Filter, String> {
        let mut l = self.and()?;
        while self.kw("OR") {
            l = Filter::Or(Box::new(l), Box::new(self.and()?));
        }
        Ok(l)
    }
    fn and(&mut self) -> Result<Filter, String> {
        let mut l = self.not()?;
        while self.kw("AND") {
            l = Filter::And(Box::new(l), Box::new(self.not()?));
        }
        Ok(l)
    }
    fn not(&mut self) -> Result<Filter, String> {
        if self.kw("NOT") {
            return Ok(Filter::Not(Box::new(self.not()?)));
        }
        self.cmp()
    }
    fn cmp(&mut self) -> Result<Filter, String> {
        let l = self.primary()?;
        let ops = [("<>", CmpOp::Ne), ("!=", CmpOp::Ne), ("<=", CmpOp::Le), (">=", CmpOp::Ge), ("==", CmpOp::Eq), ("=", CmpOp::Eq), ("<", CmpOp::Lt), (">", CmpOp::Gt)];
        for (t, op) in ops {
            if self.sym(t) {
                return Ok(Filter::Cmp(op, Box::new(l), Box::new(self.primary()?)));
            }
        }
        let save = self.pos;
        let neg = self.kw("NOT");
        if self.kw("LIKE") {
            let Filter::Lit(Some(Value::Str(p))) = self.primary()? else { return self.err("LIKE needs a string") };
            return Ok(Filter::Like(Box::new(l), p, neg));
        }
        if self.kw("IN") {
            if !self.sym("(") {
                return self.err("expected '(' after IN");
            }
            let mut list = vec![self.primary()?];
            while self.sym(",") {
                list.push(self.primary()?);
            }
            if !self.sym(")") {
                return self.err("expected ')'");
            }
            return Ok(Filter::In(Box::new(l), list, neg));
        }
        self.pos = save;
        if self.kw("IS") {
            let neg = self.kw("NOT");
            if !self.kw("NULL") {
                return self.err("expected NULL");
            }
            return Ok(Filter::IsNull(Box::new(l), neg));
        }
        Ok(l)
    }
    fn primary(&mut self) -> Result<Filter, String> {
        self.ws();
        let Some(&c) = self.s.get(self.pos) else { return self.err("unexpected end") };
        if c == '(' {
            self.pos += 1;
            let e = self.or()?;
            if !self.sym(")") {
                return self.err("expected ')'");
            }
            return Ok(e);
        }
        if c == '\'' {
            // 'text' with '' as an escaped quote
            self.pos += 1;
            let mut out = String::new();
            loop {
                match self.s.get(self.pos) {
                    None => return self.err("unterminated string"),
                    Some('\'') if self.s.get(self.pos + 1) == Some(&'\'') => {
                        out.push('\'');
                        self.pos += 2;
                    }
                    Some('\'') => {
                        self.pos += 1;
                        break;
                    }
                    Some(&ch) => {
                        out.push(ch);
                        self.pos += 1;
                    }
                }
            }
            return Ok(Filter::Lit(Some(Value::Str(out))));
        }
        if c.is_ascii_digit() || (c == '-' && self.s.get(self.pos + 1).is_some_and(|d| d.is_ascii_digit())) {
            let st = self.pos;
            self.pos += 1;
            while self.s.get(self.pos).is_some_and(|d| d.is_ascii_digit()) {
                self.pos += 1;
            }
            let t: String = self.s[st..self.pos].iter().collect();
            return Ok(Filter::Lit(Some(Value::Int(t.parse().map_err(|_| format!("DT filter: bad number {}", t))?))));
        }
        if self.kw("NULL") {
            return Ok(Filter::Lit(None));
        }
        if self.kw("TRUE") {
            return Ok(Filter::Lit(Some(Value::Int(1))));
        }
        if self.kw("FALSE") {
            return Ok(Filter::Lit(Some(Value::Int(0))));
        }
        // column: [any name] or a bare word
        let name: String = if c == '[' {
            let Some(end) = self.s[self.pos..].iter().position(|&x| x == ']') else { return self.err("unterminated [") };
            let n = self.s[self.pos + 1..self.pos + end].iter().collect();
            self.pos += end + 1;
            n
        } else {
            let st = self.pos;
            while self.s.get(self.pos).is_some_and(|&x| !x.is_whitespace() && !"=<>!(),'".contains(x)) {
                self.pos += 1;
            }
            self.s[st..self.pos].iter().collect()
        };
        if name.is_empty() {
            return self.err("expected a value");
        }
        match self.dt.col(&name) {
            Some(i) => Ok(Filter::Col(i)),
            None => Err(format!("DT filter: no column '{}' in {}", name, self.dt.name)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> DataTable {
        let mut dt = DataTable::new("t");
        dt.add_column("name", ColType::Str, true);
        dt.add_column("cost", ColType::I16, true);
        for (n, c) in [("a", 3), ("b", 1), ("c", 2), ("it's", 5)] {
            let mut r = dt.new_row();
            r[1] = Some(Value::Str(n.into()));
            r[2] = Some(Value::Int(c));
            dt.rows.push(r);
        }
        dt
    }

    fn names(dt: &DataTable, ids: &[i64]) -> Vec<String> {
        ids.iter().map(|&id| dt.rows[dt.find_row(id).unwrap()][1].clone().unwrap().as_string()).collect()
    }

    #[test]
    fn select_filter_and_sort() {
        let dt = table();
        assert_eq!(names(&dt, &dt.select(Some("name = 'b'"), None).unwrap()), ["b"]);
        assert_eq!(names(&dt, &dt.select(Some("cost >= 2 AND NOT name = 'a'"), Some("cost DESC")).unwrap()), ["it's", "c"]);
        assert_eq!(names(&dt, &dt.select(None, Some("cost ASC")).unwrap()), ["b", "c", "a", "it's"]);
        assert_eq!(names(&dt, &dt.select(Some("name = 'it''s' OR cost IN (1, 2)"), Some("[cost]")).unwrap()), ["b", "c", "it's"]);
        assert_eq!(dt.select(Some("name LIKE 'i%'"), None).unwrap().len(), 1);
        assert!(dt.select(Some("nope = 1"), None).is_err());
    }

    #[test]
    fn ids_increase() {
        let a = next_id();
        assert!(next_id() > a);
    }

    #[test]
    fn xml_round_trip() {
        let mut dt = table();
        dt.rows[1][2] = None;
        let back = DataTable::from_xml("t", &dt.to_xml_schema(), &dt.to_xml()).unwrap();
        assert_eq!(back.columns.len(), 3);
        assert_eq!(back.columns[2].ty, ColType::I16);
        assert_eq!(back.rows.len(), 4);
        assert_eq!(back.rows[1][2], None);
        assert_eq!(back.rows[3][1], Some(Value::Str("it's".into())));
        assert_eq!(back.row_id(0), dt.row_id(0));
    }
}
