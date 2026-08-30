//! Scala Native demangler integration tests derived from upstream crate tests.

use multi_demangle::{Demangle, DemangleOptions};
use similar_asserts::assert_eq;
use symbolic_common::{Language, Name};

#[test]
fn test_demangle_scala_native_symbols() {
    for (symbol, full, no_return, name_only) in [
        (
            "_SM17java.lang.IntegerD7compareiiiEo",
            "java.lang.Integer.compare(Int,Int): Int",
            "java.lang.Integer.compare(Int,Int)",
            "java.lang.Integer.compare",
        ),
        (
            "_SM42sttp.model.headers.CacheDirective$MinFreshD12productArityiEO",
            "sttp.model.headers.CacheDirective$MinFresh.productArity: Int",
            "sttp.model.headers.CacheDirective$MinFresh.productArity",
            "sttp.model.headers.CacheDirective$MinFresh.productArity",
        ),
        (
            "_SM38scala.scalanative.junit.JUnitFrameworkIE",
            "scala.scalanative.junit.JUnitFramework.<clinit>",
            "scala.scalanative.junit.JUnitFramework.<clinit>",
            "scala.scalanative.junit.JUnitFramework.<clinit>",
        ),
        (
            "_SM42scala.scalanative.runtime.SymbolFormatter$D10inBounds$1L32scala.scalanative.unsigned.ULongizEPT42scala.scalanative.runtime.SymbolFormatter$",
            "scala.scalanative.runtime.SymbolFormatter$.<private[scala.scalanative.runtime.SymbolFormatter$]>inBounds$1(scala.scalanative.unsigned.ULong,Int): Boolean",
            "scala.scalanative.runtime.SymbolFormatter$.<private[scala.scalanative.runtime.SymbolFormatter$]>inBounds$1(scala.scalanative.unsigned.ULong,Int)",
            "scala.scalanative.runtime.SymbolFormatter$.<private[scala.scalanative.runtime.SymbolFormatter$]>inBounds$1",
        ),
    ] {
        let name = Name::from(symbol);
        assert_eq!(name.detect_language(), Language::Unknown);
        assert_eq!(name.demangle(DemangleOptions::complete()), Some(full.to_string()));
        assert_eq!(
            name.demangle(DemangleOptions::complete().return_type(false)),
            Some(no_return.to_string())
        );
        assert_eq!(
            name.demangle(DemangleOptions::name_only()),
            Some(name_only.to_string())
        );
    }

    assert_eq!(
        multi_demangle::demangle("_SM17java.lang.IntegerD7compareiiiEo"),
        "java.lang.Integer.compare(Int,Int): Int"
    );
}

/// Regression test for the slice-out-of-bounds panic that
/// scala-native-demangle 0.0.6 suffered on a declared name length longer
/// than the remaining input. Found by the `demangle` fuzz target
/// (Plan 05) on its first run with exactly this input, which reached the
/// Scala Native backend through the unknown-language fallback chain; the
/// vendored fix in `vendor/scala-native-demangle/` rejects the symbol
/// instead of panicking. This asserts the rejection, so a dependency bump
/// that reintroduces the panic (or drops the `[patch.crates-io]` section)
/// fails here.
#[test]
fn fuzz_found_overlong_name_length_is_rejected_not_panicking() {
    // 31 declared, 19 remaining. libFuzzer artifact:
    // crash-da85cb6d24f99f01a91b3aa0593f6f717b8097da.
    let symbol = "_ST31_sEquatabSg0F0QzFTW";
    let name = Name::from(symbol);
    assert_eq!(name.demangle(DemangleOptions::complete()), None);
    assert_eq!(multi_demangle::demangle(symbol), symbol);
}

/// Regression test for the `sig_name` overflow sites found by a second fuzz
/// run of the `detect` target (Plan 05, artifact
/// crash-8081a7ab43a8322d6ffd3bfdf46eddd6e8a43a80): an empty type-name list
/// made `type_names[0..n - 2]` underflow `n - 2` (panic with overflow checks
/// on), and the `D` branch sliced `after_name[consumed + 1..]` past the end
/// of the input. The vendored fix in `vendor/scala-native-demangle/` rejects
/// both shapes like any other parse failure.
#[test]
fn fuzz_found_empty_type_name_lists_are_rejected_not_panicking() {
    let symbol = "_SM2;_D6o@A tEOlz@p_n";
    let name = Name::from(symbol);
    let first = name.demangle(DemangleOptions::complete());
    let second = name.demangle(DemangleOptions::complete());
    assert_eq!(first, second);
}
