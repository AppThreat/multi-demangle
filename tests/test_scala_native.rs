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
