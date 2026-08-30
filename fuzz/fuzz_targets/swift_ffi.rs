#![no_main]
//! Swift FFI target — the only place in the crate where memory-unsafety is
//! possible: `demangle_as("swift", ...)` crosses into the vendored C++
//! demangler (`src/swiftdemangle.cpp`). Run this target under ASan and UBSan;
//! the Rust-only targets catch no C++ memory error even with sanitizers on.
//!
//! The full round-trip includes detection (the `is_swift_symbol` FFI) and both
//! option sets, mirroring what `demangle` and the CLI do with a Swift symbol.

use libfuzzer_sys::fuzz_target;
use multi_demangle::{demangle_as, detect_language, DemangleOptions};

fuzz_target!(|data: &[u8]| {
    // The FFI takes C strings; interior NULs are rejected by the public API
    // (`CString::new` fails), so they are outside the exercised contract.
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    if s.contains('\0') {
        return;
    }
    let _ = detect_language(s);
    for opts in [DemangleOptions::complete(), DemangleOptions::name_only()] {
        let _ = demangle_as("swift", s, opts);
    }
});
