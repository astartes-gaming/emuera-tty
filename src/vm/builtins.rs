//! Built-in commands and functions.

use super::value::*;
use super::vars::Arr;
use super::{pad, Chara, Loc, Place, Vm};
use crate::ast::*;
use crate::console::{Rgb, Seg};
use crate::expr::parse_form;
use crate::loader::sjis_len;
use std::rc::Rc;

/// Statement-only commands (functions callable as statements are separate).
pub const COMMANDS: &[&str] = &[
    "DRAWLINE", "CUSTOMDRAWLINE", "DRAWLINEFORM", "CLEARLINE", "REDRAW", "SETCOLOR", "RESETCOLOR", "SETBGCOLOR",
    "RESETBGCOLOR", "SETCOLORBYNAME", "SETBGCOLORBYNAME", "FONTBOLD", "FONTITALIC", "FONTREGULAR", "FONTSTYLE", "SETFONT",
    "ALIGNMENT", "PRINTBUTTON", "PRINTBUTTONC", "PRINTBUTTONLC", "PRINT_IMG", "PRINT_RECT", "PRINT_SPACE", "HTML_PRINT",
    "BAR", "BARL", "REUSELASTLINE", "SKIPDISP", "OUTPUTLOG", "DEBUGPRINT", "DEBUGPRINTL", "DEBUGPRINTFORM",
    "DEBUGPRINTFORML", "DEBUGCLEAR", "ASSERT", "TOOLTIP_SETCOLOR", "TOOLTIP_SETDELAY", "TOOLTIP_SETDURATION",
    "TOOLTIP_SETFONT", "TOOLTIP_SETFONTSIZE", "TOOLTIP_CUSTOM", "TOOLTIP_SETFORMAT", "TOOLTIP_IMG", "SETANIMETIMER",
    "CLEARTEXTBOX", "UPDATECHECK", "INPUT", "INPUTS", "TINPUT", "TINPUTS", "ONEINPUT", "ONEINPUTS", "TONEINPUT",
    "TONEINPUTS", "WAIT", "FORCEWAIT", "TWAIT", "WAITANYKEY", "INPUTMOUSEKEY", "DOTRAIN", "CALLTRAIN", "STOPCALLTRAIN",
    "SAVEGAME", "LOADGAME", "SAVEDATA", "LOADDATA", "DELDATA", "SAVEGLOBAL", "LOADGLOBAL", "RESETDATA", "RESETGLOBAL",
    "VARSET", "CVARSET", "ARRAYSHIFT", "ARRAYREMOVE", "ARRAYSORT", "ARRAYCOPY", "ARRAYMSORT", "SWAP", "TIMES",
    "SETBIT", "CLEARBIT", "INVERTBIT", "SPLIT", "RANDOMIZE", "INITRAND", "DUMPRAND", "STRLEN", "STRLENU", "STRLENFORM",
    "STRLENFORMU", "PUTFORM", "ADDCHARA", "ADDSPCHARA", "ADDDEFCHARA", "ADDVOIDCHARA", "DELCHARA", "DELALLCHARA",
    "PICKUPCHARA", "SWAPCHARA", "COPYCHARA", "ADDCOPYCHARA", "SORTCHARA", "RESET_STAIN", "UPCHECK", "CUPCHECK",
    "CALLEVENT", "CLEARBUTTON", "SAVECHARA", "LOADCHARA", "MOUSESKIP_", "BITMAP_CACHE_ENABLE", "FORCEKANA",
    "SETCOLOR_", "GCREATE", "GCREATEFROMFILE", "GDISPOSE", "GCLEAR", "GFILLRECTANGLE", "GDRAWSPRITE", "GDRAWG",
    "GDRAWGWITHMASK", "GSETCOLOR", "GSETBRUSH", "GSETFONT", "GSETPEN", "GSAVE", "GLOAD", "SPRITECREATE", "SPRITEDISPOSE",
    "SPRITEMOVE", "SPRITESETPOS", "CBGSETG", "CBGSETSPRITE", "CBGCLEAR", "CBGCLEARBUTTON", "CBGSETBUTTONSPRITE",
    "CBGREMOVERANGE", "CBGREMOVEBMAP", "CBGSETBMAPG", "SETTEXTBOX", "QUIT_AND_RESTART", "RESETBGCOLOR_",
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
];

pub fn is_builtin_command(c: &str) -> bool {
    COMMANDS.contains(&c)
}
pub fn is_builtin_function(c: &str) -> bool {
    FUNCTIONS.contains(&c)
}

fn now() -> chrono_like::DateTime {
    chrono_like::now()
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

fn to_half(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{FF01}'..='\u{FF5E}' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            '\u{3000}' => ' ',
            _ => c,
        })
        .collect()
}

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

