//! Emuera's built-in game flow (TITLE / FIRST / SHOP / TRAIN / ABLUP / ...).

use super::value::*;
use super::Vm;
use crate::console::Rgb;

impl Vm {
    /// Main loop. Returns Err(message) on a fatal script error.
    pub fn run(&mut self) -> Result<(), String> {
        let mut state = "TITLE".to_string();
        loop {
            self.state = state.clone();
            let r = self.run_state(&state);
            match r {
                Ok(next) => state = next,
                Err(Ctrl::Begin(s)) => state = s,
                Err(Ctrl::Quit) => {
                    self.flush();
                    return Ok(());
                }
                Err(Ctrl::Error(m, _)) => {
                    self.frames.clear();
                    if !self.con.cur.is_empty() {
                        self.con.newline();
                    }
                    self.con.skip_disp = false;
                    let st = self.con.style.clone();
                    self.con.style.fg = Rgb(255, 80, 80);
                    for l in format!("*** script error ***\n{}", m).lines() {
                        self.con.print_ex(l, false);
                        self.con.newline();
                    }
                    self.con.style = st;
                    self.con.print_ex("(press Enter to quit)", false);
                    self.con.newline();
                    let _ = self.wait();
                    self.fe.shutdown();
                    return Err(m);
                }
            }
        }
    }

    fn call_opt(&mut self, name: &str) -> VmResult<bool> {
        self.call_name(name, vec![])
    }

    fn need_begin(&mut self, after: &str) -> VmResult<String> {
        self.warn(&format!("@{} returned without BEGIN; going to SHOP", after));
        Ok("SHOP".into())
    }

    pub fn run_state(&mut self, st: &str) -> VmResult<String> {
        match st {
            "TITLE" => {
                if self.call_opt("SYSTEM_TITLE")? {
                    return self.need_begin("SYSTEM_TITLE");
                }
                self.builtin_title()
            }
            "FIRST" => {
                if self.call_opt("EVENTFIRST")? {
                    return self.need_begin("EVENTFIRST");
                }
                err("@EVENTFIRST not found")
            }
            "__LOADED" => {
                self.call_opt("EVENTLOAD")?;
                self.shop(true)
            }
            "SHOP" => self.shop(false),
            "TRAIN" => self.train(),
            "AFTERTRAIN" => {
                self.call_opt("EVENTEND")?;
                self.need_begin("EVENTEND")
            }
            "ABLUP" => self.ablup(),
            "TURNEND" => {
                self.call_opt("EVENTTURNEND")?;
                self.need_begin("EVENTTURNEND")
            }
            "SAVEGAME" => {
                self.save_menu(true)?;
                Ok("SHOP".into())
            }
            "LOADGAME" => {
                if self.save_menu(false)? {
                    Ok("__LOADED".into())
                } else {
                    Ok("TITLE".into())
                }
            }
            "DATALOADED" => Ok("__LOADED".into()),
            other => err(format!("BEGIN {}: unknown system state", other)),
        }
    }

    pub fn new_game(&mut self) -> VmResult<()> {
        self.init_data();
        // Emuera adds chara 番号0 (if defined) as MASTER, then 最初からいるキャラ
        if self.csv.charas.iter().any(|c| c.no == 0) {
            let c = self.new_chara_from_csv(0)?;
            self.charas.push(c);
        }
        if let Some(list) = self.csv.gamebase.get("最初からいるキャラ").cloned() {
            for no in list.split(',').filter_map(|s| s.trim().parse::<i64>().ok()) {
                if no != 0 {
                    let c = self.new_chara_from_csv(no)?;
                    self.charas.push(c);
                }
            }
        }
        Ok(())
    }

