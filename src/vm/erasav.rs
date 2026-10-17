//! Emuera's binary save format ("1808", `EraBinaryDataWriter` /
//! `EraBinaryDataReader` in Emuera's source), so saves made here load in
//! Emuera and the other way round.
//!
//! ```text
//! u64 header (PNG-like magic; a second magic for gzip-compressed bodies)
//! u32 version 1808, u32 extra-word count (0) [+ that many u32]
//! body (gzip-compressed after the header when the zip magic is used):
//!   u8 file type, i64 game code, i64 game version, string comment,
//!   then the type's payload: a sequence of entries
//!   u8 kind, string key, data  …  terminated by 0xFF (EOF) / 0xFE (EOC)
//! ```
//!
//! Strings are .NET `BinaryWriter` strings: a 7-bit-varint byte count, then
//! UTF-16LE. Integers inside entries are compressed (`0..=0xCF` as one byte,
//! else a marker byte and 2/4/8 little-endian bytes); arrays store their
//! lengths, then values with runs of zeros (and of all-zero rows / planes)
//! replaced by counts.

use std::io::{Read, Write};

pub const HEADER: u64 = 0x0A1A_0A0D_4152_4589;
pub const ZIP_HEADER: u64 = 0x0A50_495A_4152_4589;
pub const VERSION: u32 = 1808;

/// `EraSaveFileType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileType {
    /// saveNN.sav
    Normal = 0,
    /// global.sav
    Global = 1,
    /// dat/var_*.dat (SAVEVAR; unused by Emuera itself)
    Var = 2,
    /// dat/chara_*.dat (SAVECHARA)
    CharVar = 3,
}

// entry kinds (`EraSaveDataType`)
const INT: u8 = 0x00;
const INT_ARRAY: u8 = 0x01;
const INT_ARRAY_2D: u8 = 0x02;
const INT_ARRAY_3D: u8 = 0x03;
const STR: u8 = 0x10;
const STR_ARRAY: u8 = 0x11;
const STR_ARRAY_2D: u8 = 0x12;
const STR_ARRAY_3D: u8 = 0x13;
const MAP: u8 = 0x20;
const XML: u8 = 0x21;
const DT: u8 = 0x22;
const SEPARATOR: u8 = 0xFD;
const EOC: u8 = 0xFE;
const EOF: u8 = 0xFF;

// bytes inside array data (`Ebdb`)
const B_MAX_BYTE: u8 = 0xCF;
const B_INT16: u8 = 0xD0;
const B_INT32: u8 = 0xD1;
const B_INT64: u8 = 0xD2;
const B_STRING: u8 = 0xD8;
const B_EOA1: u8 = 0xE0;
const B_EOA2: u8 = 0xE1;
const B_ZERO: u8 = 0xF0;
const B_ZERO_A1: u8 = 0xF1;
const B_ZERO_A2: u8 = 0xF2;
const B_EOD: u8 = 0xFF;

/// Array contents as written / read: integers or strings, row-major over
/// `dims` (1 to 3 of them; empty for a scalar).
#[derive(Clone, Copy, Debug)]
pub enum Values<'a> {
    Int(&'a [i64]),
    Str(&'a [String]),
}

/// One saved entry.
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    /// a variable: its dims (empty: scalar) and values, sparse (flat index
    /// in the saved shape → value); missing elements are 0 / ""
    Var { key: String, dims: Vec<usize>, ints: Vec<(usize, i64)>, strs: Vec<(usize, String)>, is_str: bool },
    /// Emuera-EM string map (MAP_*)
    Map { key: String, pairs: Vec<(String, String)> },
    /// Emuera-EM XML document (XML_DOCUMENT)
    Xml { key: String, xml: String },
    /// Emuera-EM data table: (schema, data) as `DataTable.WriteXmlSchema` /
    /// `WriteXml` write them
    Dt { key: String, schema: String, data: String },
    /// start of a character's `#DIM CHARADATA` variables
    Separator,
    /// end of a character
    Eoc,
    /// end of a section
    Eof,
}

// ================================================================ writing

/// Builds a save file. Call `begin`, then entries, then `finish`.
pub struct Writer {
    body: Vec<u8>,
}

impl Writer {
    /// A file of `ft` for game `code` / `version` with comment `text`.
    pub fn begin(ft: FileType, code: i64, version: i64, text: &str) -> Writer {
        let mut w = Writer { body: Vec::with_capacity(1 << 16) };
        w.body.push(ft as u8);
        w.raw_i64(code);
        w.raw_i64(version);
        w.string(text);
        w
    }