fn parse_color_name(n: &str) -> Option<Rgb> {
    let n = n.trim().to_lowercase();
    Some(match n.as_str() {
        "black" => Rgb(0, 0, 0),
        "white" => Rgb(255, 255, 255),
        "red" => Rgb(255, 0, 0),
        "lime" => Rgb(0, 255, 0),
        "green" => Rgb(0, 128, 0),
        "blue" => Rgb(0, 0, 255),
        "yellow" => Rgb(255, 255, 0),
        "cyan" | "aqua" => Rgb(0, 255, 255),
        "magenta" | "fuchsia" => Rgb(255, 0, 255),
        "gray" | "grey" => Rgb(128, 128, 128),
        "silver" => Rgb(192, 192, 192),
        "orange" => Rgb(255, 165, 0),
        "pink" => Rgb(255, 192, 203),
        "hotpink" => Rgb(255, 105, 180),
        "purple" => Rgb(128, 0, 128),
        "brown" => Rgb(165, 42, 42),
        "gold" => Rgb(255, 215, 0),
        "navy" => Rgb(0, 0, 128),
        "maroon" => Rgb(128, 0, 0),
        "olive" => Rgb(128, 128, 0),
        "teal" => Rgb(0, 128, 128),
        "violet" => Rgb(238, 130, 238),
        "skyblue" => Rgb(135, 206, 235),
        "lightblue" => Rgb(173, 216, 230),
        "lightgreen" => Rgb(144, 238, 144),
        "lightpink" => Rgb(255, 182, 193),
        "darkgray" | "darkgrey" => Rgb(169, 169, 169),
        "lightgray" | "lightgrey" => Rgb(211, 211, 211),
        "darkred" => Rgb(139, 0, 0),
        "darkgreen" => Rgb(0, 100, 0),
        "darkblue" => Rgb(0, 0, 139),
        "darkorange" => Rgb(255, 140, 0),
        "crimson" => Rgb(220, 20, 60),
        "coral" => Rgb(255, 127, 80),
        "salmon" => Rgb(250, 128, 114),
        "tomato" => Rgb(255, 99, 71),
        "khaki" => Rgb(240, 230, 140),
        "plum" => Rgb(221, 160, 221),
        "orchid" => Rgb(218, 112, 214),
        "indigo" => Rgb(75, 0, 130),
        "turquoise" => Rgb(64, 224, 208),
        "deepskyblue" => Rgb(0, 191, 255),
        "dodgerblue" => Rgb(30, 144, 255),
        "royalblue" => Rgb(65, 105, 225),
        "chocolate" => Rgb(210, 105, 30),
        "tan" => Rgb(210, 180, 140),
        "beige" => Rgb(245, 245, 220),
        "ivory" => Rgb(255, 255, 240),
        "lavender" => Rgb(230, 230, 250),
        _ => {
            let h = n.trim_start_matches('#');
            if h.len() == 6 {
                let v = i64::from_str_radix(h, 16).ok()?;
                Rgb::from_i64(v)
            } else {
                return None;
            }
        }
    })
}

pub fn color_from_str(s: &str) -> Option<Rgb> {
    parse_color_name(s)
}

impl Vm {
    fn arg(&mut self, args: &[Expr], k: usize) -> VmResult<Option<Value>> {
        match args.get(k) {
            None | Some(Expr::Empty) => Ok(None),
            Some(e) => Ok(Some(self.eval(e)?)),
        }
    }
    fn ai(&mut self, args: &[Expr], k: usize, d: i64) -> VmResult<i64> {
        Ok(self.arg(args, k)?.map(|v| v.as_int()).unwrap_or(d))
    }
    fn as_(&mut self, args: &[Expr], k: usize, d: &str) -> VmResult<String> {
        Ok(self.arg(args, k)?.map(|v| v.into_string()).unwrap_or_else(|| d.to_string()))
    }
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

    fn arr_vals(&mut self, v: &VarRef, start: i64, end: Option<i64>) -> VmResult<(Loc, usize, Vec<Value>)> {
        let (loc, base, len) = self.array_row(v)?;
        let end = end.unwrap_or(len as i64).min(len as i64).max(0) as usize;
        let start = start.max(0) as usize;
        let a = self.arr_at(&loc).unwrap();
        let vals = (start..end.max(start)).map(|i| a.get(base + i)).collect();
        Ok((loc, base + start, vals))
    }

    fn chara_no_template(&self, no: i64) -> Option<usize> {
        self.csv.charas.iter().position(|c| c.no == no)
    }

