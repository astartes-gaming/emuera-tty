//! Emuera's built-in game flow (TITLE / FIRST / SHOP / TRAIN / ABLUP / ...).
//!
//! An Emuera game is a state machine the *engine* drives: each state calls
//! the script's event functions (`@SYSTEM_TITLE`, `@EVENTFIRST`,
//! `@SHOW_SHOP`, `@USERSHOP`…) and moves on when a script says
//! `BEGIN <state>`. [`Vm::run`] is that loop; [`Vm::run_state`] runs one
//! state and returns the next. Scripts that override a phase completely
//! (most modern games, AnonTW included, replace TITLE) still end up here
//! whenever they BEGIN another state.

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

    /// Call an event function if the game defines it.
    fn call_opt(&mut self, name: &str) -> VmResult<bool> {
        self.call_name(name, vec![])
    }

    /// An event that should end with BEGIN returned normally instead.
    fn need_begin(&mut self, after: &str) -> VmResult<String> {
        self.warn(&format!("@{} returned without BEGIN; going to SHOP", after));
        Ok("SHOP".into())
    }

    /// Run one system state; returns the state to go to next.
    /// `__LOADED` is internal: after a load, run @EVENTLOAD then the shop.
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

    /// "Start a new game" from the built-in title: reset data and add the
    /// initial characters.
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

    /// The default title screen, for games without @SYSTEM_TITLE.
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

    /// SHOP: @EVENTSHOP once, then repeat @SHOW_SHOP + number input.
    /// Numbers below 販売アイテム数 (_Replace.csv, default 100) that are on
    /// sale buy the item (@EVENTBUY); any other number goes to @USERSHOP
    /// with RESULT set (so menus can use 100+ freely, as in Emuera).
    fn shop(&mut self, skip_event: bool) -> VmResult<String> {
        if !skip_event {
            self.call_opt("EVENTSHOP")?;
            // Emuera autosaves each time SHOP is entered by BEGIN (not
            // right after a load)
            if self.save_opts.autosave {
                self.autosave()?;
            }
        }
        loop {
            self.call_opt("SHOW_SHOP")?;
            // like Emuera, the item list is only shown by PRINT_SHOPITEM
            let items = self.shop_items();
            let v = self.input_int(None, None, false)?;
            let max_item = self.csv.replace.get("販売アイテム数").and_then(|s| s.trim().parse::<i64>().ok()).unwrap_or(100);
            let is_item = v >= 0 && v < max_item && items.iter().any(|(i, _, _)| *i == v) && self.get_global_i("ITEMSALES", v as usize) != 0;
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

    /// Clear the per-training temporaries at the start of TRAIN.
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

    /// Is training command `n` available? (@COM_ABLEn, default yes.)
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

    /// Run training command `n`: @EVENTCOM, @COMn, @SOURCE_CHECK, @EVENTCOMEND.
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
            // like Emuera: wait after @EVENTCOMEND so its text can be read.
            // PREVCOM is the script's business (games set it in EVENTCOMEND).
            if self.call_opt("EVENTCOMEND")? {
                self.wait()?;
            }
        }
        Ok(())
    }

    /// TRAIN: list the available commands, run the chosen one, repeat.
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

    /// ABLUP: @SHOW_JUEL / @SHOW_ABLUP_SELECT, then @ABLUPn for the
    /// choice, or @USERABLUP (RESULT = the number) when there is none.
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

    /// The autosave: @SYSTEM_AUTOSAVE if the game has one, else slot 99
    /// with a timestamp and @SAVEINFO's text.
    fn autosave(&mut self) -> VmResult<()> {
        if self.call_opt("SYSTEM_AUTOSAVE")? {
            return Ok(());
        }
        let stamp = super::builtins::save_timestamp();
        self.globals.entry("SAVEDATA_TEXT".into()).or_insert_with(|| super::vars::Arr::new(true, &[1])).set(0, Value::Str(stamp));
        self.call_opt("SAVEINFO")?;
        let text = self.globals.get("SAVEDATA_TEXT").map(|a| a.get(0).into_string()).unwrap_or_default();
        if let Err(Ctrl::Error(e, _)) = self.save_slot(99, &text) {
            self.con.print(&format!("autosave failed: {}", e));
            self.con.newline();
        }
        Ok(())
    }

    /// Built-in save/load menu. Returns true if something was saved/loaded.
    pub fn save_menu(&mut self, save: bool) -> VmResult<bool> {
        loop {
            self.con.drawline();
            self.con.print(if save { "どこにセーブしますか？" } else { "どれをロードしますか？" });
            self.con.newline();
            for n in 0..20 {
                let label = format!("[{:2}] {}", n, self.check_save(n).mes);
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
                // as Emuera: a timestamp, then whatever @SAVEINFO PUTFORMs
                let stamp = super::builtins::save_timestamp();
                self.globals.entry("SAVEDATA_TEXT".into()).or_insert_with(|| super::vars::Arr::new(true, &[1])).set(0, Value::Str(stamp));
                if self.has_func("SAVEINFO") {
                    self.call_name("SAVEINFO", vec![])?;
                }
                let text = self.globals.get("SAVEDATA_TEXT").map(|a| a.get(0).into_string()).unwrap_or_default();
                self.save_slot(v, &text)?;
                return Ok(true);
            } else if self.check_save(v).state == 0 {
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
