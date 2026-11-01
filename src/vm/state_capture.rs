//! Full VM state capture and restore for emuera-tty.
//!
//! This captures the entire interpreter state (all variables, call stack,
//! RNG, system state, EM data, translation state, etc.) to a file,
//! and can restore it later. This is emuera-tty specific and not
//! compatible with Emuera's save format.

use super::datatable::DataTable;
use super::erasav::{Entry, FileType, Reader, Values, Writer};
use super::value::{StrMap, Value};
use super::vars::{Arr, Data, Globals};
use super::{Chara, CharaNames, DimDecl, Frame, Loc, Name, Slot, Vm};
use crate::ast::Align;
use crate::console::{Console, Line, Seg};
use crate::fxhash::HashMap;
use crate::xml::Element;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::Ordering;

/// Magic header for emuera-tty full state capture files.
const STATE_HEADER: u64 = 0x0A1A_0A0D_5354_4154; // "STATE" in the magic
const STATE_VERSION: u32 = 1;
const STATE_MARKER: u8 = 0xFF; // Marker byte to identify state capture files

// Erasav entry kinds (copied from erasav.rs since they're private)
const INT: u8 = 0x00;
const INT_ARRAY: u8 = 0x01;
const INT_ARRAY_2D: u8 = 0x02;
const INT_ARRAY_3D: u8 = 0x03;
const STR: u8 = 0x10;
const STR_ARRAY: u8 = 0x11;
const STR_ARRAY_2D: u8 = 0x12;
const STR_ARRAY_3D: u8 = 0x13;
const MAP: u8 = 0x20;
const XML: u8 = 0x21;
const DT: u8 = 0x22;
const SEPARATOR: u8 = 0xFD;
const EOC: u8 = 0xFE;
const EOF: u8 = 0xFF;

// Bytes inside array data
const B_MAX_BYTE: u8 = 0xCF;
const B_INT16: u8 = 0xD0;
const B_INT32: u8 = 0xD1;
const B_INT64: u8 = 0xD2;
const B_STRING: u8 = 0xD8;
const B_EOA1: u8 = 0xE0;
const B_EOA2: u8 = 0xE1;
const B_ZERO: u8 = 0xF0;
const B_ZERO_A1: u8 = 0xF1;
const B_ZERO_A2: u8 = 0xF2;
const B_EOD: u8 = 0xFF;

/// A captured frame for serialization.
#[allow(dead_code)]
struct CapturedFrame {
    func_name: String,
    pc: usize,
    vars: HashMap<String, Arr>,
    slots: Vec<Slot>,
    refs: HashMap<String, CapturedLoc>,
}

/// Serializable version of Loc.
#[derive(Clone, Debug)]
#[allow(dead_code)]
enum CapturedLoc {
    Global(String),
    Frame(String),
    FrameAt(usize, String),
    Static(String, String), // func_name, var_name
    Chara(usize, String),
    Ro(Value),
}

/// Capture the current VM state and write to a file.
pub fn capture_state(vm: &Vm, path: &Path, zip: bool) -> Result<(), String> {
    let mut w = Writer::begin(FileType::Normal, vm.game_code(), vm.game_version().0, "emuera-tty full state capture");

    // Write globals
    write_globals(&mut w, &vm.globals)?;

    // Write statics
    write_statics(&mut w, &vm.statics, &vm.static_index)?;

    // Write characters
    write_charas(&mut w, &vm.charas, &vm.chara_names)?;

    // Write frames
    write_frames(&mut w, &vm.frames, vm)?;

    // Write user_dims
    write_user_dims(&mut w, &vm.user_dims)?;

    // Write dim_order
    if let Some(order) = vm.dim_order.get() {
        write_dim_order(&mut w, order)?;
    }

    // Write RNG
    write_rng(&mut w, vm.rng)?;

    // Write system state
    write_sys_state(&mut w, &vm.state)?;

    // Write EM data
    write_em_data(&mut w, &vm.maps, &vm.dts, &vm.xml_docs)?;

    // Write translation state
    write_tr_state(&mut w, vm)?;

    // Write VM flags
    write_vm_flags(&mut w, vm)?;

    // Write game info
    write_game_info(&mut w, vm)?;

    // Write console (scrollback)
    write_console(&mut w, &vm.con)?;

    w.eof();

    // Use finish() to get the full data with Emuera header, then skip the header
    // finish() returns: [HEADER(8)][VERSION(4)][extra(4)][FileType][game_code][game_version][text][entries...][EOF]
    let body = w.finish(false);
    // Skip the 16-byte Emuera header to get just the body: [FileType][game_code][game_version][text][entries...][EOF]
    let body_data = &body[16..];

    // Build final output with custom header
    // Format: STATE_HEADER (8) | STATE_VERSION (4) | extra (4) | STATE_MARKER (1) | body_data
    let mut out = Vec::with_capacity(body_data.len() + 17);
    out.extend_from_slice(&STATE_HEADER.to_le_bytes());
    out.extend_from_slice(&STATE_VERSION.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes()); // extra words
    out.push(STATE_MARKER); // Marker byte to identify state capture files
    out.extend_from_slice(body_data);

    if zip {
        let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        gz.write_all(&out).map_err(|e| e.to_string())?;
        out = gz.finish().map_err(|e| e.to_string())?;
    }

    fs::write(path, out).map_err(|e| e.to_string())
}

