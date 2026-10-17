//! Built-in commands and functions.
//!
//! Layout:
//!
//! * `COMMANDS` / `FUNCTIONS` — every built-in name the parser accepts.
//!   A name in neither list on a statement line is parsed as an assignment.
//! * [`Vm::command`] — statement-style commands (`Instr::Command`). Its
//!   fallback arm runs [`Vm::builtin_fn`] for functions used as statements
//!   (`STRLENS "x"` puts the value in RESULT / RESULTS, as in Emuera).
//! * [`Vm::builtin_fn`] — functions usable in expressions. Returns
//!   `Ok(None)` for names it doesn't know, so callers can report them.
//! * `map_fn` / `dt_fn` — the Emuera-EM `MAP_*` and `DT_*` families.
//!
//! Arguments arrive unevaluated (`&[Expr]`): helpers `arg` / `ai` / `as_`
//! evaluate argument *k* with a default for omitted ones, `avar` takes an
//! argument that must be a variable (for commands that write into it), and
//! `array_row` / `arr_vals` treat such a variable as an array row.

use super::value::*;
use super::vars::Arr;
use super::{pad, Chara, Loc, Vm};
use crate::ast::*;
use crate::console::{Rgb, Seg};
use crate::expr::parse_form;
use crate::loader::sjis_len;
// Arc (aliased as Rc): parsed programs are built on several threads
use std::sync::Arc as Rc;

/// Statement-only commands (functions callable as statements are separate).
pub const COMMANDS: &[&str] = &[
    "DRAWLINE", "CUSTOMDRAWLINE", "DRAWLINEFORM", "CLEARLINE", "REDRAW", "SETCOLOR", "RESETCOLOR", "SETBGCOLOR",
    "RESETBGCOLOR", "SETCOLORBYNAME", "SETBGCOLORBYNAME", "FONTBOLD", "FONTITALIC", "FONTREGULAR", "FONTSTYLE", "SETFONT",
    "ALIGNMENT", "PRINTBUTTON", "PRINTBUTTONC", "PRINTBUTTONLC", "PRINT_IMG", "PRINT_RECT", "PRINT_SPACE", "HTML_PRINT",
    "BAR", "BARL", "REUSELASTLINE", "SKIPDISP", "OUTPUTLOG", "DEBUGPRINT", "DEBUGPRINTL", "DEBUGPRINTFORM",
    "DEBUGPRINTFORML", "DEBUGCLEAR", "ASSERT", "TOOLTIP_SETCOLOR", "TOOLTIP_SETDELAY", "TOOLTIP_SETDURATION",
    "TOOLTIP_SETFONT", "TOOLTIP_SETFONTSIZE", "TOOLTIP_CUSTOM", "TOOLTIP_SETFORMAT", "TOOLTIP_FORMAT", "TOOLTIP_IMG", "SETANIMETIMER",
    "CLEARTEXTBOX", "UPDATECHECK", "INPUT", "INPUTS", "TINPUT", "TINPUTS", "ONEINPUT", "ONEINPUTS", "TONEINPUT",
    "TONEINPUTS", "BINPUT", "BINPUTS", "ONEBINPUT", "ONEBINPUTS", "INPUTANY", "BREAKBUTTON", "SKIPLOG", "FORCE_QUIT",
    "PRINT_ABL", "PRINT_TALENT", "PRINT_MARK", "PRINT_EXP", "PRINT_PALAM", "PRINT_ITEM", "HTML_PRINT_ISLAND",
    "HTML_PRINT_ISLAND_CLEAR", "MOVETEXTBOX", "RESUMETEXTBOX", "SETBGIMAGE", "REMOVEBGIMAGE", "CLEARBGIMAGE",
    "FORCE_QUIT_AND_RESTART", "CALLSHARP", "REF", "REFBYNAME", "SAVEVAR", "LOADVAR", "WAIT", "FORCEWAIT", "TWAIT", "WAITANYKEY", "INPUTMOUSEKEY", "DOTRAIN", "CALLTRAIN", "STOPCALLTRAIN",
    "SAVEGAME", "LOADGAME", "SAVEDATA", "LOADDATA", "DELDATA", "SAVEGLOBAL", "LOADGLOBAL", "RESETDATA", "RESETGLOBAL",
    "VARSET", "CVARSET", "ARRAYSHIFT", "ARRAYREMOVE", "ARRAYSORT", "ARRAYCOPY", "SWAP", "TIMES",
    "SETBIT", "CLEARBIT", "INVERTBIT", "SPLIT", "RANDOMIZE", "INITRAND", "DUMPRAND", "STRLEN", "STRLENU", "STRLENFORM",
    "STRLENFORMU", "PUTFORM", "ADDCHARA", "ADDSPCHARA", "ADDDEFCHARA", "ADDVOIDCHARA", "DELCHARA", "DELALLCHARA",
    "PICKUPCHARA", "SWAPCHARA", "COPYCHARA", "ADDCOPYCHARA", "SORTCHARA", "RESET_STAIN", "UPCHECK", "CUPCHECK",
    "CALLEVENT", "CLEARBUTTON", "SAVECHARA", "LOADCHARA", "MOUSESKIP_", "BITMAP_CACHE_ENABLE", "FORCEKANA",
    "SETCOLOR_", "SPRITESETPOS", "CBGSETG", "CBGSETSPRITE", "CBGCLEAR", "CBGCLEARBUTTON", "CBGSETBUTTONSPRITE",
    "CBGREMOVERANGE", "CBGREMOVEBMAP", "CBGSETBMAPG", "SETTEXTBOX", "QUIT_AND_RESTART", "RESETBGCOLOR_",
    "DT_COLUMN_OPTIONS", "PRINT_SHOPITEM", "AWAIT", "VARSETEX", "SPRITEANIMECREATE", "SPRITEANIMEADDFRAME",
];

pub const FUNCTIONS: &[&str] = &[
    "ABS", "SIGN", "MAX", "MIN", "LIMIT", "INRANGE", "POWER", "SQRT", "CBRT", "LOG", "LOG10", "EXPONENT", "RAND",
    "GETBIT", "SUMARRAY", "MAXARRAY", "MINARRAY", "MATCH", "CMATCH", "SUMCARRAY", "MAXCARRAY", "MINCARRAY",
    "GROUPMATCH", "NOSAMES", "ALLSAMES", "FINDELEMENT", "FINDLASTELEMENT", "INRANGEARRAY", "INRANGECARRAY", "VARSIZE",
    "STRLENS", "STRLENSU", "SUBSTRING", "SUBSTRINGU", "STRFIND", "STRFINDU", "STRCOUNT", "REPLACE", "TOSTR", "TOINT",
    "ISNUMERIC", "TOUPPER", "TOLOWER", "TOHALF", "TOFULL", "CHARATU", "STRJOIN", "ESCAPE", "UNICODE", "ENCODETOUNI",
    "CONVERT", "STRFORM", "MONEYSTR", "BARSTR", "LINEISEMPTY", "CSVNAME", "CSVCALLNAME", "CSVNICKNAME",
    "CSVMASTERNAME", "CSVBASE", "CSVABL", "CSVTALENT", "CSVEXP", "CSVMARK", "CSVJUEL", "CSVCFLAG", "CSVCSTR", "CSVEQUIP",
    "CSVRELATION", "CSVEX", "FINDCHARA", "FINDLASTCHARA", "EXISTCSV", "GETCHARA", "GETSPCHARA", "GETTIME", "GETTIMES",
    "GETMILLISECOND", "GETSECOND", "GETCOLOR", "GETDEFCOLOR", "GETBGCOLOR", "GETDEFBGCOLOR", "GETFOCUSCOLOR", "GETSTYLE",
    "GETFONT", "CURRENTALIGN", "CURRENTREDRAW", "CHKFONT", "ISSKIP", "MOUSESKIP", "MESSKIP", "GETKEY", "GETKEYTRIGGERED",
    "MOUSEX", "MOUSEY", "ISACTIVE", "PRINTCPERLINE", "PRINTCLENGTH", "SAVENOS", "GETNUM", "GETPALAMLV", "GETEXPLV",
    "CHKDATA", "SPRITECREATED", "SPRITEWIDTH", "SPRITEHEIGHT", "GETCONFIG", "GETCONFIGS", "HTML_ESCAPE",
    "HTML_TOPLAINTEXT", "HTML_GETPRINTEDSTR", "HTML_POPPRINTINGSTR", "HTML_TAGSPLIT", "EXISTFUNCTION", "EXISTVAR",
    "GETVAR", "GETVARS", "SETVAR", "ISDEFINED", "STRREPEAT", "GETLINESTR", "REGEXPMATCH", "CHKCHARADATA", "FIND_CHARADATA",
    "GCREATED", "GWIDTH", "GHEIGHT", "GETCHARA_", "__FORMS", "ARRAYMSORTEX", "GETMEMORYUSAGE", "CLEARMEMORY", "TRIM",
    "EXISTSOUND", "PLAYSOUND", "STOPSOUND", "PLAYBGM", "STOPBGM", "SETSOUNDVOLUME", "SETBGMVOLUME", "MOUSEB", "ENUMFUNCBEGINSWITH",
    "MAP_CREATE", "MAP_EXIST", "MAP_RELEASE", "MAP_GET", "MAP_CLEAR", "MAP_SIZE", "MAP_HAS", "MAP_SET", "MAP_REMOVE",
    "MAP_GETKEYS", "MAP_TOXML", "COLOR_FROMNAME", "COLOR_FROMRGB", "HTML_STRINGLEN", "GETMETH", "GETMETHS",
    "GETDOINGFUNCTION", "CLIENTWIDTH", "CLIENTHEIGHT", "EXISTFILE", "SPRITEPOSX", "SPRITEPOSY", "GGETCOLOR", "GETTEXTBOX",
    "DT_CREATE", "DT_EXIST", "DT_RELEASE", "DT_NOCASE", "DT_CLEAR", "DT_COLUMN_ADD", "DT_COLUMN_NAMES", "DT_COLUMN_EXIST",
    "DT_COLUMN_REMOVE", "DT_COLUMN_LENGTH", "DT_ROW_ADD", "DT_ROW_SET", "DT_ROW_REMOVE", "DT_ROW_LENGTH", "DT_CELL_GET",
    "DT_CELL_ISNULL", "DT_CELL_GETS", "DT_CELL_SET", "DT_SELECT", "DT_TOXML", "DT_FROMXML", "LOADTEXT", "SAVETEXT",
    "XML_DOCUMENT", "XML_RELEASE", "XML_EXIST", "XML_TOSTR", "XML_GET", "XML_GET_BYNAME", "XML_SET", "XML_SET_BYNAME",
    "XML_ADDNODE", "XML_ADDNODE_BYNAME", "XML_REMOVENODE", "XML_REMOVENODE_BYNAME", "XML_REPLACE", "XML_REPLACE_BYNAME",
    "XML_ADDATTRIBUTE", "XML_ADDATTRIBUTE_BYNAME", "XML_REMOVEATTRIBUTE", "XML_REMOVEATTRIBUTE_BYNAME",
    // graphics buffers (methods, usable as statements: result in RESULT)
    "GCREATE", "GCREATEFROMFILE", "GDISPOSE", "GCLEAR", "GFILLRECTANGLE", "GDRAWSPRITE", "GDRAWG", "GDRAWGWITHMASK",
    "GSETCOLOR", "GSETBRUSH", "GSAVE", "GLOAD", "SPRITECREATE", "SPRITEDISPOSE", "ARRAYMSORT", "GDRAWLINE", "GDRAWTEXT",
    "GDASHSTYLE", "GDRAWGWITHROTATE", "GSETPEN", "GSETFONT", "GGETBRUSH", "GGETPEN", "GGETPENWIDTH", "GGETFONT",
    "GGETFONTSIZE", "GGETFONTSTYLE", "GGETTEXTSIZE", "SPRITEDISPOSEALL", "SPRITEGETCOLOR", "SPRITEMOVE", "SPRITESETPOS",
    "FLOWINPUT", "FLOWINPUTS", "GETNUMB", "HOTKEY_STATE", "HOTKEY_STATE_INIT", "ERDNAME", "EXISTMETH", "UNICODEBYTE",
    "ENUMFUNCWITH", "ENUMFUNCENDSWITH", "ENUMVARBEGINSWITH", "ENUMVARWITH", "ENUMVARENDSWITH", "ENUMMACROBEGINSWITH",
    "ENUMMACROWITH", "ENUMMACROENDSWITH", "ENUMFILES", "HTML_SUBSTRING", "HTML_STRINGLINES", "GETDISPLAYLINE", "MAP_FROMXML",
];

/// Built-in functions that return a string (Emuera's ReturnType), for
/// EXISTMETH.
const STR_FUNCTIONS: &[&str] = &["CSVNAME", "CSVCALLNAME", "CSVNICKNAME", "CSVMASTERNAME", "CSVCSTR", "GETFONT", "BARSTR", "CURRENTALIGN", "MONEYSTR", "GETTIMES", "SUBSTRING", "SUBSTRINGU", "TOSTR", "TOUPPER", "TOLOWER", "TOHALF", "TOFULL", "REPLACE", "UNICODE", "CONVERT", "ESCAPE", "CHARATU", "GETLINESTR", "STRFORM", "STRJOIN", "HTML_GETPRINTEDSTR", "HTML_POPPRINTINGSTR", "HTML_TOPLAINTEXT", "HTML_ESCAPE", "MOUSEB", "LOADTEXT", "HTML_SUBSTRING", "GETVARS", "XML_TOSTR", "MAP_GET", "MAP_GETKEYS", "MAP_TOXML", "DT_TOXML", "GGETFONT", "GETTEXTBOX", "ERDNAME", "GETDISPLAYLINE", "GETDOINGFUNCTION", "GETMETHS"];

/// Is `c` (upper-case) a statement-only built-in? (hash set: checked for
/// every statement while parsing)
pub fn is_builtin_command(c: &str) -> bool {
    static SET: std::sync::OnceLock<crate::fxhash::HashSet<&'static str>> = std::sync::OnceLock::new();
    SET.get_or_init(|| COMMANDS.iter().copied().collect()).contains(c)
}
/// Is `c` (upper-case) a built-in function (also usable as a statement)?
pub fn is_builtin_function(c: &str) -> bool {
    static SET: std::sync::OnceLock<crate::fxhash::HashSet<&'static str>> = std::sync::OnceLock::new();
    SET.get_or_init(|| FUNCTIONS.iter().copied().collect()).contains(c)
}

fn now() -> chrono_like::DateTime {
    chrono_like::now()
}

/// What Emuera puts in SAVEDATA_TEXT before @SAVEINFO: `yyyy/MM/dd HH:mm:ss `.
pub(crate) fn save_timestamp() -> String {
    let t = now();
    format!("{:04}/{:02}/{:02} {:02}:{:02}:{:02} ", t.y, t.mo, t.d, t.h, t.mi, t.s)
}

/// Tiny local-time helper (avoids a chrono dependency).
mod chrono_like {
    pub struct DateTime {
        pub y: i64,
        pub mo: i64,
        pub d: i64,
        pub h: i64,
        pub mi: i64,
        pub s: i64,
        pub ms: i64,
    }
    pub fn now() -> DateTime {
        unsafe {
            let mut tv: libc::timeval = std::mem::zeroed();
            libc::gettimeofday(&mut tv, std::ptr::null_mut());
            let t: libc::time_t = tv.tv_sec;
            let mut tm: libc::tm = std::mem::zeroed();
            libc::localtime_r(&t, &mut tm);
            DateTime {
                y: tm.tm_year as i64 + 1900,
                mo: tm.tm_mon as i64 + 1,
                d: tm.tm_mday as i64,
                h: tm.tm_hour as i64,
                mi: tm.tm_min as i64,
                s: tm.tm_sec as i64,
                ms: (tv.tv_usec / 1000) as i64,
            }
        }
    }
}

/// TOHALF: full-width ASCII (ＡＢＣ１２３) and the ideographic space to half-width.
fn to_half(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            '\u{3000}' => ' ',
            _ => c,
        })
        .collect()
}

/// TOFULL: the reverse of [`to_half`].
fn to_full(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '!'..='~' => char::from_u32(c as u32 + 0xFEE0).unwrap_or(c),
            ' ' => '\u{3000}',
            _ => c,
        })
        .collect()
}

/// Substring by Shift_JIS byte positions (Emuera's SUBSTRING).
fn substring_sjis(s: &str, start: i64, len: i64) -> String {
    let mut out = String::new();
    let mut pos = 0i64;
    let start = start.max(0);
    for c in s.chars() {
        let w = sjis_len(&c.to_string()) as i64;
        if pos >= start && (len < 0 || pos + w <= start + len) {
            out.push(c);
        }
        pos += w;
        if len >= 0 && pos >= start + len {
            break;
        }
    }
    out
}

/// .NET KnownColor / CSS named colours (what Color.FromName accepts).
const NAMED_COLORS: &[(&str, i64)] = &[
    ("aliceblue", 0xf0f8ff),
    ("antiquewhite", 0xfaebd7),
    ("aqua", 0x00ffff),
    ("aquamarine", 0x7fffd4),
    ("azure", 0xf0ffff),
    ("beige", 0xf5f5dc),
    ("bisque", 0xffe4c4),
    ("black", 0x000000),
    ("blanchedalmond", 0xffebcd),
    ("blue", 0x0000ff),
    ("blueviolet", 0x8a2be2),
    ("brown", 0xa52a2a),
    ("burlywood", 0xdeb887),
    ("cadetblue", 0x5f9ea0),
    ("chartreuse", 0x7fff00),
    ("chocolate", 0xd2691e),
    ("coral", 0xff7f50),
    ("cornflowerblue", 0x6495ed),
    ("cornsilk", 0xfff8dc),
    ("crimson", 0xdc143c),
    ("cyan", 0x00ffff),
    ("darkblue", 0x00008b),
    ("darkcyan", 0x008b8b),
    ("darkgoldenrod", 0xb8860b),
    ("darkgray", 0xa9a9a9),
    ("darkgrey", 0xa9a9a9),
    ("darkgreen", 0x006400),
    ("darkkhaki", 0xbdb76b),
    ("darkmagenta", 0x8b008b),
    ("darkolivegreen", 0x556b2f),
    ("darkorange", 0xff8c00),
    ("darkorchid", 0x9932cc),
    ("darkred", 0x8b0000),
    ("darksalmon", 0xe9967a),
    ("darkseagreen", 0x8fbc8f),
    ("darkslateblue", 0x483d8b),
    ("darkslategray", 0x2f4f4f),
    ("darkslategrey", 0x2f4f4f),
    ("darkturquoise", 0x00ced1),
    ("darkviolet", 0x9400d3),
    ("deeppink", 0xff1493),
    ("deepskyblue", 0x00bfff),
    ("dimgray", 0x696969),
    ("dimgrey", 0x696969),
    ("dodgerblue", 0x1e90ff),
    ("firebrick", 0xb22222),
    ("floralwhite", 0xfffaf0),
    ("forestgreen", 0x228b22),
    ("fuchsia", 0xff00ff),
    ("gainsboro", 0xdcdcdc),
    ("ghostwhite", 0xf8f8ff),
    ("gold", 0xffd700),
    ("goldenrod", 0xdaa520),
    ("gray", 0x808080),
    ("grey", 0x808080),
    ("green", 0x008000),
    ("greenyellow", 0xadff2f),
    ("honeydew", 0xf0fff0),
    ("hotpink", 0xff69b4),
    ("indianred", 0xcd5c5c),
    ("indigo", 0x4b0082),
    ("ivory", 0xfffff0),
    ("khaki", 0xf0e68c),
    ("lavender", 0xe6e6fa),
    ("lavenderblush", 0xfff0f5),
    ("lawngreen", 0x7cfc00),
    ("lemonchiffon", 0xfffacd),
    ("lightblue", 0xadd8e6),
    ("lightcoral", 0xf08080),
    ("lightcyan", 0xe0ffff),
    ("lightgoldenrodyellow", 0xfafad2),
    ("lightgray", 0xd3d3d3),
    ("lightgrey", 0xd3d3d3),
    ("lightgreen", 0x90ee90),
    ("lightpink", 0xffb6c1),
    ("lightsalmon", 0xffa07a),
    ("lightseagreen", 0x20b2aa),
    ("lightskyblue", 0x87cefa),
    ("lightslategray", 0x778899),
    ("lightslategrey", 0x778899),
    ("lightsteelblue", 0xb0c4de),
    ("lightyellow", 0xffffe0),
    ("lime", 0x00ff00),
    ("limegreen", 0x32cd32),
    ("linen", 0xfaf0e6),
    ("magenta", 0xff00ff),
    ("maroon", 0x800000),
    ("mediumaquamarine", 0x66cdaa),
    ("mediumblue", 0x0000cd),
    ("mediumorchid", 0xba55d3),
    ("mediumpurple", 0x9370db),
    ("mediumseagreen", 0x3cb371),
    ("mediumslateblue", 0x7b68ee),
    ("mediumspringgreen", 0x00fa9a),
    ("mediumturquoise", 0x48d1cc),
    ("mediumvioletred", 0xc71585),
    ("midnightblue", 0x191970),
    ("mintcream", 0xf5fffa),
    ("mistyrose", 0xffe4e1),
    ("moccasin", 0xffe4b5),
    ("navajowhite", 0xffdead),
    ("navy", 0x000080),
    ("oldlace", 0xfdf5e6),
    ("olive", 0x808000),
    ("olivedrab", 0x6b8e23),
    ("orange", 0xffa500),
    ("orangered", 0xff4500),
    ("orchid", 0xda70d6),
    ("palegoldenrod", 0xeee8aa),
    ("palegreen", 0x98fb98),
    ("paleturquoise", 0xafeeee),
    ("palevioletred", 0xdb7093),
    ("papayawhip", 0xffefd5),
    ("peachpuff", 0xffdab9),
    ("peru", 0xcd853f),
    ("pink", 0xffc0cb),
    ("plum", 0xdda0dd),
    ("powderblue", 0xb0e0e6),
    ("purple", 0x800080),
    ("rebeccapurple", 0x663399),
    ("red", 0xff0000),
    ("rosybrown", 0xbc8f8f),
    ("royalblue", 0x4169e1),
    ("saddlebrown", 0x8b4513),
    ("salmon", 0xfa8072),
    ("sandybrown", 0xf4a460),
    ("seagreen", 0x2e8b57),
    ("seashell", 0xfff5ee),
    ("sienna", 0xa0522d),
    ("silver", 0xc0c0c0),
    ("skyblue", 0x87ceeb),
    ("slateblue", 0x6a5acd),
    ("slategray", 0x708090),
    ("slategrey", 0x708090),
    ("snow", 0xfffafa),
    ("springgreen", 0x00ff7f),
    ("steelblue", 0x4682b4),
    ("tan", 0xd2b48c),
    ("teal", 0x008080),
    ("thistle", 0xd8bfd8),
    ("tomato", 0xff6347),
    ("turquoise", 0x40e0d0),
    ("violet", 0xee82ee),
    ("wheat", 0xf5deb3),
    ("white", 0xffffff),
    ("whitesmoke", 0xf5f5f5),
    ("yellow", 0xffff00),
    ("yellowgreen", 0x9acd32),
];

