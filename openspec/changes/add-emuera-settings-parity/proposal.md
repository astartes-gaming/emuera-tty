# Proposal

## Why

Emuera's settings window (`UI/Framework/Forms/ConfigDialog.cs`) and debug window (`DebugDialog.cs`) are how players and game authors control the engine, and games ship `_fixed.config` / `_default.config` files that set them. emuera-tty reads only display, save and clipboard keys: every setting that changes how scripts are read or run (the System and Compatibility tabs) is ignored, so a game that turns one away from Emuera's default (21 local games turn on automatic TOSTR for user-function arguments, 19 turn off full-width spaces as whitespace, 14 turn off triple-symbol expansion in FORM) runs differently here. The debug window's console, watch-list file and `@` commands typed at the game's input have no equivalent.

## What Changes

- One parsed model of every Emuera config key (all tabs, Japanese or English names, Emuera's defaults and value ranges), read once in Emuera's file order and available to the loader, parser and VM. Fixes the unmapped English key for LOADTEXT/SAVETEXT extensions.
- Every setting of the System, Compatibility and Debug tabs honoured as Emuera does, each checked against Emuera's source and covered by a test; settings already hardwired to one behaviour (full-width space, `_Rename.csv`, `_Replace.csv`, case) follow the config.
- Every other setting mapped to a terminal equivalent or recorded as not applicable, with the reason, in one table (`doc/settings-parity.md`), so nothing is silently dropped.
- The game settings page (F3 → game settings) gains the tabs and items Emuera's window has, with settings that don't apply in a terminal shown as such rather than hidden.
- Emuera's `@` input commands: `@REBOOT`, `@OUTPUT`/`@OUTPUTLOG`, `@QUIT`/`@EXIT`, `@CONFIG`, `@DEBUG`, and with デバッグコマンドを使用する any other statement or expression, with Emuera's restrictions.
- The debug window's features in the TUI: a debug panel (opened by `@DEBUG` or a key, in debug mode) with the variable watch list (saved to and loaded from `debug/watchlist.csv` as Emuera does), the stack trace, and the debug console (DEBUGPRINT output, statement input with history, saved to `debug/console.log`); Emuera's debug-window settings (show at start).

## Capabilities

### New Capabilities

- `game-config`: which Emuera settings emuera-tty reads, from where, and what each does here.
- `debug-tools`: the debug mode features: `@` commands, debug console, variable watch, stack trace.

### Modified Capabilities

None.

## Impact

- `src/emuconfig.rs`, `src/config_names.rs` (model), `src/loader.rs`, `src/parser.rs`, `src/expr.rs`, `src/csvdata.rs`, `src/vm/` (behaviours), `src/tty/gamecfg.rs` (page), `src/tty.rs` + new `src/tty/debugpanel.rs`, `src/vm/debugger.rs`.
- Tests: ERB checks per setting (a test game per non-default value), headless `@` command tests.
- Docs: `doc/settings-parity.md` (new), wiki *Settings* and *Debugging*, `doc/differences.md` for anything deliberately different.
- Saves unaffected.
