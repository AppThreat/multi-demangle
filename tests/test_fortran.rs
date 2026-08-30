//! Fortran demangler integration tests.

use multi_demangle::{
    classify_symbol, demangle_as, detect_language, Demangle, DemangleOptions, SymbolStatus,
};
use similar_asserts::assert_eq;
use symbolic_common::{Language, Name};

#[test]
fn test_fortran_module_symbols() {
    for (symbol, expected) in [
        // gfortran with the macOS/ELF platform underscores.
        ("__my_module_MOD_my_proc", "my_module::my_proc"),
        ("m_MOD_foo", "m::foo"),
        // Some ABIs omit the leading underscores.
        ("my_module_MOD_my_proc", "my_module::my_proc"),
        // The procedure part is verbatim: gfortran appends no length or
        // disambiguation suffix, so names ending in digits keep them.
        // These are gfortran 12 output for the identically named procedures
        // in contrib/fixtures/fortran/corpus.f90.
        ("__numerics_MOD_interp_3", "numerics::interp_3"),
        ("__numerics_MOD_step_12", "numerics::step_12"),
        ("__numerics_MOD_solve_2d", "numerics::solve_2d"),
        // Intel ifort/ifx.
        ("my_module_mp_my_proc_", "my_module::my_proc"),
        ("my_module_mp_my_proc", "my_module::my_proc"),
    ] {
        let name = Name::from(symbol);
        assert_eq!(
            name.demangle(DemangleOptions::complete()),
            Some(expected.to_string()),
            "for {symbol}"
        );
    }
}

#[test]
fn test_fortran_options_have_no_effect() {
    // The mangling carries no type information, so all option combinations
    // render the same.
    let name = Name::from("__my_module_MOD_my_proc");
    for opts in [
        DemangleOptions::complete(),
        DemangleOptions::name_only(),
        DemangleOptions::complete().return_type(false),
        DemangleOptions::complete().parameters(false),
    ] {
        assert_eq!(name.demangle(opts), Some("my_module::my_proc".to_string()));
    }
}

#[test]
fn test_fortran_plain_form_is_explicit_only() {
    // The g77 form collides with any C symbol ending in `_`, so it is not
    // demangled through auto-detection...
    assert_eq!(
        Name::from("init_").demangle(DemangleOptions::complete()),
        None
    );
    assert_eq!(multi_demangle::demangle("init_"), "init_");
    // ...but is available through the explicit-request entry point.
    assert_eq!(
        demangle_as("fortran", "init_", DemangleOptions::complete()),
        Some("init".to_string())
    );
    assert_eq!(
        demangle_as("fortran", "my_sub__", DemangleOptions::complete()),
        Some("my_sub".to_string())
    );
    // gfortran appends a single underscore even to names that already
    // contain one, so this is the mangling of `subroutine two_words`.
    assert_eq!(
        demangle_as("fortran", "two_words_", DemangleOptions::complete()),
        Some("two_words".to_string())
    );
    // `init__` is not the g77 doubled form (the inner name carries no
    // underscore): it is a subprogram genuinely named `init_`.
    assert_eq!(
        demangle_as("fortran", "init__", DemangleOptions::complete()),
        Some("init_".to_string())
    );
    assert_eq!(
        demangle_as("fortran", "__", DemangleOptions::complete()),
        None
    );
}

#[test]
fn test_fortran_rejects_non_fortran() {
    for symbol in [
        "libc.so.6",
        "main",
        "__libc_start_main",
        "__imp_CreateFileW",
        "memcpy@plt",
        "foo_MOD_",
    ] {
        assert_eq!(
            Name::from(symbol).demangle(DemangleOptions::complete()),
            None,
            "for {symbol}"
        );
    }
}

#[test]
fn test_fortran_detection_and_classification() {
    assert_eq!(detect_language("__my_module_MOD_my_proc"), Some("fortran"));
    assert_eq!(detect_language("my_module_mp_my_proc_"), Some("fortran"));
    assert_eq!(detect_language("libc.so.6"), None);

    // The detection carries through to the trait-based API.
    let name = Name::from("__my_module_MOD_my_proc");
    assert_eq!(name.detect_language(), Language::Unknown);

    // Module symbols classify as mangled; ordinary C symbols stay unmangled.
    assert!(matches!(
        classify_symbol("__my_module_MOD_my_proc"),
        SymbolStatus::Mangled(Language::Unknown)
    ));
    assert_eq!(classify_symbol("init_"), SymbolStatus::Unmangled);
    // C++ and Rust symbols are claimed by their own backends first.
    assert_eq!(detect_language("_ZN3foo3barEv"), Some("cpp"));
}

#[test]
fn test_fortran_structured() {
    let info = Name::from("__my_module_MOD_my_proc")
        .demangle_structured(DemangleOptions::complete())
        .expect("structured");
    assert_eq!(info.namespace, ["my_module"]);
    assert_eq!(info.name, "my_proc");
    assert_eq!(info.kind, multi_demangle::DemangledKind::Function);
    assert_eq!(info.parameters, None);
    assert_eq!(info.display, "my_module::my_proc");
}