fn parse_color_name(n: &str) -> Option<Rgb> {
    let n = n.trim().to_lowercase();
    if let Some((_, v)) = NAMED_COLORS.iter().find(|(name, _)| *name == n) {
        return Some(Rgb::from_i64(*v));
    }
    let h = n.trim_start_matches('#');
    if h.len() == 6 {
        let v = i64::from_str_radix(h, 16).ok()?;
        return Some(Rgb::from_i64(v));
    }
    None
}

/// A colour given by name (`"red"`) or as `#RRGGBB`, for HTML and SETCOLORBYNAME.
pub fn color_from_str(s: &str) -> Option<Rgb> {
    parse_color_name(s)
}

impl Vm {
    /// Argument `k`, evaluated; None when omitted (`F(1,,3)`) or absent.
    fn arg(&mut self, args: &[Expr], k: usize) -> VmResult<Option<Value>> {
        match args.get(k) {
            None | Some(Expr::Empty) => Ok(None),
            Some(e) => Ok(Some(self.eval(e)?)),
        }
    }
    /// Integer argument `k`, or `d` if omitted.
    fn ai(&mut self, args: &[Expr], k: usize, d: i64) -> VmResult<i64> {
        Ok(self.arg(args, k)?.map(|v| v.as_int()).unwrap_or(d))
    }
    /// String argument `k`, or `d` if omitted.
    fn as_(&mut self, args: &[Expr], k: usize, d: &str) -> VmResult<String> {
        Ok(self.arg(args, k)?.map(|v| v.into_string()).unwrap_or_else(|| d.to_string()))
    }
    /// Argument `k` as a variable reference (not evaluated), for built-ins
    /// that write their results into a caller-supplied variable.
    fn avar(&self, args: &[Expr], k: usize) -> VmResult<Rc<VarRef>> {
        match args.get(k) {
            Some(Expr::Var(v)) => Ok(v.clone()),
            other => err(format!("argument {} must be a variable, got {:?}", k + 1, other)),
        }
    }
    /// Variable reference treated as an array: returns location, base flat of
    /// the row, and row length (last dimension).
    fn array_row(&mut self, v: &VarRef) -> VmResult<(Loc, usize, usize)> {
        // Locate with last index forced to 0 to get the row base
        let p = self.locate(v)?;
        let (row_len, base) = {
            let a = self.arr_at(&p.loc).ok_or_else(|| Ctrl::Error("not an array".into(), false))?;
            let rl = if a.dims.len() <= 1 { a.len().max(a.last_dim()) } else { a.last_dim() };
            let base = p.flat - (p.flat % rl.max(1));
            (rl, if a.dims.len() <= 1 { 0 } else { base })
        };
        Ok((p.loc, base, row_len))
    }

    /// Elements `start..end` of `v`'s row (end defaults to the row length):
    /// (location, flat index of `start`, values).
    fn arr_vals(&mut self, v: &VarRef, start: i64, end: Option<i64>) -> VmResult<(Loc, usize, Vec<Value>)> {
        let (loc, base, len) = self.array_row(v)?;
        let end = end.unwrap_or(len as i64).min(len as i64).max(0) as usize;
        let start = start.max(0) as usize;
        let a = self.arr_at(&loc).unwrap();
        let vals = (start..end.max(start)).map(|i| a.get(base + i)).collect();
        Ok((loc, base + start, vals))
    }

    /// Index into `csv.charas` of the template with 番号 `no`.
    fn chara_no_template(&self, no: i64) -> Option<usize> {
        self.csv.charas.iter().position(|c| c.no == no)
    }

    /// Build a character from its Chara*.csv template (ADDCHARA); `-1`
    /// makes an empty one (ADDVOIDCHARA). Arrays get VariableSize.csv sizes.
    pub fn new_chara_from_csv(&mut self, no: i64) -> VmResult<Chara> {
        let mut c = Chara::default();
        let varsize = &self.csv.varsize;
        let set = |c: &mut Chara, name: &str, idx: i64, val: Value| {
            let def = super::vars::system_var(name);
            let a = c.vars.entry(name.to_string()).or_insert_with(|| match def {
                Some(d) => Arr::new(d.is_str, &super::sized_dims(varsize, name, d)),
                None => Arr::new(val.is_str(), &[1000]),
            });
            a.set(idx.max(0) as usize, val);
        };
        set(&mut c, "NO", 0, Value::Int(no));
        if let Some(ti) = self.chara_no_template(no) {
            let t = self.csv.charas[ti].clone();
            set(&mut c, "NAME", 0, Value::Str(t.name));
            set(&mut c, "CALLNAME", 0, Value::Str(t.callname));
            set(&mut c, "NICKNAME", 0, Value::Str(t.nickname));
            set(&mut c, "MASTERNAME", 0, Value::Str(t.mastername));
            for (var, i, v) in t.ints {
                set(&mut c, &var, i, Value::Int(v));
            }
            for (var, i, s) in t.strs {
                set(&mut c, &var, i, Value::Str(s));
            }
        } else if no != -1 {
            return err(format!("ADDCHARA: no Chara CSV with 番号 {}", no));
        }
        Ok(c)
    }

