//! Ada (GNAT) demangler integration tests.
//!
//! The shape of the cases follows the `ada-demangle` crate's test suite
//! (MIT licensed, by Pernosco), which mirrors GCC's `exp_dbug.ads` encoding.

use multi_demangle::{classify_symbol, detect_language, Demangle, DemangleOptions, SymbolStatus};
use similar_asserts::assert_eq;
use symbolic_common::{Language, Name};

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
        assert_eq!(
            Name::from(symbol)
                .demangle(DemangleOptions::complete())
                .as_deref(),
            Some(expected),
            "for {symbol}"
        );
    }
}

#[test]
fn test_ada_library_level_prefix() {
    assert_eq!(
        Name::from("_ada_main")
            .demangle(DemangleOptions::complete())
            .as_deref(),
        Some("main")
    );
}

#[test]
fn test_ada_operators() {
    for (symbol, expected) in [
        ("Oeq", "\"=\""),
        ("One", "\"/=\""),
        ("module__Oadd", "module.\"+\""),
        ("module__Oconcat", "module.\"&\""),
    ] {
        assert_eq!(
            Name::from(symbol)
                .demangle(DemangleOptions::complete())
                .as_deref(),
            Some(expected),
            "for {symbol}"
        );
    }
}

#[test]
fn test_ada_character_escapes() {
    // Uppercase and non-lowercase characters are hex-escaped.
    assert_eq!(
        Name::from("module__U41bc__proc")
            .demangle(DemangleOptions::complete())
            .as_deref(),
        Some("module.Abc.proc")
    );
    assert_eq!(
        Name::from("module__W0041bc__proc")
            .demangle(DemangleOptions::complete())
            .as_deref(),
        Some("module.Abc.proc")
    );
}

#[test]
fn test_ada_structured() {
    let info = Name::from("ada__exceptions__last_chance_handlerXn")
        .demangle_structured(DemangleOptions::complete())
        .unwrap();
    assert_eq!(info.namespace, ["ada", "exceptions"]);
    assert_eq!(info.name, "last_chance_handler");
    assert_eq!(info.kind, multi_demangle::DemangledKind::Function);
}

#[test]
fn test_ada_detection_and_classification() {
    assert_eq!(detect_language("ada__exceptions__raiseXn"), Some("ada"));
    assert_eq!(detect_language("_ada_main"), Some("ada"));
    assert_eq!(detect_language("Oeq"), Some("ada"));
    assert_eq!(detect_language("libc.so.6"), None);
    // The trait API has no Ada variant; such symbols report Unknown.
    assert_eq!(
        Name::from("ada__exceptions__raiseXn").detect_language(),
        Language::Unknown
    );
    assert!(matches!(
        classify_symbol("ada__exceptions__raiseXn"),
        SymbolStatus::Mangled(Language::Unknown)
    ));
}

#[test]
fn test_ada_rejects_non_ada() {
    for symbol in [
        "libc.so.6",
        "main",
        "__libc_start_main",
        "_ada_",
        "B53b",
        "Ounknown",
    ] {
        assert_eq!(
            Name::from(symbol).demangle(DemangleOptions::complete()),
            None,
            "for {symbol}"
        );
    }
    // C++ symbols are claimed by the C++ backend before Ada is considered;
    // the Ada backend itself rejects them.
    assert_eq!(
        multi_demangle::demangle_as("ada", "_ZN3foo3barEv", DemangleOptions::complete()),
        None
    );
}
