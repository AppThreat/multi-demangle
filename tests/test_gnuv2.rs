//! GNU v2 demangler integration tests derived from upstream crate docs.

use multi_demangle::{Demangle, DemangleOptions};
use similar_asserts::assert_eq;
use symbolic_common::{Language, Name, NameMangling};

fn assert_demangle_variants(symbol: &str, full: &str, no_return: &str, name_only: &str) {
    let name = Name::new(symbol, NameMangling::Unknown, Language::Cpp);
    assert_eq!(
        name.demangle(DemangleOptions::complete()),
        Some(full.to_string())
    );
    assert_eq!(
        name.demangle(DemangleOptions::complete().return_type(false)),
        Some(no_return.to_string())
    );
    assert_eq!(
        name.demangle(DemangleOptions::name_only()),
        Some(name_only.to_string())
    );
}

#[test]
fn test_demangle_gnuv2_symbols() {
    for (symbol, full, no_return, name_only) in [
        (
            "do_thing__C6StupidRC6StupidT1",
            "Stupid::do_thing(Stupid const &, Stupid const &) const",
            "Stupid::do_thing(Stupid const &, Stupid const &) const",
            "Stupid::do_thing",
        ),
        (
            "_$_5tName",
            "tName::~tName(void)",
            "tName::~tName(void)",
            "tName::~tName",
        ),
        (
            "a_function__Q35silly8my_thing17another_namespacefffi",
            "silly::my_thing::another_namespace::a_function(float, float, float, int)",
            "silly::my_thing::another_namespace::a_function(float, float, float, int)",
            "silly::my_thing::another_namespace::a_function",
        ),
        (
            "Printf__7ConsolePce",
            "Console::Printf(char *, ...)",
            "Console::Printf(char *, ...)",
            "Console::Printf",
        ),
        (
            "_GLOBAL_$F$__default_terminate",
            "global frames keyed to __default_terminate",
            "__default_terminate",
            "__default_terminate",
        ),
    ] {
        assert_demangle_variants(symbol, full, no_return, name_only);
    }
}

/// Regression test for the unbounded argument-list expansion that
/// gnuv2_demangle 0.4.0 suffered on crafted `N<count><index>` repeat
/// arguments: the count went into the vec push verbatim, so it grew until the
/// allocator gave up (libFuzzer intercepted a single 3 GiB reallocation), and
/// its argument loop also had no zero-progress check. Found by the `detect`
/// fuzz target (Plan 05) with exactly this input, reached through
/// `detect_language`, which attempts a full GNU v2 pass on every symbol the
/// C++ prefix predicates decline; the vendored fix in
/// `vendor/gnuv2_demangle/` rejects the symbol instead. A dependency bump
/// that reintroduces either behavior fails here.
#[test]
fn fuzz_found_unbounded_repeat_expansion_is_rejected_not_looping() {
    use multi_demangle::{Demangle, DemangleOptions};
    use symbolic_common::Name;

    let symbol = "00000L<e0000000e00__2cZN1255555555555555_00]0eeeee0Nf`>_Z__0v0";
    let name = Name::from(symbol);
    assert_eq!(name.detect_language(), Language::Unknown);
    assert_eq!(name.demangle(DemangleOptions::complete()), None);

    // And a few near-misses around the same shape, so the guard is not
    // pinned to one byte sequence.
    for probe in [
        "_ZN1255555555555555_00",
        "__2cZN1255555555555555_00]0e",
        "foo__ZNe00e0eee0Nf",
    ] {
        let _ = Name::from(probe).demangle(DemangleOptions::complete());
    }
}

/// The array-length fixup overflowed on a declared length of
/// `u64::MAX`; found by the swift_ffi fuzz target under ASan (Plan 05,
/// artifact crash-f17ecdd534c3bfe1e6d4b30726db88ece630d800), reached through
/// `detect_language`. The vendored fix rejects the overflow instead.
#[test]
fn fuzz_found_array_length_overflow_is_rejected() {
    let symbol = "_SMP__FA0000000000000000000000000000000000018446744073709551615_FA0000000000000000000000000012_AFA";
    let name = Name::from(symbol);
    assert_eq!(name.detect_language(), Language::Unknown);
    assert_eq!(name.demangle(DemangleOptions::complete()), None);
}
