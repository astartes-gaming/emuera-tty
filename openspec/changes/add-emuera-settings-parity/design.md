# Design

## Context

Settings live in Emuera's config files: `CSV/_default.config`, the game's `emuera.config` (where the settings window saves), `CSV/_fixed.config` (wins, and locks the item in the window). Keys are Japanese, or English in EE builds that write English (`CONFIGファイルの内容を英語で保存する`). `emuconfig::load` reads a handful into `Console` / `SaveOpts`; `Values` models the page. Nothing else reads config, and the parser and lexer hardwire Emuera's defaults (or what most games use) for the script-behaviour settings.

The inventory (93 settings in the dialog, plus the debug window's own) is kept as `doc/settings-parity.md`: setting, Emuera's default, what local games set, emuera-tty's handling (honoured / terminal equivalent / not applicable, with why).

## Goals / Non-Goals

**Goals:** every setting accounted for; every script-behaviour setting honoured; the debug window's features usable in a terminal; one config model instead of scattered key matching.

**Non-goals:** window geometry, FPS, drawing interface, menu bar, multiple instances, text-editor integration (`関連づけるテキストエディタ`) beyond opening `$EDITOR` from the debug panel, Rikaichan (a dictionary popup; noted as a possible later feature), plugins. These are recorded as not applicable.

## Decisions

### One config model

`emuconfig::Config`: every key from `config_names::NAMES` with its kind and Emuera default (from `ConfigData.cs`), parsed once at load (and on reload). Typed accessors (`cfg.bool(Key::CompatiRAND)`) replace string matching. Parser and lexer settings are needed before the VM exists, and from parse threads, so the parsed `Config` is passed into `Parser::new` and the loader; the few lexer-level flags (full-width space, triple symbols) live in an `Arc<Config>` the lexer reads, not in globals, so `--check` of two games in one process can't mix them.

### Defaults where a game sets nothing

Emuera's defaults, not "what most games use". Today some behaviours were hardwired to the common game value (e.g. `_Rename.csv` always read): a game without the key in any config file now gets Emuera's default. Before switching each one, check the local games without a config file still behave (most ship `_default.config`; a game with none runs on Emuera's defaults in Emuera too, so this matches).

### `@` commands

Handled where an input line is accepted (`Vm::raw_input` or the frontend's line submission), before macro expansion, as `doSystemCommand` does: only for typed (not mouse-clicked) input longer than one character on a non-ONEINPUT request. Statements run through the VM's existing `EMUERA_INJECT` path (`@__INJECT`), with Emuera's restrictions (no flow control, no input waits, METHOD_SAFE only, no SAVEDATA/PUTFORM/UPCHECK/CUPCHECK), and state restored afterwards. An expression alone prints its value (`PRINTVL` / `PRINTFORMSL`), as Emuera does.

### Debug panel

A TUI overlay like the settings page, three tabs (watch, stack trace, console). It reuses `debugger.rs`'s watches and backtrace; the REPL debugger stays as the breakpoint/step tool. Files are Emuera's (`debug/watchlist.csv`, `debug/console.log`, in the game folder's `debug/`) so the lists move between engines. Debug mode is `--game-debug` (Emuera's `-Debug`).

## Risks / Trade-offs

- Honouring a setting can change a game that ran fine here with the hardwired value → each switch is tested on the local games that set the key, and their configs are checked for the value they expect.
- More config read before parsing touches start-up time → one more pass over three small files; measured on pops-tw.
- The page grows from ~25 to ~95 items → Emuera's tab layout, with not-applicable items greyed out and explained in one line.

## Open Questions

- Whether script-behaviour settings on the page should be editable (Emuera allows it, and the change applies after a restart) or shown read-only: changing one can break a game. Default here: editable, with Emuera's "takes effect after restart" note, since a `_fixed.config` key is locked anyway.