/// Restore VM state from a file.
pub fn restore_state(vm: &mut Vm, path: &Path) -> Result<(), String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;

    // Check if gzipped
    let data = if bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
        let mut gz = flate2::read::GzDecoder::new(&bytes[..]);
        let mut out = Vec::new();
        std::io::Read::read_to_end(&mut gz, &mut out).map_err(|e| e.to_string())?;
        out
    } else {
        bytes
    };

    if data.len() < 17 {
        return Err("state file too small".into());
    }

    let magic = u64::from_le_bytes(data[0..8].try_into().unwrap());
    if magic != STATE_HEADER {
        return Err("not an emuera-tty state capture file".into());
    }

    let version = u32::from_le_bytes(data[8..12].try_into().unwrap());
    if version != STATE_VERSION {
        return Err(format!("unsupported state file version {}", version));
    }

    // Create a reader for the body (skip our custom header)
    let body_start = 16;
    let marker = data[body_start];
    if marker != STATE_MARKER {
        return Err("not a state capture file (missing marker)".into());
    }

    // Build a proper Emuera-format body for the Reader.
    // The saved data after our marker is: [FileType][game_code][game_version][text][entries...][EOF]
    // We need to prepend the Emuera header: [HEADER][VERSION][extra]
    let mut reader_body = Vec::new();
    reader_body.extend_from_slice(&super::erasav::HEADER.to_le_bytes());
    reader_body.extend_from_slice(&super::erasav::VERSION.to_le_bytes());
    reader_body.extend_from_slice(&0u32.to_le_bytes()); // extra words
                                                        // Note: we do NOT add FileType here because the saved data already includes it
    reader_body.extend_from_slice(&data[body_start + 1..]); // Skip marker byte

    let mut r = Reader::open(&reader_body).ok_or("failed to open state capture")?;

    // Read the Emuera save header fields (FileType, game_code, game_version, text)
    // just like the normal save loading does.
    let _file_type = r.file_type()?;
    let _saved_game_code = r.raw_i64()?;
    let _saved_game_version = r.raw_i64()?;
    let _saved_text = r.string()?;

    // Read all entries
    let mut captured_statics = Vec::new();
    let mut static_index: HashMap<(String, String), usize> = HashMap::default();
    // For console lines - accumulate and restore after reading all entries
    let mut console_lines_data: Vec<LineData> = Vec::new();
    // For frames - accumulate and restore after reading all entries
    #[derive(Default)]
    struct FrameData {
        func_name: Option<String>,
        func_order: Option<u8>, // 0=Pri, 1=Normal, 2=Later
        func_only: Option<bool>,
        func_single: Option<bool>,
        pc: Option<usize>,
        slots: Vec<SlotData>,
        refs: Vec<(String, Loc)>,
    }
    #[derive(Default)]
    struct SlotData {
        slot_type: Option<u8>, // 0=None, 1=For, 2=Sel
        end: Option<i64>,
        step: Option<i64>,
        val_int: Option<i64>,
        val_str: Option<String>,
    }
    let mut frames_data: Vec<FrameData> = Vec::new();
    // For frame vars (#DIM DYNAMIC) - accumulate and restore after frames are created
    let mut frame_vars_data: HashMap<(usize, String), (Vec<usize>, Vec<(usize, i64)>, Vec<(usize, String)>, bool)> = HashMap::default();

    loop {
        match r.entry()? {
            Entry::Eof => break,
            Entry::Var { key, dims, ints, strs, is_str } => {
                if key.starts_with("__GLOBAL__") {
                    // Global variable
                    let name = key.strip_prefix("__GLOBAL__").unwrap();
                    restore_global(vm, name, &dims, &ints, &strs, is_str)?;
                } else if key.starts_with("__STATIC_IDX__") {
                    // Static index entry: __STATIC_IDX__func_name::var_name -> index
                    let rest = key.strip_prefix("__STATIC_IDX__").unwrap();
                    let parts: Vec<&str> = rest.splitn(2, "::").collect();
                    if parts.len() == 2 {
                        if let Some(&(_, idx)) = ints.first() {
                            static_index.insert((parts[0].to_string(), parts[1].to_string()), idx as usize);
                        }
                    }
                } else if key.starts_with("__STATIC__") {
                    // Static variable: format __STATIC__func_name::var_name
                    let rest = key.strip_prefix("__STATIC__").unwrap();
                    let parts: Vec<&str> = rest.splitn(2, "::").collect();
                    if parts.len() == 2 {
                        restore_static(vm, parts[0], parts[1], &dims, &ints, &strs, is_str, &mut captured_statics, &mut static_index)?;
                    }
                } else if key == "__STATIC_NONE__" {
                    // Marker for None static slot
                    if let Some(&(_, idx)) = ints.first() {
                        if captured_statics.len() <= idx as usize {
                            captured_statics.resize_with(idx as usize + 1, || None);
                        }
                        captured_statics[idx as usize] = None;
                    }
                } else if key.starts_with("__CHARA__") {
                    // Character variable: format __CHARA__chara_idx::var_name
                    let rest = key.strip_prefix("__CHARA__").unwrap();
                    let parts: Vec<&str> = rest.splitn(2, "::").collect();
                    if parts.len() == 2 {
                        let chara_idx = parts[0].parse::<usize>().map_err(|e| format!("bad chara index: {}", e))?;
                        restore_chara_var(vm, chara_idx, parts[1], &dims, &ints, &strs, is_str)?;
                    }
                } else if key.starts_with("__FRAME_VAR__") {
                    // Frame local variable: format __FRAME_VAR__frame_idx::var_name
                    let rest = key.strip_prefix("__FRAME_VAR__").unwrap();
                    let parts: Vec<&str> = rest.splitn(2, "::").collect();
                    if parts.len() == 2 {
                        if let Ok(frame_idx) = parts[0].parse::<usize>() {
                            let var_name = parts[1].to_string();
                            // Store for later frame restoration
                            frame_vars_data.insert((frame_idx, var_name), (dims, ints, strs, is_str));
                        }
                    }
                } else if key.starts_with("__FRAME__")
                    && !key.starts_with("__FRAME_VAR__")
                    && !key.starts_with("__FRAME_PC_$")
                    && !key.starts_with("__FRAME_SLOT_$")
                    && !key.starts_with("__FRAME_REF__")
                {
                    // Frame header: __FRAME__<idx> -> function name;order;only;single
                    let rest = key.strip_prefix("__FRAME__").unwrap();
                    if let Ok(frame_idx) = rest.parse::<usize>() {
                        if let Some((_, s)) = strs.first() {
                            // Parse header: name;order;only;single
                            let parts: Vec<&str> = s.split(';').collect();
                            if parts.len() >= 4 {
                                while frames_data.len() <= frame_idx {
                                    frames_data.push(FrameData::default());
                                }
                                frames_data[frame_idx].func_name = Some(parts[0].to_string());
                                frames_data[frame_idx].func_order = parts[1].parse().ok();
                                frames_data[frame_idx].func_only = parts[2].parse::<u8>().ok().map(|v| v != 0);
                                frames_data[frame_idx].func_single = parts[3].parse::<u8>().ok().map(|v| v != 0);
                            } else {
                                // Backward compatibility: old format without overload info
                                while frames_data.len() <= frame_idx {
                                    frames_data.push(FrameData::default());
                                }
                                frames_data[frame_idx].func_name = Some(s.clone());
                            }
                        }
                    }
                } else if key.starts_with("__FRAME_PC_$") {
                    // Frame PC: __FRAME_PC_$<idx> -> pc
                    let rest = key.strip_prefix("__FRAME_PC_$").unwrap();
                    if let Ok(frame_idx) = rest.parse::<usize>() {
                        if let Some(&(_, pc)) = ints.first() {
                            while frames_data.len() <= frame_idx {
                                frames_data.push(FrameData::default());
                            }
                            frames_data[frame_idx].pc = Some(pc as usize);
                        }
                    }
                } else if key.starts_with("__FRAME_SLOT_$") {
                    // Frame slots: __FRAME_SLOT_$<frame_idx>_$<slot_idx>_TYPE/END/STEP/VAL/STR
                    let rest = key.strip_prefix("__FRAME_SLOT_$").unwrap();
                    let parts: Vec<&str> = rest.split('_').collect();
                    if parts.len() >= 3 {
                        let frame_idx = parts[0].parse::<usize>().ok();
                        let slot_idx = parts[1].parse::<usize>().ok();
                        let field = parts[2];
                        if let (Some(frame_idx), Some(slot_idx)) = (frame_idx, slot_idx) {
                            while frames_data.len() <= frame_idx {
                                frames_data.push(FrameData::default());
                            }
                            let frame_data = &mut frames_data[frame_idx];
                            while frame_data.slots.len() <= slot_idx {
                                frame_data.slots.push(SlotData::default());
                            }
                            let slot_data = &mut frame_data.slots[slot_idx];
                            match field {
                                "TYPE" => {
                                    if let Some(&(_, v)) = ints.first() {
                                        slot_data.slot_type = Some(v as u8);
                                    }
                                }
                                "END" => {
                                    if let Some(&(_, v)) = ints.first() {
                                        slot_data.end = Some(v);
                                    }
                                }
                                "STEP" => {
                                    if let Some(&(_, v)) = ints.first() {
                                        slot_data.step = Some(v);
                                    }
                                }
                                "VAL" => {
                                    if let Some(&(_, v)) = ints.first() {
                                        slot_data.val_int = Some(v);
                                    }
                                }
                                "STR" => {
                                    if let Some((_, v)) = strs.first() {
                                        slot_data.val_str = Some(v.clone());
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                } else if key.starts_with("__FRAME_REF__") {
                    // Frame refs: __FRAME_REF__<frame_idx>::<name> -> location string
                    let rest = key.strip_prefix("__FRAME_REF__").unwrap();
                    let parts: Vec<&str> = rest.splitn(2, "::").collect();
                    if parts.len() == 2 {
                        let frame_idx = parts[0].parse::<usize>().ok();
                        if let (Some(frame_idx), Some((_, loc_str))) = (frame_idx, strs.first()) {
                            while frames_data.len() <= frame_idx {
                                frames_data.push(FrameData::default());
                            }
                            let loc = parse_loc(loc_str)?;
                            frames_data[frame_idx].refs.push((parts[1].to_string(), loc));
                        }
                    }
                } else if key == "__RNG__" {
                    // RNG state stored as a scalar
                    if let Some((_, val)) = ints.first() {
                        vm.rng = *val as u64;
                    }
                } else if key == "__SYS_STATE__" {
                    if let Some((_, s)) = strs.first() {
                        vm.state = s.clone();
                    }
                } else if key == "__VM_FLAGS__" {
                    // VM flags: inputs_seen|inject_at|ctrain|dotrain_ok|dotrain|comend_wait|text_exts|calls|code_bytes|code_budget|big_dropped_len|warned_len|long_oneinput|strict|flow_input_0|flow_input_1|flow_input_2|flow_input_3|flow_input_4|mes_skip|macro_queue_len|is_timeout|last_input|log|defer_warnings|pending_refs|trace|memstats|debugger|loop_check_count
                    if let Some((_, s)) = strs.first() {
                        let parts: Vec<&str> = s.split('|').collect();
                        if parts.len() >= 23 {
                            vm.inputs_seen = parts[0].parse().unwrap_or(0);
                            vm.inject_at = parts[1].parse::<u64>().ok().filter(|&x| x != u64::MAX);
                            // ctrain is complex, skip for now
                            vm.dotrain_ok = parts[3].parse::<u8>().ok().map(|x| x != 0).unwrap_or(false);
                            vm.dotrain = parts[4].parse::<i64>().ok();
                            vm.comend_wait = parts[5].parse::<u8>().ok().map(|x| x != 0).unwrap_or(false);
                            vm.text_exts = parts[6].split(',').filter(|x| !x.is_empty()).map(|x| x.to_string()).collect();
                            vm.calls = parts[7].parse().unwrap_or(0);
                            vm.code_bytes = parts[8].parse().unwrap_or(0);
                            vm.code_budget = parts[9].parse().unwrap_or(0);
                            // big_dropped_len and warned_len are informational
                            vm.long_oneinput = parts[12].parse::<u8>().ok().map(|x| x != 0).unwrap_or(false);
                            vm.strict = parts[13].parse::<u8>().ok().map(|x| x != 0).unwrap_or(false);
                            // flow_input: parts[14] = "a,b,c,d,e"
                            let flow_parts: Vec<&str> = parts[14].split(',').collect();
                            if flow_parts.len() == 5 {
                                vm.flow_input.0 = flow_parts[0].parse().unwrap_or(0);
                                vm.flow_input.1 = flow_parts[1].parse::<u8>().ok().map(|x| x != 0).unwrap_or(false);
                                vm.flow_input.2 = flow_parts[2].parse::<u8>().ok().map(|x| x != 0).unwrap_or(false);
                                vm.flow_input.3 = flow_parts[3].to_string();
                                vm.flow_input.4 = flow_parts[4].parse::<u8>().ok().map(|x| x != 0).unwrap_or(false);
                            }
                            vm.mes_skip = parts[15].parse::<u8>().ok().map(|x| x != 0).unwrap_or(false);
                            // macro_queue_len is informational
                            vm.is_timeout = parts[17].parse::<u8>().ok().map(|x| x != 0).unwrap_or(false);
                            vm.last_input = parts[18].to_string();
                            // log is complex, skip
                            vm.defer_warnings = parts[20].parse::<u8>().ok().map(|x| x != 0).unwrap_or(false);
                            // New fields (21-26)
                            if parts.len() > 21 {
                                // pending_refs: parse and restore
                                let pending_refs_str = parts[21];
                                if !pending_refs_str.is_empty() {
                                    for pair in pending_refs_str.split(';') {
                                        if let Some(eq_pos) = pair.find('=') {
                                            let name = pair[..eq_pos].to_string();
                                            let loc_str = &pair[eq_pos + 1..];
                                            if let Ok(loc) = parse_loc(loc_str) {
                                                vm.pending_refs.push((name, loc));
                                            }
                                        }
                                    }
                                }
                            }
                            if parts.len() > 22 {
                                vm.trace = if parts[22].is_empty() { None } else { Some(parts[22].to_string()) };
                            }
                            if parts.len() > 23 {
                                vm.memstats = if parts[23].is_empty() { None } else { Some(parts[23].to_string()) };
                            }
                            if parts.len() > 24 {
                                // debugger: "verbose" (single u8)
                                vm.debugger.verbose = parts[24].parse::<u8>().ok().map(|x| x != 0).unwrap_or(false);
                            }
                            if parts.len() > 25 {
                                vm.loop_check_count = parts[25].parse().unwrap_or(0);
                            }
                        }
                    }
                } else if key == "__TR_STATE__" {
                    // Translation state: tr_run|tr_eval|tr_fold|tr_line|tr_on|tr_last|mem_str
                    if let Some((_, s)) = strs.first() {
                        let parts: Vec<&str> = s.split('|').collect();
                        if parts.len() >= 7 {
                            vm.tr_run = parts[0].parse::<u8>().ok().map(|x| x != 0).unwrap_or(false);
                            vm.tr_eval = parts[1].parse::<u8>().ok().map(|x| x != 0).unwrap_or(false);
                            vm.tr_fold = parts[2].parse::<u8>().ok().map(|x| x != 0).unwrap_or(false);
                            vm.tr_line = parts[3].parse::<u8>().unwrap_or(0);
                            vm.tr_on = parts[4].parse::<u8>().ok().map(|x| x != 0).unwrap_or(false);
                            vm.tr_last = parts[5].to_string();
                            // tr_memory: restore from mem_str (parts[6])
                            // tr_memory is VecDeque<(String, &'static str)> - we can't easily restore the 'static lifetime
                            // For now, clear it. The translation memory will be rebuilt from TR files.
                            vm.tr_memory.clear();
                        }
                    }
                } else if key.starts_with("__CONSOLE__") {
                    // Console state - accumulate for later restoration
                    let entry = Entry::Var { key: key.clone(), dims, ints, strs, is_str };
                    process_console_entry(&mut vm.con, &key, &entry, &mut console_lines_data)?;
                }
                // Note: __USER_DIM__ and __DIM_ORDER__ are skipped for now.
                // They're complex to serialize/deserialize with Expr types.
                // The game will reinitialize them on load if needed.
            }
            Entry::Map { key, pairs } => {
                if key == "__CHARA_NAMES__" {
                    restore_chara_names(vm, &pairs)?;
                } else if key.starts_with("__MAP__") {
                    let name = key.strip_prefix("__MAP__").unwrap();
                    let mut m = StrMap::default();
                    for (k, v) in pairs {
                        m.set(k, v);
                    }
                    vm.maps.insert(name.to_string(), m);
                }
            }
            Entry::Xml { key, xml } => {
                if key.starts_with("__XML__") {
                    let name = key.strip_prefix("__XML__").unwrap();
                    if let Ok(d) = crate::xml::load_doc(&xml) {
                        vm.xml_docs.insert(name.to_string(), d);
                    }
                }
            }
            Entry::Dt { key, schema, data } => {
                if key.starts_with("__DT__") {
                    let name = key.strip_prefix("__DT__").unwrap();
                    match DataTable::from_xml(name, &schema, &data) {
                        Ok(t) => {
                            vm.dts.insert(name.to_string(), t);
                        }
                        Err(e) => vm.warn(&format!("state capture: table {}: {}", name, e)),
                    };
                }
            }
            Entry::Separator => {} // Ignore for now
            Entry::Eoc => {}       // Ignore for now
        }
    }

    // Restore console lines (accumulated during entry processing)
    console_lines_data.sort_by_key(|l| l.idx);
    vm.con.lines = console_lines_data.into_iter().map(|l| l.line).collect();

    // Restore frames (accumulated during entry processing)
    // Clear existing frames first
    vm.frames.clear();
    // Reconstruct frames from saved data
    let mut restored_count = 0;
    let total_frames = frames_data.len();
    for (frame_idx, frame_data) in frames_data.into_iter().enumerate() {
        let Some(func_name) = frame_data.func_name else {
            continue;
        };
        // Find the function by name and overload info
        let Some(funcs) = vm.prog.funcs.get(&func_name) else {
            continue;
        };
        // Try to find the matching overload
        let func = if let (Some(order), Some(only), Some(single)) = (frame_data.func_order, frame_data.func_only, frame_data.func_single) {
            funcs.iter().find(|f| {
                let f_order = match f.order {
                    crate::ast::EventOrder::Pri => 0,
                    crate::ast::EventOrder::Normal => 1,
                    crate::ast::EventOrder::Later => 2,
                };
                f_order == order && f.only == only && f.single == single
            }).cloned()
        } else {
            None
        }.or_else(|| funcs.first().cloned()); // Fallback to first overload

        let Some(func) = func else {
            continue;
        };
        let body = func.body();
        let pc = frame_data.pc.unwrap_or(0);
        // Create slots vector from body.n_slots
        let mut slots = vec![Slot::None; body.n_slots];
        let mut refs = HashMap::default();

        // Restore slots
        for (slot_idx, slot_data) in frame_data.slots.into_iter().enumerate() {
            if slot_idx >= slots.len() {
                break;
            }
            match slot_data.slot_type {
                Some(0) => {
                    slots[slot_idx] = Slot::None;
                }
                Some(1) => {
                    if let (Some(end), Some(step)) = (slot_data.end, slot_data.step) {
                        slots[slot_idx] = Slot::For(end, step);
                    }
                }
                Some(2) => {
                    if let Some(val_int) = slot_data.val_int {
                        slots[slot_idx] = Slot::Sel(Value::Int(val_int));
                    } else if let Some(val_str) = slot_data.val_str {
                        slots[slot_idx] = Slot::Sel(Value::Str(val_str));
                    }
                }
                _ => {}
            }
        }

        // Restore refs
        for (name, loc) in frame_data.refs {
            refs.insert(name, loc);
        }

        // Restore frame vars (#DIM DYNAMIC) from accumulated data
        let mut vars = HashMap::default();
        let mut remaining_vars = Vec::new();
        for ((f_idx, var_name), (dims, ints, strs, is_str)) in frame_vars_data.drain() {
            if f_idx == frame_idx {
                // Create array and fill with saved data
                let mut arr = Arr::new(is_str, &dims);
                fill_arr(&mut arr, &dims, &ints, &strs, is_str);
                vars.insert(var_name, arr);
            } else {
                // Keep for later frames
                remaining_vars.push(((f_idx, var_name), (dims, ints, strs, is_str)));
            }
        }
        // Re-insert remaining vars
        for (key, val) in remaining_vars {
            frame_vars_data.insert(key, val);
        }

        vm.frames.push(Frame { func, body, pc, vars, slots, refs });
        restored_count += 1;
    }
    // Note: restored_count may be less than total_frames if some frames couldn't be restored
    // (e.g., function not found in prog.funcs). This is not necessarily an error.

    // Assign restored statics and static_index to VM
    vm.statics = captured_statics;
    vm.static_index = static_index;

    // Restore user_dims from saved data (if available)
    // Note: user_dims is normally populated at game load from ERH files.
    // We don't clear it here because the game config is only parsed at load time.
    // The saved user_dims entries (__USER_DIM__) are complex to deserialize due to Expr types.
    // For now, we keep the original user_dims from game load.

    // Restore dim_order from saved data (if available)
    // Same reasoning as user_dims - keep original from game load.

    // Re-seed global RNG
    crate::vm::CURRENT_SEED.store(vm.rng, Ordering::Relaxed);

    Ok(())
}

// --- Writing functions ---

fn write_globals(w: &mut Writer, globals: &Globals) -> Result<(), String> {
    for (name, arr) in globals.iter() {
        let key = format!("__GLOBAL__{}", name);
        write_arr(w, &key, arr)?;
    }
    Ok(())
}

fn write_statics(w: &mut Writer, statics: &[Option<Arr>], static_index: &HashMap<(String, String), usize>) -> Result<(), String> {
    // Write static index first
    for ((func_name, var_name), idx) in static_index {
        let key = format!("__STATIC_IDX__{}::{}", func_name, var_name);
        w.var(&key, &[], Values::Int(&[*idx as i64]));
    }

    // Write static arrays
    for (idx, opt_arr) in statics.iter().enumerate() {
        if let Some(arr) = opt_arr {
            // Find the name from static_index
            let name = static_index.iter().find(|(_, &i)| i == idx).map(|(k, _)| k.clone());
            if let Some((func_name, var_name)) = name {
                let key = format!("__STATIC__{}::{}", func_name, var_name);
                write_arr(w, &key, arr)?;
            }
        } else {
            // Write a marker for None
            w.var("__STATIC_NONE__", &[], Values::Int(&[idx as i64]));
        }
    }
    Ok(())
}

fn write_charas(w: &mut Writer, charas: &[Chara], chara_names: &CharaNames) -> Result<(), String> {
    // Write chara names FIRST so they're restored before chara variables
    for (idx, name) in chara_names.names.iter().enumerate() {
        let key = format!("__CHARA_NAME__{}", idx);
        w.var(&key, &[], Values::Str(&[name.to_string()]));
    }

    // Write chara variables
    for (idx, chara) in charas.iter().enumerate() {
        for (var_idx, opt_arr) in chara.vars.iter().enumerate() {
            if let Some(arr) = opt_arr {
                let var_name = chara_names.name(var_idx as u32);
                let key = format!("__CHARA__{}::{}", idx, var_name);
                write_arr(w, &key, arr)?;
            }
        }
    }
    Ok(())
}

fn write_frames(w: &mut Writer, frames: &[Frame], _vm: &Vm) -> Result<(), String> {
    for (frame_idx, frame) in frames.iter().enumerate() {
        // Frame header - include function overload info (order, only, single)
        let key = format!("__FRAME__{}", frame_idx);
        let order = match frame.func.order {
            crate::ast::EventOrder::Pri => 0,
            crate::ast::EventOrder::Normal => 1,
            crate::ast::EventOrder::Later => 2,
        };
        let header = format!("{};{};{};{}", frame.func.name, order, frame.func.only as u8, frame.func.single as u8);
        w.var(&key, &[], Values::Str(&[header]));
        // Frame PC
        w.var(&format!("__FRAME_PC_${}", frame_idx), &[], Values::Int(&[frame.pc as i64]));

        // Frame local vars (#DIM DYNAMIC)
        for (name, arr) in &frame.vars {
            let key = format!("__FRAME_VAR__{}::{}", frame_idx, name);
            write_arr(w, &key, arr)?;
        }

        // Frame slots - serialize each slot as an entry
        for (slot_idx, slot) in frame.slots.iter().enumerate() {
            match slot {
                Slot::None => {
                    w.var(&format!("__FRAME_SLOT_${}_${}", frame_idx, slot_idx), &[], Values::Int(&[0]));
                }
                Slot::For(end, step) => {
                    w.var(&format!("__FRAME_SLOT_${}_${}_TYPE", frame_idx, slot_idx), &[], Values::Int(&[1]));
                    w.var(&format!("__FRAME_SLOT_${}_${}_END", frame_idx, slot_idx), &[], Values::Int(&[*end]));
                    w.var(&format!("__FRAME_SLOT_${}_${}_STEP", frame_idx, slot_idx), &[], Values::Int(&[*step]));
                }
                Slot::Sel(val) => {
                    w.var(&format!("__FRAME_SLOT_${}_${}_TYPE", frame_idx, slot_idx), &[], Values::Int(&[2]));
                    match val {
                        Value::Int(i) => {
                            w.var(&format!("__FRAME_SLOT_${}_${}_VAL", frame_idx, slot_idx), &[], Values::Int(&[*i]));
                        }
                        Value::Str(s) => {
                            w.var(&format!("__FRAME_SLOT_${}_${}_STR", frame_idx, slot_idx), &[], Values::Str(&[s.clone()]));
                        }
                    }
                }
            }
        }

        // Frame refs
        for (name, loc) in &frame.refs {
            let key = format!("__FRAME_REF__{}::{}", frame_idx, name);
            write_loc(w, &key, loc)?;
        }
    }
    Ok(())
}

fn write_user_dims(w: &mut Writer, user_dims: &HashMap<String, DimDecl>) -> Result<(), String> {
    for (name, dim) in user_dims {
        let key = format!("__USER_DIM__{}", name);
        // Serialize DimDecl - we need to store name, dims, init, is_chara, is_global, is_const, is_savedata, decl_name
        // For simplicity, store as JSON-like string
        let data =
            format!("{}|{:?}|{:?}|{}|{}|{}|{}|{}", dim.name, dim.dims, dim.init, dim.is_chara, dim.is_global, dim.is_const, dim.is_savedata, dim.decl_name);
        w.var(&key, &[], Values::Str(&[data]));
    }
    Ok(())
}

fn write_dim_order(w: &mut Writer, order: &[usize]) -> Result<(), String> {
    let key = "__DIM_ORDER__";
    let ints: Vec<i64> = order.iter().map(|&x| x as i64).collect();
    w.var(key, &[ints.len()], Values::Int(&ints));
    Ok(())
}

fn write_rng(w: &mut Writer, rng: u64) -> Result<(), String> {
    let key = "__RNG__";
    w.var(key, &[], Values::Int(&[rng as i64]));
    Ok(())
}

fn write_sys_state(w: &mut Writer, state: &str) -> Result<(), String> {
    let key = "__SYS_STATE__";
    w.var(key, &[], Values::Str(&[state.to_string()]));
    Ok(())
}

fn write_em_data(w: &mut Writer, maps: &HashMap<String, StrMap>, dts: &HashMap<String, DataTable>, xml_docs: &HashMap<String, Element>) -> Result<(), String> {
    for (name, map) in maps {
        let key = format!("__MAP__{}", name);
        let pairs: Vec<(&String, &String)> = map.pairs().collect();
        w.map(&key, pairs.into_iter());
    }
    for (name, xml) in xml_docs {
        let key = format!("__XML__{}", name);
        w.xml(&key, &crate::xml::doc_to_string(xml));
    }
    for (name, dt) in dts {
        let key = format!("__DT__{}", name);
        w.dt(&key, &dt.to_xml_schema(), &dt.to_xml());
    }
    Ok(())
}

fn write_tr_state(w: &mut Writer, vm: &Vm) -> Result<(), String> {
    let key = "__TR_STATE__";
    // Encode as a string with all fields
    let mem_str = vm.tr_memory.iter().map(|(s, t)| format!("{}:{}", s, t)).collect::<Vec<_>>().join("|");
    let data = format!("{}|{}|{}|{}|{}|{}|{}", vm.tr_run as u8, vm.tr_eval as u8, vm.tr_fold as u8, vm.tr_line, vm.tr_on as u8, vm.tr_last, mem_str);
    w.var(key, &[], Values::Str(&[data]));
    Ok(())
}

fn write_vm_flags(w: &mut Writer, vm: &Vm) -> Result<(), String> {
    let key = "__VM_FLAGS__";
    // Serialize pending_refs
    let pending_refs_str = vm.pending_refs.iter()
        .map(|(name, loc)| format!("{}={}", name, loc_to_string(loc)))
        .collect::<Vec<_>>()
        .join(";");
    // Serialize trace and memstats
    let trace_str = vm.trace.as_deref().unwrap_or("");
    let memstats_str = vm.memstats.as_deref().unwrap_or("");
    // Serialize debugger state (only public verbose field)
    let debugger_str = format!("{}", vm.debugger.verbose as u8);
    // loop_check_count and loop_check_time (we can't serialize Instant, so use count only)
    let loop_check_count = vm.loop_check_count;
    let data = format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
        vm.inputs_seen,
        vm.inject_at.unwrap_or(u64::MAX),
        vm.ctrain.as_ref().map(|(v, i)| format!("{}:{}", v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(","), i)).unwrap_or_default(),
        vm.dotrain_ok as u8,
        vm.dotrain.unwrap_or(-1),
        vm.comend_wait as u8,
        vm.text_exts.join(","),
        vm.calls,
        vm.code_bytes,
        vm.code_budget,
        vm.big_dropped.len(),
        vm.warned.len(),
        vm.long_oneinput as u8,
        vm.strict as u8,
        format!("{},{},{},{},{}", vm.flow_input.0, vm.flow_input.1 as u8, vm.flow_input.2 as u8, vm.flow_input.3, vm.flow_input.4 as u8),
        vm.mes_skip as u8,
        vm.macro_queue.len(),
        vm.is_timeout as u8,
        vm.last_input,
        vm.log.join("|"),
        vm.defer_warnings as u8,
        pending_refs_str,
        trace_str,
        memstats_str,
        debugger_str,
        loop_check_count,
    );
    w.var(key, &[], Values::Str(&[data]));
    Ok(())
}

fn loc_to_string(loc: &Loc) -> String {
    match loc {
        Loc::Global(idx) => format!("GLOBAL:{}", idx),
        Loc::Frame(name) => format!("FRAME:{}", name),
        Loc::FrameAt(frame_idx, name) => format!("FRAME_AT:{}:{}", frame_idx, name),
        Loc::Static(idx) => format!("STATIC:{}", idx),
        Loc::Chara(chara_idx, var_idx) => format!("CHARA:{}:{}", chara_idx, var_idx),
        Loc::Ro(val) => format!("RO:{}", val),
    }
}

fn write_game_info(w: &mut Writer, vm: &Vm) -> Result<(), String> {
    let key = "__GAME_INFO__";
    let data = format!("{}|{}|{}", vm.game_dir.display(), vm.game_code(), vm.game_version().0);
    w.var(key, &[], Values::Str(&[data]));
    Ok(())
}

fn write_console(w: &mut Writer, con: &Console) -> Result<(), String> {
    let key = "__CONSOLE__";
    // Write console state as a series of fields
    // We'll write multiple entries with prefixed keys for each field

    // Basic fields
    w.var(&format!("{}_WIDTH", key), &[], Values::Int(&[con.width as i64]));
    w.var(&format!("{}_GEN", key), &[], Values::Int(&[con.gen as i64]));
    w.var(&format!("{}_VALID_GEN", key), &[], Values::Int(&[con.valid_gen as i64]));
    w.var(&format!("{}_WAITS", key), &[], Values::Int(&[con.waits as i64]));
    w.var(&format!("{}_RUNNING", key), &[], Values::Str(&[con.running.clone()]));
    w.var(&format!("{}_ALIGN", key), &[], Values::Int(&[con.align as i64]));
    w.var(&format!("{}_CUR_ALIGN", key), &[], Values::Int(&[con.cur_align.map_or(-1, |a| a as i64)]));
    w.var(&format!("{}_STYLE_FG", key), &[], Values::Int(&[con.style.fg.0 as i64, con.style.fg.1 as i64, con.style.fg.2 as i64]));
    w.var(
        &format!("{}_STYLE_BG", key),
        &[],
        Values::Int(&[con.style.bg.map_or(-1, |c| c.0 as i64), con.style.bg.map_or(-1, |c| c.1 as i64), con.style.bg.map_or(-1, |c| c.2 as i64)]),
    );
    w.var(&format!("{}_STYLE_BOLD", key), &[], Values::Int(&[con.style.bold as i64]));
    w.var(&format!("{}_STYLE_ITALIC", key), &[], Values::Int(&[con.style.italic as i64]));
    w.var(&format!("{}_STYLE_UNDERLINE", key), &[], Values::Int(&[con.style.underline as i64]));
    w.var(&format!("{}_STYLE_STRIKE", key), &[], Values::Int(&[con.style.strike as i64]));
    w.var(&format!("{}_DEFAULT_FG", key), &[], Values::Int(&[con.default_fg.0 as i64, con.default_fg.1 as i64, con.default_fg.2 as i64]));
    w.var(&format!("{}_DEFAULT_BG", key), &[], Values::Int(&[con.default_bg.0 as i64, con.default_bg.1 as i64, con.default_bg.2 as i64]));
    w.var(&format!("{}_SCREEN_BG", key), &[], Values::Int(&[con.screen_bg.0 as i64, con.screen_bg.1 as i64, con.screen_bg.2 as i64]));
    w.var(&format!("{}_FOCUS_FG", key), &[], Values::Int(&[con.focus_fg.0 as i64, con.focus_fg.1 as i64, con.focus_fg.2 as i64]));
    w.var(&format!("{}_LOG_FG", key), &[], Values::Int(&[con.log_fg.0 as i64, con.log_fg.1 as i64, con.log_fg.2 as i64]));
    w.var(
        &format!("{}_TIP_COLORS", key),
        &[],
        Values::Int(&[
            con.tip_colors.map_or(-1, |c| c.0 .0 as i64),
            con.tip_colors.map_or(-1, |c| c.0 .1 as i64),
            con.tip_colors.map_or(-1, |c| c.0 .2 as i64),
            con.tip_colors.map_or(-1, |c| c.1 .0 as i64),
            con.tip_colors.map_or(-1, |c| c.1 .1 as i64),
            con.tip_colors.map_or(-1, |c| c.1 .2 as i64),
        ]),
    );
    w.var(&format!("{}_TIP_DELAY", key), &[], Values::Int(&[con.tip_delay as i64]));
    w.var(&format!("{}_TIP_DURATION", key), &[], Values::Int(&[con.tip_duration as i64]));
    w.var(&format!("{}_FONT_SIZE", key), &[], Values::Int(&[(con.font_size * 1000.0) as i64]));
    w.var(&format!("{}_LINE_HEIGHT", key), &[], Values::Int(&[(con.line_height * 1000.0) as i64]));
    w.var(&format!("{}_PRINTC_LEN", key), &[], Values::Int(&[con.printc_len as i64]));
    w.var(&format!("{}_PRINTC_PER_LINE", key), &[], Values::Int(&[con.printc_per_line as i64]));
    w.var(&format!("{}_BUTTON_WRAP", key), &[], Values::Int(&[con.button_wrap as i64]));
    w.var(&format!("{}_KEY_MACRO", key), &[], Values::Int(&[con.key_macro as i64]));
    w.var(&format!("{}_IN_KOJO", key), &[], Values::Int(&[con.in_kojo as i64]));
    w.var(&format!("{}_SOUND_ON", key), &[], Values::Int(&[con.sound_on as i64]));
    w.var(&format!("{}_NOTE_WATCH", key), &[], Values::Int(&[con.note_watch as i64]));
    w.var(&format!("{}_NOTE_LINE", key), &[], Values::Str(&[con.note_line.clone().unwrap_or_default()]));
    w.var(&format!("{}_TRACK_SRC", key), &[], Values::Int(&[con.track_src as i64]));
    w.var(&format!("{}_SRC", key), &[], Values::Int(&[con.src.0 as i64, con.src.1 as i64]));
    w.var(&format!("{}_LAST_TEMP", key), &[], Values::Int(&[con.last_temp as i64]));
    w.var(&format!("{}_DRAWLINE_STR", key), &[], Values::Str(&[con.drawline_str.clone()]));
    w.var(&format!("{}_REDRAW", key), &[], Values::Int(&[con.redraw as i64]));
    w.var(&format!("{}_SKIP_DISP", key), &[], Values::Int(&[con.skip_disp as i64]));
    w.var(&format!("{}_SKIP_USER", key), &[], Values::Int(&[con.skip_user as i64]));
    w.var(&format!("{}_FONT_NAME", key), &[], Values::Str(&[con.font_name.clone()]));
    w.var(&format!("{}_DEFAULT_FONT", key), &[], Values::Str(&[con.default_font.clone()]));
    w.var(&format!("{}_TITLE", key), &[], Values::Str(&[con.title.clone()]));
    w.var(&format!("{}_INFINITE_LOOP_ALERT", key), &[], Values::Int(&[con.infinite_loop_alert_time as i64]));
    w.var(&format!("{}_MAX_LINES", key), &[], Values::Int(&[con.max_lines as i64]));
    w.var(&format!("{}_DROPPED_FRONT", key), &[], Values::Int(&[con.dropped_front as i64]));
    w.var(&format!("{}_LOW_WATER", key), &[], Values::Int(&[con.low_water as i64]));
    w.var(&format!("{}_DIRTY", key), &[], Values::Int(&[con.dirty as i64]));

    // ClipConfig
    w.var(&format!("{}_CLIP_ENABLED", key), &[], Values::Int(&[con.clip.enabled as i64]));
    w.var(&format!("{}_CLIP_IGNORE_TAGS", key), &[], Values::Int(&[con.clip.ignore_tags as i64]));
    w.var(&format!("{}_CLIP_REPLACE_TAGS", key), &[], Values::Str(&[con.clip.replace_tags.clone()]));
    w.var(&format!("{}_CLIP_NEW_LINES_ONLY", key), &[], Values::Int(&[con.clip.new_lines_only as i64]));
    w.var(&format!("{}_CLIP_CLEAR_ON_REFRESH", key), &[], Values::Int(&[con.clip.clear_on_refresh as i64]));
    w.var(&format!("{}_CLIP_LEFT_CLICK", key), &[], Values::Int(&[con.clip.left_click as i64]));
    w.var(&format!("{}_CLIP_MIDDLE_CLICK", key), &[], Values::Int(&[con.clip.middle_click as i64]));
    w.var(&format!("{}_CLIP_DOUBLE_CLICK", key), &[], Values::Int(&[con.clip.double_click as i64]));
    w.var(&format!("{}_CLIP_ON_WAIT", key), &[], Values::Int(&[con.clip.on_wait as i64]));
    w.var(&format!("{}_CLIP_ON_INPUT", key), &[], Values::Int(&[con.clip.on_input as i64]));
    w.var(&format!("{}_CLIP_MAX_LINES", key), &[], Values::Int(&[con.clip.max_lines as i64]));
    w.var(&format!("{}_CLIP_BUFFER", key), &[], Values::Int(&[con.clip.buffer as i64]));
    w.var(&format!("{}_CLIP_MIN_INTERVAL", key), &[], Values::Int(&[con.clip.min_interval.as_millis() as i64]));

    // Tooltips (as a map)
    let tip_pairs: Vec<(String, String)> = con.tooltips.iter().map(|(k, (gen, v))| (k.clone(), format!("{}:{}", gen, v))).collect();
    if !tip_pairs.is_empty() {
        // Create a vec of references for the map call
        let tip_refs: Vec<(&String, &String)> = tip_pairs.iter().map(|(k, v)| (k, v)).collect();
        w.map(&format!("{}_TOOLTIPS", key), tip_refs.into_iter());
    }

    // Src files (as array)
    if !con.src_files.is_empty() {
        let src_files: Vec<String> = con.src_files.iter().cloned().collect();
        w.var(&format!("{}_SRC_FILES", key), &[src_files.len()], Values::Str(&src_files));
    }

    // Current line segments
    if !con.cur.is_empty() {
        // Serialize cur segments as a string
        let cur_str = serialize_segments(&con.cur);
        w.var(&format!("{}_CUR", key), &[], Values::Str(&[cur_str]));
    }

    // Lines (scrollback)
    if !con.lines.is_empty() {
        // We'll serialize each line as a separate entry
        for (i, line) in con.lines.iter().enumerate() {
            let line_key = format!("{}_LINE_{}", key, i);
            let segs_str = serialize_segments(&line.segs);
            w.var(&format!("{}_SEGS", line_key), &[], Values::Str(&[segs_str]));
            w.var(&format!("{}_ALIGN", line_key), &[], Values::Int(&[line.align as i64]));
            w.var(&format!("{}_GEN", line_key), &[], Values::Int(&[line.gen as i64]));
            w.var(&format!("{}_TEMPORARY", line_key), &[], Values::Int(&[line.temporary as i64]));
            w.var(&format!("{}_COLS", line_key), &[], Values::Int(&[line.cols as i64]));
            w.var(&format!("{}_KOJO", line_key), &[], Values::Int(&[line.kojo as i64]));
            w.var(&format!("{}_SRC", line_key), &[], Values::Int(&[line.src as i64]));
            w.var(&format!("{}_WAIT", line_key), &[], Values::Int(&[line.wait as i64]));
        }
    }

    Ok(())
}

fn serialize_segments(segs: &[Seg]) -> String {
    segs.iter()
        .map(|s| match s {
            Seg::Text { text, style, button, auto } => {
                let btn = button.as_ref().map(|v| v.to_string()).unwrap_or_default();
                format!(
                    "T:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
                    escape(text),
                    style.fg.0,
                    style.fg.1,
                    style.fg.2,
                    style.bg.map_or(-1, |c| c.0 as i64),
                    style.bg.map_or(-1, |c| c.1 as i64),
                    style.bg.map_or(-1, |c| c.2 as i64),
                    style.bold as i64,
                    style.italic as i64,
                    style.underline as i64,
                    style.strike as i64,
                    btn,
                    *auto as i64
                )
            }
            Seg::Image { name, w, h, ypos, button, hover } => {
                let btn = button.as_ref().map(|v| v.to_string()).unwrap_or_default();
                let hov = hover.as_ref().cloned().unwrap_or_default();
                format!("I:{}:{}:{}:{}:{}:{}", escape(name), w, h, ypos, btn, escape(&hov))
            }
            Seg::Space { w } => {
                format!("S:{}", w)
            }
            Seg::Rect { w, h, color, button } => {
                let btn = button.as_ref().map(|v| v.to_string()).unwrap_or_default();
                format!("R:{}:{}:{}:{}:{}:{}", w, h, color.0, color.1, color.2, btn)
            }
            Seg::Div { dx, dy, cols, rows, lines, html, deco } => {
                let rows_str = rows.map_or(-1, |r| r as i64);
                let lines_str = lines.iter().map(|l| serialize_segments(&l.segs)).collect::<Vec<_>>().join("|");
                let border_str = deco.border.iter().map(|b| (*b as i64).to_string()).collect::<Vec<_>>().join(",");
                let pad_str = deco.pad.iter().map(|p| (*p as i64).to_string()).collect::<Vec<_>>().join(",");
                let deco_str = format!(
                    "{}:{}:{}:{}:{}:{}:{}:{}",
                    deco.bg.map_or(-1, |c| c.0 as i64),
                    deco.bg.map_or(-1, |c| c.1 as i64),
                    deco.bg.map_or(-1, |c| c.2 as i64),
                    deco.bcolor.map_or(-1, |c| c.0 as i64),
                    deco.bcolor.map_or(-1, |c| c.1 as i64),
                    deco.bcolor.map_or(-1, |c| c.2 as i64),
                    border_str,
                    pad_str,
                );
                format!("D:{}:{}:{}:{}:{}:{}:{}", dx, dy, cols, rows_str, escape(html), lines_str, deco_str)
            }
        })
        .collect::<Vec<_>>()
        .join(";")
}

fn escape(s: &str) -> String {
    s.replace(':', "\\:").replace(';', "\\;").replace('|', "\\|").replace(',', "\\,")
}

#[allow(dead_code)]
fn restore_console(_vm: &mut Vm, _r: &mut Reader) -> Result<(), String> {
    // This function is no longer used - console restoration is handled inline in restore_state
    Ok(())
}

struct LineData {
    idx: usize,
    line: Line,
    has_segs: bool,
}

impl LineData {
    fn new() -> Self {
        use crate::ast::Align;
        LineData {
            idx: 0,
            line: Line { segs: Vec::new(), align: Align::Left, gen: 0, temporary: false, cols: 80, kojo: false, src: 0, wait: 0 },
            has_segs: false,
        }
    }
}

fn process_console_entry(con: &mut Console, name: &str, entry: &Entry, lines_data: &mut Vec<LineData>) -> Result<(), String> {
    use crate::ast::Align;
    use crate::console::Rgb;

    let key_prefix = "__CONSOLE__";
    let rest = &name[key_prefix.len()..];

    // Extract values from Entry::Var variant
    let (ints, strs, is_str) = match entry {
        crate::vm::erasav::Entry::Var { ints, strs, is_str, .. } => (ints, strs, *is_str),
        crate::vm::erasav::Entry::Map { pairs, .. } => {
            // Handle MAP entries (like TOOLTIPS)
            if rest == "_TOOLTIPS" {
                for (k, v) in pairs {
                    if let Some(colon) = v.find(':') {
                        let gen: u64 = v[..colon].parse().unwrap_or(0);
                        let text = v[colon + 1..].to_string();
                        con.tooltips.insert(k.clone(), (gen, text));
                    }
                }
            }
            return Ok(());
        }
        _ => return Ok(()), // Only Var/Map entries have scalar values we need
    };

    // Helper to get first int value
    let get_int = || ints.first().map(|(_, v)| *v).unwrap_or(0);
    // Helper to get first str value
    let get_str = || strs.first().map(|(_, v)| v.clone()).unwrap_or_default();

    // Handle line entries: __CONSOLE__LINE_0_SEGS, __CONSOLE__LINE_0_ALIGN, etc.
    if rest.starts_with("LINE_") {
        let parts: Vec<&str> = rest.split('_').collect();
        if parts.len() >= 3 {
            let idx: usize = parts[1].parse().map_err(|_| "invalid line index")?;
            let field = parts[2];

            // Ensure we have space for this line
            while lines_data.len() <= idx {
                lines_data.push(LineData::new());
            }
            let line_data = &mut lines_data[idx];
            line_data.idx = idx;

            match field {
                "SEGS" => {
                    if is_str {
                        if let Some((_, s)) = strs.first() {
                            line_data.line.segs = deserialize_segments(s)?;
                            line_data.has_segs = true;
                        }
                    }
                }
                "ALIGN" => {
                    if !is_str {
                        if let Some((_, v)) = ints.first() {
                            line_data.line.align = match *v {
                                0 => Align::Left,
                                1 => Align::Center,
                                2 => Align::Right,
                                _ => Align::Left,
                            };
                        }
                    }
                }
                "GEN" => {
                    if !is_str {
                        if let Some((_, v)) = ints.first() {
                            line_data.line.gen = *v as u64;
                        }
                    }
                }
                "TEMPORARY" => {
                    if !is_str {
                        if let Some((_, v)) = ints.first() {
                            line_data.line.temporary = *v != 0;
                        }
                    }
                }
                "COLS" => {
                    if !is_str {
                        if let Some((_, v)) = ints.first() {
                            line_data.line.cols = *v as usize;
                        }
                    }
                }
                "KOJO" => {
                    if !is_str {
                        if let Some((_, v)) = ints.first() {
                            line_data.line.kojo = *v != 0;
                        }
                    }
                }
                "SRC" => {
                    if !is_str {
                        if let Some((_, v)) = ints.first() {
                            line_data.line.src = *v as u64;
                        }
                    }
                }
                "WAIT" => {
                    if !is_str {
                        if let Some((_, v)) = ints.first() {
                            line_data.line.wait = *v as u32;
                        }
                    }
                }
                _ => {}
            }
        }
        return Ok(());
    }

    // Handle scalar fields
    let field = rest;

    match field {
        "_WIDTH" => con.width = get_int() as usize,
        "_GEN" => con.gen = get_int() as u64,
        "_VALID_GEN" => con.valid_gen = get_int() as u64,
        "_WAITS" => con.waits = get_int() as u32,
        "_RUNNING" => con.running = get_str(),
        "_ALIGN" => {
            con.align = match get_int() {
                0 => Align::Left,
                1 => Align::Center,
                2 => Align::Right,
                _ => Align::Left,
            }
        }
        "_CUR_ALIGN" => {
            let v = get_int();
            con.cur_align = if v >= 0 {
                Some(match v {
                    0 => Align::Left,
                    1 => Align::Center,
                    2 => Align::Right,
                    _ => Align::Left,
                })
            } else {
                None
            };
        }
        "_STYLE_FG" => {
            if !is_str {
                if let Some((_, _v)) = ints.first() {
                    // We need to read 3 ints - they might be in the ints vec
                    if ints.len() >= 3 {
                        con.style.fg = Rgb(ints[0].1 as u8, ints[1].1 as u8, ints[2].1 as u8);
                    }
                }
            }
        }
        "_STYLE_BG" => {
            if !is_str {
                if ints.len() >= 3 {
                    let r = ints[0].1;
                    if r >= 0 {
                        con.style.bg = Some(Rgb(ints[0].1 as u8, ints[1].1 as u8, ints[2].1 as u8));
                    } else {
                        con.style.bg = None;
                    }
                }
            }
        }
        "_STYLE_BOLD" => con.style.bold = get_int() != 0,
        "_STYLE_ITALIC" => con.style.italic = get_int() != 0,
        "_STYLE_UNDERLINE" => con.style.underline = get_int() != 0,
        "_STYLE_STRIKE" => con.style.strike = get_int() != 0,
        "_DEFAULT_FG" => {
            if !is_str && ints.len() >= 3 {
                con.default_fg = Rgb(ints[0].1 as u8, ints[1].1 as u8, ints[2].1 as u8);
            }
        }
        "_DEFAULT_BG" => {
            if !is_str && ints.len() >= 3 {
                con.default_bg = Rgb(ints[0].1 as u8, ints[1].1 as u8, ints[2].1 as u8);
            }
        }
        "_SCREEN_BG" => {
            if !is_str && ints.len() >= 3 {
                con.screen_bg = Rgb(ints[0].1 as u8, ints[1].1 as u8, ints[2].1 as u8);
            }
        }
        "_FOCUS_FG" => {
            if !is_str && ints.len() >= 3 {
                con.focus_fg = Rgb(ints[0].1 as u8, ints[1].1 as u8, ints[2].1 as u8);
            }
        }
        "_LOG_FG" => {
            if !is_str && ints.len() >= 3 {
                con.log_fg = Rgb(ints[0].1 as u8, ints[1].1 as u8, ints[2].1 as u8);
            }
        }
        "_TIP_COLORS" => {
            if !is_str && ints.len() >= 6 {
                let fg = Rgb(ints[0].1 as u8, ints[1].1 as u8, ints[2].1 as u8);
                let bg = Rgb(ints[3].1 as u8, ints[4].1 as u8, ints[5].1 as u8);
                con.tip_colors = Some((fg, bg));
            }
        }
        "_TIP_DELAY" => con.tip_delay = get_int() as u64,
        "_TIP_DURATION" => con.tip_duration = get_int() as u64,
        "_FONT_SIZE" => con.font_size = get_int() as f32 / 1000.0,
        "_LINE_HEIGHT" => con.line_height = get_int() as f32 / 1000.0,
        "_PRINTC_LEN" => con.printc_len = get_int() as usize,
        "_PRINTC_PER_LINE" => con.printc_per_line = get_int() as usize,
        "_BUTTON_WRAP" => con.button_wrap = get_int() != 0,
        "_KEY_MACRO" => con.key_macro = get_int() != 0,
        "_IN_KOJO" => con.in_kojo = get_int() != 0,
        "_SOUND_ON" => con.sound_on = get_int() != 0,
        "_NOTE_WATCH" => con.note_watch = get_int() != 0,
        "_NOTE_LINE" => {
            let s = get_str();
            con.note_line = if s.is_empty() { None } else { Some(s) };
        }
        "_TRACK_SRC" => con.track_src = get_int() != 0,
        "_SRC" => {
            if !is_str && ints.len() >= 2 {
                con.src = (ints[0].1 as u32, ints[1].1 as u32);
            }
        }
        "_LAST_TEMP" => con.last_temp = get_int() != 0,
        "_DRAWLINE_STR" => con.drawline_str = get_str(),
        "_REDRAW" => con.redraw = get_int() != 0,
        "_SKIP_DISP" => con.skip_disp = get_int() != 0,
        "_SKIP_USER" => con.skip_user = get_int() != 0,
        "_FONT_NAME" => con.font_name = get_str(),
        "_DEFAULT_FONT" => con.default_font = get_str(),
        "_TITLE" => con.title = get_str(),
        "_INFINITE_LOOP_ALERT" => con.infinite_loop_alert_time = get_int() as u64,
        "_MAX_LINES" => con.max_lines = get_int() as usize,
        "_DROPPED_FRONT" => con.dropped_front = get_int() as usize,
        "_LOW_WATER" => con.low_water = get_int() as usize,
        "_DIRTY" => con.dirty = get_int() != 0,
        "_CLIP_ENABLED" => con.clip.enabled = get_int() != 0,
        "_CLIP_IGNORE_TAGS" => con.clip.ignore_tags = get_int() != 0,
        "_CLIP_REPLACE_TAGS" => con.clip.replace_tags = get_str(),
        "_CLIP_NEW_LINES_ONLY" => con.clip.new_lines_only = get_int() != 0,
        "_CLIP_CLEAR_ON_REFRESH" => con.clip.clear_on_refresh = get_int() != 0,
        "_CLIP_LEFT_CLICK" => con.clip.left_click = get_int() != 0,
        "_CLIP_MIDDLE_CLICK" => con.clip.middle_click = get_int() != 0,
        "_CLIP_DOUBLE_CLICK" => con.clip.double_click = get_int() != 0,
        "_CLIP_ON_WAIT" => con.clip.on_wait = get_int() != 0,
        "_CLIP_ON_INPUT" => con.clip.on_input = get_int() != 0,
        "_CLIP_MAX_LINES" => con.clip.max_lines = get_int() as usize,
        "_CLIP_BUFFER" => con.clip.buffer = get_int() as usize,
        "_CLIP_MIN_INTERVAL" => con.clip.min_interval = std::time::Duration::from_millis(get_int() as u64),
        "_SRC_FILES" => {
            if is_str {
                con.src_files = strs.iter().map(|(_, v)| v.clone()).collect();
            }
        }
        "_CUR" => {
            if is_str {
                if let Some((_, s)) = strs.first() {
                    con.cur = deserialize_segments(s)?;
                }
            }
        }
        _ => {}
    }

    Ok(())
}

fn deserialize_segments(s: &str) -> Result<Vec<Seg>, String> {
    use crate::console::{DivBox, Line, Rgb, Seg, Style};
    use crate::vm::value::Value;

    if s.is_empty() {
        return Ok(Vec::new());
    }

    let mut segs = Vec::new();
    for seg_str in s.split(';') {
        if seg_str.is_empty() {
            continue;
        }
        let parts: Vec<&str> = seg_str.split(':').collect();
        if parts.is_empty() {
            continue;
        }

        match parts[0] {
            "T" => {
                // T:escaped_text:fg_r:fg_g:fg_b:bg_r:bg_g:bg_b:bold:italic:underline:strike:button:auto
                if parts.len() >= 13 {
                    let text = unescape(parts[1]);
                    let fg_r = parts[2].parse().unwrap_or(0) as u8;
                    let fg_g = parts[3].parse().unwrap_or(0) as u8;
                    let fg_b = parts[4].parse().unwrap_or(0) as u8;
                    let bg_r = parts[5].parse().unwrap_or(-1);
                    let bg_g = parts[6].parse().unwrap_or(-1);
                    let bg_b = parts[7].parse().unwrap_or(-1);
                    let bold = parts[8].parse().unwrap_or(0) != 0;
                    let italic = parts[9].parse().unwrap_or(0) != 0;
                    let underline = parts[10].parse().unwrap_or(0) != 0;
                    let strike = parts[11].parse().unwrap_or(0) != 0;
                    let button = if parts[12].is_empty() { None } else { Some(Value::Str(parts[12].to_string())) };
                    let auto = parts[13].parse().unwrap_or(0) != 0;

                    let bg = if bg_r >= 0 { Some(Rgb(bg_r as u8, bg_g as u8, bg_b as u8)) } else { None };
                    let style = Style { fg: Rgb(fg_r, fg_g, fg_b), bg, bold, italic, underline, strike };

                    segs.push(Seg::Text { text, style, button, auto });
                }
            }
            "I" => {
                // I:escaped_name:w:h:ypos:button:hover
                if parts.len() >= 6 {
                    let name = unescape(parts[1]);
                    let w = parts[2].parse().unwrap_or(0.0);
                    let h = parts[3].parse().unwrap_or(0.0);
                    let ypos = parts[4].parse().unwrap_or(0.0);
                    let button = if parts[5].is_empty() { None } else { Some(Value::Str(parts[5].to_string())) };
                    let hover = if parts.len() > 6 { Some(unescape(parts[6])) } else { None };

                    segs.push(Seg::Image { name, w, h, ypos, button, hover });
                }
            }
            "S" => {
                // S:w
                if parts.len() >= 2 {
                    let w = parts[1].parse().unwrap_or(0.0);
                    segs.push(Seg::Space { w });
                }
            }
            "R" => {
                // R:w:h:r:g:b:button
                if parts.len() >= 6 {
                    let w = parts[1].parse().unwrap_or(0.0);
                    let h = parts[2].parse().unwrap_or(0.0);
                    let color = Rgb(parts[3].parse().unwrap_or(0) as u8, parts[4].parse().unwrap_or(0) as u8, parts[5].parse().unwrap_or(0) as u8);
                    let button = if parts.len() > 6 && !parts[6].is_empty() { Some(Value::Str(parts[6].to_string())) } else { None };

                    segs.push(Seg::Rect { w, h, color, button });
                }
            }
            "D" => {
                // D:dx:dy:cols:rows:escaped_html:lines_str:deco_str
                if parts.len() >= 7 {
                    let dx = parts[1].parse().unwrap_or(0);
                    let dy = parts[2].parse().unwrap_or(0);
                    let cols = parts[3].parse().unwrap_or(0);
                    let rows = if parts[4] == "-1" { None } else { Some(parts[4].parse().unwrap_or(0)) };
                    let html = unescape(parts[5]);

                    // Parse lines_str
                    let mut lines = Vec::new();
                    let lines_str = if parts.len() > 6 { parts[6] } else { "" };
                    if !lines_str.is_empty() {
                        for line_str in lines_str.split('|') {
                            if !line_str.is_empty() {
                                let line_segs = deserialize_segments(line_str)?;
                                lines.push(Line { segs: line_segs, align: Align::Left, gen: 0, temporary: false, cols, kojo: false, src: 0, wait: 0 });
                            }
                        }
                    }

                    // Parse deco_str (8 parts)
                    let deco = if parts.len() > 7 {
                        let deco_parts: Vec<&str> = parts[7].split(':').collect();
                        if deco_parts.len() >= 8 {
                            let bg = if deco_parts[0].parse::<i64>().unwrap_or(-1) >= 0 {
                                Some(Rgb(
                                    deco_parts[0].parse().unwrap_or(0) as u8,
                                    deco_parts[1].parse().unwrap_or(0) as u8,
                                    deco_parts[2].parse().unwrap_or(0) as u8,
                                ))
                            } else {
                                None
                            };
                            let bcolor = if deco_parts[3].parse::<i64>().unwrap_or(-1) >= 0 {
                                Some(Rgb(
                                    deco_parts[3].parse().unwrap_or(0) as u8,
                                    deco_parts[4].parse().unwrap_or(0) as u8,
                                    deco_parts[5].parse().unwrap_or(0) as u8,
                                ))
                            } else {
                                None
                            };
                            let border = deco_parts[6].split(',').filter_map(|s| s.parse::<i64>().ok()).map(|b| b != 0).collect::<Vec<_>>();
                            let mut border_arr = [false; 4];
                            for (i, b) in border.iter().enumerate().take(4) {
                                border_arr[i] = *b;
                            }
                            let pad = deco_parts[7].split(',').filter_map(|s| s.parse::<usize>().ok()).collect::<Vec<_>>();
                            let mut pad_arr = [0usize; 4];
                            for (i, p) in pad.iter().enumerate().take(4) {
                                pad_arr[i] = *p;
                            }

                            DivBox { bg, bcolor, border: border_arr, pad: pad_arr, round: [false; 4] }
                        } else {
                            DivBox::default()
                        }
                    } else {
                        DivBox::default()
                    };

                    segs.push(Seg::Div { dx, dy, cols, rows, lines, html, deco: Box::new(deco) });
                }
            }
            _ => {}
        }
    }

    Ok(segs)
}

fn unescape(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                match next {
                    ':' | ';' | '|' | ',' | '\\' => {
                        chars.next();
                        result.push(next);
                    }
                    _ => result.push(c),
                }
            } else {
                result.push(c);
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn write_arr(w: &mut Writer, key: &str, arr: &Arr) -> Result<(), String> {
    let dims: Vec<usize> = if arr.dims.is_empty() { vec![1] } else { arr.dims.clone() };
    match &arr.data {
        Data::I(v) => w.var(key, &dims, Values::Int(v)),
        Data::S(v) => w.var(key, &dims, Values::Str(v)),
    }
    Ok(())
}

fn write_loc(w: &mut Writer, key: &str, loc: &Loc) -> Result<(), String> {
    let data = match loc {
        Loc::Global(idx) => format!("GLOBAL:{}", idx),
        Loc::Frame(name) => format!("FRAME:{}", name),
        Loc::FrameAt(frame_idx, name) => format!("FRAME_AT:{}:{}", frame_idx, name),
        Loc::Static(idx) => format!("STATIC:{}", idx),
        Loc::Chara(chara_idx, var_idx) => format!("CHARA:{}:{}", chara_idx, var_idx),
        Loc::Ro(val) => format!("RO:{}", val),
    };
    w.var(key, &[], Values::Str(&[data]));
    Ok(())
}

fn parse_loc(s: &str) -> Result<Loc, String> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.is_empty() {
        return Err("empty loc string".into());
    }
    match parts[0] {
        "GLOBAL" => {
            if parts.len() >= 2 {
                let idx = parts[1].parse().map_err(|_| "bad GLOBAL idx")?;
                Ok(Loc::Global(idx))
            } else {
                Err("GLOBAL missing idx".into())
            }
        }
        "FRAME" => {
            if parts.len() >= 2 {
                Ok(Loc::Frame(parts[1].into()))
            } else {
                Err("FRAME missing name".into())
            }
        }
        "FRAME_AT" => {
            if parts.len() >= 3 {
                let frame_idx = parts[1].parse().map_err(|_| "bad FRAME_AT frame_idx")?;
                Ok(Loc::FrameAt(frame_idx, parts[2].into()))
            } else {
                Err("FRAME_AT missing parts".into())
            }
        }
        "STATIC" => {
            if parts.len() >= 2 {
                let idx = parts[1].parse().map_err(|_| "bad STATIC idx")?;
                Ok(Loc::Static(idx))
            } else {
                Err("STATIC missing idx".into())
            }
        }
        "CHARA" => {
            if parts.len() >= 3 {
                let chara_idx = parts[1].parse().map_err(|_| "bad CHARA chara_idx")?;
                let var_idx = parts[2].parse().map_err(|_| "bad CHARA var_idx")?;
                Ok(Loc::Chara(chara_idx, var_idx))
            } else {
                Err("CHARA missing parts".into())
            }
        }
        "RO" => {
            if parts.len() >= 2 {
                let val_str = parts[1];
                // Try to parse as integer first, otherwise treat as string
                let val = if let Ok(i) = val_str.parse::<i64>() { Value::Int(i) } else { Value::Str(val_str.to_string()) };
                Ok(Loc::Ro(val))
            } else {
                Err("RO missing val".into())
            }
        }
        _ => Err(format!("unknown loc type: {}", parts[0])),
    }
}

// --- Reading functions ---

fn restore_global(vm: &mut Vm, name: &str, dims: &[usize], ints: &[(usize, i64)], strs: &[(usize, String)], is_str: bool) -> Result<(), String> {
    let arr = vm.global_arr(name);
    fill_arr(arr, dims, ints, strs, is_str);
    Ok(())
}

fn restore_static(
    vm: &mut Vm,
    func_name: &str,
    var_name: &str,
    dims: &[usize],
    ints: &[(usize, i64)],
    strs: &[(usize, String)],
    is_str: bool,
    captured_statics: &mut Vec<Option<Arr>>,
    static_index: &mut HashMap<(String, String), usize>,
) -> Result<(), String> {
    // Find or create the static slot
    let key = (func_name.to_string(), var_name.to_string());
    let idx = *static_index.entry(key.clone()).or_insert_with(|| {
        let idx = captured_statics.len();
        captured_statics.push(None);
        idx
    });

    let mut arr = Arr::new(is_str, dims);
    fill_arr(&mut arr, dims, ints, strs, is_str);
    if captured_statics.len() <= idx {
        captured_statics.resize_with(idx + 1, || None);
    }
    captured_statics[idx] = Some(arr);
    Ok(())
}

fn restore_chara_var(
    vm: &mut Vm,
    chara_idx: usize,
    var_name: &str,
    dims: &[usize],
    ints: &[(usize, i64)],
    strs: &[(usize, String)],
    is_str: bool,
) -> Result<(), String> {
    if chara_idx >= vm.charas.len() {
        vm.charas.resize_with(chara_idx + 1, || Chara { names_unset: 15, ..Chara::default() });
    }
    let _var_id = vm.chara_names.id(var_name);
    let arr = vm.chara_arr(chara_idx, var_name);
    fill_arr(arr, dims, ints, strs, is_str);
    Ok(())
}

fn restore_chara_names(vm: &mut Vm, pairs: &[(String, String)]) -> Result<(), String> {
    for (k, v) in pairs {
        if let Ok(idx) = k.parse::<u32>() {
            // The pairs are idx -> name
            // We need to ensure the names match
            if vm.chara_names.names.len() <= idx as usize {
                vm.chara_names.names.resize(idx as usize + 1, Name::from(""));
            }
            vm.chara_names.names[idx as usize] = v.clone().into();
            vm.chara_names.ids.insert(v.clone(), idx);
        }
    }
    Ok(())
}

#[allow(dead_code)]
fn restore_frames(_vm: &mut Vm, _r: &mut Reader) -> Result<(), String> {
    // This function is no longer used - frame restoration is handled inline in restore_state
    Ok(())
}

fn fill_arr(arr: &mut Arr, dims: &[usize], ints: &[(usize, i64)], strs: &[(usize, String)], is_str: bool) {
    if is_str != arr.is_str() {
        return;
    }
    let ours: Vec<usize> = if arr.dims.is_empty() { vec![1] } else { arr.dims.clone() };
    let saved: Vec<usize> = if dims.is_empty() { vec![1] } else { dims.to_vec() };
    if ours.len() != saved.len() {
        return;
    }
    arr.clear();
    let map = |f: usize| -> Option<usize> {
        let mut rest = f;
        let mut coords = vec![0usize; saved.len()];
        for k in (0..saved.len()).rev() {
            let n = saved[k].max(1);
            coords[k] = rest % n;
            rest /= n;
        }
        let mut out = 0usize;
        for k in 0..ours.len() {
            if coords[k] >= ours[k] {
                return None;
            }
            out = out * ours[k] + coords[k];
        }
        Some(out)
    };
    if is_str {
        for (f, v) in strs {
            if let Some(i) = map(*f) {
                arr.set(i, Value::Str(v.clone()));
            }
        }
    } else {
        for (f, v) in ints {
            if let Some(i) = map(*f) {
                arr.set(i, Value::Int(*v));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_state_header() {
        assert_eq!(STATE_HEADER, 0x0A1A_0A0D_5354_4154);
    }
}