    /// Header + body, the body gzip-compressed when `zip` (Emuera's
    /// "compress save data" option).
    pub fn finish(self, zip: bool) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.body.len() + 16);
        out.extend_from_slice(&(if zip { ZIP_HEADER } else { HEADER }).to_le_bytes());
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        if zip {
            let mut gz = flate2::write::GzEncoder::new(out, flate2::Compression::default());
            let _ = gz.write_all(&self.body);
            return gz.finish().unwrap_or_default();
        }
        out.extend_from_slice(&self.body);
        out
    }

    /// An uncompressed i64 (header fields, character counts).
    pub fn raw_i64(&mut self, v: i64) {
        self.body.extend_from_slice(&v.to_le_bytes());
    }

    /// A .NET string: 7-bit varint byte length, UTF-16LE.
    pub fn string(&mut self, s: &str) {
        let units: Vec<u16> = s.encode_utf16().collect();
        let mut n = units.len() * 2;
        loop {
            let b = (n & 0x7F) as u8;
            n >>= 7;
            if n == 0 {
                self.body.push(b);
                break;
            }
            self.body.push(b | 0x80);
        }
        for u in units {
            self.body.extend_from_slice(&u.to_le_bytes());
        }
    }

    pub fn separator(&mut self) {
        self.body.push(SEPARATOR);
    }
    pub fn eoc(&mut self) {
        self.body.push(EOC);
    }
    pub fn eof(&mut self) {
        self.body.push(EOF);
    }

    /// Compressed integer (`m_WriteInt`).
    fn int(&mut self, v: i64) {
        if (0..=B_MAX_BYTE as i64).contains(&v) {
            self.body.push(v as u8);
        } else if (i16::MIN as i64..=i16::MAX as i64).contains(&v) {
            self.body.push(B_INT16);
            self.body.extend_from_slice(&(v as i16).to_le_bytes());
        } else if (i32::MIN as i64..=i32::MAX as i64).contains(&v) {
            self.body.push(B_INT32);
            self.body.extend_from_slice(&(v as i32).to_le_bytes());
        } else {
            self.body.push(B_INT64);
            self.body.extend_from_slice(&v.to_le_bytes());
        }
    }

    /// A variable. `dims` empty writes a scalar (only the character
    /// variables NO, ISASSI, NAME… are scalars in Emuera); up to 3 dims.
    pub fn var(&mut self, key: &str, dims: &[usize], vals: Values) {
        let kind = match (vals, dims.len()) {
            (Values::Int(_), 0) => INT,
            (Values::Int(_), 1) => INT_ARRAY,
            (Values::Int(_), 2) => INT_ARRAY_2D,
            (Values::Int(_), _) => INT_ARRAY_3D,
            (Values::Str(_), 0) => STR,
            (Values::Str(_), 1) => STR_ARRAY,
            (Values::Str(_), 2) => STR_ARRAY_2D,
            (Values::Str(_), _) => STR_ARRAY_3D,
        };
        self.body.push(kind);
        self.string(key);
        if dims.is_empty() {
            match vals {
                Values::Int(v) => self.int(v.first().copied().unwrap_or(0)),
                Values::Str(v) => self.string(v.first().map(|s| s.as_str()).unwrap_or("")),
            }
            return;
        }
        // shape as Emuera sees it: at most 3 dims (deeper ones folded in)
        let mut d: Vec<usize> = dims.to_vec();
        while d.len() > 3 {
            let last = d.pop().unwrap();
            *d.last_mut().unwrap() *= last;
        }
        for &n in &d {
            self.body.extend_from_slice(&(n as i32).to_le_bytes());
        }
        let is_zero = |i: usize| match vals {
            Values::Int(v) => v.get(i).is_none_or(|&x| x == 0),
            Values::Str(v) => v.get(i).is_none_or(|s| s.is_empty()),
        };
        let put = |w: &mut Writer, i: usize| match vals {
            Values::Int(v) => w.int(v[i]),
            Values::Str(v) => {
                w.body.push(B_STRING);
                w.string(&v[i]);
            }
        };
        let total: usize = d.iter().product();
        match d.len() {
            1 => {
                let mut zeros = 0;
                for i in 0..total {
                    if is_zero(i) {
                        zeros += 1;
                    } else {
                        if zeros > 0 {
                            self.body.push(B_ZERO);
                            self.int(zeros);
                            zeros = 0;
                        }
                        put(self, i);
                    }
                }
            }
            2 => {
                let (l0, l1) = (d[0], d[1]);
                let (mut zeros, mut zero_rows) = (0i64, 0i64);
                for x in 0..l0 {
                    for y in 0..l1 {
                        let i = x * l1 + y;
                        if is_zero(i) {
                            zeros += 1;
                        } else {
                            if zero_rows > 0 {
                                self.body.push(B_ZERO_A1);
                                self.int(zero_rows);
                                zero_rows = 0;
                            }
                            if zeros > 0 {
                                self.body.push(B_ZERO);
                                self.int(zeros);
                                zeros = 0;
                            }
                            put(self, i);
                        }
                    }
                    if zeros == l1 as i64 {
                        zero_rows += 1;
                    } else {
                        self.body.push(B_EOA1);
                    }
                    zeros = 0;
                }
            }
            _ => {
                let (l0, l1, l2) = (d[0], d[1], d[2]);
                let (mut zeros, mut zero_rows, mut zero_planes) = (0i64, 0i64, 0i64);
                for x in 0..l0 {
                    for y in 0..l1 {
                        for z in 0..l2 {
                            let i = (x * l1 + y) * l2 + z;
                            if is_zero(i) {
                                zeros += 1;
                            } else {
                                if zero_planes > 0 {
                                    self.body.push(B_ZERO_A2);
                                    self.int(zero_planes);
                                    zero_planes = 0;
                                }
                                if zero_rows > 0 {
                                    self.body.push(B_ZERO_A1);
                                    self.int(zero_rows);
                                    zero_rows = 0;
                                }
                                if zeros > 0 {
                                    self.body.push(B_ZERO);
                                    self.int(zeros);
                                    zeros = 0;
                                }
                                put(self, i);
                            }
                        }
                        if zeros == l2 as i64 {
                            zero_rows += 1;
                        } else {
                            self.body.push(B_EOA1);
                        }
                        zeros = 0;
                    }
                    if zero_rows == l1 as i64 {
                        zero_planes += 1;
                    } else {
                        self.body.push(B_EOA2);
                    }
                    zero_rows = 0;
                }
            }
        }
        self.body.push(B_EOD);
    }

    pub fn map<'a>(&mut self, key: &str, pairs: impl ExactSizeIterator<Item = (&'a String, &'a String)>) {
        self.body.push(MAP);
        self.string(key);
        self.body.extend_from_slice(&(pairs.len() as i32).to_le_bytes());
        for (k, v) in pairs {
            self.string(k);
            self.string(v);
        }
    }

    pub fn xml(&mut self, key: &str, xml: &str) {
        self.body.push(XML);
        self.string(key);
        self.string(xml);
    }

    pub fn dt(&mut self, key: &str, schema: &str, data: &str) {
        self.body.push(DT);
        self.string(key);
        self.string(schema);
        self.string(data);
    }
}