    pub fn new_chara_from_csv(&mut self, no: i64) -> VmResult<Chara> {
        let mut c = Chara::default();
        let mut set = |c: &mut Chara, name: &str, idx: i64, val: Value| {
            let def = super::vars::system_var(name);
            let a = c.vars.entry(name.to_string()).or_insert_with(|| match def {
                Some(d) => Arr::new(d.is_str, d.dims),
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
                    Some(Expr::Var(v)) if v.idx.is_empty() && !self.var_exists(&v.name) => v.name.clone(),
                    _ => self.as_(a, 0, "")?,
                };
                self.print_img(&n, None, None, 0.0, None);
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
                self.flush_partial();
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
                self.flush_partial();
                let v = self.input_str(default, timeout, one)?;
                self.set_results(v);
            }
            "WAIT" | "FORCEWAIT" | "WAITANYKEY" => self.wait()?,
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
                let _ = std::fs::remove_file(self.save_path(n));
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
                let back = matches!(a.get(1), Some(Expr::Var(o)) if o.name == "BACK");
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
                let sv = VarRef { name: src, idx: vec![] };
                let dv = VarRef { name: dst, idx: vec![] };
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
            n if n.starts_with('G') || n.starts_with("SPRITE") || n.starts_with("CBG") || n == "SETTEXTBOX" => {
                self.warn(&format!("graphics command {} not implemented (ignored)", n));
                self.set_result(0);
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
                        self.warn(&format!("command {} not implemented (ignored)", name));
                    }
                }
            }
        }
        Ok(())
    }

    fn flush_partial(&mut self) {}

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

    fn upcheck(&mut self, cup: bool, a: &[Expr]) -> VmResult<()> {
        // PALAM += UP - DOWN for TARGET (or CUP/CDOWN for chara arg), printing changes
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

    pub fn print_img(&mut self, name: &str, w_pct: Option<f32>, h_pct: Option<f32>, ypos_pct: f32, button: Option<Value>) {
        let fs = self.con.font_size;
        let (sw, sh) = self.res.sprite_size(name).unwrap_or((fs as u32, fs as u32));
        let h = h_pct.map(|p| fs * p / 100.0).unwrap_or(fs);
        let w = match w_pct {
            Some(p) if p != 0.0 => fs * p / 100.0,
            _ => h * sw as f32 / sh.max(1) as f32,
        };
        if !self.res.exists(name) {
            self.warn(&format!("image resource '{}' not found", name));
        }
        self.con.push_seg(Seg::Image { name: name.to_string(), w, h, ypos: fs * ypos_pct / 100.0, button });
    }

    // ============================================================== functions
    pub fn builtin_fn(&mut self, name: &str, a: &[Expr]) -> VmResult<Option<Value>> {
        let i = |v: i64| Ok(Some(Value::Int(v)));
        let s = |v: String| Ok(Some(Value::Str(v)));
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
                let vr = VarRef { name: n, idx: vec![] };
                let p = self.locate(&vr)?;
                let arr = self.arr_at(&p.loc);
                let dims = arr.map(|a| if a.dims.is_empty() { vec![1] } else { a.dims.clone() }).unwrap_or(vec![1]);
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
                i(to_half(x.trim()).parse().unwrap_or(0))
            }
            "ISNUMERIC" => {
                let x = self.as_(a, 0, "")?;
                i(to_half(x.trim()).parse::<i64>().is_ok() as i64)
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
            "ISSKIP" | "MOUSESKIP" | "MESSKIP" | "GETKEY" | "GETKEYTRIGGERED" | "MOUSEX" | "MOUSEY" | "MOUSEB" => i(0),
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
                let r = self.peek_save(n);
                match r {
                    Some(text) => {
                        self.set_results(text);
                        i(0)
                    }
                    None => i(1),
                }
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
            "GCREATED" | "GWIDTH" | "GHEIGHT" => i(0),
            "GETCONFIG" => {
                let k = self.as_(a, 0, "")?;
                i(match k.as_str() {
                    "フォントサイズ" | "FontSize" => self.con.font_size as i64,
                    "一行の高さ" | "LineHeight" => self.con.line_height as i64,
                    "ウィンドウ幅" | "WindowX" => (self.con.width as f32 * self.con.font_size / 2.0) as i64,
                    _ => 0,
                })
            }
            "GETCONFIGS" => s(String::new()),
            "HTML_ESCAPE" => {
                let x = self.as_(a, 0, "")?;
                s(x.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&apos;"))
            }
            "HTML_TOPLAINTEXT" => {
                let x = self.as_(a, 0, "")?;
                s(crate::html::to_plain(&x))
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

pub fn barstr(v: i64, m: i64, l: i64) -> String {
    let l = l.max(0);
    let filled = if m <= 0 { 0 } else { (v.max(0) * l / m).min(l) };
    format!("[{}{}]", "*".repeat(filled as usize), ".".repeat((l - filled) as usize))
}

/// .NET-style numeric format subset: "0000", "#,##0", "00.0"-less ints, "X"/"x"
pub fn format_number(v: i64, fmt: &str) -> String {
    if fmt.is_empty() {
        return v.to_string();
    }
    if fmt.eq_ignore_ascii_case("x") {
        return if fmt == "X" { format!("{:X}", v) } else { format!("{:x}", v) };
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

/// Minimal pattern matcher: supports literal text, '.', '*', '+', '?', '^', '$'
/// and character classes [abc]. Enough for typical FINDELEMENT/REGEXPMATCH use.
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
        Some('\\') => a.get(1) == Some(&c),
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

#[allow(dead_code)]
fn unused(_: Place) {}
