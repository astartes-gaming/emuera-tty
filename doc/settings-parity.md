# Emuera settings in emuera-tty

Every setting of Emuera-EM/EE's settings window (`UI/Framework/Forms/ConfigDialog.cs`) and its debug settings, with Emuera's default (`Runtime/Config/ConfigData.cs`), how many of the local test games set it in a config file (and to what), and what emuera-tty does with it. Kept up to date by the `add-emuera-settings-parity` change (`openspec/changes/`).

| Setting | English key | Emuera's default | Games setting it | emuera-tty |
|---|---|---|---|---|
| 大文字小文字の違いを無視する (`IgnoreCase`) | Ignore case | `true` | 32 (YES ×32) | always on; a game turning it off gets a note (`doc/differences.md`) |
| _Rename.csvを利用する (`UseRenameFile`) | Use _Rename.csv file | `false` | 28 (YES ×20, NO ×8) | honoured (`GameCsv::load`; missing file: Emuera's message) |
| _Replace.csvを利用する (`UseReplaceFile`) | Use _Replace.csv file | `true` | 31 (YES ×31) | honoured (`GameCsv::load`) |
| マウスを使用する (`UseMouse`) | Use mouse | `true` | 19 (YES ×19) | ignored (task 2/3) |
| メニューを使用する (`UseMenu`) | Show menu | `true` | 18 (YES ×18) | not applicable (task 3) |
| デバッグコマンドを使用する (`UseDebugCommand`) | Allow debug commands | `false` | 19 (NO ×14, YES ×5) | ignored (task 2/3) |
| 多重起動を許可する (`AllowMultipleInstances`) | Allow multiple instances | `true` | 19 (YES ×19) | not applicable (task 3) |
| オートセーブを行なう (`AutoSave`) | Make autosaves | `true` | 19 (YES ×18, NO ×1) | honoured (`emuconfig::load`) |
| キーボードマクロを使用する (`UseKeyMacro`) | Use keyboard macros | `true` | 18 (YES ×18) | honoured (`emuconfig::load`) |
| ウィンドウの高さを可変にする (`SizableWindow`) | Changeable window height | `true` | 18 (YES ×18) | not applicable (task 3) |
| 描画インターフェース (`TextDrawingMode`) | Drawing interface | `TextDrawingMode.TEXTRENDERER` | 28 (TEXTRENDERER ×23, WINAPI ×3, GRAPHICS ×2) | not applicable; GETCONFIG answers TEXTRENDERER (games check it isn't WINAPI) |
| ウィンドウ幅 (`WindowX`) | Window width | `760` | 31 (1200 ×7, 800 ×4, 1000 ×3) | honoured (`emuconfig::load`) |
| ウィンドウ高さ (`WindowY`) | Window height | `480` | 32 (800 ×11, 720 ×3, 1080 ×3) | not applicable (task 3) |
| ウィンドウ位置X (`WindowPosX`) | Window X position | `0` | 18 (0 ×12, 200 ×2, 210 ×1) | not applicable (task 3) |
| ウィンドウ位置Y (`WindowPosY`) | Window Y position | `0` | 18 (0 ×16, 50 ×1, 1 ×1) | not applicable (task 3) |
| 起動時のウィンドウ位置を指定する (`SetWindowPos`) | Fixed window starting position | `false` | 18 (NO ×14, YES ×4) | not applicable (task 3) |
| 起動時にウィンドウを最大化する (`WindowMaximixed`) | Maximize window on startup | `false` | 17 (NO ×16, YES ×1) | not applicable (task 3) |
| 履歴ログの行数 (`MaxLog`) | Max history log lines | `5000` | 22 (5000 ×11, 10000 ×7, 1000 ×1) | honoured (`emuconfig::load`) |
| PRINTCを並べる数 (`PrintCPerLine`) | Items per line for PRINTC | `3` | 32 (5 ×13, 3 ×12, 4 ×7) | honoured (`emuconfig::load`) |
| PRINTCの文字数 (`PrintCLength`) | Number of Item characters for PRINTC | `25` | 30 (25 ×10, 29 ×9, 26 ×4) | honoured (`emuconfig::load`) |
| フォント名 (`FontName`) | Font name | `"ＭＳ ゴシック"` | 31 (ＭＳ ゴシック ×24, ＭＳ 明朝 ×3, Era Mono SC ×1) | honoured (`emuconfig::load`) |
| フォントサイズ (`FontSize`) | Font size | `18` | 32 (16 ×15, 18 ×8, 14 ×5) | honoured (`emuconfig::load`) |
| 一行の高さ (`LineHeight`) | Line height | `19` | 32 (16 ×10, 19 ×6, 14 ×5) | honoured (`emuconfig::load`) |
| 文字色 (`ForeColor`) | Text color | `Color.FromArgb(192, 192, 192)` | 31 (192,192,192 ×27, 255,255,255 ×2, 0,255,0 ×2) | honoured (`emuconfig::load`) |
| 背景色 (`BackColor`) | Background color | `Color.FromArgb(0, 0, 0)` | 31 (0,0,0 ×28, 10,10,10 ×1, 25,25,25 ×1) | honoured (`emuconfig::load`) |
| 選択中文字色 (`FocusColor`) | Highlight color | `Color.FromArgb(255, 255, 0)` | 31 (255,255,0 ×28, 255,0,0 ×3) | honoured (`emuconfig::load`) |
| 履歴文字色 (`LogColor`) | History log color | `Color.FromArgb(192, 192, 192)` | 31 (192,192,192 ×26, 128,128,128 ×3, 91,91,91 ×1) | honoured (`emuconfig::load`) |
| フレーム毎秒 (`FPS`) | FPS | `5` | 20 (5 ×15, 20 ×2, 7 ×2) | not applicable (task 3) |
| スクロール行数 (`ScrollHeight`) | Lines per scroll | `1` | 19 (1 ×14, 5 ×3, 7 ×1) | ignored (task 2/3) |
| 無限ループ警告までのミリ秒数 (`InfiniteLoopAlertTime`) | Milliseconds for infinite loop warning | `5000` | 21 (5000 ×12, 10000 ×6, 20000 ×1) | honoured (`emuconfig::load`) |
| 表示する最低警告レベル (`DisplayWarningLevel`) | Minimum warning level | `1` | 24 (0 ×15, 1 ×8, 3 ×1) | ignored (task 2/3) |
| ロード時にレポートを表示する (`DisplayReport`) | Display loading report | `false` | 22 (NO ×22) | ignored (task 2/3) |
| ロード時に引数を解析する (`ReduceArgumentOnLoad`) | Reduce argument on load | `ReduceArgumentOnLoadFlag.NO` | 25 (YES ×18, NO ×7) | ignored (task 2/3) |
| 呼び出されなかった関数を無視する (`IgnoreUncalledFunction`) | Ignore uncalled functions | `true` | 22 (YES ×17, NO ×5) | ignored (task 2/3) |
| 関数が見つからない警告の扱い (`FunctionNotFoundWarning`) | Function is not found warning | `DisplayWarningFlag.IGNORE` | 22 (DISPLAY ×13, IGNORE ×9) | ignored (task 2/3) |
| 関数が呼び出されなかった警告の扱い (`FunctionNotCalledWarning`) | Function not called warning | `DisplayWarningFlag.IGNORE` | 21 (IGNORE ×19, DISPLAY ×2) | ignored (task 2/3) |
| ボタンの途中で行を折りかえさない (`ButtonWrap`) | Button wrapping | `false` | 32 (YES ×28, NO ×4) | honoured (`emuconfig::load`) |
| サブディレクトリを検索する (`SearchSubdirectory`) | Search subfolders | `false` | 32 (YES ×30, NO ×2) | honoured (`loader::Listing`) |
| 読み込み順をファイル名順にソートする (`SortWithFilename`) | Sort filenames | `false` | 31 (YES ×27, NO ×4) | honoured (`loader::Listing`) |
| 表示するセーブデータ数 (`SaveDataNos`) | Save data count per page | `20` | 18 (20 ×16, 80 ×2) | honoured (`emuconfig::load`) |
| eramaker互換性に関する警告を表示する (`WarnBackCompatibility`) | Eramaker compatibility warning | `true` | 31 (NO ×29, YES ×2) | ignored (task 2/3) |
| システム関数の上書きを許可する (`AllowFunctionOverloading`) | Allow overriding system functions | `true` | 25 (YES ×18, NO ×7) | ignored: Emuera default not followed either (task 2) |
| システム関数が上書きされたとき警告を表示する (`WarnFunctionOverloading`) | System function override warning | `true` | 22 (YES ×21, NO ×1) | ignored (task 2/3) |
| 関連づけるテキストエディタ (`TextEditor`) | Text editor | `"notepad"` | 17 (notepad ×15, C:\Program Files (x86)\sakura\sakura.exe ×1, C:\Program Files\Notepad++\notepad++.exe ×1) | not applicable (task 3) |
| テキストエディタコマンドライン指定 (`EditorType`) | Text editor command line setting | `TextEditorType.USER_SETTING` | 17 (USER_SETTING ×17) | not applicable (task 3) |
| エディタに渡す行指定引数 (`EditorArgument`) | Text editor command line arguments | `""` | 17 ((empty) ×17) | not applicable (task 3) |
| 同名の非イベント関数が複数定義されたとき警告する (`WarnNormalFunctionOverloading`) | Duplicated functions warning | `false` | 22 (YES ×12, NO ×10) | ignored (task 2/3) |
| 解釈不可能な行があっても実行する (`CompatiErrorLine`) | Execute error lines | `false` | 18 (NO ×18) | ignored: Emuera default not followed either (task 2) |
| CALLNAMEが空文字列の時にNAMEを代入する (`CompatiCALLNAME`) | Use NAME if CALLNAME is empty | `false` | 18 (NO ×17, YES ×1) | ignored: Emuera default not followed either (task 2) |
| セーブデータをsavフォルダ内に作成する (`UseSaveFolder`) | Use sav folder | `false` | 25 (YES ×23, NO ×2) | honoured (`emuconfig::load`) |
| 擬似変数RANDの仕様をeramakerに合わせる (`CompatiRAND`) | Imitate behavior for RAND | `false` | 13 (NO ×13) | ignored: Emuera default not followed either (task 2) |
| 関数・属性については大文字小文字を無視しない (`CompatiFunctionNoignoreCase`) | Do not ignore case for functions and attributes | `false` | 18 (NO ×18) | always off; a game turning it on gets a note (`doc/differences.md`) |
| 全角スペースをホワイトスペースに含める (`SystemAllowFullSpace`) | Whitespace includes full-width space | `true` | 23 (NO ×15, YES ×8) | honoured: off, a full-width space in code is Emuera's error (`expr::check_full_space`, `Parser::statement`) |
| ver1739以前の非ボタン折り返しを再現する (`CompatiLinefeedAs1739`) | Reproduce wrapping behavior like in pre ver1739 | `false` | 12 (NO ×12) | ignored: Emuera default not followed either (task 2) |
| 内部で使用する東アジア言語 (`useLanguage`) | Default ANSI encoding | `UseLanguage.JAPANESE` | 18 (JAPANESE ×16, KOREAN ×1, CHINESE_HANS ×1) | ignored (task 2/3) |
| イベント関数のCALLを許可する (`CompatiCallEvent`) | Allow CALL on event functions | `false` | 18 (NO ×18) | ignored: Emuera default not followed either (task 2) |
| SPキャラを使用する (`CompatiSPChara`) | Allow SP characters | `false` | 19 (NO ×16, YES ×3) | ignored: Emuera default not followed either (task 2) |
| セーブデータをバイナリ形式で保存する (`SystemSaveInBinary`) | Use the binary format for saving data | `false` | 29 (YES ×26, NO ×3) | honoured (`emuconfig::load`) |
| ユーザー関数の全ての引数の省略を許可する (`CompatiFuncArgOptional`) | Allow arguments omission for user functions | `false` | 21 (NO ×21) | honoured (`Vm::call_func_in`'s parameter binding) |
| ユーザー関数の引数に自動的にTOSTRを補完する (`CompatiFuncArgAutoConvert`) | Auto TOSTR conversion for user function arguments | `false` | 25 (YES ×16, NO ×9) | honoured (`Vm::call_func_in`'s parameter binding) |
| FORM中の三連記号を展開しない (`SystemIgnoreTripleSymbol`) | Do not process triple symbols inside FORM | `false` | 21 (YES ×11, NO ×10) | honoured: `***` `+++` `===` `///` `$$$` expand unless on (`expr::parse_form`) |
| TIMESの計算をeramakerにあわせる (`TimesNotRigorousCalculation`) | Imitate behavior for TIMES | `false` | 12 (NO ×12) | ignored: Emuera default not followed either (task 2) |
| キャラクタ変数の引数を補完しない (`SystemNoTarget`) | Do not auto-complete arguments for character variables | `false` | 17 (NO ×16, YES ×1) | honoured for too few indices (`Vm::locate_chara`); a bare name read as a value still means TARGET's, where Emuera errors |
| UPDATECHECKを許可しない (`ForbidUpdateCheck`) | Disallow UPDATECHECK | `false` | 10 (NO ×9, YES ×1) | ignored (task 2/3) |
| ERD機能を利用する (`UseERD`) | Use ERD | `true` | 10 (YES ×9, NO ×1) | ignored: Emuera default not followed either (task 2) |
| VARSIZEの次元指定をERD機能に合わせる (`VarsizeDimConfig`) | Imitate ERD to VARSIZE dimension specification | `false` | 9 (NO ×8, YES ×1) | ignored: Emuera default not followed either (task 2) |
| ERDで定義した識別子とローカル変数の重複を確認する (`CheckDuplicateIdentifier`) | Check duplicate ERD identifier and private variablea | `false` | 8 (NO ×7, YES ×1) | ignored: Emuera default not followed either (task 2) |
| 行連結の改行コードの置換文字列 (`ReplaceContinuationBR`) | String of replacing new line code inside continuation | `"\" \""` | 4 (" " ×4) | honoured (`Parser::logical_lines`) |
| 外部プラグインが有効時に警告を表示する (`PluginAvailableWarn`) | If available pllugins, Show warning | `true` | 2 (YES ×2) | not applicable (task 3) |
| セーブデータを圧縮して保存する (`ZipSaveData`) | Compress save data | `false` | 8 (NO ×8) | honoured (`emuconfig::load`) |
| CONFIGファイルの内容を英語で保存する (`EnglishConfigOutput`) | Output English items in the config file | `false` | 9 (NO ×5, YES ×4) | ignored (task 2/3) |
| Emueraの表示言語 (`EmueraLang`) | Emuera interface language | `string.Empty` | 4 (English ×2, 简体中文 ×1, 日本語 ×1) | ignored (task 2/3) |
| テキスト中の<>タグを無視する (`CBIgnoreTags`) | Clipboard- ignore <> tags in text | `false` | 15 (NO ×10, YES ×5) | honoured (`emuconfig::load`) |
| <>を次の文で置き換える (`CBReplaceTags`) | Clipboard- Replace <> with this | `"."` | 14 (. ×14) | honoured (`emuconfig::load`) |
| 新しい行のみコピーする (`CBNewLinesOnly`) | Clipboard- Show new lines only | `true` | 14 (YES ×13, NO ×1) | honoured (`emuconfig::load`) |
| 画面のリフレッシュ時にクリップボードとバッファを消去する (`CBClearBuffer`) | Clipboard- Clear Buffer when game clears screen | `false` | 14 (NO ×14) | honoured (`emuconfig::load`) |
| 左クリックをトリガーにする (`CBTriggerLeftClick`) | Clipboard- LeftClick Trigger | `true` | 14 (YES ×14) | honoured (`emuconfig::load`) |
| ホイールクリックをトリガーにする (`CBTriggerMiddleClick`) | Clipboard- MiddleClick Trigger | `false` | 14 (NO ×13, YES ×1) | honoured (`emuconfig::load`) |
| ダブルクリックをトリガーにする (`CBTriggerDoubleLeftClick`) | Clipboard- Double Left Click Trigger | `false` | 14 (NO ×14) | honoured (`emuconfig::load`) |
| WAITをトリガーにする (`CBTriggerAnyKeyWait`) | Clipboard- AnyKey Wait Trigger  | `false` | 5 (NO ×5) | honoured (`emuconfig::load`) |
| INPUTをトリガーにする (`CBTriggerInputWait`) | Clipboard- Wait for Input Trigger | `true` | 14 (YES ×11, NO ×3) | honoured (`emuconfig::load`) |
| クリップボードに貼り付ける行数 (`CBMaxCB`) | Clipboard- Length of Clipboard | `25` | 14 (25 ×14) | honoured (`emuconfig::load`) |
| 総バッファサイズ (`CBBufferSize`) | Clipboard- Buffer Size | `300` | 14 (300 ×14) | honoured (`emuconfig::load`) |
| スクロールの行数 (`CBScrollCount`) | Clipboard- Scrolled Lines per Key | `5` | 15 (5 ×15) | honoured (`emuconfig::load`) |
| クリップボードの更新間隔(ミリ秒) (`CBMinTimer`) | Clipboard- min time between pastes | `800` | 14 (800 ×13, 300 ×1) | honoured (`emuconfig::load`) |
| Rikaichanを使用する (`RikaiEnabled`) | Rikai- Enabled | `false` | 3 (NO ×3) | not applicable (task 3) |
| Rikaichanのファイルパス (`RikaiFilename`) | Rikai- Dictionary Filename | `"Emuera-Rikai-edict.txt-eucjp"` | 3 (Emuera-Rikai-edict.txt-eucjp ×2, Emuera-Rikai-dictionaries\Emuera-Rikai-edict.txt-eucjp ×1) | not applicable (task 3) |
| ポップアップの背景色 (`RikaiColorBack`) | Rikai- Back Color | `Color.FromArgb(0, 0, 0x8B)` | 12 (0,0,139 ×7, 255,255,255 ×5) | not applicable (task 3) |
| ポップアップの文字色 (`RikaiColorText`) | Rikai- Text Color | `Color.FromArgb(0xFF, 0xFF, 0xFF)` | 12 (255,255,255 ×7, 0,0,0 ×5) | not applicable (task 3) |
| 翻訳中の語句を強調表示する (`RikaiUseSeparateBoxes`) | Rikai- Use Separate Boxes | `true` | 12 (YES ×7, NO ×5) | not applicable (task 3) |
| Ctrl-Zで元に戻す機能を有効にする (`Ctrl_Z_Enabled`) | Enable undo with ctrl-z | `false` | 2 (NO ×2) | ignored (task 2/3) |
| 起動時にデバッグウインドウを表示する (`DebugShowWindow`) | Show debug window on startup | `true` |  | to do (task 6) |
| デバッグウインドウを最前面に表示する (`DebugWindowTopMost`) | Debug window always on top | `true` |  | to do (task 6) |
| デバッグウィンドウ幅 (`DebugWindowWidth`) | Debug window width | `400` |  | to do (task 6) |
| デバッグウィンドウ高さ (`DebugWindowHeight`) | Debug window height | `300` |  | to do (task 6) |
| デバッグウィンドウ位置を指定する (`DebugSetWindowPos`) | Fixed debug window starting position | `false` |  | to do (task 6) |
| デバッグウィンドウ位置X (`DebugWindowPosX`) | Debug window X position | `0` |  | to do (task 6) |
| デバッグウィンドウ位置Y (`DebugWindowPosY`) | Debug window Y position | `0` |  | to do (task 6) |