// ================================================================ reading

/// Reads a save file's body.
pub struct Reader {
    b: Vec<u8>,
    p: usize,
}

fn bad<T>() -> Result<T, String> {
    Err("broken save data".into())
}

impl Reader {
    /// Check the header (and decompress); None if this isn't an Emuera
    /// binary save (an old text-format save, or something else).
    pub fn open(data: &[u8]) -> Option<Reader> {
        if data.len() < 16 {
            return None;
        }
        let magic = u64::from_le_bytes(data[0..8].try_into().unwrap());
        if magic != HEADER && magic != ZIP_HEADER {
            return None;
        }
        let version = u32::from_le_bytes(data[8..12].try_into().unwrap());
        let extra = u32::from_le_bytes(data[12..16].try_into().unwrap()) as usize;
        if version != VERSION {
            return None;
        }
        let start = 16usize.checked_add(extra.checked_mul(4)?)?;
        let rest = data.get(start..)?;
        let b = if magic == ZIP_HEADER {
            let mut out = Vec::new();
            flate2::read::GzDecoder::new(rest).read_to_end(&mut out).ok()?;
            out
        } else {
            rest.to_vec()
        };
        Some(Reader { b, p: 0 })
    }

    /// Nothing left (Emuera's `EOF()`: the optional Emuera-EM section).
    pub fn at_end(&self) -> bool {
        self.p >= self.b.len()
    }

