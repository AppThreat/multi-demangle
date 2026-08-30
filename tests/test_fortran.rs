//! Fortran demangler integration tests.

use multi_demangle::{
    classify_symbol, demangle_as, detect_language, Demangle, DemangleOptions, SymbolStatus,
};
use similar_asserts::assert_eq;
use symbolic_common::Name;

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
        assert_eq!(
            demangle_as("fortran", symbol, DemangleOptions::complete()),
            Some(expected.to_string()),
            "for {symbol}"
        );
    }
}

#[test]
fn test_fortran_options_have_no_effect() {
    // The mangling carries no type information, so all option combinations
    // render the same.
    for opts in [
        DemangleOptions::complete(),
        DemangleOptions::name_only(),
        DemangleOptions::complete().return_type(false),
        DemangleOptions::complete().parameters(false),
    ] {
        assert_eq!(
            demangle_as("fortran", "__my_module_MOD_my_proc", opts),
            Some("my_module::my_proc".to_string())
        );
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
    // Rejected even when Fortran is named explicitly. (`foo_MOD_` is absent
    // on purpose: under an explicit request the g77 form accepts it as the
    // subprogram `foo_MOD`, which is the documented behavior.)
    for symbol in ["libc.so.6", "__imp_CreateFileW", "memcpy@plt"] {
        assert_eq!(
            demangle_as("fortran", symbol, DemangleOptions::complete()),
            None,
            "for {symbol}"
        );
    }
}

#[test]
fn test_fortran_is_not_auto_detected() {
    // `mod_MOD_proc` and `mod_mp_proc` are flat identifier shapes with no
    // reserved prefix; C uses `_mp_` (multi-precision, multi-party) far more
    // often than Fortran does, so claiming them by shape rewrote correct C
    // names into plausible-looking wrong ones — OpenSSL's multi-prime RSA
    // tables became `ossl_rsa::coeff_names`.
    for symbol in [
        "__my_module_MOD_my_proc",
        "my_module_mp_my_proc_",
        "ossl_rsa_mp_coeff_names",
        "libc.so.6",
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
    // Languages with a reserved prefix are unaffected.
    assert_eq!(detect_language("_ZN3foo3barEv"), Some("cpp"));
}

#[test]
fn test_fortran_structured_is_not_auto_detected() {
    // A structured view has no language parameter to request Fortran
    // through; `demangle_as` returns the scope and name in full.
    assert!(Name::from("__my_module_MOD_my_proc")
        .demangle_structured(DemangleOptions::complete())
        .is_none());
}
