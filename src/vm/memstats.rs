//! `EMUERA_MEMSTATS=<file>`: a report of what the memory holds, rewritten
//! at every input (for finding what to cut on small machines). Sizes are
//! the heap bytes of the data itself; allocator overhead comes on top.

use super::Vm;
use std::fmt::Write as _;

/// Bytes as "12.3 MB".
fn mb(b: usize) -> String {
    format!("{:.1} MB", b as f64 / (1 << 20) as f64)
}

impl Vm {
    pub fn mem_report(&self) -> String {
        let mut out = String::new();
        #[cfg(target_os = "linux")]
        if let Ok(st) = std::fs::read_to_string("/proc/self/status") {
            for l in st.lines().filter(|l| l.starts_with("VmRSS") || l.starts_with("VmHWM")) {
                let _ = writeln!(out, "{}", l.split_whitespace().collect::<Vec<_>>().join(" "));
            }
        }
        // functions: all have a header, the ones run lately a body
        let (mut n, mut compiled, mut instrs) = (0, 0, 0);
        for f in self.prog.funcs.values().flatten() {
            n += 1;
            if let Some(b) = f.body.get() {
                compiled += 1;
                instrs += b.code.len();
            }
        }
        let _ = writeln!(
            out,
            "functions: {} ({} compiled, {} instructions, about {}; may drop beyond {})",
            n,
            compiled,
            instrs,
            mb(self.code_bytes),
            if self.code_budget == usize::MAX { "-".to_string() } else { mb(self.code_budget) }
        );
        let mut big: Vec<(usize, &str)> = self.prog.funcs.values().flatten().filter_map(|f| f.body.get().map(|b| (b.code.len(), f.name.as_str()))).collect();
        big.sort_unstable_by(|a, b| b.cmp(a));
        for (n, name) in big.iter().take(5) {
            let _ = writeln!(out, "  @{:<23} {} instructions", name, n);
        }
        let mut globals: Vec<(&str, usize)> = self.globals.iter().map(|(n, a)| (n.as_str(), a.bytes())).collect();
        globals.sort_by_key(|g| std::cmp::Reverse(g.1));
        let _ = writeln!(out, "global variables: {} ({})", globals.len(), mb(globals.iter().map(|g| g.1).sum()));
        for (name, b) in globals.iter().take(12) {
            let _ = writeln!(out, "  {:<24} {}", name, mb(*b));
        }
        let statics: usize = self.statics.iter().flatten().map(|a| a.bytes()).sum();
        let _ = writeln!(out, "function variables: {} ({})", self.statics.iter().flatten().count(), mb(statics));
        let chara: usize = self.charas.iter().map(|c| c.vars.values().map(|a| a.bytes()).sum::<usize>()).sum();
        let _ = writeln!(out, "characters: {} ({})", self.charas.len(), mb(chara));
        // the game's own pictures, then the sprite pictures decoded for the
        // screen (the ones Images: none skips)
        let (game, sprites) = self.res.mem_stats();
        for (title, group) in [("game-made graphics (kept whatever Images is set to)", game), ("sprite images for the screen (skipped while Images is none)", sprites)] {
            // "  of them…" rows are part of the row above, not extra memory
            let total: usize = group.iter().filter(|g| !g.0.starts_with("  ")).map(|g| g.2).sum();
            let _ = writeln!(out, "{}: {}", title, mb(total));
            for (what, n, b) in group {
                let _ = writeln!(out, "  {}: {} ({})", what, n, mb(b));
            }
        }
        let _ = writeln!(out, "console: {} lines", self.con.lines.len());
        let _ = writeln!(
            out,
            "maps: {}, data tables: {} ({} rows), xml documents: {}",
            self.maps.len(),
            self.dts.len(),
            self.dts.values().map(|d| d.rows().len()).sum::<usize>(),
            self.xml_docs.len()
        );
        out
    }
}