    fn builtin_title(&mut self) -> VmResult<String> {
        let gb = |k: &str, vm: &Vm| vm.csv.gamebase.get(k).cloned().unwrap_or_default();
        let save_align = self.con.align;
        self.con.align = crate::ast::Align::Center;
        self.con.drawline();
        for k in ["タイトル", "作者", "製作年", "追加情報"] {
            let v = gb(k, self);
            if !v.is_empty() {
                self.con.print(&v);
                self.con.newline();
            }
        }
        let ver = gb("バージョン", self);
        if let Ok(v) = ver.trim().parse::<i64>() {
            self.con.print(&format!("v{}.{:03}", v / 1000, v % 1000));
            self.con.newline();
        }
        self.con.drawline();
        self.con.print_button("[0] 最初からはじめる", Value::Int(0));
        self.con.newline();
        self.con.print_button("[1] ロードしてはじめる", Value::Int(1));
        self.con.newline();
        self.con.align = save_align;
        loop {
            let v = self.input_int(None, None, false)?;
            match v {
                0 => {
                    self.new_game()?;
                    return Ok("FIRST".into());
                }
                1 => {
                    if self.save_menu(false)? {
                        return Ok("__LOADED".into());
                    }
                    return Ok("TITLE".into());
                }
                _ => {}
            }
        }
    }

    fn shop(&mut self, skip_event: bool) -> VmResult<String> {
        if !skip_event {
            self.call_opt("EVENTSHOP")?;
        }
        loop {
            self.call_opt("SHOW_SHOP")?;
            // like Emuera, the item list is only shown by PRINT_SHOPITEM
            let items = self.shop_items();
            let v = self.input_int(None, None, false)?;
            let is_item = v >= 0 && items.iter().any(|(i, _, _)| *i == v) && self.get_global_i("ITEMSALES", v as usize) != 0;
            if is_item {
                let price = self.get_global_i("ITEMPRICE", v as usize);
                let money = self.get_global_i("MONEY", 0);
                if money >= price {
                    self.set_global_i("MONEY", 0, money - price);
                    let n = self.get_global_i("ITEM", v as usize);
                    self.set_global_i("ITEM", v as usize, n + 1);
                    self.set_global_i("BOUGHT", 0, v);
                    self.call_opt("EVENTBUY")?;
                } else {
                    self.con.print("お金が足りません");
                    self.con.newline();
                }
            } else {
                self.set_result(v);
                self.call_opt("USERSHOP")?;
            }
        }
    }

    fn train_reset(&mut self) {
        self.set_global_i("ASSIPLAY", 0, 0);
        self.set_global_i("PREVCOM", 0, -1);
        self.set_global_i("NEXTCOM", 0, -1);
        self.global_arr("TFLAG").clear();
        for c in 0..self.charas.len() {
            for v in ["TEQUIP", "PALAM", "SOURCE", "GOTJUEL", "NOWEX", "EX"] {
                self.chara_arr(c, v).clear();
            }
        }
    }

    fn com_able(&mut self, n: i64) -> VmResult<bool> {
        let name = format!("COM_ABLE{}", n);
        if self.has_func(&name) {
            self.set_result(0);
            self.call_name(&name, vec![])?;
            Ok(self.get_global_i("RESULT", 0) != 0)
        } else {
            Ok(true)
        }
    }

    pub fn do_train_command(&mut self, n: i64) -> VmResult<()> {
        self.set_global_i("SELECTCOM", 0, n);
        self.global_arr("UP").clear();
        self.global_arr("DOWN").clear();
        self.global_arr("LOSEBASE").clear();
        let t = self.target();
        if t >= 0 && (t as usize) < self.charas.len() {
            self.chara_arr(t as usize, "SOURCE").clear();
        }
        self.call_opt("EVENTCOM")?;
        self.set_result(1);
        self.call_name(&format!("COM{}", n), vec![])?;
        if self.get_global_i("RESULT", 0) != 0 {
            self.call_opt("SOURCE_CHECK")?;
            self.call_opt("EVENTCOMEND")?;
        }
        self.set_global_i("PREVCOM", 0, n);
        Ok(())
    }

