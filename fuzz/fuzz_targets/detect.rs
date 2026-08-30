#![no_main]
//! Detection target: `detect_language(bytes)` must not panic and must
//! terminate (Plan 05 §1). Detection tries up to four backends per symbol —
//! a full demangling pass for GNU v2, CodeWarrior, and D — so this also
//! exercises every backend's entry guard with untrusted bytes. Termination is
//! not assertable in-process; libFuzzer's `-timeout` is the enforcement.
//!
//! `classify_symbol` runs the same detection plus the decoration walk, and
//! `looks_mangled` the cheap prefix over-approximation; all three are
//! documented as demangle-free paths.

use libfuzzer_sys::fuzz_target;
use multi_demangle::{classify_symbol, detect_language, looks_mangled};

fuzz_target!(|data: &[u8]| {
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    let _ = detect_language(s);
    let _ = looks_mangled(s);
    let _ = classify_symbol(s);
});
