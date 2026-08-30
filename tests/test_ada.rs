//! Ada (GNAT) demangler integration tests.
//!
//! The shape of the cases follows the `ada-demangle` crate's test suite
//! (MIT licensed, by Pernosco), which mirrors GCC's `exp_dbug.ads` encoding.

use multi_demangle::{
    classify_symbol, demangle_as, detect_language, Demangle, DemangleOptions, SymbolStatus,
};
use similar_asserts::assert_eq;
use symbolic_common::Name;

/// Ada is explicit-request-only (see `test_ada_is_not_auto_detected`), so
/// every demangling case below names the language.
fn dem(symbol: &str) -> Option<String> {
    demangle_as("ada", symbol, DemangleOptions::complete())
}

#[test]
fn test_ada_package_paths() {
    for (symbol, expected) in [
        (
            "ada__exceptions__exception_traces__last_chance_handlerXn",
            "ada.exceptions.exception_traces.last_chance_handler",
        ),
        // Body/overload number suffixes.
        ("module__pcontrolled__l2", "module.pcontrolled.l2"),
        ("module__square__2", "module.square"),
        // The `T` task suffix inside a component is stripped.
        (
            "ada_main__finalize_library__B_4__reraise_library_exception_if_any",
            "ada_main.finalize_library.reraise_library_exception_if_any",
        ),
    ] {
        assert_eq!(dem(symbol).as_deref(), Some(expected), "for {symbol}");
    }
}

#[test]
fn test_ada_library_level_prefix() {
    assert_eq!(dem("_ada_main").as_deref(), Some("main"));
}

#[test]
fn test_ada_operators() {
    for (symbol, expected) in [
        ("Oeq", "\"=\""),
        ("One", "\"/=\""),
        ("module__Oadd", "module.\"+\""),
        ("module__Oconcat", "module.\"&\""),
    ] {
        assert_eq!(dem(symbol).as_deref(), Some(expected), "for {symbol}");
    }
}

#[test]
fn test_ada_character_escapes() {
    // Uppercase and non-lowercase characters are hex-escaped.
    assert_eq!(
        dem("module__U41bc__proc").as_deref(),
        Some("module.Abc.proc")
    );
    assert_eq!(
        dem("module__W0041bc__proc").as_deref(),
        Some("module.Abc.proc")
    );
}

#[test]
fn test_ada_rejects_non_ada() {
    // Rejected even when Ada is named explicitly: these do not parse as GNAT
    // manglings at all.
    for symbol in ["libc.so.6", "_ada_", "B53b", "Ounknown", "_ZN3foo3barEv"] {
        assert_eq!(dem(symbol), None, "for {symbol}");
    }
}

#[test]
fn test_ada_is_not_auto_detected() {
    // GNAT's `pkg__sub` shape has no reserved prefix, so it also describes a
    // great many ordinary C symbols — and on Mach-O every symbol picks up a
    // leading underscore, which turns C's `ada_copy` into GNAT's spelling of
    // the library-level subprogram `Copy`. These are real symbols from
    // libuv, llhttp, hwloc, glibc's nptl, and the ada-url C library; each
    // used to be demangled into a plausible-looking, wrong Ada name.
    for symbol in [
        "_uv__io_close",
        "_llhttp__on_header_field_complete",
        "_hwloc___nolibxml_prepare_export_diff",
        "_thread_db___pthread_keys",
        "_thread_db_rtld_global__dl_stack_used",
        "_c_nio_llhttp__internal__run",
        "llhttp__debug",
        "_ada_copy",
        "_ada_get_hostname",
        // Genuine GNAT symbols are equally not auto-detected; the shape
        // simply is not evidence either way.
        "ada__exceptions__last_chance_handlerXn",
        "_ada_main",
    ] {
        assert_eq!(detect_language(symbol), None, "for {symbol}");
        assert_eq!(
            Name::from(symbol).demangle(DemangleOptions::complete()),
            None,
            "for {symbol}"
        );
        assert_eq!(
            classify_symbol(symbol),
            SymbolStatus::Unmangled,
            "for {symbol}"
        );
    }
}

#[test]
fn test_ada_demangles_on_explicit_request() {
    // Naming the language is what makes the Ada backend reachable.
    for (symbol, expected) in [
        (
            "ada__exceptions__last_chance_handlerXn",
            "ada.exceptions.last_chance_handler",
        ),
        ("_ada_main", "main"),
        ("corpus__compute", "corpus.compute"),
        ("corpus___elabb", "corpus'Elab_Body"),
        ("corpus__Oeq", "corpus.\"=\""),
    ] {
        assert_eq!(
            multi_demangle::demangle_as("ada", symbol, DemangleOptions::complete()),
            Some(expected.to_string()),
            "for {symbol}"
        );
    }
}