    fn train(&mut self) -> VmResult<String> {
        self.train_reset();
        self.call_opt("EVENTTRAIN")?;
        loop {
            let next = self.get_global_i("NEXTCOM", 0);
            if next >= 0 {
                self.set_global_i("NEXTCOM", 0, -1);
                self.do_train_command(next)?;
                continue;
            }
            self.call_opt("SHOW_STATUS")?;
            let mut coms: Vec<(i64, String)> = self
                .csv
                .tables
                .get("TRAIN")
                .map(|t| t.by_index.iter().map(|(k, v)| (*k, v.clone())).collect())
                .unwrap_or_default();
            coms.sort();
            let mut avail = Vec::new();
            let mut col = 0;
            for (i, name) in coms {
                if self.com_able(i)? {
                    let s = crate::vm::pad(&format!("{}[{:3}]", name, i), self.con.printc_len, crate::ast::Align::Right);
                    self.con.print(&s);
                    col += 1;
                    if col % 3 == 0 {
                        self.con.newline();
                    }
                    avail.push(i);
                }
            }
            if !self.con.cur.is_empty() {
                self.con.newline();
            }
            self.call_opt("SHOW_USERCOM")?;
            let v = self.input_int(None, None, false)?;
            if avail.contains(&v) {
                self.do_train_command(v)?;
            } else {
                self.set_result(v);
                self.call_opt("USERCOM")?;
            }
        }
    }

    fn ablup(&mut self) -> VmResult<String> {
        loop {
            self.call_opt("SHOW_JUEL")?;
            self.call_opt("SHOW_ABLUP_SELECT")?;
            let v = self.input_int(None, None, false)?;
            let name = format!("ABLUP{}", v);
            if v >= 0 && self.has_func(&name) {
                self.call_name(&name, vec![])?;
            } else {
                self.set_result(v);
                self.call_opt("USERABLUP")?;
            }
        }
    }

    /// Built-in save/load menu. Returns true if something was saved/loaded.
    pub fn save_menu(&mut self, save: bool) -> VmResult<bool> {
        loop {
            self.con.drawline();
            self.con.print(if save { "どこにセーブしますか？" } else { "どれをロードしますか？" });
            self.con.newline();
            for n in 0..20 {
                let t = self.peek_save(n);
                let label = match &t {
                    Some(t) => format!("[{:2}] {}", n, t),
                    None => format!("[{:2}] ----", n),
                };
                self.con.print_button(&label, Value::Int(n));
                self.con.newline();
            }
            self.con.print_button("[100] 戻る", Value::Int(100));
            self.con.newline();
            let v = self.input_int(None, None, false)?;
            if v == 100 {
                return Ok(false);
            }
            if !(0..20).contains(&v) {
                continue;
            }
            if save {
                self.globals.remove("SAVEDATA_TEXT");
                if self.has_func("SAVEINFO") {
                    self.call_name("SAVEINFO", vec![])?;
                }
                let text = self
                    .globals
                    .get("SAVEDATA_TEXT")
                    .map(|a| a.get(0).into_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| {
                        let t = super::builtins::format_number(self.get_global_i("DAY", 0), "");
                        format!("DAY {}", t)
                    });
                self.save_slot(v, &text)?;
                return Ok(true);
            } else if self.peek_save(v).is_some() {
                return self.load_slot(v);
            }
        }
    }
}

impl Vm {
    /// (number, name) of every item in Item.csv.
    fn shop_items(&self) -> Vec<(i64, String, i64)> {
        let mut items: Vec<(i64, String, i64)> = Vec::new();
        if let Some(t) = self.csv.tables.get("ITEM") {
            let mut idx: Vec<i64> = t.by_index.keys().copied().collect();
            idx.sort();
            for i in idx {
                items.push((i, t.by_index[&i].clone(), 0));
            }
        }
        items
    }

    /// PRINT_SHOPITEM: the items whose ITEMSALES is set, three per line.
    pub fn print_shop_items(&mut self) {
        let items = self.shop_items();
        let mut shown = 0;
        for (i, name, _) in &items {
            let i = *i;
            if self.get_global_i("ITEMSALES", i.max(0) as usize) != 0 {
                let price = self.get_global_i("ITEMPRICE", i.max(0) as usize);
                let label = self.csv.gamebase.get("お金の単位").cloned().unwrap_or_else(|| "$".into());
                let s = crate::vm::pad(&format!("[{}] {}({}{})", i, name, price, label), self.con.printc_len, crate::ast::Align::Left);
                self.con.print(&s);
                shown += 1;
                if shown % 3 == 0 {
                    self.con.newline();
                }
            }
        }
        if !self.con.cur.is_empty() {
            self.con.newline();
        }
    }
}
