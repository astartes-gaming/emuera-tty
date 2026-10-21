//! The environment-variables page, opened from the settings overlay: every
//! `EMUERA_*` variable emuera-tty reads (see [`crate::settings::ENV_VARS`]),
//! with the value it sees now or "(not set)". Read-only: the variables are
//! read when emuera-tty starts, so they are changed in the shell before it
//! is launched. Enter on a row says what the variable does.

use super::saves::Pick;
use super::Tty;
use crate::console::Console;
use crate::frontend::InputReq;
use crate::resources::Resources;
use crate::settings::ENV_VARS;

/// `v` on one line, cut to `max` characters with a `…`.
fn short(v: &str, max: usize) -> String {
    let one: String = v.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    if one.chars().count() > max {
        format!("{}…", one.chars().take(max.saturating_sub(1)).collect::<String>())
    } else {
        one
    }
}

impl Tty {
    /// Run the page. Returns true when the player asked to quit.
    pub(super) fn env_panel(&mut self, con: &mut Console, res: &mut Resources, req: &InputReq) -> bool {
        let mut sel = 0usize;
        let mut help = String::from("● set  ○ not set. Enter: what it does. Read when emuera-tty starts: set it in the shell before launching");
        loop {
            let rows: Vec<String> = ENV_VARS
                .iter()
                .map(|(name, _)| match std::env::var_os(name) {
                    Some(v) => format!("● {:<24} = {}", name, short(&v.to_string_lossy(), 40)),
                    None => format!("○ {:<24}   (not set)", name),
                })
                .collect();
            match self.list_box(con, res, req, "Environment variables (EMUERA_*)", &rows, rows.len(), &mut sel, &help) {
                Pick::Back => return false,
                Pick::Quit => return true,
                Pick::Row(i) => {
                    let (name, what) = ENV_VARS[i];
                    help = format!("{}: {}", name, what);
                }
            }
        }
    }
}