    // =============================================================== commands
    /// Run a statement-style built-in. Unknown names fall through to
    /// `builtin_fn`; if that doesn't know them either, they are warned
    /// about once and skipped (errors with `--strict`).
    pub fn command(&mut self, name: &str, args: &Args) -> VmResult<()> {
        let ex: Vec<Expr> = match args {
            Args::Exprs(v) => v.clone(),
            _ => Vec::new(),
        };
        let a = &ex[..];
        match name {
            "DRAWLINE" => self.con.drawline(),
            "CUSTOMDRAWLINE" => {
                if let Args::Raw(s) = args {
                    self.con.custom_drawline(s)
                }
            }
            "DRAWLINEFORM" => {
                if let Args::Form(f) = args {
                    let s = self.eval_form(f)?;
                    self.con.custom_drawline(&s)
                }
            }
            "CLEARLINE" => {
                let n = self.ai(a, 0, 1)?;
                self.con.clear_lines(n.max(0) as usize)
            }
            "REDRAW" => {
                let n = self.ai(a, 0, 1)?;
                self.con.redraw = n != 0;
                if n != 0 {
                    self.flush();
                }
            }
            "SETCOLOR" | "SETBGCOLOR" => {
                let c = if a.len() >= 3 {
                    Rgb(self.ai(a, 0, 0)? as u8, self.ai(a, 1, 0)? as u8, self.ai(a, 2, 0)? as u8)
                } else {
                    Rgb::from_i64(self.ai(a, 0, 0)?)
                };
                if name == "SETCOLOR" {
                    self.con.style.fg = c
                } else {
                    self.con.style.bg = Some(c)
                }
            }
            "SETCOLORBYNAME" | "SETBGCOLORBYNAME" => {
                if let Args::Raw(s) = args {
                    match parse_color_name(s) {
                        Some(c) if name == "SETCOLORBYNAME" => self.con.style.fg = c,
                        Some(c) => self.con.style.bg = Some(c),
                        None => self.warn(&format!("unknown color name {}", s)),
                    }
                }
            }
            "RESETCOLOR" => self.con.style.fg = self.con.default_fg,
            "RESETBGCOLOR" => self.con.style.bg = None,
            "FONTBOLD" => self.con.style.bold = true,
            "FONTITALIC" => self.con.style.italic = true,
            "FONTREGULAR" => {
                let s = &mut self.con.style;
                s.bold = false;
                s.italic = false;
                s.underline = false;
                s.strike = false;
            }
            "FONTSTYLE" => {
                let n = self.ai(a, 0, 0)?;
                let s = &mut self.con.style;
                s.bold = n & 1 != 0;
                s.italic = n & 2 != 0;
                s.strike = n & 4 != 0;
                s.underline = n & 8 != 0;
            }
            "SETFONT" => {
                let s = self.as_(a, 0, "")?;
                self.con.font_name = s;
            }
            "ALIGNMENT" => {
                if let Args::Raw(s) = args {
                    self.con.align = match s.trim().to_uppercase().as_str() {
                        "CENTER" => Align::Center,
                        "RIGHT" => Align::Right,
                        _ => Align::Left,
                    };
                }
            }
            "PRINTBUTTON" | "PRINTBUTTONC" | "PRINTBUTTONLC" => {
                let text = self.as_(a, 0, "")?;
                let v = self.arg(a, 1)?.unwrap_or(Value::Int(0));
                let text = match name {
                    "PRINTBUTTONC" => pad(&text, self.con.printc_len, Align::Right),
                    "PRINTBUTTONLC" => pad(&text, self.con.printc_len, Align::Left),
                    _ => text,
                };
                self.con.print_button(&text, v);
            }
            "PRINT_IMG" => {
                // accept both a string expression and a bare sprite name
                let n = match a.first() {
                    Some(Expr::Var(v)) if v.idx.is_empty() && !self.var_exists(&v.name) => v.name.to_string(),
                    _ => self.as_(a, 0, "")?,
                };
                self.print_img(&n, None, None, None, None);
            }
            "PRINT_RECT" => {
                let w = self.ai(a, 0, 100)? as f32;
                let fs = self.con.font_size;
                let fg = self.con.style.fg;
                self.con.push_seg(Seg::Rect { w: w * fs / 100.0, h: fs, color: fg, button: None });
            }
            "PRINT_SPACE" => {
                let w = self.ai(a, 0, 100)? as f32;
                let fs = self.con.font_size;
                self.con.push_seg(Seg::Space { w: w * fs / 100.0 });
            }
            "HTML_PRINT" => {
                let s = self.as_(a, 0, "")?;
                self.html_print(&s);
            }
            "BAR" | "BARL" => {
                let v = self.ai(a, 0, 0)?;
                let m = self.ai(a, 1, 1)?;
                let l = self.ai(a, 2, 10)?;
                let s = barstr(v, m, l);
                self.con.print(&s);
                if name == "BARL" {
                    self.con.newline();
                }
            }
            "REUSELASTLINE" => {
                if let Args::Form(f) = args {
                    let s = self.eval_form(f)?;
                    self.con.temp_line(&s);
                }
            }
            "SKIPDISP" => {
                let n = self.ai(a, 0, 0)?;
                self.con.skip_disp = n != 0;
            }
            "INPUT" | "TINPUT" | "ONEINPUT" | "TONEINPUT" => {
                let timed = name.starts_with('T');
                let one = name.contains("ONE");
                let (timeout, default) = if timed {
                    (Some(self.ai(a, 0, 1000)?.max(0) as u64), Some(self.ai(a, 1, 0)?))
                } else {
                    (None, self.arg(a, 0)?.map(|v| v.as_int()))
                };
                let v = self.input_int(default, timeout, one)?;
                if timed && self.is_timeout && a.len() >= 4 {
                    let msg = self.as_(a, 3, "")?;
                    self.con.print(&msg);
                    self.con.newline();
                }
                self.set_result(v);
            }
            "INPUTS" | "TINPUTS" | "ONEINPUTS" | "TONEINPUTS" => {
                let timed = name.starts_with('T');
                let one = name.contains("ONE");
                let (timeout, default) = if timed {
                    (Some(self.ai(a, 0, 1000)?.max(0) as u64), self.arg(a, 1)?.map(|v| v.into_string()))
                } else {
                    (None, self.arg(a, 0)?.map(|v| v.into_string()))
                };
                let v = self.input_str(default, timeout, one)?;
                self.set_results(v);
            }
            "BINPUT" | "BINPUTS" | "ONEBINPUT" | "ONEBINPUTS" => {
                // like INPUT / INPUTS, but only a button on screen answers
                let strs = name.ends_with('S');
                let one = name.starts_with("ONE");
                let def = self.arg(a, 0)?;
                if !self.con.cur.is_empty() {
                    self.con.newline();
                }
                let buttons: Vec<Value> = self.con.button_values().into_iter().filter(|v| strs || matches!(v, Value::Int(_))).collect();
                if buttons.is_empty() {
                    let Some(d) = def else { return err(format!("{}: there are no buttons to choose", name)) };
                    if strs {
                        self.set_results(d.into_string());
                    } else {
                        self.set_result(d.as_int());
                    }
                    return Ok(());
                }
                loop {
                    if strs {
                        let v = self.input_str(def.clone().map(|d| d.into_string()), None, one)?;
                        if buttons.iter().any(|b| b.as_string() == v) {
                            self.set_results(v);
                            break;
                        }
                    } else {
                        let v = self.input_int(def.as_ref().map(|d| d.as_int()), None, one)?;
                        if buttons.contains(&Value::Int(v)) || def.as_ref().is_some_and(|d| d.as_int() == v) {
                            self.set_result(v);
                            break;
                        }
                    }
                }
            }
            "INPUTANY" => {
                // a number goes to RESULT, anything else to RESULTS
                let v = self.input_str(None, None, false)?;
                match v.trim().parse::<i64>() {
                    Ok(n) => self.set_result(n),
                    Err(_) => self.set_results(v),
                }
            }
            "BREAKBUTTON" => self.con.break_buttons(),
            "SKIPLOG" => self.mes_skip = self.ai(a, 0, 0)? != 0,
            "PRINT_ABL" | "PRINT_TALENT" | "PRINT_MARK" | "PRINT_EXP" => {
                // eramaker's one-line summaries of a character
                let c = self.ai(a, 0, 0)?;
                let ci = self.chara_index(c, name)?;
                let (var, table) = match name {
                    "PRINT_ABL" => ("ABL", "ABL"),
                    "PRINT_TALENT" => ("TALENT", "TALENT"),
                    "PRINT_MARK" => ("MARK", "MARK"),
                    _ => ("EXP", "EXP"),
                };
                let arr = self.chara_arr(ci, var).clone();
                let mut out = String::new();
                for k in 0..arr.len() {
                    let v = arr.get_i(k);
                    let Some(n) = self.csv.name_of(table, k as i64).filter(|n| !n.is_empty()) else { continue };
                    if v == 0 {
                        continue;
                    }
                    match name {
                        "PRINT_TALENT" => out.push_str(&format!("[{}]", n)),
                        "PRINT_EXP" => out.push_str(&format!("{}{} ", n, v)),
                        _ => out.push_str(&format!("{}LV{} ", n, v)),
                    }
                }
                self.con.print(&out);
                self.con.newline();
            }
            "PRINT_PALAM" => {
                // PALAM bars in PRINTC columns: name[====......]  value
                let c = self.ai(a, 0, 0)?;
                let ci = self.chara_index(c, name)?;
                let arr = self.chara_arr(ci, "PALAM").clone();
                let lv: Vec<i64> = (0..5).map(|k| self.get_global_i("PALAMLV", k)).collect();
                let per_line = 3;
                let mut count = 0;
                for k in 0..100.min(arr.len()) {
                    let p = arr.get_i(k);
                    let n = self.csv.name_of("PALAM", k as i64).unwrap_or("").to_string();
                    if p == 0 && n.is_empty() {
                        continue;
                    }
                    let (mut c, mut border) = ('-', lv[1]);
                    for (ch, next) in [('=', 2), ('>', 3), ('*', 4)] {
                        if p >= border {
                            c = ch;
                            border = lv[next];
                        }
                    }
                    let bar = if border <= 0 || border <= p {
                        c.to_string().repeat(10)
                    } else if p <= 0 {
                        ".".repeat(10)
                    } else {
                        let k = (p * 10 / border) as usize;
                        format!("{}{}", c.to_string().repeat(k), ".".repeat(10 - k))
                    };
                    let text = format!("{}[{}]{:>6}", n, bar, p);
                    let padded = super::pad(&text, self.con.printc_len, crate::ast::Align::Right);
                    self.con.print(&padded);
                    count += 1;
                    if count % per_line == 0 {
                        self.con.newline();
                    }
                }
                if !self.con.cur.is_empty() {
                    self.con.newline();
                }
            }
            "PRINT_ITEM" => {
                let mut out = String::from("所持アイテム：");
                let mut any = false;
                let n = self.global_arr("ITEM").len();
                for k in 0..n {
                    let v = self.get_global_i("ITEM", k);
                    if v == 0 {
                        continue;
                    }
                    any = true;
                    out.push_str(&format!("{}({}) ", self.csv.name_of("ITEM", k as i64).unwrap_or(""), v));
                }
                if !any {
                    out.push_str("なし");
                }
                self.con.print(&out);
                self.con.newline();
            }
            // Emuera-EM's fixed overlay; the terminal prints it in the log
            "HTML_PRINT_ISLAND" => {
                let s = self.as_(a, 0, "")?;
                self.html_print(&s);
            }
            // window chrome a terminal doesn't have: tooltips, the input
            // box's position, background images and layers
            "HTML_PRINT_ISLAND_CLEAR" | "MOVETEXTBOX" | "RESUMETEXTBOX" | "SETBGIMAGE" | "REMOVEBGIMAGE" | "CLEARBGIMAGE"
            | "SETTEXTBOX" | "TOOLTIP_SETCOLOR" | "TOOLTIP_SETDELAY" | "TOOLTIP_SETDURATION" | "TOOLTIP_SETFONT"
            | "TOOLTIP_SETFONTSIZE" | "TOOLTIP_CUSTOM" | "TOOLTIP_SETFORMAT" | "TOOLTIP_FORMAT" | "TOOLTIP_IMG" | "CBGSETG"
            | "CBGSETSPRITE" | "CBGCLEAR" | "CBGCLEARBUTTON" | "CBGSETBUTTONSPRITE" | "CBGREMOVERANGE" | "CBGREMOVEBMAP"
            | "CBGSETBMAPG" => self.set_result(1),
            "FORCE_QUIT" => return Err(Ctrl::Quit),
            "QUIT_AND_RESTART" | "FORCE_QUIT_AND_RESTART" => {
                // start the game again from scratch: run this program anew
                self.fe.shutdown();
                use std::os::unix::process::CommandExt;
                let exe = std::env::current_exe().map_err(|e| Ctrl::Error(format!("{}: {}", name, e), false))?;
                let e = std::process::Command::new(exe).args(std::env::args_os().skip(1)).exec();
                return err(format!("{}: {}", name, e));
            }
            // as in Emuera, which doesn't implement these either
            "REF" | "REFBYNAME" | "SAVEVAR" | "LOADVAR" => return err(format!("{} is not implemented (in Emuera neither)", name)),
            "CALLSHARP" => return err("CALLSHARP: C# plugins can't run in emuera-tty"),
            "WAIT" | "FORCEWAIT" | "WAITANYKEY" => self.wait()?,
            "AWAIT" => {
                // Emuera-EE: show what's printed so far and pause (no input);
                // at most 10 s. Headless runs don't sleep.
                let ms = self.ai(a, 0, 0)?;
                if !(0..=10000).contains(&ms) {
                    return err(format!("AWAIT: {} ms is out of range (0 to 10000)", ms));
                }
                self.fe.flush(&mut self.con, &mut self.res);
                if self.fe.is_fullscreen() && ms > 0 {
                    std::thread::sleep(std::time::Duration::from_millis(ms as u64));
                }
            }
            "VARSETEX" => {
                // VARSETEX "VAR:i:j", value, all_rows = 1, start = 0, end
                let n = self.as_(a, 0, "")?;
                let e = crate::expr::parse_expr(&n, &self.prog.defines).map_err(|e| Ctrl::Error(e, false))?;
                let Expr::Var(v) = crate::parser::upper_expr(e) else { return err(format!("VARSETEX: '{}' is not a variable", n)) };
                let val = self.arg(a, 1)?;
                let all_rows = self.ai(a, 2, 1)? != 0;
                let start = self.ai(a, 3, 0)?.max(0) as usize;
                let end = self.arg(a, 4)?.map(|x| x.as_int().max(0) as usize);
                let p = self.locate(&v)?;
                let arr = self.arr_at(&p.loc).ok_or_else(|| Ctrl::Error(format!("VARSETEX: {} is read-only", n), false))?;
                let is_str = arr.is_str();
                let val = match val {
                    Some(x) if x.is_str() != is_str => return err(format!("VARSETEX: value type doesn't match {}", n)),
                    Some(x) => x,
                    None if is_str => Value::Str(String::new()),
                    None => Value::Int(0),
                };
                let rl = if arr.dims.len() <= 1 { arr.len() } else { arr.last_dim() }.max(1);
                let end = end.unwrap_or(rl).min(rl);
                if arr.dims.len() <= 1 {
                    for i in start..end {
                        arr.set(i, val.clone());
                    }
                } else {
                    // from the given index on, in its row, or (integer
                    // arrays, all_rows) in every row
                    let col = p.flat % rl;
                    let rows: Vec<usize> = if all_rows && !is_str { (0..arr.len() / rl).collect() } else { vec![p.flat / rl] };
                    for r in rows {
                        for i in start.max(col)..end {
                            arr.set(r * rl + i, val.clone());
                        }
                    }
                }
                self.set_result(1);
            }
            // sprite animations: no animated sprites in a terminal
            "SPRITEANIMECREATE" | "SPRITEANIMEADDFRAME" => self.set_result(0),
            "TWAIT" => {
                let t = self.ai(a, 0, 1000)?.max(0) as u64;
                let force = self.ai(a, 1, 0)?;
                let _ = force;
                if !self.con.cur.is_empty() {
                    self.con.newline();
                }
                self.raw_input(&crate::frontend::InputReq {
                    kind: crate::frontend::InputKind::Wait,
                    one: true,
                    timeout_ms: Some(t),
                    mouse: false,
                })?;
            }
            "INPUTMOUSEKEY" => {
                // RESULT:0 = 1 click / 3 key ; RESULT:1.. details. We map to button input.
                let to = self.arg(a, 0)?.map(|v| v.as_int().max(0) as u64);
                let s = self.input_str(None, to, false)?;
                match s.trim().parse::<i64>() {
                    Ok(v) => {
                        self.set_global_i("RESULT", 0, 1);
                        self.set_global_i("RESULT", 1, 0);
                        self.set_global_i("RESULT", 2, 0);
                        self.set_global_i("RESULT", 5, v);
                    }
                    Err(_) => {
                        self.set_global_i("RESULT", 0, 3);
                        self.set_global_i("RESULT", 1, s.chars().next().map(|c| c as i64).unwrap_or(13));
                    }
                }
            }
            "DOTRAIN" => {
                let n = self.ai(a, 0, 0)?;
                self.do_train_command(n)?;
            }
            "CALLTRAIN" | "STOPCALLTRAIN" => self.warn(&format!("{} not implemented", name)),
            "SAVEGAME" => {
                self.save_menu(true)?;
            }
            "LOADGAME" => {
                if self.save_menu(false)? {
                    return Err(Ctrl::Begin("__LOADED".into()));
                }
            }
            "SAVEDATA" => {
                let n = self.ai(a, 0, 0)?;
                let text = self.as_(a, 1, "")?;
                self.save_slot(n, &text)?;
            }
            "LOADDATA" => {
                let n = self.ai(a, 0, 0)?;
                if self.load_slot(n)? {
                    return Err(Ctrl::Begin("__LOADED".into()));
                }
            }
            "DELDATA" => {
                let n = self.ai(a, 0, 0)?;
                self.delete_slot(n);
            }
            "SAVECHARA" => {
                // SAVECHARA file, comment, character, character…
                let file = self.as_(a, 0, "")?;
                let text = self.as_(a, 1, "")?;
                let mut list = Vec::new();
                for k in 2..a.len() {
                    let c = self.ai(a, k, 0)?;
                    if c < 0 || c as usize >= self.charas.len() {
                        return err(format!("SAVECHARA: character {} out of range", c));
                    }
                    if list.contains(&(c as usize)) {
                        return err(format!("SAVECHARA: character {} given twice", c));
                    }
                    list.push(c as usize);
                }
                self.save_chara(&file, &text, &list)?;
            }
            "LOADCHARA" => {
                let file = self.as_(a, 0, "")?;
                let ok = self.load_chara(&file)?;
                self.set_result(ok as i64);
            }
            "SAVEGLOBAL" => self.save_global()?,
            "LOADGLOBAL" => {
                let ok = self.load_global()?;
                self.set_result(ok as i64);
            }
            "RESETDATA" => self.init_data(),
            "RESETGLOBAL" => {
                self.globals.remove("GLOBAL");
                self.globals.remove("GLOBALS");
            }
            "VARSET" => {
                let v = self.avar(a, 0)?;
                let val = self.arg(a, 1)?;
                let start = self.ai(a, 2, 0)?;
                let end = self.arg(a, 3)?.map(|v| v.as_int());
                let p = self.locate(&v)?;
                let a2 = self.arr_at(&p.loc).ok_or_else(|| Ctrl::Error("VARSET on read-only".into(), false))?;
                if v.idx.is_empty() && end.is_none() && start == 0 {
                    // whole array
                    match val {
                        None => a2.clear(),
                        Some(val) => {
                            for i in 0..a2.len() {
                                a2.set(i, val.clone());
                            }
                        }
                    }
                } else {
                    let rl = if a2.dims.len() <= 1 { a2.len() } else { a2.last_dim() };
                    let base = if a2.dims.len() <= 1 { 0 } else { p.flat - p.flat % rl };
                    let end = end.unwrap_or(rl as i64).max(0) as usize;
                    let val = val.unwrap_or(if a2.is_str() { Value::Str(String::new()) } else { Value::Int(0) });
                    for i in start.max(0) as usize..end {
                        a2.set(base + i, val.clone());
                    }
                }
            }
            "CVARSET" => {
                let v = self.avar(a, 0)?;
                let idx = match a.get(1) {
                    Some(e) => self.eval_index_pub(e, &v.name)?,
                    None => 0,
                };
                let val = self.arg(a, 2)?.unwrap_or(Value::Int(0));
                let start = self.ai(a, 3, 0)?.max(0) as usize;
                let end = self.ai(a, 4, self.charas.len() as i64)?.max(0) as usize;
                for c in start..end.min(self.charas.len()) {
                    self.chara_arr(c, &v.name).set(idx.max(0) as usize, val.clone());
                }
            }
            "ARRAYSHIFT" => {
                let v = self.avar(a, 0)?;
                let shift = self.ai(a, 1, 0)?;
                let empty = self.arg(a, 2)?.unwrap_or(Value::Int(0));
                let start = self.ai(a, 3, 0)?;
                let (loc, base, len) = self.array_row(&v)?;
                let count = self.ai(a, 4, len as i64 - start)?;
                let arr = self.arr_at(&loc).unwrap();
                let s = start.max(0) as usize;
                let n = (count.max(0) as usize).min(len.saturating_sub(s));
                let vals: Vec<Value> = (0..n).map(|i| arr.get(base + s + i)).collect();
                for i in 0..n {
                    let src = i as i64 - shift;
                    let nv = if src >= 0 && (src as usize) < n { vals[src as usize].clone() } else { empty.clone() };
                    arr.set(base + s + i, nv);
                }
            }
            "ARRAYREMOVE" => {
                let v = self.avar(a, 0)?;
                let start = self.ai(a, 1, 0)?.max(0) as usize;
                let cnt = self.ai(a, 2, 1)?;
                let (loc, base, len) = self.array_row(&v)?;
                let arr = self.arr_at(&loc).unwrap();
                let cnt = if cnt < 0 { len.saturating_sub(start) } else { cnt as usize };
                let vals: Vec<Value> = (0..len).map(|i| arr.get(base + i)).collect();
                let blank = if arr.is_str() { Value::Str(String::new()) } else { Value::Int(0) };
                let mut out: Vec<Value> = vals[..start.min(len)].to_vec();
                out.extend(vals.iter().skip(start + cnt).cloned());
                while out.len() < len {
                    out.push(blank.clone());
                }
                for (i, x) in out.into_iter().enumerate() {
                    arr.set(base + i, x);
                }
            }
            "ARRAYSORT" => {
                let v = self.avar(a, 0)?;
                let back = matches!(a.get(1), Some(Expr::Var(o)) if &*o.name == "BACK");
                let start = self.ai(a, 2, 0)?.max(0) as usize;
                let (loc, base, len) = self.array_row(&v)?;
                let cnt = self.ai(a, 3, (len - start.min(len)) as i64)?.max(0) as usize;
                let arr = self.arr_at(&loc).unwrap();
                let mut vals: Vec<Value> = (start..(start + cnt).min(len)).map(|i| arr.get(base + i)).collect();
                vals.sort_by(|x, y| match (x, y) {
                    (Value::Int(a), Value::Int(b)) => a.cmp(b),
                    _ => x.as_string().cmp(&y.as_string()),
                });
                if back {
                    vals.reverse();
                }
                for (i, x) in vals.into_iter().enumerate() {
                    arr.set(base + start + i, x);
                }
            }
            "ARRAYCOPY" => {
                let src = self.as_(a, 0, "")?.to_uppercase();
                let dst = self.as_(a, 1, "")?.to_uppercase();
                let sv = VarRef { name: src.into(), idx: vec![] };
                let dv = VarRef { name: dst.into(), idx: vec![] };
                let sp = self.locate(&sv)?;
                let data = self.arr_at(&sp.loc).cloned();
                let dp = self.locate(&dv)?;
                if let (Some(s), Some(d)) = (data, self.arr_at(&dp.loc)) {
                    for i in 0..s.len().min(d.len()) {
                        d.set(i, s.get(i));
                    }
                }
            }
            "SWAP" => {
                let x = self.avar(a, 0)?;
                let y = self.avar(a, 1)?;
                let vx = self.get_var(&x)?;
                let vy = self.get_var(&y)?;
                self.set_var(&x, vy)?;
                self.set_var(&y, vx)?;
            }
            "TIMES" => {
                if let Args::Raw(s) = args {
                    let parts: Vec<&str> = s.splitn(2, ',').collect();
                    let v = crate::expr::parse_expr(parts[0].trim(), &self.prog.defines).map_err(|e| Ctrl::Error(e, false))?;
                    let v = crate::parser::upper_expr(v);
                    let f: f64 = parts.get(1).map(|x| x.trim().parse().unwrap_or(1.0)).unwrap_or(1.0);
                    if let Expr::Var(vr) = v {
                        let cur = self.get_var(&vr)?.as_int();
                        self.set_var(&vr, Value::Int((cur as f64 * f) as i64))?;
                    }
                }
            }
            "SETBIT" | "CLEARBIT" | "INVERTBIT" => {
                let v = self.avar(a, 0)?;
                let mut x = self.get_var(&v)?.as_int();
                for k in 1..a.len().max(2) {
                    let b = self.ai(a, k, 0)?;
                    if !(0..64).contains(&b) {
                        return err("bit index out of range");
                    }
                    match name {
                        "SETBIT" => x |= 1 << b,
                        "CLEARBIT" => x &= !(1 << b),
                        _ => x ^= 1 << b,
                    }
                }
                self.set_var(&v, Value::Int(x))?;
            }
            "SPLIT" => {
                let s = self.as_(a, 0, "")?;
                let sep = self.as_(a, 1, ",")?;
                let v = self.avar(a, 2)?;
                let parts: Vec<String> = if sep.is_empty() { s.chars().map(|c| c.to_string()).collect() } else { s.split(sep.as_str()).map(|x| x.to_string()).collect() };
                let (loc, base, _) = self.array_row(&v)?;
                let start = {
                    let p = self.locate(&v)?;
                    p.flat - base
                };
                let n = parts.len();
                let arr = self.arr_at(&loc).unwrap();
                for (i, p) in parts.into_iter().enumerate() {
                    arr.set(base + start + i, Value::Str(p));
                }
                if let Some(Expr::Var(cv)) = a.get(3) {
                    let cv = cv.clone();
                    self.set_var(&cv, Value::Int(n as i64))?;
                }
                self.set_result(n as i64);
            }
            "PRINT_SHOPITEM" => self.print_shop_items(),
            "DT_COLUMN_OPTIONS" => {
                // DT_COLUMN_OPTIONS dt, column, DEFAULT, value {, option, value}
                let key = self.as_(a, 0, "")?;
                let cname = self.as_(a, 1, "")?;
                let mut k = 2;
                while k + 1 < a.len() {
                    let opt = match &a[k] {
                        Expr::Var(v) => v.name.to_string(),
                        e => self.eval(e)?.into_string().to_uppercase(),
                    };
                    let v = self.eval(&a[k + 1])?;
                    let Some(dt) = self.dts.get_mut(&key) else {
                        self.set_result(-1);
                        break;
                    };
                    let Some(c) = dt.col(&cname) else {
                        self.set_result(0);
                        break;
                    };
                    if opt == "DEFAULT" {
                        let v = dt.check_value(c, Some(v)).map_err(|_| Ctrl::Error(format!("DT_COLUMN_OPTIONS: wrong value type for {}.{}", key, cname), false))?;
                        dt.columns[c].default = v;
                    } else {
                        return err(format!("DT_COLUMN_OPTIONS: unknown option {}", opt));
                    }
                    k += 2;
                }
            }
            "HTML_TAGSPLIT" => {
                // HTML_TAGSPLIT html{, ARR{, COUNT}} : pieces are tags or text runs
                let html = self.as_(a, 0, "")?;
                let target = if a.len() >= 2 { self.avar(a, 1)? } else { Rc::new(VarRef { name: "RESULTS".into(), idx: vec![] }) };
                let pieces = html_tag_split(&html);
                let n = pieces.as_ref().map_or(-1, |p| p.len() as i64);
                if let Some(pieces) = pieces {
                    let (loc, base, len) = self.array_row(&target)?;
                    let arr = self.arr_at(&loc).unwrap();
                    for (k, p) in pieces.into_iter().take(len).enumerate() {
                        arr.set(base + k, Value::Str(p));
                    }
                }
                match a.get(2) {
                    Some(Expr::Var(cv)) => {
                        let cv = cv.clone();
                        self.set_var(&cv, Value::Int(n))?;
                    }
                    _ => self.set_result(n),
                }
            }
            "RANDOMIZE" | "INITRAND" => {
                let s = self.ai(a, 0, 0)?;
                self.seed(s);
            }
            "DUMPRAND" => {}
            "STRLEN" | "STRLENU" => {
                if let Args::Raw(s) = args {
                    let n = if name == "STRLEN" { sjis_len(s) } else { s.chars().count() };
                    self.set_result(n as i64);
                }
            }
            "STRLENFORM" | "STRLENFORMU" => {
                if let Args::Form(f) = args {
                    let s = self.eval_form(f)?;
                    let n = if name == "STRLENFORM" { sjis_len(&s) } else { s.chars().count() };
                    self.set_result(n as i64);
                }
            }
            "PUTFORM" => {
                if let Args::Form(f) = args {
                    let s = self.eval_form(f)?;
                    let mut cur = self.globals.get("SAVEDATA_TEXT").map(|a| a.get(0).into_string()).unwrap_or_default();
                    cur.push_str(&s);
                    self.globals.entry("SAVEDATA_TEXT".into()).or_insert_with(|| Arr::new(true, &[1])).set(0, Value::Str(cur));
                }
            }
            "ADDCHARA" | "ADDSPCHARA" => {
                for k in 0..a.len() {
                    let no = self.ai(a, k, 0)?;
                    let c = self.new_chara_from_csv(no)?;
                    self.charas.push(c);
                }
            }
            "ADDDEFCHARA" => {
                let c = self.new_chara_from_csv(0).or_else(|_| self.new_chara_from_csv(-1))?;
                self.charas.push(c);
            }
            "ADDVOIDCHARA" => {
                let c = self.new_chara_from_csv(-1)?;
                self.charas.push(c);
            }
            "DELCHARA" => {
                let mut idx = Vec::new();
                for k in 0..a.len() {
                    idx.push(self.ai(a, k, 0)?);
                }
                idx.sort_unstable();
                idx.dedup();
                for i in idx.into_iter().rev() {
                    if i >= 0 && (i as usize) < self.charas.len() {
                        self.charas.remove(i as usize);
                    } else {
                        return err(format!("DELCHARA: no character {}", i));
                    }
                }
            }
            "DELALLCHARA" => self.charas.clear(),
            "PICKUPCHARA" => {
                let mut keep = Vec::new();
                for k in 0..a.len() {
                    let i = self.ai(a, k, 0)?;
                    if i >= 0 && (i as usize) < self.charas.len() {
                        keep.push(self.charas[i as usize].clone());
                    }
                }
                self.charas = keep;
            }
            "SWAPCHARA" => {
                let x = self.ai(a, 0, 0)? as usize;
                let y = self.ai(a, 1, 0)? as usize;
                if x < self.charas.len() && y < self.charas.len() {
                    self.charas.swap(x, y);
                }
            }
            "COPYCHARA" => {
                let x = self.ai(a, 0, 0)? as usize;
                let y = self.ai(a, 1, 0)? as usize;
                if x < self.charas.len() && y < self.charas.len() {
                    self.charas[y] = self.charas[x].clone();
                }
            }
            "ADDCOPYCHARA" => {
                let x = self.ai(a, 0, 0)? as usize;
                if x < self.charas.len() {
                    let c = self.charas[x].clone();
                    self.charas.push(c);
                }
            }
            "SORTCHARA" => {
                // SORTCHARA [var], [FORWARD|BACK] — default NO, forward; MASTER stays put
                let raw = if let Args::Raw(s) = args { s.clone() } else { String::new() };
                let parts: Vec<String> = raw.split(',').map(|s| s.trim().to_uppercase()).collect();
                let key = parts.first().filter(|s| !s.is_empty()).cloned().unwrap_or_else(|| "NO".into());
                let back = parts.get(1).map(|s| s == "BACK").unwrap_or(false);
                let (kname, kidx) = match key.split_once(':') {
                    Some((n, i)) => (n.to_string(), i.parse::<usize>().unwrap_or(0)),
                    None => (key.clone(), 0),
                };
                let master = self.get_global_i("MASTER", 0);
                let mut items: Vec<(usize, Value)> = Vec::new();
                for c in 0..self.charas.len() {
                    if c as i64 == master {
                        continue;
                    }
                    let v = self.chara_arr(c, &kname).get(kidx);
                    items.push((c, v));
                }
                items.sort_by(|x, y| match (&x.1, &y.1) {
                    (Value::Int(a), Value::Int(b)) => a.cmp(b),
                    (a, b) => a.as_string().cmp(&b.as_string()),
                });
                if back {
                    items.reverse();
                }
                let old = self.charas.clone();
                let mut new = Vec::new();
                let mut it = items.into_iter();
                for c in 0..old.len() {
                    if c as i64 == master {
                        new.push(old[c].clone());
                    } else if let Some((i, _)) = it.next() {
                        new.push(old[i].clone());
                    }
                }
                self.charas = new;
            }
            "RESET_STAIN" => {
                let c = self.ai(a, 0, 0)?;
                if c >= 0 && (c as usize) < self.charas.len() {
                    self.chara_arr(c as usize, "STAIN").clear();
                }
            }
            "UPCHECK" | "CUPCHECK" => self.upcheck(name == "CUPCHECK", a)?,
            "CALLEVENT" => {
                let n = self.as_(a, 0, "")?.to_uppercase();
                self.call_name(&n, vec![])?;
            }
            "DEBUGPRINT" | "DEBUGPRINTL" | "DEBUGPRINTFORM" | "DEBUGPRINTFORML" | "DEBUGCLEAR" | "ASSERT" | "OUTPUTLOG"
            | "UPDATECHECK" | "SETANIMETIMER" | "CLEARTEXTBOX" | "BITMAP_CACHE_ENABLE" | "FORCEKANA" | "CLEARBUTTON" => {}
            n if n.starts_with("TOOLTIP_") => {}
            "GETTIME" => {
                // as a statement: RESULT = number, RESULTS = text
                if let Some(Value::Int(t)) = self.builtin_fn("GETTIME", a)? {
                    self.set_result(t);
                }
                if let Some(Value::Str(t)) = self.builtin_fn("GETTIMES", a)? {
                    self.set_results(t);
                }
            }
            _ => {
                // functions used as statements: result into RESULT / RESULTS
                match self.builtin_fn(name, a)? {
                    Some(Value::Int(i)) => self.set_result(i),
                    Some(Value::Str(s)) => self.set_results(s),
                    None => {
                        if self.strict {
                            return err(format!("command {} not implemented", name));
                        }
                        let graphics = name.starts_with('G') || name.starts_with("SPRITE") || name.starts_with("CBG") || name == "SETTEXTBOX";
                        self.warn(&format!("{}command {} not implemented (ignored)", if graphics { "graphics " } else { "" }, name));
                        if graphics {
                            self.set_result(0);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Evaluate an index for variable `var` with CSV-name lookup, for
    /// built-ins that take "variable + index" as separate arguments.
    pub fn eval_index_pub(&mut self, e: &Expr, var: &str) -> VmResult<i64> {
        // CSV-aware index evaluation for an explicit variable table
        if let Expr::Var(v) = e {
            if v.idx.is_empty() && !self.var_exists(&v.name) {
                if let Some(t) = crate::csvdata::table_for_var(var) {
                    if let Some(i) = self.csv.lookup(t, &v.name) {
                        return Ok(i);
                    }
                }
            }
        }
        Ok(self.eval(e)?.as_int())
    }

    /// UPCHECK / CUPCHECK: apply this turn's parameter changes,
    /// PALAM += UP - DOWN for TARGET (CUP/CDOWN for the given character),
    /// print a "name old+up-down=new" line per changed entry, then clear
    /// UP/DOWN.
    fn upcheck(&mut self, cup: bool, a: &[Expr]) -> VmResult<()> {
        let c = if cup { self.ai(a, 0, 0)? } else { self.target() };
        if c < 0 || c as usize >= self.charas.len() {
            return Ok(());
        }
        let c = c as usize;
        let n = self.chara_arr(c, "PALAM").len();
        for i in 0..n {
            let (up, down) = if cup {
                (self.chara_arr(c, "CUP").get_i(i), self.chara_arr(c, "CDOWN").get_i(i))
            } else {
                (self.get_global_i("UP", i), self.get_global_i("DOWN", i))
            };
            if up == 0 && down == 0 {
                continue;
            }
            let old = self.chara_arr(c, "PALAM").get_i(i);
            let new = old + up - down;
            self.chara_arr(c, "PALAM").set_i(i, new);
            let pname = self.csv.name_of("PALAM", i as i64).unwrap_or("").to_string();
            let mut s = format!("{} {}", pname, old);
            if up != 0 {
                s.push_str(&format!("+{}", up));
            }
            if down != 0 {
                s.push_str(&format!("-{}", down));
            }
            s.push_str(&format!("={}", new));
            self.con.print(&s);
            self.con.newline();
        }
        if cup {
            self.chara_arr(c, "CUP").clear();
            self.chara_arr(c, "CDOWN").clear();
        } else {
            self.global_arr("UP").clear();
            self.global_arr("DOWN").clear();
        }
        Ok(())
    }

    /// PRINT_IMG / `<img>`: queue an image segment. Sizes are percentages
    /// of the font size (Emuera's units); a zero/omitted width keeps the
    /// sprite's aspect ratio.
    /// Sizes are Emuera's "mixed numbers" `(value, is_px)`: pixels with
    /// `px`, else percent of the font size. No height (or 0) is one font
    /// size tall; no width keeps the sprite's aspect ratio.
    pub fn print_img(&mut self, name: &str, w: Option<(f32, bool)>, h: Option<(f32, bool)>, ypos: Option<(f32, bool)>, button: Option<Value>) {
        let fs = self.con.font_size;
        let px = |(v, is_px): (f32, bool)| if is_px { v } else { fs * v / 100.0 };
        let (sw, sh) = self.res.sprite_size(name).unwrap_or((fs as u32, fs as u32));
        let h = h.filter(|(v, _)| *v != 0.0).map(px).unwrap_or(fs);
        let w = match w.filter(|(v, _)| *v != 0.0) {
            Some(m) => px(m),
            None => h * sw as f32 / sh.max(1) as f32,
        };
        let ypos_px = ypos.map(px).unwrap_or(0.0);
        if !self.res.exists(name) {
            self.warn(&format!("image resource '{}' not found", name));
        }
        self.con.push_seg(Seg::Image { name: name.to_string(), w, h, ypos: ypos_px, button });
    }

    /// Emuera-EM string maps. Mirrors MapManagementMethod / MapDataOperationMethod /
    /// MapGetStrMethod: ops on a missing map return -1 (or "" for strings).
    fn map_fn(&mut self, name: &str, a: &[Expr]) -> VmResult<Option<Value>> {
        let i = |v: i64| Ok(Some(Value::Int(v)));
        let s = |v: String| Ok(Some(Value::Str(v)));
        let map = self.as_(a, 0, "")?;
        match name {
            "MAP_FROMXML" => {
                // <map><p><k>key</k><v>value</v></p>…</map> into an existing map
                let m = self.as_(a, 0, "")?;
                let x = self.as_(a, 1, "")?;
                if !self.maps.contains_key(&m) {
                    return i(0);
                }
                let doc = crate::xml::parse(&x).map_err(|e| Ctrl::Error(format!("MAP_FROMXML: {}", e), false))?;
                let map = self.maps.get_mut(&m).unwrap();
                for p in doc.elems().filter(|e| e.name == "p") {
                    let ks: Vec<_> = p.elems().filter(|e| e.name == "k").collect();
                    let vs: Vec<_> = p.elems().filter(|e| e.name == "v").collect();
                    if ks.len() == 1 && vs.len() == 1 {
                        map.set(ks[0].text(), vs[0].text());
                    }
                }
                i(1)
            }
            "MAP_CREATE" => {
                if self.maps.contains_key(&map) {
                    return i(0);
                }
                self.maps.insert(map, StrMap::default());
                i(1)
            }
            "MAP_EXIST" => i(self.maps.contains_key(&map) as i64),
            "MAP_RELEASE" => {
                self.maps.remove(&map);
                i(1)
            }
            "MAP_GET" => {
                let key = self.as_(a, 1, "")?;
                s(self.maps.get(&map).and_then(|m| m.get(&key)).cloned().unwrap_or_default())
            }
            "MAP_CLEAR" | "MAP_SIZE" | "MAP_HAS" | "MAP_SET" | "MAP_REMOVE" => {
                let key = if matches!(name, "MAP_CLEAR" | "MAP_SIZE") { String::new() } else { self.as_(a, 1, "")? };
                let val = if name == "MAP_SET" { self.as_(a, 2, "")? } else { String::new() };
                let Some(m) = self.maps.get_mut(&map) else { return i(-1) };
                match name {
                    "MAP_CLEAR" => m.clear(),
                    "MAP_SIZE" => return i(m.len() as i64),
                    "MAP_HAS" => return i(m.has(&key) as i64),
                    "MAP_REMOVE" => m.remove(&key),
                    _ => m.set(key, val),
                }
                i(1)
            }
            "MAP_GETKEYS" => {
                let Some(keys) = self.maps.get(&map).map(|m| m.keys().to_vec()) else { return s(String::new()) };
                if a.len() < 2 {
                    return s(keys.join(","));
                }
                // MAP_GETKEYS map, doit  -> RESULTS:*;  MAP_GETKEYS map, ARR, doit -> ARR:*
                let (target, flag) = if a.len() >= 3 {
                    (self.avar(a, 1)?, 2)
                } else {
                    (Rc::new(VarRef { name: "RESULTS".into(), idx: vec![] }), 1)
                };
                if self.ai(a, flag, 0)? == 0 {
                    return s(String::new());
                }
                let (loc, base, len) = self.array_row(&target)?;
                let arr = self.arr_at(&loc).unwrap();
                for (k, key) in keys.iter().take(len).enumerate() {
                    arr.set(base + k, Value::Str(key.clone()));
                }
                self.set_result(keys.len() as i64);
                let first = if a.len() == 2 { keys.first().cloned().unwrap_or_default() } else { String::new() };
                s(first)
            }
            "MAP_TOXML" => {
                let Some(m) = self.maps.get(&map) else { return s(String::new()) };
                // upstream does not escape keys/values either
                let body: String = m.pairs().map(|(k, v)| format!("<p><k>{}</k><v>{}</v></p>", k, v)).collect();
                s(format!("<map>{}</map>", body))
            }
            _ => Ok(None),
        }
    }

    /// Write `vals` into the row of array variable `v` (capped at its length).
    fn fill_array(&mut self, v: &VarRef, vals: Vec<Value>) -> VmResult<()> {
        let (loc, base, len) = self.array_row(v)?;
        let arr = self.arr_at(&loc).unwrap();
        for (k, x) in vals.into_iter().take(len).enumerate() {
            arr.set(base + k, x);
        }
        Ok(())
    }

    /// A config item for GETCONFIG(S), by its Japanese name, English name
    /// or internal name, any case (`"LINE HEIGHT"`, `"一行の高さ"`,
    /// `"LineHeight"`), as Emuera's `ConfigData.GetItem`.
    fn config_value(&self, key: &str) -> Option<Value> {
        let k = key.trim().to_uppercase();
        let int = |v: i64| Some(Value::Int(v));
        let rep = |jp: &str| self.csv.replace.get(jp).map(|s| s.trim().to_string());
        let rep_i = |jp: &str, d: i64| rep(jp).and_then(|s| s.parse().ok()).unwrap_or(d);
        let names = |jp: &str, en: &str, code: &str| k == jp.to_uppercase() || k == en.to_uppercase() || k == code.to_uppercase();
        if names("フォントサイズ", "Font size", "FontSize") {
            int(self.con.font_size as i64)
        } else if names("一行の高さ", "Line height", "LineHeight") {
            int(self.con.line_height as i64)
        } else if names("ウィンドウ幅", "Window width", "WindowX") {
            int((self.con.width as f32 * self.con.font_size / 2.0) as i64)
        } else if names("PRINTCを並べる数", "Items per line for PRINTC", "PrintCPerLine") {
            int(3)
        } else if names("PRINTCの文字数", "Number of Item characters for PRINTC", "PrintCLength") {
            int(self.con.printc_len as i64)
        } else if names("表示するセーブデータ数", "Save data count per page", "SaveDataNos") {
            int(20)
        } else if names("オートセーブを行なう", "Make autosaves", "AutoSave") {
            int(self.save_opts.autosave as i64)
        } else if names("文字色", "Text color", "ForeColor") {
            int(self.con.default_fg.to_i64())
        } else if names("背景色", "Background color", "BackColor") {
            int(self.con.default_bg.to_i64())
        } else if names("選択中文字色", "Highlight color", "FocusColor") {
            int(self.con.focus_fg.to_i64())
        } else if names("履歴文字色", "History log color", "LogColor") {
            int(self.con.log_fg.to_i64())
        } else if names("販売アイテム数", "Max shop item storage", "MaxShopItem") {
            int(rep_i("販売アイテム数", 100))
        } else if names("COM_ABLE初期値", "Default COM_ABLE", "ComAbleDefault") {
            int(rep_i("COM_ABLE初期値", 1))
        } else if names("PBANDの初期値", "Default PBAND", "pbandDef") {
            int(rep_i("PBANDの初期値", 4))
        } else if names("RELATIONの初期値", "Default RELATION", "RelationDef") {
            int(rep_i("RELATIONの初期値", 0))
        } else if names("単位の位置", "Currency symbol position", "MoneyFirst") {
            int(rep("単位の位置").map_or(1, |s| matches!(s.to_uppercase().as_str(), "YES" | "TRUE" | "1" | "前") as i64))
        } else if names("お金の単位", "Currency symbol", "MoneyLabel") {
            Some(Value::Str(self.csv.gamebase.get("お金の単位").cloned().or_else(|| rep("お金の単位")).unwrap_or_else(|| "$".into())))
        } else if names("フォント名", "Font name", "FontName") {
            Some(Value::Str(if self.con.font_name.is_empty() { "ＭＳ ゴシック".into() } else { self.con.font_name.clone() }))
        } else if names("描画インターフェース", "Drawing interface", "TextDrawingMode") {
            // Emuera's default; games check it isn't WINAPI
            Some(Value::Str("TEXTRENDERER".into()))
        } else if names("DRAWLINE文字", "DRAWLINE character", "DrawLineString") {
            Some(Value::Str(self.con.drawline_str.clone()))
        } else {
            None
        }
    }

    /// ARRAYMSORT key, arrays… (and ARRAYMSORTEX key, names, ascending,
    /// length): sort the key array's leading elements (up to the first 0 /
    /// empty one, or `length`) and put every listed array's first-dimension
    /// entries in the same order. 1 done, 0 if an array is too short.
    fn array_msort(&mut self, name: &str, a: &[Expr]) -> VmResult<Option<Value>> {
        let ex = name == "ARRAYMSORTEX";
        let whole = |v: &VarRef| VarRef { name: v.name.clone(), idx: vec![] };
        let by_name = |vm: &mut Self, n: &str| -> VmResult<VarRef> {
            match crate::parser::upper_expr(crate::expr::parse_expr(n, &vm.prog.defines).map_err(|e| Ctrl::Error(e, false))?) {
                Expr::Var(v) => Ok(whole(&v)),
                _ => err(format!("{}: '{}' is not a variable", name, n)),
            }
        };
        let key = match a.first() {
            Some(Expr::Var(v)) => whole(v),
            _ if ex => {
                let n = self.as_(a, 0, "")?;
                by_name(self, &n)?
            }
            _ => return err(format!("{}: the first argument must be an array", name)),
        };
        let asc = !ex || self.ai(a, 2, 1)? != 0;
        let fixed = if ex { self.ai(a, 3, -1)? } else { -1 };
        if fixed == 0 {
            return Ok(Some(Value::Int(0)));
        }
        let kp = self.locate(&key)?;
        let karr = self.arr_at(&kp.loc).ok_or_else(|| Ctrl::Error(format!("{}: {} is read-only", name, key.name), false))?.clone();
        let len = if fixed > 0 { (fixed as usize).min(karr.len()) } else { karr.len() };
        let mut keys: Vec<(Value, usize)> = Vec::new();
        for i in 0..len {
            let v = karr.get(i);
            let empty = match &v {
                Value::Int(x) => *x == 0,
                Value::Str(s) => s.is_empty(),
            };
            if empty && fixed == -1 {
                if ex && v.is_str() {
                    return Ok(Some(Value::Int(0)));
                }
                break;
            }
            keys.push((v, i));
        }
        keys.sort_by(|x, y| {
            let o = match (&x.0, &y.0) {
                (Value::Int(p), Value::Int(q)) => p.cmp(q),
                (p, q) => p.as_string().cmp(&q.as_string()),
            };
            if asc { o } else { o.reverse() }
        });
        let order: Vec<usize> = keys.into_iter().map(|(_, i)| i).collect();
        let targets: Vec<VarRef> = if ex {
            let names = match a.get(1) {
                Some(Expr::Var(v)) => {
                    let p = self.locate(&whole(v))?;
                    let arr = self.arr_at(&p.loc).ok_or_else(|| Ctrl::Error(format!("{}: bad name list", name), false))?;
                    (0..arr.len()).map(|i| arr.get(i).as_string()).filter(|s| !s.is_empty()).collect::<Vec<_>>()
                }
                _ => return err(format!("{}: the second argument must be a string array", name)),
            };
            let mut t = Vec::new();
            for n in names {
                t.push(by_name(self, &n)?);
            }
            t
        } else {
            a.iter()
                .map(|e| match e {
                    Expr::Var(v) => Ok(whole(v)),
                    _ => err(format!("{}: arguments must be arrays", name)),
                })
                .collect::<VmResult<_>>()?
        };
        for t in targets {
            let p = self.locate(&t)?;
            let Some(arr) = self.arr_at(&p.loc) else { return err(format!("{}: {} is read-only", name, t.name)) };
            let first = arr.dims.first().copied().unwrap_or(1);
            let chunk: usize = arr.dims.iter().skip(1).product::<usize>().max(1);
            if first < order.len() {
                return Ok(Some(Value::Int(0)));
            }
            let old = arr.clone();
            for (i, &src) in order.iter().enumerate() {
                for k in 0..chunk {
                    arr.set(i * chunk + k, old.get(src * chunk + k));
                }
            }
        }
        Ok(Some(Value::Int(1)))
    }

    /// Emuera's graphics buffers (`G*`) and SPRITECREATE: drawn in memory
    /// and shown through sprites, so composed portraits work. Return values
    /// as in Emuera: 1 done, 0 not (missing buffer / sprite, already
    /// exists…).
    fn graphics_fn(&mut self, name: &str, a: &[Expr]) -> VmResult<Option<Value>> {
        use crate::resources::{argb, draw_image, Graphic};
        use image::RgbaImage;
        let i = |v: i64| Ok(Some(Value::Int(v)));
        let n = a.iter().rposition(|e| !matches!(e, Expr::Empty)).map_or(0, |p| p + 1);
        let a = &a[..n];
        let ints = |vm: &mut Self, from: usize, k: usize| -> VmResult<Vec<i32>> {
            (from..from + k).map(|j| vm.ai(a, j, 0).map(|v| v.clamp(i32::MIN as i64, i32::MAX as i64) as i32)).collect()
        };
        let color = |c: i64| -> VmResult<[u8; 4]> {
            if !(0..=0xFFFF_FFFF).contains(&c) {
                return err(format!("{}: bad colour {}", name, c));
            }
            Ok(argb(c))
        };
        const MAX: i64 = 8192;
        match name {
            "SPRITECREATE" => {
                let sname = self.as_(a, 0, "")?;
                let gid = self.ai(a, 1, 0)?;
                let rect = if a.len() >= 6 {
                    let r = ints(self, 2, 4)?;
                    Some((r[0], r[1], r[2], r[3]))
                } else {
                    None
                };
                return i(self.res.create_g_sprite(&sname, gid, rect) as i64);
            }
            "SPRITEDISPOSE" => {
                let sname = self.as_(a, 0, "")?;
                return i(self.res.dispose_sprite(&sname) as i64);
            }
            _ => {}
        }
        let gid = self.ai(a, 0, 0)?;
        let r = match name {
            "GCREATE" => {
                if self.res.graphics.contains_key(&gid) {
                    return i(0);
                }
                let (w, h) = (self.ai(a, 1, 0)?, self.ai(a, 2, 0)?);
                if !(1..=MAX).contains(&w) || !(1..=MAX).contains(&h) {
                    return err(format!("GCREATE: bad size {}x{}", w, h));
                }
                self.res.graphics.insert(gid, Graphic::new(RgbaImage::new(w as u32, h as u32)));
                1
            }
            "GCREATEFROMFILE" => {
                if self.res.graphics.contains_key(&gid) {
                    return i(0);
                }
                let f = self.as_(a, 1, "")?;
                let rel = self.ai(a, 2, 0)? != 0;
                let p = std::path::PathBuf::from(f.replace('\\', "/"));
                let p = if p.is_absolute() || rel { p } else { self.game_dir.join("resources").join(&p) };
                match image::open(&p) {
                    Ok(img) if img.width() as i64 <= MAX && img.height() as i64 <= MAX => {
                        self.res.graphics.insert(gid, Graphic::new(img.to_rgba8()));
                        1
                    }
                    _ => 0,
                }
            }
            "GLOAD" => {
                if self.res.graphics.contains_key(&gid) {
                    return i(0);
                }
                let k = self.ai(a, 1, 0)?;
                match image::open(self.save_dir().join(format!("img{:04}.png", k))) {
                    Ok(img) => {
                        self.res.graphics.insert(gid, Graphic::new(img.to_rgba8()));
                        1
                    }
                    Err(_) => 0,
                }
            }
            _ => {
                if !self.res.graphics.contains_key(&gid) {
                    return i(0);
                }
                match name {
                    "GDISPOSE" => {
                        self.res.graphics.remove(&gid);
                        1
                    }
                    "GSAVE" => {
                        let k = self.ai(a, 1, 0)?;
                        let dir = self.save_dir();
                        let _ = std::fs::create_dir_all(&dir);
                        self.res.graphics[&gid].img.save(dir.join(format!("img{:04}.png", k))).is_ok() as i64
                    }
                    "GSETBRUSH" => {
                        let c = color(self.ai(a, 1, 0)?)?;
                        let g = self.res.graphics.get_mut(&gid).unwrap();
                        g.brush = c;
                        g.brush_set = true;
                        1
                    }
                    "GCLEAR" => {
                        let c = color(self.ai(a, 1, 0)?)?;
                        let rect = if a.len() >= 6 { Some(ints(self, 2, 4)?) } else { None };
                        let g = self.res.graphics.get_mut(&gid).unwrap();
                        let (w, h) = (g.img.width() as i32, g.img.height() as i32);
                        let (x0, y0, x1, y1) = match rect {
                            Some(r) => (r[0].max(0), r[1].max(0), (r[0] + r[2]).min(w), (r[1] + r[3]).min(h)),
                            None => (0, 0, w, h),
                        };
                        for y in y0..y1 {
                            for x in x0..x1 {
                                g.img.put_pixel(x as u32, y as u32, image::Rgba(c));
                            }
                        }
                        1
                    }
                    "GFILLRECTANGLE" => {
                        let r = ints(self, 1, 4)?;
                        let g = self.res.graphics.get_mut(&gid).unwrap();
                        let mut tile = RgbaImage::new(r[2].unsigned_abs().max(1), r[3].unsigned_abs().max(1));
                        for p in tile.pixels_mut() {
                            *p = image::Rgba(g.brush);
                        }
                        draw_image(&mut g.img, &tile, (r[0], r[1], r[2], r[3]), (0, 0, tile.width() as i32, tile.height() as i32), None);
                        1
                    }
                    "GSETCOLOR" => {
                        let c = color(self.ai(a, 1, 0)?)?;
                        let (x, y) = (self.ai(a, 2, 0)?, self.ai(a, 3, 0)?);
                        let g = self.res.graphics.get_mut(&gid).unwrap();
                        if (0..g.img.width() as i64).contains(&x) && (0..g.img.height() as i64).contains(&y) {
                            g.img.put_pixel(x as u32, y as u32, image::Rgba(c));
                            1
                        } else {
                            0
                        }
                    }
                    "GDRAWSPRITE" => {
                        // GDRAWSPRITE id, sprite[, x, y[, w, h[, colour matrix]]]
                        let sname = self.as_(a, 1, "")?;
                        let Some(img) = self.res.sprite(&sname) else { return i(0) };
                        let (sw, sh) = (img.width() as i32, img.height() as i32);
                        let dest = match a.len() {
                            0..=2 => (0, 0, sw, sh),
                            3 | 4 => {
                                let p = ints(self, 2, 2)?;
                                (p[0], p[1], sw, sh)
                            }
                            _ => {
                                let r = ints(self, 2, 4)?;
                                (r[0], r[1], r[2], r[3])
                            }
                        };
                        let cm = if a.len() >= 7 { Some(self.color_matrix_arg(&a[6])?) } else { None };
                        let g = self.res.graphics.get_mut(&gid).unwrap();
                        draw_image(&mut g.img, &img, dest, (0, 0, sw, sh), cm.as_ref());
                        1
                    }
                    "GDRAWG" => {
                        // GDRAWG dest, src, dx, dy, dw, dh, sx, sy, sw, sh[, colour matrix]
                        let src = self.ai(a, 1, 0)?;
                        let r = ints(self, 2, 8)?;
                        let cm = if a.len() >= 11 { Some(self.color_matrix_arg(&a[10])?) } else { None };
                        let Some(s) = self.res.graphics.get(&src).map(|g| g.img.clone()) else { return i(0) };
                        let g = self.res.graphics.get_mut(&gid).unwrap();
                        draw_image(&mut g.img, &s, (r[0], r[1], r[2], r[3]), (r[4], r[5], r[6], r[7]), cm.as_ref());
                        1
                    }
                    "GSETPEN" => {
                        let c = color(self.ai(a, 1, 0)?)?;
                        let w = self.ai(a, 2, 1)?;
                        self.res.graphics.get_mut(&gid).unwrap().pen = (c, w);
                        1
                    }
                    "GSETFONT" => {
                        let f = self.as_(a, 1, "")?;
                        let sz = self.ai(a, 2, 100)?;
                        let st = self.ai(a, 3, 0)?;
                        self.res.graphics.get_mut(&gid).unwrap().font = (f, sz, st);
                        1
                    }
                    "GDASHSTYLE" => {
                        let (st, cap) = (self.ai(a, 1, 0)?, self.ai(a, 2, 0)?);
                        self.res.graphics.get_mut(&gid).unwrap().dash = (st, cap);
                        1
                    }
                    "GDRAWLINE" => {
                        let r = ints(self, 1, 4)?;
                        let g = self.res.graphics.get_mut(&gid).unwrap();
                        let (c, w, d) = (g.pen.0, g.pen.1, g.dash.0);
                        crate::resources::draw_line(&mut g.img, (r[0], r[1]), (r[2], r[3]), w, c, d);
                        1
                    }
                    "GDRAWTEXT" => {
                        // GDRAWTEXT id, text[, x, y]: in the buffer's font,
                        // filled with its brush (else the text colour)
                        let t = self.as_(a, 1, "")?;
                        let p = if a.len() >= 4 { ints(self, 2, 2)? } else { vec![0, 0] };
                        let fg = self.con.default_fg;
                        let g = self.res.graphics.get_mut(&gid).unwrap();
                        let c = if g.brush_set { g.brush } else { [fg.0, fg.1, fg.2, 255] };
                        let (f, sz) = (g.font.0.clone(), g.font.1.max(1) as f32);
                        crate::resources::draw_text(&mut g.img, &t, (p[0], p[1]), &f, sz, c);
                        let (w, h) = crate::resources::text_size(&t, &f, sz);
                        self.global_arr("RESULT").set_i(1, w);
                        self.global_arr("RESULT").set_i(2, h);
                        1
                    }
                    "GDRAWGWITHROTATE" => {
                        // GDRAWGWITHROTATE dest, src, angle[, x, y]: rotate
                        // about (x, y) of src (default its middle)
                        let src = self.ai(a, 1, 0)?;
                        let ang = self.ai(a, 2, 0)? as f32;
                        let Some(s) = self.res.graphics.get(&src).map(|g| g.img.clone()) else { return i(0) };
                        let p = if a.len() >= 5 { ints(self, 3, 2)? } else { vec![s.width() as i32 / 2, s.height() as i32 / 2] };
                        let g = self.res.graphics.get_mut(&gid).unwrap();
                        crate::resources::draw_rotated(&mut g.img, &s, ang, (p[0] as f32, p[1] as f32));
                        1
                    }
                    "GDRAWGWITHMASK" => {
                        // the mask's blue channel is the source's opacity
                        let (src, mask) = (self.ai(a, 1, 0)?, self.ai(a, 2, 0)?);
                        let p = ints(self, 3, 2)?;
                        let (Some(s), Some(m)) = (self.res.graphics.get(&src).map(|g| g.img.clone()), self.res.graphics.get(&mask).map(|g| g.img.clone())) else {
                            return i(0);
                        };
                        let g = self.res.graphics.get_mut(&gid).unwrap();
                        if s.dimensions() != m.dimensions() || p[0] < 0 || p[1] < 0 || p[0] as u32 + s.width() > g.img.width() || p[1] as u32 + s.height() > g.img.height() {
                            return i(0);
                        }
                        for (x, y, sp) in s.enumerate_pixels() {
                            let k = m.get_pixel(x, y).0[2] as u32 + 1;
                            let d = g.img.get_pixel_mut(p[0] as u32 + x, p[1] as u32 + y);
                            if k == 256 {
                                *d = *sp;
                            } else if k > 1 {
                                for c in 0..4 {
                                    d.0[c] = ((sp.0[c] as u32 * k + d.0[c] as u32 * (256 - k)) >> 8) as u8;
                                }
                            }
                        }
                        1
                    }
                    _ => 0,
                }
            }
        };
        if r != 0 && !matches!(name, "GSAVE" | "GSETBRUSH" | "GSETPEN" | "GSETFONT" | "GDASHSTYLE") {
            self.res.changed();
        }
        i(r)
    }

    /// A colour matrix argument: 5x5 integers (/256) starting at the given
    /// element of a 2D or 3D integer array (`CM:0:0`).
    fn color_matrix_arg(&mut self, e: &Expr) -> VmResult<[[f32; 5]; 5]> {
        let Expr::Var(v) = e else { return err("colour matrix: not an array variable") };
        let p = self.locate(v)?;
        let arr = self.arr_at(&p.loc).ok_or_else(|| Ctrl::Error("colour matrix: not an array".into(), false))?;
        if arr.dims.len() < 2 || arr.is_str() {
            return err(format!("colour matrix: {} must be a 2D or 3D integer array", v.name));
        }
        let cols = *arr.dims.last().unwrap();
        let rows = arr.dims[arr.dims.len() - 2];
        let (r0, c0) = ((p.flat / cols) % rows, p.flat % cols);
        if r0 + 5 > rows || c0 + 5 > cols {
            return err(format!("colour matrix: {} has no 5x5 block at ({}, {})", v.name, r0, c0));
        }
        let mut m = [[0f32; 5]; 5];
        for (x, row) in m.iter_mut().enumerate() {
            for (y, cell) in row.iter_mut().enumerate() {
                *cell = arr.get_i(p.flat + x * cols + y) as f32 / 256.0;
            }
        }
        Ok(m)
    }

    /// Emuera-EM's XML_* functions (return codes as in emuera.em). The first
    /// argument is either XML text, edited copies of which go back into it
    /// when it is a variable, or the key of a document XML_DOCUMENT holds
    /// (an integer, or any string for the *_BYNAME forms). Most return the
    /// number of nodes the XPath matched; -1 for an unknown key.
    fn xml_fn(&mut self, name: &str, a: &[Expr]) -> VmResult<Option<Value>> {
        use crate::xml;
        let i = |v: i64| Ok(Some(Value::Int(v)));
        let n = a.iter().rposition(|e| !matches!(e, Expr::Empty)).map_or(0, |p| p + 1);
        let a = &a[..n];
        let (base, byname) = match name.strip_suffix("_BYNAME") {
            Some(b) => (b, true),
            None => (name, false),
        };
        let xerr = |e: String| Ctrl::Error(format!("{}: {}", name, e), false);
        let first = self.arg(a, 0)?.unwrap_or(Value::Int(0));
        let key = first.as_string();
        match base {
            "XML_DOCUMENT" => {
                if self.xml_docs.contains_key(&key) {
                    return i(0);
                }
                let src = self.as_(a, 1, "")?;
                let d = xml::load_doc(&src).map_err(xerr)?;
                self.xml_docs.insert(key, d);
                return i(1);
            }
            "XML_RELEASE" => return i(self.xml_docs.remove(&key).is_some() as i64),
            "XML_EXIST" => return i(self.xml_docs.contains_key(&key) as i64),
            "XML_TOSTR" => return Ok(Some(Value::Str(self.xml_docs.get(&key).map(xml::doc_to_string).unwrap_or_default()))),
            _ => {}
        }
        let held = matches!(first, Value::Int(_)) || byname || (base == "XML_REPLACE" && a.len() == 2);
        if held && base == "XML_REPLACE" && a.len() == 2 {
            // XML_REPLACE key, xml: swap the whole document
            if !self.xml_docs.contains_key(&key) {
                return i(-1);
            }
            let d = xml::load_doc(&self.as_(a, 1, "")?).map_err(xerr)?;
            self.xml_docs.insert(key, d);
            return i(1);
        }
        let mut doc = if held {
            match self.xml_docs.remove(&key) {
                Some(d) => d,
                None => return i(-1),
            }
        } else {
            xml::load_doc(&key).map_err(xerr)?
        };
        let r = self.xml_op(base, a, &mut doc);
        let changed = matches!(r, Ok((_, true)));
        if held {
            self.xml_docs.insert(key, doc);
        } else if changed {
            if let Some(Expr::Var(v)) = a.first() {
                self.set_var(v, Value::Str(xml::doc_to_string(&doc)))?;
            }
        }
        i(r?.0)
    }

    /// The node operations of `xml_fn`: (return value, document changed).
    fn xml_op(&mut self, base: &str, a: &[Expr], doc: &mut crate::xml::Element) -> VmResult<(i64, bool)> {
        use crate::xml;
        let xerr = |e: String| Ctrl::Error(format!("{}: {}", base, e), false);
        let path = self.as_(a, 1, "")?;
        let sels = xml::select(doc, &path).map_err(xerr)?;
        let count = sels.len() as i64;
        // an XPath matching several nodes changes them only when asked to
        // (the "all nodes" flag); one node is changed always
        let targets = |all: bool| -> Vec<xml::Sel> {
            match sels.len() {
                1 => sels.clone(),
                _ if all => sels.iter().rev().cloned().collect(),
                _ => vec![],
            }
        };
        let new_node = |src: &str| -> VmResult<xml::Node> {
            xml::parse_fragment(src)
                .map_err(xerr)?
                .into_iter()
                .find(|n| matches!(n, xml::Node::Elem(_)))
                .ok_or_else(|| xerr(format!("no element in '{}'", src)))
        };
        if sels.is_empty() && base != "XML_GET" {
            return Ok((0, false));
        }
        match base {
            "XML_GET" => {
                let style = self.ai(a, 3, 0)?;
                let vals: Vec<Value> = sels.iter().map(|s| Value::Str(xml::node_output(doc, s, style))).collect();
                match a.get(2) {
                    Some(Expr::Var(v)) if self.var_is_str(v)? => self.fill_array(v, vals)?,
                    Some(_) if self.ai(a, 2, 0)? != 0 => {
                        self.fill_array(&VarRef { name: "RESULTS".into(), idx: vec![] }, vals)?;
                    }
                    _ => {}
                }
                Ok((count, false))
            }
            "XML_SET" => {
                let val = self.as_(a, 2, "")?;
                let all = self.ai(a, 3, 0)? != 0;
                let style = self.ai(a, 4, 0)?.clamp(0, 2);
                for s in targets(all) {
                    xml::set_node(doc, &s, &val, style).map_err(xerr)?;
                }
                Ok((count, true))
            }
            "XML_ADDNODE" => {
                let child = new_node(&self.as_(a, 2, "")?)?;
                let method = Some(self.ai(a, 3, 0)?).filter(|m| (0..=2).contains(m)).unwrap_or(0);
                let all = self.ai(a, 4, 0)? != 0;
                if sels.len() == 1 && !xml::insert_node(doc, &sels[0], child.clone(), method) && method > 0 {
                    return Ok((0, false));
                }
                if sels.len() > 1 {
                    for s in targets(all) {
                        xml::insert_node(doc, &s, child.clone(), method);
                    }
                }
                Ok((count, true))
            }
            "XML_ADDATTRIBUTE" => {
                let an = self.as_(a, 2, "")?;
                let av = self.as_(a, 3, "")?;
                let method = Some(self.ai(a, 4, 0)?).filter(|m| (0..=2).contains(m)).unwrap_or(0);
                let all = self.ai(a, 5, 0)? != 0;
                if sels.len() == 1 && !xml::insert_attr(doc, &sels[0], &an, &av, method) && method > 0 {
                    return Ok((0, false));
                }
                if sels.len() > 1 {
                    for s in targets(all) {
                        xml::insert_attr(doc, &s, &an, &av, method);
                    }
                }
                Ok((count, true))
            }
            "XML_REMOVENODE" | "XML_REMOVEATTRIBUTE" => {
                let attrs = base == "XML_REMOVEATTRIBUTE";
                let all = self.ai(a, 2, 0)? != 0;
                if sels.len() == 1 && !xml::remove(doc, &sels[0], attrs) {
                    return Ok((0, false));
                }
                if sels.len() > 1 {
                    for s in targets(all) {
                        xml::remove(doc, &s, attrs);
                    }
                }
                Ok((count, true))
            }
            "XML_REPLACE" => {
                let node = new_node(&self.as_(a, 2, "")?)?;
                let all = self.ai(a, 3, 0)? != 0;
                if sels.len() == 1 && !xml::replace_node(doc, &sels[0], node.clone()) {
                    return Ok((0, false));
                }
                if sels.len() > 1 {
                    for s in targets(all) {
                        xml::replace_node(doc, &s, node.clone());
                    }
                }
                Ok((count, true))
            }
            _ => Ok((0, false)),
        }
    }

    /// Emuera-EM data tables; return codes follow emuera.em
    /// (-1: no such table, -2/-3: bad row / column, 0: nothing done).
    fn dt_fn(&mut self, name: &str, a: &[Expr]) -> VmResult<Option<Value>> {
        use super::datatable::{ColType, DataTable};
        let i = |v: i64| Ok(Some(Value::Int(v)));
        let s = |v: String| Ok(Some(Value::Str(v)));
        // trailing empty arguments (`DT_ROW_ADD t, "a", 1,`) are not values
        let n = a.iter().rposition(|e| !matches!(e, Expr::Empty)).map_or(0, |p| p + 1);
        let a = &a[..n];
        let key = self.as_(a, 0, "")?;
        let present = |k: usize| a.get(k).is_some_and(|e| !matches!(e, Expr::Empty));
        match name {
            "DT_CREATE" => {
                if self.dts.contains_key(&key) {
                    return i(0);
                }
                self.dts.insert(key.clone(), DataTable::new(&key));
                i(1)
            }
            "DT_EXIST" => i(self.dts.contains_key(&key) as i64),
            "DT_RELEASE" => {
                self.dts.remove(&key);
                i(1)
            }
            "DT_CLEAR" | "DT_NOCASE" => {
                let nocase = self.ai(a, 1, 0)? != 0;
                let Some(dt) = self.dts.get_mut(&key) else { return i(-1) };
                if name == "DT_CLEAR" {
                    dt.rows.clear();
                } else {
                    dt.case_sensitive = !nocase;
                }
                i(1)
            }
            "DT_ROW_LENGTH" | "DT_COLUMN_LENGTH" => match self.dts.get(&key) {
                Some(dt) => i(if name == "DT_ROW_LENGTH" { dt.rows.len() } else { dt.columns.len() } as i64),
                None => i(-1),
            },
            "DT_COLUMN_ADD" => {
                let cname = self.as_(a, 1, "")?;
                let ty = match self.arg(a, 2)? {
                    None => ColType::Str,
                    Some(Value::Str(t)) => ColType::from_name(&t).ok_or_else(|| Ctrl::Error(format!("{}: unsupported column type '{}'", name, t), false))?,
                    Some(Value::Int(t)) => ColType::from_int(t).ok_or_else(|| Ctrl::Error(format!("{}: unsupported column type {}", name, t), false))?,
                };
                let nullable = if present(3) { self.ai(a, 3, 1)? != 0 } else { true };
                let Some(dt) = self.dts.get_mut(&key) else { return i(-1) };
                if dt.col(&cname).is_some() {
                    return i(0);
                }
                dt.add_column(&cname, ty, nullable);
                i(1)
            }
            "DT_COLUMN_EXIST" | "DT_COLUMN_REMOVE" => {
                let cname = self.as_(a, 1, "")?;
                let Some(dt) = self.dts.get_mut(&key) else { return i(-1) };
                let c = dt.col(&cname);
                if name == "DT_COLUMN_EXIST" {
                    return i(c.map_or(0, |c| dt.columns[c].ty.to_int()));
                }
                match c {
                    Some(c) if c != 0 => {
                        dt.remove_column(c);
                        i(1)
                    }
                    _ => i(0),
                }
            }
            "DT_COLUMN_NAMES" => {
                let Some(names) = self.dts.get(&key).map(|dt| dt.columns.iter().map(|c| Value::Str(c.name.clone())).collect::<Vec<_>>()) else {
                    return i(-1);
                };
                let target = if present(1) { self.avar(a, 1)? } else { Rc::new(VarRef { name: "RESULTS".into(), idx: vec![] }) };
                let n = names.len() as i64;
                self.fill_array(&target, names)?;
                i(n)
            }
            "DT_ROW_ADD" | "DT_ROW_SET" => {
                let b = if name == "DT_ROW_ADD" { 0 } else { 1 };
                if !self.dts.contains_key(&key) {
                    return i(-1);
                }
                let mut row = if b == 0 {
                    self.dts[&key].new_row()
                } else {
                    let id = self.ai(a, 1, 0)?;
                    let dt = &self.dts[&key];
                    match dt.find_row(id) {
                        Some(r) => dt.rows[r].clone(),
                        None => return i(-2),
                    }
                };
                // (name, value) pairs, or NAMES_ARRAY, VALUES_ARRAY, count
                let mut sets: Vec<(String, Option<Value>)> = Vec::new();
                if a.len() == b + 4 {
                    let nv = self.avar(a, b + 1)?;
                    let vv = self.avar(a, b + 2)?;
                    let count = self.ai(a, b + 3, 0)?.max(0) as usize;
                    let (_, _, names) = self.arr_vals(&nv, 0, None)?;
                    let (_, _, vals) = self.arr_vals(&vv, 0, None)?;
                    for (nm, v) in names.into_iter().zip(vals).take(count) {
                        sets.push((nm.into_string(), Some(v)));
                    }
                } else {
                    let mut pos = b + 1;
                    while pos < a.len() {
                        let nm = self.as_(a, pos, "")?;
                        let v = if present(pos + 1) { Some(self.eval(&a[pos + 1])?) } else { None };
                        sets.push((nm, v));
                        pos += 2;
                    }
                }
                let dt = &self.dts[&key];
                let count = sets.len() as i64;
                for (nm, v) in sets {
                    if nm == "id" {
                        return err(format!("{}: the id column of {} can't be edited", name, key));
                    }
                    let c = dt.col(&nm).ok_or_else(|| Ctrl::Error(format!("{}: {} has no column '{}'", name, key, nm), false))?;
                    row[c] = dt.check_value(c, v).map_err(|_| Ctrl::Error(format!("{}: wrong value type for {}.{}", name, key, nm), false))?;
                }
                let dt = self.dts.get_mut(&key).unwrap();
                if b == 0 {
                    let id = row[0].as_ref().map_or(0, |v| v.as_int());
                    dt.rows.push(row);
                    return i(id);
                }
                let id = row[0].as_ref().map_or(0, |v| v.as_int());
                let r = dt.find_row(id).unwrap();
                dt.rows[r] = row;
                i(count)
            }
            "DT_ROW_REMOVE" => {
                let ids: Vec<i64> = if a.len() >= 3 {
                    let v = self.avar(a, 1)?;
                    let count = self.ai(a, 2, 0)?;
                    if count <= 0 {
                        return i(0);
                    }
                    let (_, _, vals) = self.arr_vals(&v, 0, Some(count))?;
                    vals.iter().map(|v| v.as_int()).collect()
                } else {
                    vec![self.ai(a, 1, 0)?]
                };
                let Some(dt) = self.dts.get_mut(&key) else { return i(-1) };
                let before = dt.rows.len();
                dt.rows.retain(|r| !ids.contains(&r[0].as_ref().map_or(i64::MIN, |v| v.as_int())));
                i((before - dt.rows.len()) as i64)
            }
            "DT_CELL_GET" | "DT_CELL_ISNULL" | "DT_CELL_GETS" | "DT_CELL_SET" => {
                let set = name == "DT_CELL_SET";
                let idx = self.ai(a, 1, 0)?;
                let cname = self.as_(a, 2, "")?;
                let as_id = if set { self.ai(a, 4, 0)? != 0 } else { self.ai(a, 3, 0)? != 0 };
                let v = if set && present(3) { Some(self.eval(&a[3])?) } else { None };
                let Some(dt) = self.dts.get_mut(&key) else {
                    return match name {
                        "DT_CELL_GETS" => s(String::new()),
                        "DT_CELL_GET" => i(0),
                        _ => i(-1),
                    };
                };
                let r = if as_id { dt.find_row(idx) } else { usize::try_from(idx).ok().filter(|&r| r < dt.rows.len()) };
                let c = dt.col(&cname);
                let (Some(r), Some(c)) = (r, c) else {
                    return match name {
                        "DT_CELL_GETS" => s(String::new()),
                        "DT_CELL_GET" => i(0),
                        "DT_CELL_ISNULL" => i(-2),
                        _ => i(if cname.eq_ignore_ascii_case("id") { 0 } else { -3 }),
                    };
                };
                let cell = &dt.rows[r][c];
                match name {
                    "DT_CELL_GET" => i(cell.as_ref().map_or(0, |v| v.as_int())),
                    "DT_CELL_GETS" => s(cell.as_ref().map(|v| v.as_string()).unwrap_or_default()),
                    "DT_CELL_ISNULL" => i(cell.is_none() as i64),
                    _ => {
                        if c == 0 {
                            return i(0);
                        }
                        match dt.check_value(c, v) {
                            Ok(v) => {
                                dt.rows[r][c] = v;
                                i(1)
                            }
                            Err(()) => i(-2),
                        }
                    }
                }
            }
            "DT_SELECT" => {
                let filter = if present(1) { Some(self.as_(a, 1, "")?) } else { None };
                let sort = if present(2) { Some(self.as_(a, 2, "")?) } else { None };
                let Some(dt) = self.dts.get(&key) else { return i(-1) };
                let ids = dt.select(filter.as_deref(), sort.as_deref()).map_err(|e| Ctrl::Error(format!("DT_SELECT {}: {}", key, e), false))?;
                let n = ids.len() as i64;
                let vals: Vec<Value> = ids.into_iter().map(Value::Int).collect();
                if a.len() == 4 {
                    let target = self.avar(a, 3)?;
                    self.fill_array(&target, vals)?;
                } else {
                    // RESULT:0 = count, RESULT:1.. = ids
                    let arr = self.global_arr("RESULT");
                    arr.set(0, Value::Int(n));
                    for (k, v) in vals.into_iter().enumerate() {
                        arr.set(k + 1, v);
                    }
                }
                i(n)
            }
            "DT_TOXML" => {
                let Some(dt) = self.dts.get(&key) else { return s(String::new()) };
                let (schema, data) = (dt.to_xml_schema(), dt.to_xml());
                if present(1) {
                    let v = self.avar(a, 1)?;
                    self.set_var(&v, Value::Str(schema))?;
                } else {
                    self.global_arr("RESULTS").set(1, Value::Str(schema));
                }
                s(data)
            }
            "DT_FROMXML" => {
                let schema = self.as_(a, 1, "")?;
                let data = self.as_(a, 2, "")?;
                match DataTable::from_xml(&key, &schema, &data) {
                    Ok(dt) => {
                        self.dts.insert(key, dt);
                        i(1)
                    }
                    Err(_) => i(0),
                }
            }
            _ => Ok(None),
        }
    }

    /// LOADTEXT / SAVETEXT target: `sav/txtNN.txt` for a number, else a
    /// path inside the game directory with an allowed extension.
    fn text_path(&self, target: &Value, saving: bool) -> Option<std::path::PathBuf> {
        match target {
            Value::Int(n) => (0..=i32::MAX as i64).contains(n).then(|| self.save_dir().join(format!("txt{:02}.txt", n))),
            Value::Str(p) => {
                let rel = std::path::Path::new(p);
                if rel.is_absolute() || rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
                    return None;
                }
                let ext = rel.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
                let mut path = self.game_dir.join(rel);
                if !self.text_exts.contains(&ext) {
                    if !saving {
                        return None;
                    }
                    path.set_extension("txt");
                }
                Some(path)
            }
        }
    }

    // ============================================================== functions
    /// Evaluate a built-in function; `Ok(None)` if `name` isn't one.
    pub fn builtin_fn(&mut self, name: &str, a: &[Expr]) -> VmResult<Option<Value>> {
        let i = |v: i64| Ok(Some(Value::Int(v)));
        let s = |v: String| Ok(Some(Value::Str(v)));
        if name.starts_with("MAP_") {
            return self.map_fn(name, a);
        }
        if name.starts_with("DT_") {
            return self.dt_fn(name, a);
        }
        if name.starts_with("XML_") {
            return self.xml_fn(name, a);
        }
        match name {
            "__FORMS" => {
                let src = self.as_(a, 0, "")?;
                let f = parse_form(&src, &self.prog.defines).map_err(|e| Ctrl::Error(e, false))?;
                let f = crate::parser::upper_form(f);
                let r = self.eval_form(&f)?;
                s(r)
            }
            "ABS" => i(self.ai(a, 0, 0)?.abs()),
            "SIGN" => i(self.ai(a, 0, 0)?.signum()),
            "MAX" | "MIN" => {
                let mut best: Option<i64> = None;
                for k in 0..a.len() {
                    let v = self.ai(a, k, 0)?;
                    best = Some(match best {
                        None => v,
                        Some(b) => {
                            if name == "MAX" {
                                b.max(v)
                            } else {
                                b.min(v)
                            }
                        }
                    });
                }
                i(best.unwrap_or(0))
            }
            "LIMIT" => {
                let v = self.ai(a, 0, 0)?;
                let lo = self.ai(a, 1, i64::MIN)?;
                let hi = self.ai(a, 2, i64::MAX)?;
                i(v.max(lo).min(hi))
            }
            "INRANGE" => {
                let v = self.ai(a, 0, 0)?;
                let lo = self.ai(a, 1, 0)?;
                let hi = self.ai(a, 2, 0)?;
                i((v >= lo && v <= hi) as i64)
            }
            "POWER" => {
                if a.len() == 3 {
                    // POWER var, x, y (statement form)
                    let v = self.avar(a, 0)?;
                    let x = self.ai(a, 1, 0)?;
                    let y = self.ai(a, 2, 0)?;
                    let r = (x as f64).powf(y as f64) as i64;
                    self.set_var(&v, Value::Int(r))?;
                    return Ok(Some(Value::Int(r)));
                }
                let x = self.ai(a, 0, 0)?;
                let y = self.ai(a, 1, 0)?;
                i((x as f64).powf(y as f64) as i64)
            }
            "SQRT" => i((self.ai(a, 0, 0)? as f64).sqrt() as i64),
            "CBRT" => i((self.ai(a, 0, 0)? as f64).cbrt() as i64),
            "LOG" => i((self.ai(a, 0, 1)? as f64).ln() as i64),
            "LOG10" => i((self.ai(a, 0, 1)? as f64).log10() as i64),
            "EXPONENT" => i((self.ai(a, 0, 0)? as f64).exp() as i64),
            "RAND" => {
                if a.len() >= 2 {
                    let lo = self.ai(a, 0, 0)?;
                    let hi = self.ai(a, 1, 0)?;
                    let r = self.rand(hi - lo);
                    i(lo + r)
                } else {
                    let n = self.ai(a, 0, 0)?;
                    i(self.rand(n))
                }
            }
            "GETBIT" => {
                let x = self.ai(a, 0, 0)?;
                let b = self.ai(a, 1, 0)?;
                i(if (0..64).contains(&b) { (x >> b) & 1 } else { 0 })
            }
            "SUMARRAY" | "MAXARRAY" | "MINARRAY" => {
                let v = self.avar(a, 0)?;
                let st = self.ai(a, 1, 0)?;
                let en = self.arg(a, 2)?.map(|v| v.as_int());
                let (_, _, vals) = self.arr_vals(&v, st, en)?;
                let it = vals.iter().map(|v| v.as_int());
                i(match name {
                    "SUMARRAY" => it.sum(),
                    "MAXARRAY" => it.max().unwrap_or(0),
                    _ => it.min().unwrap_or(0),
                })
            }
            "MATCH" => {
                let v = self.avar(a, 0)?;
                let target = self.arg(a, 1)?.unwrap_or(Value::Int(0));
                let st = self.ai(a, 2, 0)?;
                let en = self.arg(a, 3)?.map(|v| v.as_int());
                let (_, _, vals) = self.arr_vals(&v, st, en)?;
                i(vals.iter().filter(|x| **x == target || (x.as_string() == target.as_string())).count() as i64)
            }
            "FINDELEMENT" | "FINDLASTELEMENT" => {
                let v = self.avar(a, 0)?;
                let target = self.arg(a, 1)?.unwrap_or(Value::Int(0));
                let st = self.ai(a, 2, 0)?;
                let en = self.arg(a, 3)?.map(|v| v.as_int());
                let exact = self.ai(a, 4, 0)? != 0;
                let (_, _, vals) = self.arr_vals(&v, st, en)?;
                let m = |x: &Value| -> bool {
                    match (x, &target) {
                        (Value::Str(a), Value::Str(b)) => {
                            if exact {
                                a == b
                            } else {
                                regex_lite_match(b, a)
                            }
                        }
                        _ => x.as_int() == target.as_int(),
                    }
                };
                let pos = if name == "FINDELEMENT" { vals.iter().position(m) } else { vals.iter().rposition(m) };
                i(pos.map(|p| p as i64 + st.max(0)).unwrap_or(-1))
            }
            "INRANGEARRAY" => {
                let v = self.avar(a, 0)?;
                let lo = self.ai(a, 1, 0)?;
                let hi = self.ai(a, 2, 0)?;
                let st = self.ai(a, 3, 0)?;
                let en = self.arg(a, 4)?.map(|v| v.as_int());
                let (_, _, vals) = self.arr_vals(&v, st, en)?;
                i(vals.iter().filter(|x| {
                    let x = x.as_int();
                    x >= lo && x < hi
                })
                .count() as i64)
            }
            "CMATCH" | "SUMCARRAY" | "MAXCARRAY" | "MINCARRAY" | "INRANGECARRAY" => {
                let v = self.avar(a, 0)?;
                let idx = match v.idx.last() {
                    Some(e) => self.eval_index_pub(e, &v.name)?.max(0) as usize,
                    None => 0,
                };
                let (target, st_k) = if name == "CMATCH" { (self.arg(a, 1)?, 2) } else if name == "INRANGECARRAY" { (None, 3) } else { (None, 1) };
                let st = self.ai(a, st_k, 0)?.max(0) as usize;
                let en = self.ai(a, st_k + 1, self.charas.len() as i64)?.max(0) as usize;
                let mut vals = Vec::new();
                for c in st..en.min(self.charas.len()) {
                    vals.push(self.chara_arr(c, &v.name).get(idx));
                }
                match name {
                    "CMATCH" => {
                        let t = target.unwrap_or(Value::Int(0));
                        i(vals.iter().filter(|x| x.as_string() == t.as_string()).count() as i64)
                    }
                    "SUMCARRAY" => i(vals.iter().map(|x| x.as_int()).sum()),
                    "MAXCARRAY" => i(vals.iter().map(|x| x.as_int()).max().unwrap_or(0)),
                    "MINCARRAY" => i(vals.iter().map(|x| x.as_int()).min().unwrap_or(0)),
                    _ => {
                        let lo = self.ai(a, 1, 0)?;
                        let hi = self.ai(a, 2, 0)?;
                        i(vals.iter().filter(|x| (lo..hi).contains(&x.as_int())).count() as i64)
                    }
                }
            }
            "GROUPMATCH" => {
                let x = self.arg(a, 0)?.unwrap_or(Value::Int(0));
                let mut n = 0;
                for k in 1..a.len() {
                    if self.arg(a, k)?.map(|v| v.as_string() == x.as_string()).unwrap_or(false) {
                        n += 1;
                    }
                }
                i(n)
            }
            "NOSAMES" | "ALLSAMES" => {
                let mut vals = Vec::new();
                for k in 0..a.len() {
                    vals.push(self.arg(a, k)?.map(|v| v.as_string()).unwrap_or_default());
                }
                let r = if name == "ALLSAMES" {
                    vals.windows(2).all(|w| w[0] == w[1])
                } else {
                    let mut set = std::collections::HashSet::new();
                    vals.iter().all(|v| set.insert(v.clone()))
                };
                i(r as i64)
            }
            "VARSIZE" => {
                let n = self.as_(a, 0, "")?.to_uppercase();
                let d = self.ai(a, 1, -1)?;
                let dims = match super::vars::system_var(&n) {
                    // character variables have a size even when no character exists
                    Some(def) if def.chara => super::sized_dims(&self.csv.varsize, &n, def),
                    _ => {
                        let vr = VarRef { name: n.into(), idx: vec![] };
                        let p = self.locate(&vr)?;
                        let arr = self.arr_at(&p.loc);
                        arr.map(|a| a.dims.clone()).unwrap_or_default()
                    }
                };
                let dims = if dims.is_empty() { vec![1] } else { dims };
                let k = if d < 0 { dims.len() - 1 } else { (d as usize).min(dims.len() - 1) };
                i(dims[k] as i64)
            }
            "STRLENS" => {
                let x = self.as_(a, 0, "")?;
                i(sjis_len(&x) as i64)
            }
            "STRLENSU" => {
                let x = self.as_(a, 0, "")?;
                i(x.chars().count() as i64)
            }
            "SUBSTRING" => {
                let x = self.as_(a, 0, "")?;
                let st = self.ai(a, 1, 0)?;
                let len = self.ai(a, 2, -1)?;
                s(substring_sjis(&x, st, len))
            }
            "SUBSTRINGU" => {
                let x = self.as_(a, 0, "")?;
                let st = self.ai(a, 1, 0)?.max(0) as usize;
                let len = self.ai(a, 2, -1)?;
                let it = x.chars().skip(st);
                s(if len < 0 { it.collect() } else { it.take(len as usize).collect() })
            }
            "STRFIND" | "STRFINDU" => {
                let x = self.as_(a, 0, "")?;
                let sub = self.as_(a, 1, "")?;
                let st = self.ai(a, 2, 0)?.max(0) as usize;
                let chars: Vec<char> = x.chars().collect();
                // convert start (sjis or char units) into char index
                let mut ci = 0;
                let mut units = 0;
                while ci < chars.len() && units < st {
                    units += if name == "STRFIND" { sjis_len(&chars[ci].to_string()) } else { 1 };
                    ci += 1;
                }
                let hay: String = chars[ci.min(chars.len())..].iter().collect();
                match hay.find(&sub) {
                    Some(bi) => {
                        let prefix = &hay[..bi];
                        let off = if name == "STRFIND" { sjis_len(prefix) } else { prefix.chars().count() };
                        i((units + off) as i64)
                    }
                    None => i(-1),
                }
            }
            "STRCOUNT" => {
                let x = self.as_(a, 0, "")?;
                let sub = self.as_(a, 1, "")?;
                i(if sub.is_empty() { 0 } else { x.matches(&sub).count() as i64 })
            }
            "REPLACE" => {
                let x = self.as_(a, 0, "")?;
                let from = self.as_(a, 1, "")?;
                let to = self.as_(a, 2, "")?;
                s(if from.is_empty() { x } else { x.replace(&from, &to) })
            }
            "TOSTR" => {
                let v = self.ai(a, 0, 0)?;
                let fmt = self.as_(a, 1, "")?;
                s(format_number(v, &fmt))
            }
            "TOINT" => {
                let x = self.as_(a, 0, "")?;
                i(emu_str_int(&x, false).unwrap_or(0))
            }
            "ISNUMERIC" => {
                let x = self.as_(a, 0, "")?;
                i(emu_str_int(&x, true).is_some() as i64)
            }
            "TOUPPER" => {
                let x = self.as_(a, 0, "")?;
                s(x.to_uppercase())
            }
            "TOLOWER" => {
                let x = self.as_(a, 0, "")?;
                s(x.to_lowercase())
            }
            "TOHALF" => {
                let x = self.as_(a, 0, "")?;
                s(to_half(&x))
            }
            "TOFULL" => {
                let x = self.as_(a, 0, "")?;
                s(to_full(&x))
            }
            "TRIM" => {
                let x = self.as_(a, 0, "")?;
                s(x.trim().to_string())
            }
            "STRREPEAT" => {
                let x = self.as_(a, 0, "")?;
                let n = self.ai(a, 1, 0)?;
                s(x.repeat(n.max(0) as usize))
            }
            "CHARATU" => {
                let x = self.as_(a, 0, "")?;
                let k = self.ai(a, 1, 0)?;
                s(x.chars().nth(k.max(0) as usize).map(|c| c.to_string()).unwrap_or_default())
            }
            "STRJOIN" => {
                let v = self.avar(a, 0)?;
                let sep = self.as_(a, 1, ",")?;
                let st = self.ai(a, 2, 0)?;
                let cnt = self.arg(a, 3)?.map(|v| v.as_int());
                let en = cnt.map(|c| st + c);
                let (_, _, vals) = self.arr_vals(&v, st, en)?;
                s(vals.iter().map(|v| v.as_string()).collect::<Vec<_>>().join(&sep))
            }
            "ESCAPE" => {
                let x = self.as_(a, 0, "")?;
                let mut o = String::new();
                for c in x.chars() {
                    if "\\*+?|{}[]()^$.#".contains(c) {
                        o.push('\\');
                    }
                    o.push(c);
                }
                s(o)
            }
            "UNICODE" => {
                let n = self.ai(a, 0, 0)?;
                s(char::from_u32(n as u32).map(|c| c.to_string()).unwrap_or_default())
            }
            "ENCODETOUNI" => {
                let x = self.as_(a, 0, "")?;
                let k = self.ai(a, 1, 0)?;
                i(x.chars().nth(k.max(0) as usize).map(|c| c as i64).unwrap_or(-1))
            }
            "CONVERT" => {
                let v = self.ai(a, 0, 0)?;
                let base = self.ai(a, 1, 10)?;
                s(match base {
                    2 => format!("{:b}", v),
                    8 => format!("{:o}", v),
                    16 => format!("{:x}", v),
                    _ => v.to_string(),
                })
            }
            "STRFORM" => {
                let x = self.as_(a, 0, "")?;
                let f = parse_form(&x, &self.prog.defines).map_err(|e| Ctrl::Error(e, false))?;
                let f = crate::parser::upper_form(f);
                let r = self.eval_form(&f)?;
                s(r)
            }
            "MONEYSTR" => {
                let v = self.ai(a, 0, 0)?;
                let fmt = self.as_(a, 1, "")?;
                let label = self.csv.gamebase.get("お金の単位").cloned().unwrap_or_else(|| "$".into());
                let n = format_number(v, &fmt);
                s(format!("{}{}", n, label))
            }
            "BARSTR" => {
                let v = self.ai(a, 0, 0)?;
                let m = self.ai(a, 1, 1)?;
                let l = self.ai(a, 2, 10)?;
                s(barstr(v, m, l))
            }
            "LINEISEMPTY" => i(self.con.line_is_empty() as i64),
            "CSVNAME" | "CSVCALLNAME" | "CSVNICKNAME" | "CSVMASTERNAME" => {
                let no = self.ai(a, 0, 0)?;
                let t = self.chara_no_template(no).map(|k| self.csv.charas[k].clone());
                s(t.map(|t| match name {
                    "CSVNAME" => t.name,
                    "CSVCALLNAME" => t.callname,
                    "CSVNICKNAME" => t.nickname,
                    _ => t.mastername,
                })
                .unwrap_or_default())
            }
            "CSVBASE" | "CSVABL" | "CSVTALENT" | "CSVEXP" | "CSVMARK" | "CSVJUEL" | "CSVCFLAG" | "CSVEQUIP" | "CSVRELATION" | "CSVEX" | "CSVCSTR" => {
                let no = self.ai(a, 0, 0)?;
                let var = &name[3..];
                let idx = match a.get(1) {
                    Some(e) => self.eval_index_pub(e, var)?,
                    None => 0,
                };
                let t = self.chara_no_template(no).map(|k| self.csv.charas[k].clone());
                if var == "CSTR" {
                    return s(t.and_then(|t| t.strs.iter().find(|(v, i, _)| v == "CSTR" && *i == idx).map(|x| x.2.clone())).unwrap_or_default());
                }
                i(t.and_then(|t| t.ints.iter().find(|(v, i, _)| v == var && *i == idx).map(|x| x.2)).unwrap_or(0))
            }
            "EXISTCSV" => {
                let no = self.ai(a, 0, 0)?;
                i(self.chara_no_template(no).is_some() as i64)
            }
            "GETCHARA" | "GETSPCHARA" => {
                let no = self.ai(a, 0, 0)?;
                for c in 0..self.charas.len() {
                    if self.chara_arr(c, "NO").get_i(0) == no {
                        return i(c as i64);
                    }
                }
                i(-1)
            }
            "FINDCHARA" | "FINDLASTCHARA" => {
                let v = self.avar(a, 0)?;
                let target = self.arg(a, 1)?.unwrap_or(Value::Int(0));
                let st = self.ai(a, 2, 0)?.max(0) as usize;
                let en = self.ai(a, 3, self.charas.len() as i64)?.max(0) as usize;
                let idx = match v.idx.last() {
                    Some(e) => self.eval_index_pub(e, &v.name)?.max(0) as usize,
                    None => 0,
                };
                let range: Vec<usize> = (st..en.min(self.charas.len())).collect();
                let it: Box<dyn Iterator<Item = &usize>> = if name == "FINDCHARA" { Box::new(range.iter()) } else { Box::new(range.iter().rev()) };
                for &c in it {
                    if self.chara_arr(c, &v.name).get(idx).as_string() == target.as_string() {
                        return i(c as i64);
                    }
                }
                i(-1)
            }
            "GETTIME" => {
                let t = now();
                i(t.y * 10_000_000_000_000 + t.mo * 100_000_000_000 + t.d * 1_000_000_000 + t.h * 10_000_000 + t.mi * 100_000 + t.s * 1000 + t.ms)
            }
            "GETTIMES" => {
                let t = now();
                s(format!("{:04}年{:02}月{:02}日 {:02}:{:02}:{:02}", t.y, t.mo, t.d, t.h, t.mi, t.s))
            }
            "GETMILLISECOND" => {
                let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
                i(d.as_millis() as i64)
            }
            "GETSECOND" => {
                let d = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
                i(d.as_secs() as i64)
            }
            "GETCOLOR" => i(self.con.style.fg.to_i64()),
            "GETDEFCOLOR" => i(self.con.default_fg.to_i64()),
            "GETBGCOLOR" => i(self.con.style.bg.unwrap_or(self.con.default_bg).to_i64()),
            "GETDEFBGCOLOR" => i(self.con.default_bg.to_i64()),
            "GETFOCUSCOLOR" => i(self.con.focus_fg.to_i64()),
            "GETSTYLE" => {
                let st = &self.con.style;
                i(st.bold as i64 | (st.italic as i64) << 1 | (st.strike as i64) << 2 | (st.underline as i64) << 3)
            }
            "GETFONT" => s(self.con.font_name.clone()),
            "CURRENTALIGN" => i(match self.con.align {
                Align::Left => 0,
                Align::Center => 1,
                Align::Right => 2,
            }),
            "CURRENTREDRAW" => i(self.con.redraw as i64),
            "CHKFONT" => i(1),
            "MESSKIP" => i(self.mes_skip as i64),
            "ISSKIP" | "MOUSESKIP" | "GETKEY" | "GETKEYTRIGGERED" | "MOUSEX" | "MOUSEY" | "MOUSEB" => i(0),
            "ISACTIVE" => i(1),
            "PRINTCPERLINE" => i(3),
            "PRINTCLENGTH" => i(self.con.printc_len as i64),
            "SAVENOS" => i(20),
            "GETNUM" => {
                let v = self.avar(a, 0)?;
                let n = self.as_(a, 1, "")?;
                let t = crate::csvdata::table_for_var(&v.name).unwrap_or(&v.name).to_string();
                i(self.csv.lookup(&t, &n).or_else(|| self.csv.lookup(&t, &n.to_uppercase())).unwrap_or(-1))
            }
            "GETPALAMLV" | "GETEXPLV" => {
                let v = self.ai(a, 0, 0)?;
                let m = self.ai(a, 1, 0)?;
                let arr = if name == "GETPALAMLV" { "PALAMLV" } else { "EXPLV" };
                let mut lv = 0;
                for k in 0..=m.max(0) as usize {
                    if v >= self.get_global_i(arr, k) {
                        lv = k as i64;
                    } else {
                        break;
                    }
                }
                i(lv)
            }
            "CHKDATA" => {
                let n = self.ai(a, 0, 0)?;
                let r = self.check_save(n);
                self.set_results(r.mes);
                i(r.state)
            }
            "EXISTFUNCTION" => {
                let n = self.as_(a, 0, "")?.to_uppercase();
                i(match self.prog.funcs.get(&n) {
                    Some(f) if f[0].is_function && f[0].returns_str => 3,
                    Some(f) if f[0].is_function => 2,
                    Some(_) => 1,
                    None => 0,
                })
            }
            "EXISTVAR" | "ISDEFINED" => {
                let n = self.as_(a, 0, "")?.to_uppercase();
                i(self.var_exists(&n) as i64)
            }
            "GETVAR" | "GETVARS" => {
                let n = self.as_(a, 0, "")?;
                let e = crate::expr::parse_expr(&n, &self.prog.defines).map_err(|e| Ctrl::Error(e, false))?;
                let e = crate::parser::upper_expr(e);
                Ok(Some(self.eval(&e)?))
            }
            "SETVAR" => {
                let n = self.as_(a, 0, "")?;
                let v = self.arg(a, 1)?.unwrap_or(Value::Int(0));
                let e = crate::expr::parse_expr(&n, &self.prog.defines).map_err(|e| Ctrl::Error(e, false))?;
                if let Expr::Var(vr) = crate::parser::upper_expr(e) {
                    self.set_var(&vr, v)?;
                }
                i(1)
            }
            "SPRITECREATED" => {
                let n = self.as_(a, 0, "")?;
                i(self.res.exists(&n) as i64)
            }
            "SPRITEWIDTH" | "SPRITEHEIGHT" => {
                let n = self.as_(a, 0, "")?;
                let sz = self.res.sprite_size(&n).unwrap_or((0, 0));
                i(if name == "SPRITEWIDTH" { sz.0 } else { sz.1 } as i64)
            }
            "ARRAYMSORT" | "ARRAYMSORTEX" => self.array_msort(name, a),
            "FLOWINPUT" => {
                self.flow_input.0 = self.ai(a, 0, 0)?;
                if a.len() > 1 {
                    self.flow_input.1 = self.ai(a, 1, 0)? != 0;
                }
                if a.len() > 3 {
                    self.flow_input.4 = self.ai(a, 3, 0)? != 0;
                }
                i(0)
            }
            "FLOWINPUTS" => {
                self.flow_input.2 = self.ai(a, 0, 0)? != 0;
                if a.len() > 1 {
                    self.flow_input.3 = self.as_(a, 1, "")?;
                }
                i(0)
            }
            "GETNUMB" => {
                // GETNUMB "VAR", "name": the name's number in VAR's CSV table
                let v = self.as_(a, 0, "")?.to_uppercase();
                let n = self.as_(a, 1, "")?;
                if !self.var_exists(&v) {
                    return err(format!("GETNUMB: '{}' is not a variable", v));
                }
                let t = crate::csvdata::table_for_var(&v).unwrap_or(&v).to_string();
                i(self.csv.lookup(&t, &n).or_else(|| self.csv.lookup(&t, &n.to_uppercase())).unwrap_or(-1))
            }
            "ERDNAME" => {
                // ERDNAME var, value[, dimension]: the name for a number
                let v = self.avar(a, 0)?;
                let n = self.ai(a, 1, 0)?;
                let table = match a.get(2) {
                    Some(e) if !matches!(e, Expr::Empty) => format!("{}@{}", v.name, self.eval(e)?.as_int()),
                    _ => crate::csvdata::table_for_var(&v.name).unwrap_or(&v.name).to_string(),
                };
                s(self.csv.name_of(&table, n).unwrap_or("").to_string())
            }
            "EXISTMETH" => {
                // 1: returns a number, 2: a string, 0: no such function
                let n = self.as_(a, 0, "")?.to_uppercase();
                i(match self.prog.funcs.get(&n) {
                    Some(f) if f[0].is_function => if f[0].returns_str { 2 } else { 1 },
                    Some(_) => 0,
                    None if is_builtin_function(&n) => if STR_FUNCTIONS.contains(&n.as_str()) { 2 } else { 1 },
                    None => 0,
                })
            }
            "ENUMFUNCBEGINSWITH" | "ENUMFUNCWITH" | "ENUMFUNCENDSWITH" | "ENUMVARBEGINSWITH" | "ENUMVARWITH" | "ENUMVARENDSWITH"
            | "ENUMMACROBEGINSWITH" | "ENUMMACROWITH" | "ENUMMACROENDSWITH" => {
                // ENUM{FUNC,VAR,MACRO}{BEGINSWITH,WITH,ENDSWITH} text[, ARRAY]:
                // matching names (case-insensitive) into ARRAY or RESULTS
                let key = self.as_(a, 0, "")?.to_uppercase();
                let mut names: Vec<String> = if name.starts_with("ENUMFUNC") {
                    self.prog.funcs.iter().filter(|(_, fs)| fs.len() == 1 && !fs[0].single && !fs[0].only).map(|(k, _)| k.clone()).collect()
                } else if name.starts_with("ENUMVAR") {
                    let mut v: Vec<String> = super::vars::SYSTEM_VARS.iter().map(|d| d.name.to_string()).collect();
                    v.extend(self.prog.global_dims.iter().map(|d| d.name.clone()));
                    v
                } else {
                    self.prog.defines.keys().cloned().collect()
                };
                names.sort();
                let hit = |n: &str| {
                    !key.is_empty()
                        && if name.ends_with("BEGINSWITH") {
                            n.starts_with(&key)
                        } else if name.ends_with("ENDSWITH") {
                            n.ends_with(&key)
                        } else {
                            n.contains(&key)
                        }
                };
                let found: Vec<Value> = names.into_iter().filter(|n| hit(n)).map(Value::Str).collect();
                let target = match a.get(1) {
                    Some(Expr::Var(v)) => VarRef { name: v.name.clone(), idx: vec![] },
                    _ => VarRef { name: "RESULTS".into(), idx: vec![] },
                };
                let (loc, base, len) = self.array_row(&target)?;
                let n = found.len().min(len);
                let arr = self.arr_at(&loc).unwrap();
                for (k, v) in found.into_iter().take(len).enumerate() {
                    arr.set(base + k, v);
                }
                i(n as i64)
            }
            "ENUMFILES" => {
                // ENUMFILES dir[, pattern = "*"[, subfolders[, ARRAY]]]:
                // paths relative to the game folder
                let dir = self.as_(a, 0, "")?.replace('\\', "/");
                let pat: Vec<char> = self.as_(a, 1, "*")?.to_lowercase().chars().collect();
                let deep = self.ai(a, 2, 0)? != 0;
                let root = self.game_dir.join(&dir);
                if !root.is_dir() {
                    return i(-1);
                }
                fn walk(d: &std::path::Path, deep: bool, out: &mut Vec<std::path::PathBuf>) {
                    let Ok(rd) = std::fs::read_dir(d) else { return };
                    let mut es: Vec<_> = rd.flatten().map(|e| e.path()).collect();
                    es.sort();
                    for p in es {
                        if p.is_dir() {
                            if deep {
                                walk(&p, deep, out);
                            }
                        } else {
                            out.push(p);
                        }
                    }
                }
                fn wild(p: &[char], s: &[char]) -> bool {
                    match (p.first(), s.first()) {
                        (None, None) => true,
                        (Some('*'), _) => wild(&p[1..], s) || (!s.is_empty() && wild(p, &s[1..])),
                        (Some('?'), Some(_)) => wild(&p[1..], &s[1..]),
                        (Some(x), Some(y)) if x == y => wild(&p[1..], &s[1..]),
                        _ => false,
                    }
                }
                let mut files = Vec::new();
                walk(&root, deep, &mut files);
                let found: Vec<Value> = files
                    .into_iter()
                    .filter(|p| p.file_name().is_some_and(|n| wild(&pat, &n.to_string_lossy().to_lowercase().chars().collect::<Vec<_>>())))
                    .filter_map(|p| p.strip_prefix(&self.game_dir).ok().map(|r| Value::Str(r.to_string_lossy().into_owned())))
                    .collect();
                let target = match a.get(3) {
                    Some(Expr::Var(v)) => VarRef { name: v.name.clone(), idx: vec![] },
                    _ => VarRef { name: "RESULTS".into(), idx: vec![] },
                };
                let (loc, base, len) = self.array_row(&target)?;
                let n = found.len().min(len);
                let arr = self.arr_at(&loc).unwrap();
                for (k, v) in found.into_iter().take(len).enumerate() {
                    arr.set(base + k, v);
                }
                i(n as i64)
            }
            "HTML_SUBSTRING" => {
                // first part (fitting the width) and rest into RESULTS:0/1
                let t = self.as_(a, 0, "")?;
                let w = self.ai(a, 1, 0)?.max(0) as usize;
                let (x, y) = crate::html::html_substring(&t, w);
                let r = self.global_arr("RESULTS");
                r.set(0, Value::Str(x.clone()));
                r.set(1, Value::Str(y));
                s(x)
            }
            "HTML_STRINGLINES" => {
                // how many lines of `width` the HTML text takes
                let mut t = self.as_(a, 0, "")?;
                let w = self.ai(a, 1, 0)?.max(1) as usize;
                let mut n = 0;
                while !t.is_empty() && n < 100_000 {
                    let (_, rest) = crate::html::html_substring(&t, w);
                    n += 1;
                    if rest.len() >= t.len() {
                        break;
                    }
                    t = rest;
                }
                i(n)
            }
            "GETDISPLAYLINE" => {
                let n = self.ai(a, 0, 0)?;
                s(if n >= 0 && (n as usize) < self.con.lines.len() { crate::console::Console::line_text(&self.con.lines[n as usize]) } else { String::new() })
            }
            "UNICODEBYTE" => {
                let t = self.as_(a, 0, "")?;
                i(t.chars().next().map_or(0, |c| c as i64))
            }
            // window hotkeys / state: nothing to do in a terminal
            "HOTKEY_STATE" | "HOTKEY_STATE_INIT" => i(0),
            "GCREATED" | "GWIDTH" | "GHEIGHT" => {
                let id = self.ai(a, 0, 0)?;
                i(match self.res.graphics.get(&id) {
                    None => 0,
                    Some(g) if name == "GWIDTH" => g.img.width() as i64,
                    Some(g) if name == "GHEIGHT" => g.img.height() as i64,
                    Some(_) => 1,
                })
            }
            "GCREATE" | "GCREATEFROMFILE" | "GDISPOSE" | "GCLEAR" | "GFILLRECTANGLE" | "GDRAWSPRITE" | "GDRAWG" | "GDRAWGWITHMASK"
            | "GSETCOLOR" | "GSETBRUSH" | "GSAVE" | "GLOAD" | "SPRITECREATE" | "SPRITEDISPOSE" | "GDRAWLINE" | "GDRAWTEXT"
            | "GDASHSTYLE" | "GDRAWGWITHROTATE" | "GSETPEN" | "GSETFONT" => self.graphics_fn(name, a),
            "GGETBRUSH" | "GGETPEN" | "GGETPENWIDTH" | "GGETFONTSIZE" | "GGETFONTSTYLE" => {
                let id = self.ai(a, 0, 0)?;
                let Some(g) = self.res.graphics.get(&id) else { return i(-1) };
                i(match name {
                    "GGETBRUSH" => crate::resources::to_argb(g.brush),
                    "GGETPEN" => crate::resources::to_argb(g.pen.0),
                    "GGETPENWIDTH" => g.pen.1,
                    "GGETFONTSIZE" => g.font.1,
                    _ => g.font.2,
                })
            }
            "GGETFONT" => {
                let id = self.ai(a, 0, 0)?;
                s(self.res.graphics.get(&id).map(|g| g.font.0.clone()).unwrap_or_default())
            }
            "GGETTEXTSIZE" => {
                // GGETTEXTSIZE text, font, size[, style]: width; RESULT:1 height
                let t = self.as_(a, 0, "")?;
                let f = self.as_(a, 1, "")?;
                let sz = self.ai(a, 2, 18)?.max(1) as f32;
                let (w, h) = crate::resources::text_size(&t, &f, sz);
                self.global_arr("RESULT").set_i(1, h);
                i(w)
            }
            "SPRITEDISPOSEALL" => i(self.res.dispose_all_sprites()),
            "SPRITEGETCOLOR" => {
                let n = self.as_(a, 0, "")?;
                let (x, y) = (self.ai(a, 1, 0)?, self.ai(a, 2, 0)?);
                i(match self.res.sprite(&n) {
                    Some(img) if (0..img.width() as i64).contains(&x) && (0..img.height() as i64).contains(&y) => {
                        crate::resources::to_argb(img.get_pixel(x as u32, y as u32).0)
                    }
                    _ => -1,
                })
            }
            "SPRITEMOVE" | "SPRITESETPOS" => {
                // a sprite's drawing offset (SPRITEPOSX / SPRITEPOSY)
                let n = self.as_(a, 0, "")?.to_uppercase();
                if !self.res.exists(&n) {
                    return i(0);
                }
                let (x, y) = (self.ai(a, 1, 0)?, self.ai(a, 2, 0)?);
                let p = self.sprite_pos.entry(n).or_insert((0, 0));
                *p = if name == "SPRITEMOVE" { (p.0 + x, p.1 + y) } else { (x, y) };
                i(1)
            }
            "GETCONFIG" | "GETCONFIGS" => {
                let k = self.as_(a, 0, "")?;
                match self.config_value(&k) {
                    Some(v) if v.is_str() == (name == "GETCONFIGS") => Ok(Some(v)),
                    Some(_) => err(format!("{}: '{}' is a {} setting (use {})", name, k, if name == "GETCONFIG" { "text" } else { "number" }, if name == "GETCONFIG" { "GETCONFIGS" } else { "GETCONFIG" })),
                    None => err(format!("{}: unknown config item '{}'", name, k)),
                }
            }
            "HTML_ESCAPE" => {
                let x = self.as_(a, 0, "")?;
                s(crate::xml::escape(&x))
            }
            "COLOR_FROMNAME" => {
                let n = self.as_(a, 0, "")?;
                if n.trim().eq_ignore_ascii_case("transparent") {
                    return err("COLOR_FROMNAME: transparent is not supported");
                }
                i(parse_color_name(&n).map_or(-1, |c| c.to_i64()))
            }
            "COLOR_FROMRGB" => {
                let mut v = 0;
                for k in 0..3 {
                    let c = self.ai(a, k, 0)?;
                    if !(0..=255).contains(&c) {
                        return err(format!("COLOR_FROMRGB: argument {} ({}) is outside 0..255", k + 1, c));
                    }
                    v = (v << 8) | c;
                }
                i(v)
            }
            "HTML_STRINGLEN" => {
                // displayed width: half-width cells, or pixels with a non-zero 2nd arg
                let x = self.as_(a, 0, "")?;
                let cells = crate::html::to_plain(&x).lines().map(sjis_len).max().unwrap_or(0) as i64;
                let px = cells * (self.con.font_size / 2.0).round().max(1.0) as i64;
                i(if self.ai(a, 1, 0)? != 0 { px } else { cells })
            }
            "GETMETH" | "GETMETHS" => {
                // call a function by name, or return the default if there is none
                let fname = self.as_(a, 0, "")?.to_uppercase();
                let exists = self.has_func(&fname) || is_builtin_function(&fname);
                if !exists {
                    return match a.get(1).filter(|e| !matches!(e, Expr::Empty)) {
                        Some(e) => Ok(Some(self.eval(e)?)),
                        None => err(format!("{}: function {} is not defined", name, fname)),
                    };
                }
                let v = self.call_expr(&fname, a.get(2..).unwrap_or(&[]))?;
                match (name, &v) {
                    ("GETMETH", Value::Str(_)) => err(format!("GETMETH: {} does not return an integer", fname)),
                    ("GETMETHS", Value::Int(_)) => err(format!("GETMETHS: {} does not return a string", fname)),
                    _ => Ok(Some(v)),
                }
            }
            "GETDOINGFUNCTION" => s(self.frames.last().map(|f| f.func.name.clone()).unwrap_or_default()),
            "CLIENTWIDTH" => i(((self.con.width + 1) as f32 * self.con.font_size / 2.0) as i64),
            "CLIENTHEIGHT" => i((self.fe.rows() as f32 * self.con.line_height) as i64),
            "EXISTFILE" => {
                let p = self.as_(a, 0, "")?;
                let rel = std::path::Path::new(&p);
                let ok = !rel.is_absolute() && !rel.components().any(|c| matches!(c, std::path::Component::ParentDir)) && self.game_dir.join(rel).is_file();
                i(ok as i64)
            }
            // no graphics surfaces / sprite positions / text box in this frontend
            "SPRITEPOSX" | "SPRITEPOSY" => {
                let n = self.as_(a, 0, "")?.to_uppercase();
                let p = self.sprite_pos.get(&n).copied().unwrap_or((0, 0));
                i(if name == "SPRITEPOSX" { p.0 } else { p.1 })
            }
            "GGETCOLOR" => {
                let (id, x, y) = (self.ai(a, 0, 0)?, self.ai(a, 1, 0)?, self.ai(a, 2, 0)?);
                i(match self.res.graphics.get(&id) {
                    Some(g) if (0..g.img.width() as i64).contains(&x) && (0..g.img.height() as i64).contains(&y) => {
                        crate::resources::to_argb(g.img.get_pixel(x as u32, y as u32).0)
                    }
                    _ => -1,
                })
            }
            "GETTEXTBOX" => s(String::new()),
            "HTML_TOPLAINTEXT" => {
                let x = self.as_(a, 0, "")?;
                s(crate::html::to_plain(&x))
            }
            "HTML_POPPRINTINGSTR" => {
                // the pending (not yet newlined) line, as HTML, removed from the console
                // (Emuera's DisplayLine2Html: buttons, colours and font
                // styles survive the trip through HTML_PRINT)
                let segs = crate::console::auto_buttons(std::mem::take(&mut self.con.cur));
                let def_fg = self.con.default_fg;
                let mut out = String::new();
                let mut open: Option<Value> = None;
                for seg in &segs {
                    let btn = match seg {
                        Seg::Text { button, .. } | Seg::Image { button, .. } | Seg::Rect { button, .. } => button.clone(),
                        Seg::Space { .. } => None,
                    };
                    if btn != open {
                        if open.is_some() {
                            out.push_str("</button>");
                        }
                        if let Some(v) = &btn {
                            out.push_str(&format!("<button value='{}'>", crate::xml::escape(&v.as_string())));
                        }
                        open = btn;
                    }
                    match seg {
                        Seg::Text { text, style, .. } => {
                            let font = style.fg != def_fg;
                            if font {
                                out.push_str(&format!("<font color='#{:02X}{:02X}{:02X}'>", style.fg.0, style.fg.1, style.fg.2));
                            }
                            for (on, tag) in [(style.strike, "s"), (style.underline, "u"), (style.italic, "i"), (style.bold, "b")] {
                                if on {
                                    out.push_str(&format!("<{}>", tag));
                                }
                            }
                            out.push_str(&crate::xml::escape(text));
                            for (on, tag) in [(style.bold, "b"), (style.italic, "i"), (style.underline, "u"), (style.strike, "s")] {
                                if on {
                                    out.push_str(&format!("</{}>", tag));
                                }
                            }
                            if font {
                                out.push_str("</font>");
                            }
                        }
                        Seg::Image { name, .. } => out.push_str(&format!("<img src='{}'>", crate::xml::escape(name))),
                        Seg::Space { w } => out.push_str(&format!("<space value='{}'>", (*w).round() as i64)),
                        Seg::Rect { .. } => {}
                    }
                }
                if open.is_some() {
                    out.push_str("</button>");
                }
                s(out)
            }
            "LOADTEXT" => {
                let target = self.arg(a, 0)?.unwrap_or(Value::Int(0));
                let Some(path) = self.text_path(&target, false) else { return s(String::new()) };
                // consistent line endings, as emuera.em does
                s(crate::loader::read_text(&path).map(|t| t.replace('\r', "")).unwrap_or_default())
            }
            "SAVETEXT" => {
                let text = self.as_(a, 0, "")?;
                let target = self.arg(a, 1)?.unwrap_or(Value::Int(0));
                let Some(path) = self.text_path(&target, true) else { return i(0) };
                if let Some(d) = path.parent() {
                    let _ = std::fs::create_dir_all(d);
                }
                i(std::fs::write(&path, text).is_ok() as i64)
            }
            "FIND_CHARADATA" => {
                // names (without chara_ / .dat) into RESULTS:0…
                let pat = self.as_(a, 0, "*")?;
                let names = self.find_chara_files(&pat);
                let n = names.len() as i64;
                let r = self.global_arr("RESULTS");
                for (k, s) in names.into_iter().take(r.len()).enumerate() {
                    r.set(k, Value::Str(s));
                }
                i(n)
            }
            "CHKCHARADATA" => {
                let file = self.as_(a, 0, "")?;
                let r = self.check_chara(&file);
                self.set_results(r.mes);
                i(r.state)
            }
            "HTML_GETPRINTEDSTR" => {
                let n = self.ai(a, 0, 0)?;
                let l = self.con.lines.len() as i64 - 1 - n;
                s(if l >= 0 { crate::console::Console::line_text(&self.con.lines[l as usize]) } else { String::new() })
            }
            "GETLINESTR" => {
                let x = self.as_(a, 0, "-")?;
                s(crate::console::repeat_to_width(&x, self.con.width))
            }
            "REGEXPMATCH" => {
                let x = self.as_(a, 0, "")?;
                let p = self.as_(a, 1, "")?;
                i(regex_lite_match(&p, &x) as i64)
            }
            "GETMEMORYUSAGE" | "CLEARMEMORY" => i(0),
            "EXISTSOUND" | "PLAYSOUND" | "STOPSOUND" | "PLAYBGM" | "STOPBGM" | "SETSOUNDVOLUME" | "SETBGMVOLUME" => i(0),
            _ => Ok(None),
        }
    }
}

/// BARSTR / BAR: `[***...]` of length `l` filled in proportion v/m.
pub fn barstr(v: i64, m: i64, l: i64) -> String {
    let l = l.max(0);
    let filled = if m <= 0 { 0 } else { (v.max(0) * l / m).min(l) };
    format!("[{}{}]", "*".repeat(filled as usize), ".".repeat((l - filled) as usize))
}

/// .NET-style numeric format subset: "0000", "#,##0", "00.0"-less ints, "X"/"x"
/// TOSTR's .NET-style number formats (`"0000"`, `"#,0"`…), the common subset.
pub fn format_number(v: i64, fmt: &str) -> String {
    if fmt.is_empty() {
        return v.to_string();
    }
    // .NET standard formats: a letter plus an optional digit count
    let mut ch = fmt.chars();
    let (letter, prec) = (ch.next().unwrap(), ch.as_str());
    if prec.is_empty() || prec.bytes().all(|b| b.is_ascii_digit()) {
        let n: usize = prec.parse().unwrap_or(0);
        match letter {
            // hex of the two's-complement value, like C#
            'X' => return format!("{:0n$X}", v, n = n),
            'x' => return format!("{:0n$x}", v, n = n),
            'D' | 'd' => {
                let d = format!("{:0n$}", v.unsigned_abs(), n = n);
                return if v < 0 { format!("-{}", d) } else { d };
            }
            'N' | 'n' => {
                let g = format_number(v, "#,0");
                return if n > 0 { format!("{}.{}", g, "0".repeat(n)) } else { g };
            }
            'F' | 'f' => return if n > 0 { format!("{}.{}", v, "0".repeat(n)) } else { v.to_string() },
            _ => {}
        }
    }
    let comma = fmt.contains(',');
    let zeros = fmt.chars().filter(|c| *c == '0').count();
    let neg = v < 0;
    let mut digits = v.unsigned_abs().to_string();
    while digits.len() < zeros {
        digits.insert(0, '0');
    }
    if comma {
        let mut out = String::new();
        for (k, c) in digits.chars().enumerate() {
            if k > 0 && (digits.len() - k) % 3 == 0 {
                out.push(',');
            }
            out.push(c);
        }
        digits = out;
    }
    if neg {
        format!("-{}", digits)
    } else {
        digits
    }
}

/// A small backtracking regex matcher (literals, `.`, `[...]`, `* + ?`,
/// `^ $`, `\d \s \w` and their negations, escapes) for REGEXPMATCH /
/// FINDELEMENT, avoiding a regex crate. No groups or `|` alternation.
/// Unanchored: true if the pattern matches anywhere in `text`.
pub fn regex_lite_match(pat: &str, text: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    let t: Vec<char> = text.chars().collect();
    if p.first() == Some(&'^') {
        return m_here(&p[1..], &t);
    }
    for i in 0..=t.len() {
        if m_here(&p, &t[i..]) {
            return true;
        }
    }
    false
}

fn atom_len(p: &[char]) -> usize {
    match p.first() {
        Some('\\') => 2.min(p.len()),
        Some('[') => p.iter().position(|c| *c == ']').map(|i| i + 1).unwrap_or(p.len()),
        Some(_) => 1,
        None => 0,
    }
}

fn atom_match(a: &[char], c: char) -> bool {
    match a.first() {
        Some('.') => true,
        Some('\\') => match a.get(1) {
            Some('d') => c.is_ascii_digit(),
            Some('D') => !c.is_ascii_digit(),
            Some('s') => c.is_whitespace(),
            Some('S') => !c.is_whitespace(),
            Some('w') => c.is_alphanumeric() || c == '_',
            Some('W') => !(c.is_alphanumeric() || c == '_'),
            Some('n') => c == '\n',
            Some('t') => c == '\t',
            x => x == Some(&c),
        },
        Some('[') => {
            let inner = &a[1..a.len().saturating_sub(1)];
            let (neg, inner) = if inner.first() == Some(&'^') { (true, &inner[1..]) } else { (false, inner) };
            let mut hit = false;
            let mut k = 0;
            while k < inner.len() {
                if k + 2 < inner.len() && inner[k + 1] == '-' {
                    if c >= inner[k] && c <= inner[k + 2] {
                        hit = true;
                    }
                    k += 3;
                } else {
                    if inner[k] == c {
                        hit = true;
                    }
                    k += 1;
                }
            }
            hit != neg
        }
        Some(x) => *x == c,
        None => false,
    }
}

fn m_here(p: &[char], t: &[char]) -> bool {
    if p.is_empty() {
        return true;
    }
    if p == ['$'] {
        return t.is_empty();
    }
    let al = atom_len(p);
    let atom = &p[..al];
    match p.get(al) {
        Some('*') | Some('+') | Some('?') => {
            let q = p[al];
            let rest = &p[al + 1..];
            let mut n = 0;
            while n < t.len() && atom_match(atom, t[n]) {
                n += 1;
            }
            let min = if q == '+' { 1 } else { 0 };
            let max = if q == '?' { n.min(1) } else { n };
            (min..=max).rev().any(|k| k >= min && m_here(rest, &t[k..]))
        }
        _ => !t.is_empty() && atom_match(atom, t[0]) && m_here(&p[al..], &t[1..]),
    }
}

/// Emuera's HtmlManager.HtmlTagSplit: alternating text runs and `<...>` tags;
/// None if a tag is unterminated.
fn html_tag_split(s: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut rest = s;
    while !rest.is_empty() {
        let Some(lt) = rest.find('<') else {
            out.push(rest.to_string());
            break;
        };
        if lt > 0 {
            out.push(rest[..lt].to_string());
        }
        let gt = rest[lt..].find('>')? + lt;
        out.push(rest[lt..=gt].to_string());
        rest = &rest[gt + 1..];
    }
    Some(out)
}


#[cfg(test)]
mod tests {
    use super::regex_lite_match as m;

    #[test]
    fn dotnet_number_formats() {
        use super::format_number as f;
        assert_eq!(f(0x007878, "X6"), "007878");
        assert_eq!(f(255, "x"), "ff");
        assert_eq!(f(42, "D3"), "042");
        assert_eq!(f(-7, "D2"), "-07");
        assert_eq!(f(1234567, "N0"), "1,234,567");
        assert_eq!(f(5, "F2"), "5.00");
        assert_eq!(f(42, "0000"), "0042");
        assert_eq!(f(1234567, "#,0"), "1,234,567");
    }

    #[test]
    fn regex_lite_basics() {
        assert!(m("a.c", "xxabcxx"));
        assert!(m("^ab+c$", "abbbc") && !m("^ab+c$", "ac"));
        assert!(m("[0-9]+", "x42") && !m("^[^0-9]+$", "x42"));
        assert!(m("\\s", "two words") && !m("\\s", "oneword"));
        assert!(m("^\\d\\d$", "42") && !m("^\\d\\d$", "4a"));
        assert!(m("\\.", "a.b") && !m("\\.", "ab"));
    }
}

/// Emuera's TOINT / ISNUMERIC reading of a string: an optional sign, then
/// a decimal, `0x` hex or `0b` binary number, an optional `e` / `p`
/// exponent and a `.digits` fraction that is dropped. Anything else
/// (spaces, full-width characters, trailing text) is not a number.
/// `strict` is ISNUMERIC's check that an exponent has digits.
pub(crate) fn emu_str_int(s: &str, strict: bool) -> Option<i64> {
    let c: Vec<char> = s.chars().collect();
    let at = |k: usize| c.get(k).copied().unwrap_or('\0');
    let first = at(0);
    if !(first.is_ascii_digit() || ((first == '+' || first == '-') && at(1).is_ascii_digit())) {
        return None;
    }
    let mut k = 0;
    let mut base = 10;
    if first == '0' && matches!(at(1), 'x' | 'X') {
        base = 16;
        k = 2;
    } else if first == '0' && matches!(at(1), 'b' | 'B') {
        base = 2;
        k = 2;
    }
    let digits = |k: &mut usize, base: u32| -> Option<i64> {
        let start = *k;
        if matches!(at(*k), '+' | '-') {
            *k += 1;
        }
        while at(*k).is_digit(base) || (base == 2 && at(*k).is_ascii_digit()) {
            *k += 1;
        }
        let t: String = c[start..*k].iter().collect();
        if base == 10 {
            t.parse::<i64>().ok()
        } else {
            let (neg, d) = t.strip_prefix('-').map(|d| (true, d)).unwrap_or((false, t.trim_start_matches('+')));
            let v = u64::from_str_radix(d, base).ok()? as i64;
            Some(if neg { v.wrapping_neg() } else { v })
        }
    };
    if base != 10 && !at(k).is_digit(base) {
        return None;
    }
    let mut v = digits(&mut k, base)?;
    let exp_base = match at(k) {
        'p' | 'P' => 2.0,
        'e' | 'E' => 10.0,
        _ => 0.0,
    };
    if exp_base != 0.0 {
        k += 1;
        if strict && !at(k).is_ascii_digit() {
            return None;
        }
        let e = digits(&mut k, base).unwrap_or(0);
        if e != 0 {
            let d = v as f64 * f64::powi(exp_base, e as i32);
            if !d.is_finite() || d > i64::MAX as f64 || d < i64::MIN as f64 {
                return None;
            }
            v = d as i64;
        }
    }
    if k < c.len() {
        if c[k] != '.' || !c[k + 1..].iter().all(|ch| ch.is_ascii_digit()) {
            return None;
        }
    }
    Some(v)
}

#[cfg(test)]
mod str_int_tests {
    use super::emu_str_int;

    #[test]
    fn emuera_number_strings() {
        assert_eq!(emu_str_int("0x797979", false), Some(0x797979));
        assert_eq!(emu_str_int("0X0b0B0b", true), Some(0x0b0b0b));
        assert_eq!(emu_str_int("0b101", false), Some(5));
        assert_eq!(emu_str_int("-42", false), Some(-42));
        assert_eq!(emu_str_int("+7", false), Some(7));
        assert_eq!(emu_str_int("12.75", false), Some(12));
        assert_eq!(emu_str_int("3e2", false), Some(300));
        assert_eq!(emu_str_int("3e", false), Some(3));
        assert_eq!(emu_str_int("3e", true), None);
        for bad in ["", " 1", "1 ", "１２", "abc", "0x", "-", "1a", "def", "floras", "12.a"] {
            assert_eq!(emu_str_int(bad, false), None, "{:?}", bad);
        }
    }
}