    fn take(&mut self, n: usize) -> Result<&[u8], String> {
        let end = self.p.checked_add(n).filter(|&e| e <= self.b.len()).ok_or("save data ends early")?;
        let s = &self.b[self.p..end];
        self.p = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, String> {
        Ok(self.take(1)?[0])
    }
    fn i16(&mut self) -> Result<i16, String> {
        Ok(i16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32, String> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn raw_i64(&mut self) -> Result<i64, String> {
        Ok(i64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    pub fn file_type(&mut self) -> Result<FileType, String> {
        Ok(match self.u8()? {
            0 => FileType::Normal,
            1 => FileType::Global,
            2 => FileType::Var,
            3 => FileType::CharVar,
            _ => return bad(),
        })
    }

    pub fn string(&mut self) -> Result<String, String> {
        let mut n = 0usize;
        let mut shift = 0;
        loop {
            let b = self.u8()?;
            n |= ((b & 0x7F) as usize) << shift;
            if b & 0x80 == 0 {
                break;
            }
            shift += 7;
            if shift > 28 {
                return bad();
            }
        }
        let bytes = self.take(n)?;
        let units: Vec<u16> = bytes.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        Ok(String::from_utf16_lossy(&units))
    }

    fn int_after(&mut self, b: u8) -> Result<i64, String> {
        Ok(match b {
            0..=B_MAX_BYTE => b as i64,
            B_INT16 => self.i16()? as i64,
            B_INT32 => self.i32()? as i64,
            B_INT64 => self.raw_i64()?,
            _ => return bad(),
        })
    }
    fn int(&mut self) -> Result<i64, String> {
        let b = self.u8()?;
        self.int_after(b)
    }
    fn count(&mut self) -> Result<usize, String> {
        let n = self.int()?;
        if n < 0 {
            return bad();
        }
        Ok(n as usize)
    }

    /// The next entry of the current section.
    pub fn entry(&mut self) -> Result<Entry, String> {
        let kind = self.u8()?;
        match kind {
            EOF => return Ok(Entry::Eof),
            EOC => return Ok(Entry::Eoc),
            SEPARATOR => return Ok(Entry::Separator),
            _ => {}
        }
        let key = self.string()?;
        Ok(match kind {
            INT => Entry::Var { key, dims: vec![], ints: vec![(0, self.int()?)], strs: vec![], is_str: false },
            STR => Entry::Var { key, dims: vec![], ints: vec![], strs: vec![(0, self.string()?)], is_str: true },
            INT_ARRAY | INT_ARRAY_2D | INT_ARRAY_3D | STR_ARRAY | STR_ARRAY_2D | STR_ARRAY_3D => {
                let nd = (kind & 0x0F) as usize;
                let is_str = kind & 0x10 != 0;
                let mut dims = Vec::with_capacity(nd);
                for _ in 0..nd {
                    let n = self.i32()?;
                    if n < 0 {
                        return bad();
                    }
                    dims.push(n as usize);
                }
                let (ints, strs) = self.array(&dims, is_str)?;
                Entry::Var { key, dims, ints, strs, is_str }
            }
            MAP => {
                let n = self.i32()?.max(0) as usize;
                let mut pairs = Vec::with_capacity(n.min(1 << 16));
                for _ in 0..n {
                    let k = self.string()?;
                    let v = self.string()?;
                    pairs.push((k, v));
                }
                Entry::Map { key, pairs }
            }
            XML => Entry::Xml { key, xml: self.string()? },
            DT => {
                let schema = self.string()?;
                let data = self.string()?;
                Entry::Dt { key, schema, data }
            }
            _ => return bad(),
        })
    }

    /// Array data for `dims` (1 to 3): the non-zero elements by flat index.
    #[allow(clippy::type_complexity)]
    fn array(&mut self, dims: &[usize], is_str: bool) -> Result<(Vec<(usize, i64)>, Vec<(usize, String)>), String> {
        let (mut ints, mut strs) = (Vec::new(), Vec::new());
        let l1 = dims.get(1).copied().unwrap_or(1);
        let l2 = dims.get(2).copied().unwrap_or(1);
        let (mut x, mut y, mut z) = (0usize, 0usize, 0usize);
        let nd = dims.len();
        loop {
            let b = self.u8()?;
            match b {
                B_EOD => break,
                B_ZERO_A2 if nd == 3 => {
                    x += self.count()?;
                    y = 0;
                    z = 0;
                }
                B_EOA2 if nd == 3 => {
                    x += 1;
                    y = 0;
                    z = 0;
                }
                B_ZERO_A1 if nd >= 2 => {
                    let n = self.count()?;
                    if nd == 2 {
                        x += n;
                        y = 0;
                    } else {
                        y += n;
                        z = 0;
                    }
                }
                B_EOA1 if nd >= 2 => {
                    if nd == 2 {
                        x += 1;
                        y = 0;
                    } else {
                        y += 1;
                        z = 0;
                    }
                }
                B_ZERO => {
                    let n = self.count()?;
                    match nd {
                        1 => x += n,
                        2 => y += n,
                        _ => z += n,
                    }
                }
                _ => {
                    let flat = match nd {
                        1 => x,
                        2 => x * l1 + y,
                        _ => (x * l1 + y) * l2 + z,
                    };
                    if is_str {
                        if b != B_STRING {
                            return bad();
                        }
                        strs.push((flat, self.string()?));
                    } else {
                        ints.push((flat, self.int_after(b)?));
                    }
                    match nd {
                        1 => x += 1,
                        2 => y += 1,
                        _ => z += 1,
                    }
                }
            }
        }
        Ok((ints, strs))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(dims: &[usize], vals: Values<'_>) -> Entry {
        let mut w = Writer::begin(FileType::Normal, 7153, 0, "テスト");
        w.var("V", dims, vals);
        w.eof();
        for zip in [false, true] {
            let bytes = w.body.clone();
            let file = Writer { body: bytes }.finish(zip);
            let mut r = Reader::open(&file).unwrap();
            assert_eq!(r.file_type().unwrap(), FileType::Normal);
            assert_eq!(r.raw_i64().unwrap(), 7153);
            assert_eq!(r.raw_i64().unwrap(), 0);
            assert_eq!(r.string().unwrap(), "テスト");
            let e = r.entry().unwrap();
            assert_eq!(r.entry().unwrap(), Entry::Eof);
            assert!(r.at_end());
            if !zip {
                return e;
            }
        }
        unreachable!()
    }

    #[test]
    fn known_bytes() {
        // DAY:0 = 5 as Emuera writes it: kind 01, key "DAY" (6 bytes UTF-16),
        // length 1, the value 5, end of data
        let mut w = Writer::begin(FileType::Normal, 0, 0, "");
        w.var("DAY", &[1], Values::Int(&[5]));
        assert_eq!(&w.body[18..], &[0x01, 6, b'D', 0, b'A', 0, b'Y', 0, 1, 0, 0, 0, 5, 0xFF]);
        // FLAG with zeros, a negative and a large value
        let mut w = Writer::begin(FileType::Normal, 0, 0, "");
        w.var("F", &[6], Values::Int(&[0, 0, 300, 0, -1, 0]));
        assert_eq!(&w.body[18..], &[0x01, 2, b'F', 0, 6, 0, 0, 0, 0xF0, 2, 0xD0, 0x2C, 0x01, 0xF0, 1, 0xD0, 0xFF, 0xFF, 0xFF]);
    }

    #[test]
    fn arrays_roundtrip() {
        let mut v = vec![0i64; 3 * 4 * 5];
        v[7] = 1;
        v[13] = -70000;
        v[59] = i64::MAX;
        match roundtrip(&[3, 4, 5], Values::Int(&v)) {
            Entry::Var { dims, ints, .. } => {
                assert_eq!(dims, vec![3, 4, 5]);
                assert_eq!(ints, vec![(7, 1), (13, -70000), (59, i64::MAX)]);
            }
            e => panic!("{:?}", e),
        }
        let mut s = vec![String::new(); 4 * 3];
        s[0] = "a".into();
        s[11] = "名前".into();
        match roundtrip(&[4, 3], Values::Str(&s)) {
            Entry::Var { strs, is_str, .. } => {
                assert!(is_str);
                assert_eq!(strs, vec![(0, "a".to_string()), (11, "名前".to_string())]);
            }
            e => panic!("{:?}", e),
        }
        match roundtrip(&[], Values::Str(&["霊夢".to_string()])) {
            Entry::Var { dims, strs, .. } => {
                assert!(dims.is_empty());
                assert_eq!(strs, vec![(0, "霊夢".to_string())]);
            }
            e => panic!("{:?}", e),
        }
        // all-zero 2D rows between values
        let mut v = vec![0i64; 5 * 2];
        v[1] = 9;
        v[8] = 4;
        match roundtrip(&[5, 2], Values::Int(&v)) {
            Entry::Var { ints, .. } => assert_eq!(ints, vec![(1, 9), (8, 4)]),
            e => panic!("{:?}", e),
        }
    }

    #[test]
    fn long_strings_use_multibyte_lengths() {
        let long = "x".repeat(200);
        let mut w = Writer::begin(FileType::Global, 1, 2, &long);
        w.eof();
        let mut r = Reader::open(&w.finish(false)).unwrap();
        r.file_type().unwrap();
        r.raw_i64().unwrap();
        r.raw_i64().unwrap();
        assert_eq!(r.string().unwrap(), long);
    }
}
