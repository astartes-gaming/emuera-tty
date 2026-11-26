//! Writes the app's AndroidManifest.xml in Android's binary XML format.
//!
//! This is normally aapt2's job, but that's the only thing this app would need aapt2 for:
//! it has no resources at all. Google publishes aapt2 for x86-64 Linux only, so writing
//! the format directly lets the app build on ARM Linux with no third-party binary.
//!
//!   mkmanifest OUT PACKAGE VERSION_CODE VERSION_NAME MIN_SDK TARGET_SDK ACTIVITY LABEL
const ANDROID_NS: &str = "http://schemas.android.com/apk/res/android";

#[derive(Clone)]
enum V {
    Str(String),
    Int(u32),
    Hex(u32),
    Bool(bool),
}

/// An attribute: name, its id in the framework (0: no namespace, like `package`), value.
#[derive(Clone)]
struct A(&'static str, u32, V);

struct El {
    name: &'static str,
    attrs: Vec<A>,
    kids: Vec<El>,
}

fn el(name: &'static str, attrs: Vec<A>, kids: Vec<El>) -> El {
    El { name, attrs, kids }
}

struct Pool {
    strings: Vec<String>,
}

impl Pool {
    fn id(&mut self, s: &str) -> u32 {
        if let Some(i) = self.strings.iter().position(|x| x == s) {
            return i as u32;
        }
        self.strings.push(s.to_string());
        (self.strings.len() - 1) as u32
    }
}

fn u16le(o: &mut Vec<u8>, v: u16) {
    o.extend_from_slice(&v.to_le_bytes());
}
fn u32le(o: &mut Vec<u8>, v: u32) {
    o.extend_from_slice(&v.to_le_bytes());
}

fn collect_attrs(e: &El, out: &mut Vec<(&'static str, u32)>) {
    for a in &e.attrs {
        if a.1 != 0 && !out.iter().any(|x| x.0 == a.0) {
            out.push((a.0, a.1));
        }
    }
    for k in &e.kids {
        collect_attrs(k, out);
    }
}

fn write_el(e: &El, pool: &mut Pool, ns: u32, body: &mut Vec<u8>) {
    let name = pool.id(e.name);
    // the framework reads attributes assuming they are sorted by resource id
    let mut attrs = e.attrs.clone();
    attrs.sort_by_key(|a| if a.1 == 0 { u32::MAX } else { a.1 });
    u16le(body, 0x0102);
    u16le(body, 16);
    u32le(body, 36 + 20 * attrs.len() as u32);
    u32le(body, 1);
    u32le(body, u32::MAX);
    u32le(body, u32::MAX);
    u32le(body, name);
    u16le(body, 20);
    u16le(body, 20);
    u16le(body, attrs.len() as u16);
    u16le(body, 0);
    u16le(body, 0);
    u16le(body, 0);
    for a in &attrs {
        let an = pool.id(a.0);
        u32le(body, if a.1 != 0 { ns } else { u32::MAX });
        u32le(body, an);
        let (raw, ty, data) = match &a.2 {
            V::Str(s) => {
                let i = pool.id(s);
                (i, 0x03u8, i)
            }
            V::Int(v) => (u32::MAX, 0x10, *v),
            V::Hex(v) => (u32::MAX, 0x11, *v),
            V::Bool(b) => (u32::MAX, 0x12, if *b { u32::MAX } else { 0 }),
        };
        u32le(body, raw);
        u16le(body, 8);
        body.push(0);
        body.push(ty);
        u32le(body, data);
    }
    for k in &e.kids {
        write_el(k, pool, ns, body);
    }
    u16le(body, 0x0103);
    u16le(body, 16);
    u32le(body, 24);
    u32le(body, 1);
    u32le(body, u32::MAX);
    u32le(body, u32::MAX);
    u32le(body, name);
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    assert!(a.len() == 9, "usage: mkmanifest OUT PACKAGE VERSION_CODE VERSION_NAME MIN_SDK TARGET_SDK ACTIVITY LABEL");
    let (out, package, vcode, vname, min_sdk, target_sdk, activity, label) =
        (&a[1], &a[2], a[3].parse::<u32>().unwrap(), &a[4], a[5].parse::<u32>().unwrap(), a[6].parse::<u32>().unwrap(), &a[7], &a[8]);
    let s = |v: &str| V::Str(v.to_string());
    let game_activity = format!("{}.GameActivity", package);
    let game_activity = &game_activity;
    // orientation, keyboard, keyboardHidden, navigation, screenLayout, uiMode, screenSize, smallestScreenSize,
    // and what can change in Settings while a game waits in the background: mcc, mnc, locale, touchscreen,
    // density, layoutDirection, colorMode, grammaticalGender, fontWeightAdjustment, fontScale
    const CONFIG_CHANGES: u32 = 0x80 | 0x10 | 0x20 | 0x40 | 0x100 | 0x200 | 0x400 | 0x800 | 0x1 | 0x2 | 0x4 | 0x8 | 0x1000 | 0x2000 | 0x4000 | 0x8000 | 0x10000000 | 0x40000000;

    let root = el(
        "manifest",
        vec![A("versionCode", 0x0101021b, V::Int(vcode)), A("versionName", 0x0101021c, s(vname)), A("package", 0, s(package))],
        vec![
            el("uses-sdk", vec![A("minSdkVersion", 0x0101020c, V::Int(min_sdk)), A("targetSdkVersion", 0x01010270, V::Int(target_sdk))], vec![]),
            el("uses-permission", vec![A("name", 0x01010003, s("android.permission.FOREGROUND_SERVICE"))], vec![]),
            el("uses-permission", vec![A("name", 0x01010003, s("android.permission.FOREGROUND_SERVICE_SPECIAL_USE"))], vec![]),
            el("uses-permission", vec![A("name", 0x01010003, s("android.permission.POST_NOTIFICATIONS"))], vec![]),
            el(
                "application",
                vec![
                    A("label", 0x01010001, s(label)),
                    A("debuggable", 0x0101000f, V::Bool(true)),
                    A("hasCode", 0x0101000c, V::Bool(true)),
                    A("hardwareAccelerated", 0x010102d3, V::Bool(true)),
                    // compressed in the APK and unpacked at install, so no zipalign is needed
                    A("extractNativeLibs", 0x010104ea, V::Bool(true)),
                ],
                vec![
                    // the game list
                    el(
                        "activity",
                        vec![
                            A("name", 0x01010003, s(activity)),
                            A("exported", 0x01010010, V::Bool(true)),
                            // keeps an import running through a rotation
                            A("configChanges", 0x0101001f, V::Hex(CONFIG_CHANGES)),
                            // a long absence mustn't clear the task back to this list, with the game still running
                            A("alwaysRetainTaskState", 0x01010203, V::Bool(true)),
                        ],
                        vec![el(
                            "intent-filter",
                            vec![],
                            vec![
                                el("action", vec![A("name", 0x01010003, s("android.intent.action.MAIN"))], vec![]),
                                el("category", vec![A("name", 0x01010003, s("android.intent.category.LAUNCHER"))], vec![]),
                            ],
                        )],
                    ),
                    // a running game
                    el(
                        "activity",
                        vec![
                            A("name", 0x01010003, s(game_activity)),
                            // adb's way in (games, replays, engine settings): only the shell holds this permission, not other apps
                            A("exported", 0x01010010, V::Bool(true)),
                            A("permission", 0x01010006, s("android.permission.DUMP")),
                            // its own process: leaving a game by ending it frees all its memory
                            A("process", 0x01010011, s(":game")),
                            // sensorLandscape: era games are laid out for a wide window (GameActivity lets a tablet turn)
                            A("screenOrientation", 0x0101001e, V::Int(6)),
                            // singleTask: the VM starts once per process, so every way in must find this screen
                            A("launchMode", 0x0101001d, V::Int(2)),
                            // the VM lives in the process, so the activity must survive these
                            A("configChanges", 0x0101001f, V::Hex(CONFIG_CHANGES)),
                        ],
                        vec![],
                    ),
                    // the engine's settings: same screen whether opened from the list or a game
                    el(
                        "activity",
                        vec![
                            A("name", 0x01010003, s(&format!("{}.SettingsActivity", package))),
                            // keeps settings through a rotation
                            A("configChanges", 0x0101001f, V::Hex(CONFIG_CHANGES)),
                        ],
                        vec![],
                    ),
                    // keeps a game alive in the background, so in the game's process.
                    // specialUse: the other types don't fit, and some are limited to a few hours
                    el(
                        "service",
                        vec![
                            A("name", 0x01010003, s(&format!("{}.GameService", package))),
                            A("exported", 0x01010010, V::Bool(false)),
                            A("process", 0x01010011, s(":game")),
                            A("foregroundServiceType", 0x01010599, V::Hex(0x40000000)),
                        ],
                        vec![],
                    ),
                    // the games in the system's file manager: only it holds this permission
                    el(
                        "provider",
                        vec![
                            A("name", 0x01010003, s(&format!("{}.GamesProvider", package))),
                            A("permission", 0x01010006, s("android.permission.MANAGE_DOCUMENTS")),
                            A("exported", 0x01010010, V::Bool(true)),
                            A("authorities", 0x01010018, s(&format!("{}.games", package))),
                            A("grantUriPermissions", 0x0101001b, V::Bool(true)),
                        ],
                        vec![el("intent-filter", vec![], vec![el("action", vec![A("name", 0x01010003, s("android.content.action.DOCUMENTS_PROVIDER"))], vec![])])],
                    ),
                ],
            ),
        ],
    );

    // attribute names come first in the string pool, in the order of the resource map
    let mut res_attrs = vec![];
    collect_attrs(&root, &mut res_attrs);
    let mut pool = Pool { strings: res_attrs.iter().map(|x| x.0.to_string()).collect() };
    let (prefix, uri) = (pool.id("android"), pool.id(ANDROID_NS));

    let mut body = vec![];
    u16le(&mut body, 0x0100);
    u16le(&mut body, 16);
    u32le(&mut body, 24);
    u32le(&mut body, 1);
    u32le(&mut body, u32::MAX);
    u32le(&mut body, prefix);
    u32le(&mut body, uri);
    write_el(&root, &mut pool, uri, &mut body);
    u16le(&mut body, 0x0101);
    u16le(&mut body, 16);
    u32le(&mut body, 24);
    u32le(&mut body, 1);
    u32le(&mut body, u32::MAX);
    u32le(&mut body, prefix);
    u32le(&mut body, uri);

    // the string pool, UTF-16
    let mut data = vec![];
    let mut offsets = vec![];
    for st in &pool.strings {
        offsets.push(data.len() as u32);
        let units: Vec<u16> = st.encode_utf16().collect();
        u16le(&mut data, units.len() as u16);
        for c in units {
            u16le(&mut data, c);
        }
        u16le(&mut data, 0);
    }
    while data.len() % 4 != 0 {
        data.push(0);
    }
    let header = 28u32;
    let strings_start = header + 4 * offsets.len() as u32;
    let mut sp = vec![];
    u16le(&mut sp, 0x0001);
    u16le(&mut sp, header as u16);
    u32le(&mut sp, strings_start + data.len() as u32);
    u32le(&mut sp, offsets.len() as u32);
    u32le(&mut sp, 0);
    u32le(&mut sp, 0);
    u32le(&mut sp, strings_start);
    u32le(&mut sp, 0);
    for off in &offsets {
        u32le(&mut sp, *off);
    }
    sp.extend_from_slice(&data);

    // the resource map: the framework ids of the first strings
    let mut rm = vec![];
    u16le(&mut rm, 0x0180);
    u16le(&mut rm, 8);
    u32le(&mut rm, 8 + 4 * res_attrs.len() as u32);
    for x in &res_attrs {
        u32le(&mut rm, x.1);
    }

    let mut file = vec![];
    u16le(&mut file, 0x0003);
    u16le(&mut file, 8);
    u32le(&mut file, 8 + (sp.len() + rm.len() + body.len()) as u32);
    file.extend_from_slice(&sp);
    file.extend_from_slice(&rm);
    file.extend_from_slice(&body);
    std::fs::write(out, file).expect("write manifest");
}
