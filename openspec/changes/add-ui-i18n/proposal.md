## Why

Most players of these games read Japanese, with large Simplified Chinese,
Traditional Chinese and Korean communities, but everything emuera-tty itself
says (settings, launcher, save manager, prompts, its own errors) is in
English and can't be translated. Adding translations later should only need
a translator, not code changes. While checking that other scripts lay out
correctly, it turned out combining marks (Hindi's vowel signs, accents
written as separate characters) are dropped from the screen entirely, in
game text as well as the interface.

## What Changes

- emuera-tty's own interface text goes through a message catalogue: gettext
  `.po` files compiled into the binary, English as the source language and
  the fallback for anything untranslated.
- A **Language** setting (`auto`, English, 日本語, 简体中文, 繁體中文, 한국어),
  `auto` following the system's locale.
- Catalogue files for Japanese, Simplified Chinese, Traditional Chinese and
  Korean start empty (English shows until someone translates). Only the
  framework is in scope, not the translations.
- Interface layout measures display width everywhere (two columns for CJK
  characters), so boxes, menus and the bottom row stay aligned in any
  language.
- Combining marks are drawn with the character they belong to instead of
  being dropped, in game text and interface text alike.
- Tests render the interface in pseudo-translations written in CJK, Arabic,
  Cyrillic, Devanagari and Old Avestan scripts and check the layout.

Out of scope: translating game text (Emuera-Anchor's translation files and
the live translator already cover that), translating the README and man
page, and right-to-left reordering (terminals handle bidirectional text
themselves, or don't; emuera-tty writes text in logical order).

## Capabilities

### New Capabilities

- `ui-localization`: emuera-tty's own interface text in the player's
  language, chosen by setting or locale, with English as the fallback, and
  catalogues a translator can fill in without touching code.
- `text-cell-layout`: how text of any script occupies terminal cells: display
  widths, combining marks kept with their base character, characters beyond
  the Basic Multilingual Plane, and right-to-left text left in logical order.

### Modified Capabilities

(none: no specs exist yet)

## Impact

- New `src/i18n.rs` (catalogue loading, lookup, locale detection) and
  `i18n/*.po` plus a `.pot` template.
- Every interface string in `tty.rs`, `tty/*.rs`, `picker.rs`,
  `settings.rs`, `emuconfig.rs` (setting labels), `main.rs` (`--help`,
  errors) goes through the lookup: a few hundred strings.
- `tty.rs` / `retro.rs` cells: a cell holds a whole grapheme (base character
  plus combining marks) instead of one `char`.
- A new setting in `~/.config/emuera-tty/config`.
- No new crate dependency planned (see design.md); memory cost: one small
  table for the selected language, loaded on first use.
- Saves, game behaviour and game text are unaffected.
