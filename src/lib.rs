//! The engine as a shared library for the Android app (android/build.sh builds it as a cdylib).
//! The whole of main.rs is included so its modules and `run` stay at the crate root, exactly as in
//! the terminal program. Elsewhere the library is empty, so a build compiles the engine only once.
#![cfg(target_os = "android")]
#![allow(dead_code, unused_imports, unused_macros)]

include!("main.rs");

mod android;
