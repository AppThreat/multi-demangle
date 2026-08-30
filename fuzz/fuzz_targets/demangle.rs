#![no_main]
//! Full-pipeline target: `demangle(bytes)` with every backend compiled in.
//!
//! Properties (Plan 05 §1): no panic, no abort, no OOM, and the output stays
//! within a sane bound for any input. Note the honest scope: these are safety
//! and liveness properties only — every bug this crate has actually shipped
//! was silent wrong output, which this target cannot see (that is the
//! differential generator's job, see `contrib/scripts/gen_dlang_symbols.py`).

use libfuzzer_sys::fuzz_target;
use multi_demangle::{Demangle, DemangleOptions};
use symbolic_common::Name;

/// Output bound. The crate's documented caps are 4 KiB for the C++ path
/// (`BoundedString`) and 1 MiB for the Swift FFI path (`SWIFT_BUFFER_MAX`,
/// needed by real nested-closure symbols), so 1 MiB is the largest output any
/// correct run can produce. An input that renders longer than that means a
/// substitution-expansion bound failed to hold.
const OUTPUT_BOUND: usize = 1024 * 1024;

fuzz_target!(|data: &[u8]| {
    // The public pipeline takes `&str`; inputs that are not UTF-8 are outside
    // every entry point's contract.
    let Ok(s) = std::str::from_utf8(data) else {
        return;
    };
    let name = Name::from(s);
    for opts in [DemangleOptions::complete(), DemangleOptions::name_only()] {
        if let Some(demangled) = name.demangle(opts) {
            assert!(
                demangled.len() <= OUTPUT_BOUND,
                "demangled output of {} bytes exceeds the {}-byte bound: {s:?} -> {demangled:?}",
                demangled.len(),
                OUTPUT_BOUND,
            );
        }
        let _ = name.try_demangle(opts);
    }
});
