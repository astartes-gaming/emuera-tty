# Tasks

## 1. Config model

- [x] 1.1 `doc/settings-parity.md`: every setting of ConfigDialog and the debug settings, Emuera's default, what local games set, and emuera-tty's handling (to fill in as tasks land). Verify: 93 dialog settings + debug settings listed. Done: 91 dialog settings (two codes are labels only) + 7 debug settings, generated from ConfigData.cs and the local games' config files.
- [x] 1.2 `emuconfig::Config`: all keys typed, Emuera's defaults, read once in Emuera's file order, which file set each (for locking). Add the missing English names (`Valid extensions for LOADTEXT and SAVETEXT`, `EXPLV…`, `PALAMLV…`, `STAIN…`). Verify: unit tests for precedence, English keys, bad values falling back to the default; the existing readers (`load`, `Values`) go through it with no behaviour change. Done as `emuconfig::Values` (it already tracked each key's file): `value` / `bool` / `int` with Emuera's defaults (`config_names::DEFAULTS`, generated from ConfigData.cs), `lines()` in file order for `load`, and `config/emuera.config` read as `load` did.
- [x] 1.3 Pass `Arc<Config>` to the loader, parser and VM; GETCONFIG / GETCONFIGS (`Vm::config_value`, which hardcodes several answers today) read the model, keeping its deliberate answers (drawing interface always TEXTRENDERER). Done: one `Arc<Values>` read in `main`, passed to `Listing::of`, `Parser` and `Vm` (re-read on reload). GETCONFIG needs no change: Emuera answers only a fixed list of keys (`ConfigData.GetConfigValueInERB`), which `Vm::config_value` already mirrors. Verify: `cargo test`, AnonTW smoke unchanged, start-up time on pops-tw unchanged within noise.

## 2. Script-behaviour settings (one commit each, Emuera source checked, ERB/inline-game test with the non-default value)

- [ ] 2.1 _Rename.csvを利用する, _Replace.csvを利用する, 大文字小文字の違いを無視する, 関数・属性については大文字小文字を無視しない
- [ ] 2.2 全角スペースをホワイトスペースに含める, FORM中の三連記号を展開しない, 行連結の改行コードの置換文字列
- [ ] 2.3 ユーザー関数の引数に自動的にTOSTRを補完する, ユーザー関数の全ての引数の省略を許可する, キャラクタ変数の引数を補完しない, 文字列変数の代入に文字列式を強制する
- [ ] 2.4 擬似変数RANDの仕様をeramakerに合わせる, TIMESの計算をeramakerにあわせる, CALLNAMEが空文字列の時にNAMEを代入する, SPキャラを使用する, イベント関数のCALLを許可する
- [ ] 2.5 解釈不可能な行があっても実行する, システム関数の上書きを許可する, ver1739以前の非ボタン折り返しを再現する
- [ ] 2.6 ERD機能を利用する, VARSIZEの次元指定をERD機能に合わせる, ERDで定義した識別子とローカル変数の重複を確認する, サブディレクトリを検索する / 読み込み順をファイル名順にソートする (already read; move onto the model)
- [ ] 2.7 Warnings and load report: 表示する最低警告レベル, 関数が見つからない/呼び出されなかった警告の扱い, 呼び出されなかった関数を無視する, システム関数が上書きされたとき警告, 同名の非イベント関数警告, eramaker互換性警告, ロード時にレポートを表示する (on `--check` and the load messages)
- [ ] 2.8 UPDATECHECKを許可しない, 内部で使用する東アジア言語 (ANSI file decoding), Emueraの表示言語 (map to the interface language when the player hasn't chosen one)

## 3. Terminal equivalents and not-applicable settings

- [ ] 3.1 マウスを使用する (mouse off), スクロール行数 (wheel step when the player hasn't set one), Ctrl-Z, クリップボード scroll lines; record window/FPS/drawing/menu/instances/editor/Rikai/plugin settings as not applicable. Verify: each row of the parity table filled.

## 4. Game settings page

- [ ] 4.1 Emuera's tabs and all items on the page; not-applicable items shown greyed with the reason; `_fixed.config` items locked; script-behaviour items note "after restart". Verify: render test of each tab at 80 columns; editing writes `emuera.config` as Emuera does.

## 5. @ commands

- [ ] 5.1 Recognise `@…` input as Emuera's `doSystemCommand` (typed, length > 1, not ONEINPUT). `@QUIT`/`@EXIT`, `@REBOOT` (reload, as the reload hotkey), `@OUTPUT(LOG)` (as OUTPUTLOG), `@CONFIG` (opens the game settings page). Verify: headless tests for each.
- [ ] 5.2 Debug commands with Emuera's checks and error messages, expressions printed, state restored, `デバッグコマンドを使用した時にMASTERの名前を変更する`. Verify: headless tests for the spec's scenarios.

## 6. Debug panel

- [ ] 6.1 Panel with watch / stack trace / console tabs, opened by `@DEBUG` and a key in debug mode, `起動時にデバッグウインドウを表示する`. Verify: render tests; DEBUGPRINT output appears in the console tab.
- [ ] 6.2 `debug/watchlist.csv` and `debug/console.log` read and written as Emuera does. Verify: round trip test; file format identical to Emuera's (one expression per line, config encoding).

## 7. Docs

- [ ] 7.1 Wiki *Settings* and *Debugging*, man page, `doc/differences.md`. Verify: `tools/wiki_sync.sh --dry-run` (or equivalent) shows the pages.
