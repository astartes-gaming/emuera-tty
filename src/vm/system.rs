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
        // like Emuera, each system phase starts with SKIPDISP off
        self.con.skip_disp = false;
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
            // FLOWINPUT can give the shop input a default, make it a
            // string input or skip it
            let (fdef, fon, fstr, fdef_s, fskip) = self.flow_input.clone();
            let v = if fskip {
                if fstr {
                    self.set_results(fdef_s);
                }
                fdef
            } else if fstr {
                let s = self.input_str(fon.then_some(fdef_s), None, false)?;
                let n = s.trim().parse().unwrap_or(fdef);
                self.set_results(s);
                n
            } else {
                self.input_int(fon.then_some(fdef), None, false)?
            };
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

    /// STAIN's starting values: `_Replace.csv` 汚れの初期値 (`a/b/c…`),
    /// else Emuera's 0/0/2/1/8.
    pub fn stain_default(&self) -> Vec<i64> {
        match self.csv.replace.get("汚れの初期値") {
            Some(s) => s.split('/').filter_map(|x| x.trim().parse().ok()).collect(),
            None => vec![0, 0, 2, 1, 8],
        }
    }

    /// Character `c`'s STAIN back to the defaults (RESET_STAIN, TRAIN start).
    pub fn set_default_stain(&mut self, c: usize) {
        let d = self.stain_default();
        let a = self.chara_arr(c, "STAIN");
        a.clear();
        for (i, v) in d.into_iter().enumerate().take(a.len()) {
            a.set_i(i, v);
        }
    }

    /// The start of TRAIN (Emuera's UpdateInBeginTrain): every character's
    /// training temporaries, not just TARGET's.
    fn train_reset(&mut self) {
        self.set_global_i("ASSIPLAY", 0, 0);
        self.set_global_i("PREVCOM", 0, -1);
        self.set_global_i("NEXTCOM", 0, -1);
        self.global_arr("TFLAG").clear();
        self.global_arr("TSTR").clear();
        for c in 0..self.charas.len() {
            for v in ["GOTJUEL", "TEQUIP", "EX", "PALAM", "SOURCE", "TCVAR"] {
                self.chara_arr(c, v).clear();
            }
            self.set_default_stain(c);
        }
    }

    /// After SHOW_USERCOM (and before DOTRAIN): UP, DOWN, LOSEBASE and the
    /// characters' DOWNBASE, CUP, CDOWN start from zero.
    pub fn train_after_usercom(&mut self) {
        for v in ["UP", "DOWN", "LOSEBASE"] {
            self.global_arr(v).clear();
        }
        for c in 0..self.charas.len() {
            for v in ["DOWNBASE", "CUP", "CDOWN"] {
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

    /// Call a TRAIN event in which DOTRAIN may be used. Returns the command
    /// DOTRAIN chose: Emuera runs it once the event has ended, in place of
    /// what would have come next.
    fn train_event(&mut self, name: &str) -> VmResult<Option<i64>> {
        self.dotrain_ok = true;
        let r = self.call_opt(name);
        self.dotrain_ok = false;
        r?;
        Ok(self.dotrain.take())
    }

    /// Run training command `n` as Emuera does: NOWEX cleared, @EVENTCOM,
    /// @COMn; if that succeeded (RESULT ≠ 0) @SOURCE_CHECK, SOURCE cleared,
    /// @EVENTCOMEND and, unless it waited, a WAIT so its text can be read.
    /// PREVCOM is the script's business. Returns a DOTRAIN from @EVENTCOMEND.
    pub fn do_train_command(&mut self, n: i64) -> VmResult<Option<i64>> {
        for c in 0..self.charas.len() {
            self.chara_arr(c, "NOWEX").clear();
        }
        self.set_global_i("SELECTCOM", 0, n);
        self.call_opt("EVENTCOM")?;
        self.set_result(1);
        self.call_name(&format!("COM{}", n), vec![])?;
        if self.get_global_i("RESULT", 0) != 0 {
            self.call_opt("SOURCE_CHECK")?;
            for c in 0..self.charas.len() {
                self.chara_arr(c, "SOURCE").clear();
            }
            self.comend_wait = true;
            if let Some(d) = self.train_event("EVENTCOMEND")? {
                return Ok(Some(d));
            }
            if self.comend_wait {
                self.con.skip_disp = false;
                self.wait()?;
            }
        }
        Ok(None)
    }

    /// TRAIN: the commands TRAIN.csv defines that @COM_ABLEn allows, listed
    /// with their position among TRAIN.csv's entries (that is the number
    /// typed, as in Emuera); any other number goes to @USERCOM. NEXTCOM ≥ 0
    /// runs that command first and is set to 0 (eramaker's rule: a script
    /// that doesn't reset it repeats it). A DOTRAIN in an event runs its
    /// command when the event ends, the screens in between skipped.
    fn train(&mut self) -> VmResult<String> {
        self.train_reset();
        self.dotrain = None;
        let mut dotrain = self.train_event("EVENTTRAIN")?;
        loop {
            if let Some(n) = dotrain.take() {
                self.con.skip_disp = false;
                self.train_after_usercom();
                dotrain = self.do_train_command(n)?;
                continue;
            }
            let next = self.get_global_i("NEXTCOM", 0);
            if next >= 0 {
                self.set_global_i("NEXTCOM", 0, 0);
                dotrain = self.do_train_command(next)?;
                continue;
            }
            // CALLTRAIN: the screens are run without output, the next
            // queued command instead of an input
            let ctrain = self.ctrain.is_some();
            if ctrain {
                self.con.skip_disp = true;
                self.con.skip_user = false;
            }
            dotrain = self.train_event("SHOW_STATUS")?;
            if dotrain.is_some() {
                continue;
            }
            let mut coms: Vec<(i64, String)> = self
                .csv
                .tables
                .get("TRAIN")
                .map(|t| t.by_index.iter().map(|(k, v)| (*k, v.clone())).collect())
                .unwrap_or_default();
            coms.sort();
            // position in TRAIN.csv → command number, for the allowed ones
            let mut avail: Vec<Option<i64>> = vec![None; coms.len()];
            let per_line = self.con.printc_per_line.max(1);
            let mut shown = 0;
            for (k, (id, name)) in coms.into_iter().enumerate() {
                if self.com_able(id)? {
                    avail[k] = Some(id);
                    if ctrain {
                        continue;
                    }
                    let s = crate::vm::pad(&format!("{}[{:3}]", self.csv.tr.or_same("TRAIN", &name), k), self.con.printc_len, crate::ast::Align::Right);
                    self.con.print(&s);
                    shown += 1;
                    if shown % per_line == 0 {
                        self.con.newline();
                    }
                }
            }
            if !self.con.cur.is_empty() {
                self.con.newline();
            }
            dotrain = self.train_event("SHOW_USERCOM")?;
            if dotrain.is_some() {
                continue;
            }
            self.con.skip_disp = false;
            self.train_after_usercom();
            if let Some((list, done)) = self.ctrain.clone() {
                if done >= list.len() {
                    self.ctrain = None;
                    self.call_opt("CALLTRAINEND")?;
                    continue;
                }
                // queued commands are command numbers, not list positions
                let id = list[done];
                self.ctrain = Some((list.clone(), done + 1));
                self.con.print(&format!("＜コマンド連続実行：{}/{}＞", done + 1, list.len()));
                self.con.newline();
                if avail.contains(&Some(id)) {
                    dotrain = self.do_train_command(id)?;
                } else {
                    self.con.print("コマンドを実行できませんでした");
                    self.con.newline();
                    self.set_result(id);
                    dotrain = self.train_event("USERCOM")?;
                }
                if self.ctrain.as_ref().is_some_and(|(l, d)| *d >= l.len()) {
                    self.ctrain = None;
                    self.call_opt("CALLTRAINEND")?;
                }
                continue;
            }
            let v = self.input_int(None, None, false)?;
            dotrain = match usize::try_from(v).ok().and_then(|k| avail.get(k).copied().flatten()) {
                Some(id) => self.do_train_command(id)?,
                None => {
                    self.set_result(v);
                    self.train_event("USERCOM")?
                }
            };
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
        self.globals.get_or_insert_with("SAVEDATA_TEXT", || super::vars::Arr::new(true, &[1])).set(0, Value::Str(stamp));
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
        // as Emuera: the configured number of slots, 20 a page, the other
        // pages as buttons; loading also offers the autosave (99)
        let slots = self.save_opts.slots.clamp(20, 80);
        let mut page = 0;
        loop {
            self.con.drawline();
            self.con.print(if save { "どこにセーブしますか？" } else { "どれをロードしますか？" });
            self.con.newline();
            let page_button = |vm: &mut Vm, p: i64| {
                vm.con.print_button(&format!("[{:2}] セーブデータ{:2}～{:2}を表示", p * 20, p * 20, p * 20 + 19), Value::Int(p * 20));
                vm.con.newline();
            };
            for p in 0..page {
                page_button(self, p);
            }
            for n in page * 20..(page * 20 + 20).min(slots) {
                let label = format!("[{:2}] {}", n, self.check_save(n).mes);
                self.con.print_button(&label, Value::Int(n));
                self.con.newline();
            }
            for p in page + 1..(slots + 19) / 20 {
                page_button(self, p);
            }
            if !save {
                let label = format!("[{:2}] {}", 99, self.check_save(99).mes);
                self.con.print_button(&label, Value::Int(99));
                self.con.newline();
            }
            self.con.print_button("[100] 戻る", Value::Int(100));
            self.con.newline();
            let v = self.input_int(None, None, false)?;
            if v == 100 {
                return Ok(false);
            }
            let autosave = !save && v == 99;
            if !(0..slots).contains(&v) && !autosave {
                continue;
            }
            if !autosave && v / 20 != page {
                page = v / 20;
                continue;
            }
            if save {
                // as Emuera: a timestamp, then whatever @SAVEINFO PUTFORMs
                let stamp = super::builtins::save_timestamp();
                self.globals.get_or_insert_with("SAVEDATA_TEXT", || super::vars::Arr::new(true, &[1])).set(0, Value::Str(stamp));
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

    /// A `_Replace.csv` setting by its Japanese or English name.
    pub fn replace_value(&self, jp: &str) -> Option<String> {
        self.csv.replace.get(jp).or_else(|| self.csv.replace.iter().find(|(k, _)| crate::config_names::canonical(k) == jp).map(|(_, v)| v)).map(|v| v.trim().to_string())
    }

    /// The currency unit (`_Replace.csv` お金の単位, default `$`) and whether
    /// it goes before the number (単位の位置, default yes).
    pub fn money_label(&self) -> (String, bool) {
        let label = self.replace_value("お金の単位").or_else(|| self.csv.gamebase.get("お金の単位").cloned()).unwrap_or_else(|| "$".into());
        let first = self.replace_value("単位の位置").map_or(true, |s| !matches!(s.to_uppercase().as_str(), "後" | "NO" | "FALSE" | "0" | "AFTER"));
        (label, first)
    }

    /// `amount` with the currency unit, as MONEYSTR / PRINT_SHOPITEM write it.
    pub fn with_money_label(&self, amount: &str) -> String {
        let (label, first) = self.money_label();
        if first { format!("{}{}", label, amount) } else { format!("{}{}", amount, label) }
    }

    /// PRINT_SHOPITEM: the items whose ITEMSALES is set, PRINTCPERLINE per
    /// line, as `[n] name($price)`.
    pub fn print_shop_items(&mut self) {
        let items = self.shop_items();
        let mut shown = 0;
        for (i, name, _) in &items {
            let i = *i;
            if self.get_global_i("ITEMSALES", i.max(0) as usize) != 0 {
                let price = self.get_global_i("ITEMPRICE", i.max(0) as usize);
                let s = crate::vm::pad(&format!("[{}] {}({})", i, self.csv.tr.or_same("ITEM", name), self.with_money_label(&price.to_string())), self.con.printc_len + 1, crate::ast::Align::Left);
                self.con.print(&s);
                shown += 1;
                if shown % self.con.printc_per_line.max(1) == 0 {
                    self.con.newline();
                }
            }
        }
        if !self.con.cur.is_empty() {
            self.con.newline();
        }
    }
}
