## Translating the game

In the settings, *Translate to* picks the language to
show the game in (en, es, pt, fr, de, it, ru, ja, zh, ko), *Translate
from* the game's own (auto: Japanese lines from Japanese, English ones from
English, so an English translation like AnonTW can be read in Spanish), and
*Translate what* either the characters' **dialog** or **all** text (menus,
buttons, labels too); *auto* (the default) is dialog when the game is shown
in English and all text in any other language. Text is translated as it
appears. Translation is off while *Translator* is *off* (the default). While it is off, *Translate to / from / what* and *Test the translator…* are hidden from the settings.

### Choosing a translator

With *Translator* on *auto* it uses whatever translator is there,
the ones **on this computer** first (nothing is sent online then): the
first it finds of

- [Ollama](https://ollama.com) running a model (`ollama pull qwen2.5:7b`
  translates Japanese well; `EMUERA_TRANSLATE_MODEL` picks the model),
- a [LibreTranslate](https://libretranslate.com) server on port 5000
  (`--libretranslate host:port`, or `EMUERA_LIBRETRANSLATE=host:port`, for another),
- `translateLocally` (Bergamot, `EMUERA_TRANSLATE_MODEL`, default
  `ja-en-tiny`), or Argos Translate (`pipx install argostranslate`, then
  `argospm install translate-ja_en`; one Argos process is kept running,
  so only the first line waits for the model to load),
- or any command in `EMUERA_TRANSLATE_CMD`: it gets a line on stdin and
  prints its translation (`EMUERA_TRANSLATE_FROM` and `EMUERA_TRANSLATE_TO`
  hold the languages).

and, when none of those is there, the online ones below. *Translator*
can also name one (`local` for this computer's only, `argos`), or an
**online** service through its command-line tool (these send the text over
the internet):

- `translate-shell`: the `trans` command (Google Translate by default;
  `EMUERA_TRANS_ENGINE=bing` and so on for another of its engines);
- `deepl`: DeepL, through `curl`. With your API key in `DEEPL_AUTH_KEY`
  (a free key works) it uses DeepL's API; without one, the free keyless
  endpoint DeepL's browser extension uses, asked the way
  [egg-translator](https://gitgud.io/Shakes4cc/egg-translator) does. That
  one is unofficial: it limits how fast you can go and may change; the
  settings say so when it refuses. (Without curl, DeepL's own `deepl` tool and the
  key do.)

### What gets translated

*Dialog* is the lines printed by a character's dialogue code (the `口上`
files, `KOJO` functions) and lines in quotes (「」 『』, or "" in English);
menus, buttons and the rest stay as they are. *All* translates every piece
of text, keeping its colours and buttons, and sends only the words: the
`[100]` numbers, brackets and figures around them stay put, and buttons
still send the same numbers. **Menus keep their layout**: where a piece
sits in a column (two spaces or more before the next one), its
translation keeps its width, padded when shorter, using the spaces after
it when longer, and cut with `…` only past those; at the end of a line or
before ordinary text it takes the room it needs.

Dialog goes to the translator **a click at a time**: the lines a
character says before the next wait (a speech over several lines) are
sent together, so a sentence split over lines is translated whole and
with its context. The translation comes back line by line when the
translator keeps the lines, and is spread over them by length when it
doesn't (Argos). The log has one entry per batch, its lines joined by
spaces.

Maps and box drawings are never translated, nor text already in the
target language, and the game itself still sees the text it printed. Text
shows untranslated until its translation comes in, which happens in the
background; text seen again is instant. If the translator fails, the settings say
why on that row's help line.

### Original, forced translation and quotes

**Original or translation**: middle-click a translated line (anywhere,
also at a wait) to see the original text; middle-click again for the
translation. It is remembered per line for the session.

**Force a translation**: middle-click a line that is shown as printed (a
menu, narration, anything the *Translate* setting leaves alone) to
translate it now; click again to flip between the two as above. This
needs a translator to be on.

**Quote marks**: the translation keeps the quote marks of the original
(「」, 『』, “” or "") instead of the plain `"` translators write. It is
done line by line for a speech over several lines, and a pair of quotes a
translator puts round a line that had none (they often quote every line
of a speech on its own) is taken off.

### Testing and the cache

**Test the translator…** in the settings translates a sample line with the chosen
translator and shows the result and how long it took, or why it failed.
**Clear the translation cache** forgets every translation made this
session (they are kept in memory only), so what is on screen is
translated again: after changing translator or fixing one that failed.

##### Export translation cache

**Export translation cache…** writes the current translation cache to a file
(formatted like a translation log, see [A log for translators](#a-log-for-translators)),
so it can be reused with `--translate-load` later. This option only appears
when translation is on and no `--translate-log` was given (since the log
already persists the cache in that case). The export includes both
translations loaded from `--translate-load` files and translations obtained
during the current session.

### A log for translators

`emuera-tty --translate-log translations.tsv` appends every new translation
to that file, one per line:
the ERB file and line that printed the text, the languages, the translator
used, the original and the translation, separated by tabs, ready to find
and fix in the game's scripts:

```text
ERB/口上/K9.ERB:3	ja>en	Ollama (llama3)	「おはようございます、お嬢様」	「Good morning, my lady.」
```

A line translated by a middle click that can't be traced to the file that
printed it has `(force-translated, cannot trace original file)` in the
first column.

Dialog sent to the translator several lines at a time (a speech over
several lines) is logged one row per line, each with its own file and line,
when the translation comes back with as many lines; if it merged or split
them, the batch is one row.

The log is appended to, never reset, and **a row it already has isn't
written again**: the same languages, translator, original and translation,
wherever the text was printed from. The same original with a different
translation (or different translator) is added as a new row (the later one
wins when the log is [read back](#reusing-a-log-as-translations)), and the
header line each session writes is only added when the file doesn't have it
yet.

### Reusing a log as translations

`emuera-tty --translate-load translations.tsv` reads a translation log
(the file `--translate-log` writes, or one of the same shape) and
shows its translations at once, before any translator is asked. Set
*Translator* to `file` to read a game again from its log with no translator
at all; with one chosen, it only gets the lines the file doesn't have, and
with *auto* and no translator found, the file alone is used.

- **What is read**: the last two columns of each row, the original and its
  translation. The file and line in the first column are not used: it can
  name an intermediary file rather than the script that held the text. The
  column before them, the languages (`ja>en`), picks the rows translated
  into the language set in *Translate to*; rows into another one are
  skipped. A file of plain `original`, tab, `translation` rows works too,
  and `#` lines are comments.
- **Several files**: repeat the option (`--translate-load a.tsv
  --translate-load b.tsv`). A later file wins over an earlier one, and a
  later row over an earlier row
  of the same file, so a line corrected at the end of a log replaces the
  old translation. Translations are shown as written, quote marks too.
- **How lines are matched**: by the whole original line (a speech over
  several lines is found line by line, or as the one row the log has when
  the translator merged its lines).
- **The same file for both**: `--translate-storage translations.tsv` is
  `--translate-load` and `--translate-log` with the one file. What was
  loaded isn't written again, only new translations are added. With
  `--translate-log` too, new translations go there instead, and the
  storage file is only read; `--translate-load` files are read after it,
  so they win.
- **When it was read**: at startup, and when the translation settings
  change. Edit the file and change a setting (or restart) to use the new
  version.

### Keeping files and lines untranslated

`emuera-tty --translation-filter filter.txt` reads a list of what is never
translated: script files, and single lines of dialogue. Both are shown as the
game printed them. An example `filter.txt`:

```
# comments and empty lines are skipped
ERB/口上・メッセージ関連/個人口上/049 Satori [さとり]/さとり/FONT_AA(通用字符画).erb
「意外と早起きさんだねー」
```

- **Files**: an entry ending in `.erb` or `.erh` is a script file, written
  as the game's folder names it (`ERB/…`; `/` or `\`, upper or lower case).
  Everything that file prints stays untranslated and untouched. The file is
  the one that held the `PRINT` statement, so text a file prints from
  another file's function counts as the first one's.
- **Lines**: any other entry is the text of one line (spaces around it
  don't matter). A line of that text stays untranslated wherever it was
  printed from. For a speech over several lines, list each line.
- **The file's format**: plain text, one entry per line, or a `.tsv`
  (each cell of a row is an entry). `#` lines are comments.
- **Several files**: repeat the option.
- **Middle-click** on a filtered line still translates it (click it again
  for the original).
- **With a log**: filtered lines are not translated, so they are not
  written to `--translate-log` either, and `--translate-load` rows for
  them are not shown.

## Translated games (Emuera-Anchor)

Games translated with Emuera-Anchor's system play translated here too:

- `Abl_TR.csv`, `Talent_TR.csv`, `Item_TR.csv`… (any CSV name table, also
  `Palam`/`Param`, `Str`, `CSTR`, `Flag`…) in the `CSV/` folder itself
  (not its subfolders, as Anchor): `original,translation` lines, `;`
  starts a comment. As in Anchor, only spaces before the original are
  dropped: `技巧, Technique` translates to " Technique", which some games
  use for alignment. Lines from a `{` line to a `}` line are read as
  one, as Anchor's file reader does. A line without exactly one comma, a
  misplaced `{` or `}`, or a name given twice, is shown as a warning when
  the game loads (with the file and line) and skipped; Anchor refuses to
  start the game instead.
- When a name comes out translated, as in Anchor: when a script prints
  it (`PRINTV TALENTNAME:LOCAL`, `PRINTFORM %TALENTNAME:LOCAL%`, PRINTS,
  PRINTDATA, HTML_PRINT), stores it in a string (`LOCALS '=
  TALENTNAME:LOCAL`, `LOCALS = [%TALENTNAME:LOCAL%]`, `+=`), or returns
  it with RETURNF from a `#FUNCTIONS` that is being printed or stored.
  Joining strings with `+` keeps the translation. Everything else sees
  the original name, so the scripts keep working: conditions (`IF
  TALENTNAME:LOCAL == "処女"`), CALL arguments, PRINTBUTTON's text, the
  branches of `? #`, and the arguments of built-in functions other than
  TOUPPER, TOLOWER, TOHALF, TOFULL, REPLACE and STRFORM. The values of
  STR, TSTR, SAVESTR, RESULTS and GLOBALS translate the same way, through
  `Str_TR.csv`. Because stored text is translated, saves made in a
  translated game hold the translations, as Anchor's do (Emuera loads
  them like any other save; saves from older emuera-tty versions hold the
  originals). PRINT_ABL and the like, the TRAIN command list, the shop
  list and PRINTDATA are translated too, though Anchor prints those
  untranslated.
- A name with a fixed index (`TALENTNAME:5`) follows Anchor's own rule:
  inside a comparison, a calculation, a function call or a `%…%` it is
  translated even in a condition (`IF TALENTNAME:5 == "処女"` is false in
  a translated game, as in Anchor), while stored or returned on its own
  (`LOCALS '= TALENTNAME:5`) it stays the original. Quoted text printed
  with PRINTS, PRINTV or PRINTPLAIN (`PRINTS "男"`) is looked up in
  `ALL_TR.csv`, or in the table of the first `XXXNAME` on the same line.
  STRLENS and STRLENSU measure the translated text, so layouts built on
  them fit the translation.
- `ALL_TR.csv`: also looked up for a whole `%…%` value when it is
  printed or stored (or has a width, `%X,10%`). Failing that, as in
  Anchor, a name the script read untranslated shortly before (Anchor
  remembers the last 19) is translated inside the value, and then the
  value without its digits, `LV` and brackets is tried (`技巧LV3` →
  `TechniqueLV3`); a `%…%` in a RETURNF line gets none of this.
- `TRCHARA*.csv` (any case, anywhere under `CSV/`): character names
  (`NAME`, `CALLNAME`, `NICKNAME`, `MASTERNAME`, `CSTR,n,text`) in blocks
  from `NO,n` (or `番号,n`) to `ENDCHR` or `ENDCHAR`; characters added in
  a new game come out translated, while those in a save keep their saved
  names. As in Anchor, keys are matched in any case, a value is the field
  up to the next comma, and a field replaces only what the character's
  own `Chara*.csv` gives (a TRCHARA CALLNAME for a character whose CSV
  has none is ignored). Problems (an unknown key, data before `NO`, a
  missing `ENDCHR`…) are shown as warnings when the game loads, with the
  file and line.
- `TR_NAME(ABL, 25)` / `TR_NAME("TALENT", "処女")`: a translated name for
  scripts, the original when there is none. As in Anchor, the table is a
  bare variable or its name in capitals (`ABL`, `"ABLNAME"`…), and the key
  an index that has a name or a name in that CSV; anything else is a
  script error ("Invalid first argument" / "Invalid second argument").
