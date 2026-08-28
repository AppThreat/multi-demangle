//! Symbol hygiene tests: mangling detection, classification, normalization.

use multi_demangle::{
    classify_symbol, detect_language, looks_mangled, normalize_symbol, Decoration, Demangle,
    DemangleOptions, Normalizer, SymbolStatus,
};
use symbolic_common::{Language, Name};

use similar_asserts::assert_eq;

#[test]
fn test_looks_mangled() {
    // C++ Itanium, with and without platform underscore prefixes.
    assert!(looks_mangled("_Z1hic"));
    assert!(looks_mangled("__Z1hic"));
    // MSVC.
    assert!(looks_mangled("?h@@YAXH@Z"));
    // Rust legacy (incl. macOS double underscore) and v0.
    assert!(looks_mangled("_ZN3foo3barEv"));
    assert!(looks_mangled(
        "__ZN3std2io4Read11read_to_end17hb85a0f6802e14499E"
    ));
    assert!(looks_mangled("_RNvNtCs1234_7mycrate3foo3bar"));
    // Swift, with and without underscore prefix, and the old scheme.
    assert!(looks_mangled("$s8mangling6curry1yyF"));
    assert!(looks_mangled(
        "_$s8mangling12GenericUnionO3FooyACyxGSicAEmlF"
    ));
    assert!(looks_mangled("_T08mangling3barSiyKF"));
    // ObjC selectors and Scala Native.
    assert!(looks_mangled("-[Foo bar:blub:]"));
    assert!(looks_mangled("_SM17java.lang.IntegerD7compareiiiEo"));
    // The check is a deliberate over-approximation on shared prefixes.
    assert!(looks_mangled("_Rb_tree_insert_and_rebalance"));

    assert!(!looks_mangled(""));
    assert!(!looks_mangled("hello"));
    assert!(!looks_mangled("libc.so.6"));
    assert!(!looks_mangled("GCC_except_table0"));
    // Known limitation: GNU v2 and CodeWarrior have no stable prefix, so the
    // cheap check cannot see them.
    assert!(!looks_mangled("do_thing__C6StupidRC6StupidT1"));
}

#[test]
fn test_detect_language_free_function() {
    assert_eq!(detect_language("_Z1hic"), Language::Cpp);
    assert_eq!(
        detect_language("_RNvNtCs1234_7mycrate3foo3bar"),
        Language::Rust
    );
    assert_eq!(detect_language("libc.so.6"), Language::Unknown);
}

#[test]
fn test_classify_unmangled() {
    assert_eq!(classify_symbol("hello"), SymbolStatus::Unmangled);
    assert_eq!(classify_symbol("libc.so.6"), SymbolStatus::Unmangled);
    assert_eq!(classify_symbol(""), SymbolStatus::Unmangled);
}

#[test]
fn test_classify_mangled() {
    assert_eq!(
        classify_symbol("_Z1hic"),
        SymbolStatus::Mangled(Language::Cpp)
    );
    assert_eq!(
        classify_symbol("?h@@YAXH@Z"),
        SymbolStatus::Mangled(Language::Cpp)
    );
    assert_eq!(
        classify_symbol("__ZN3std2io4Read11read_to_end17hb85a0f6802e14499E"),
        SymbolStatus::Mangled(Language::Rust)
    );
    assert_eq!(
        classify_symbol("-[Foo bar:blub:]"),
        SymbolStatus::Mangled(Language::ObjC)
    );
}

#[test]
fn test_classify_decorations() {
    assert_eq!(
        classify_symbol("__imp_?h@@YAXH@Z"),
        SymbolStatus::Decorated {
            decoration: Decoration::ImportPointer,
            inner: Box::new(SymbolStatus::Mangled(Language::Cpp)),
        }
    );
    assert_eq!(
        classify_symbol(".rdata$_Z1hic"),
        SymbolStatus::Decorated {
            decoration: Decoration::ImportPointer,
            inner: Box::new(SymbolStatus::Mangled(Language::Cpp)),
        }
    );
    assert_eq!(
        classify_symbol("_Z1hic@plt"),
        SymbolStatus::Decorated {
            decoration: Decoration::CallStub,
            inner: Box::new(SymbolStatus::Mangled(Language::Cpp)),
        }
    );
    assert_eq!(
        classify_symbol("_Z1hic@GLIBC_2.2.5"),
        SymbolStatus::Decorated {
            decoration: Decoration::Version("GLIBC_2.2.5".to_string()),
            inner: Box::new(SymbolStatus::Mangled(Language::Cpp)),
        }
    );
    assert_eq!(
        classify_symbol("libc.so.6@@GLIBC_2.2.5"),
        SymbolStatus::Decorated {
            decoration: Decoration::Version("GLIBC_2.2.5".to_string()),
            inner: Box::new(SymbolStatus::Unmangled),
        }
    );
    assert_eq!(
        classify_symbol("_Z1hic$0123456789abcdef0123456789abcdef"),
        SymbolStatus::Decorated {
            decoration: Decoration::LinkerHash,
            inner: Box::new(SymbolStatus::Mangled(Language::Cpp)),
        }
    );
    assert_eq!(
        classify_symbol("_Z1hic.cold"),
        SymbolStatus::Decorated {
            decoration: Decoration::ColdSection,
            inner: Box::new(SymbolStatus::Mangled(Language::Cpp)),
        }
    );
    assert_eq!(
        classify_symbol("@feat.00"),
        SymbolStatus::Decorated {
            decoration: Decoration::SafeSeh,
            inner: Box::new(SymbolStatus::Unmangled),
        }
    );
    assert_eq!(
        classify_symbol("__imp_anon.1234"),
        SymbolStatus::Decorated {
            decoration: Decoration::Anonymous,
            inner: Box::new(SymbolStatus::Unmangled),
        }
    );
    assert_eq!(
        classify_symbol("GCC_except_table12"),
        SymbolStatus::Decorated {
            decoration: Decoration::ExceptTable,
            inner: Box::new(SymbolStatus::Unmangled),
        }
    );
}

#[test]
fn test_classify_msvc_names_keep_their_at_signs() {
    // MSVC symbols are full of '@' characters; suffix stripping must not fire.
    assert_eq!(
        classify_symbol("?h@@YAXH@Z"),
        SymbolStatus::Mangled(Language::Cpp)
    );
    assert_eq!(Normalizer::all().normalize("?h@@YAXH@Z"), "?h@@YAXH@Z");
    // The dllimport decoration rewriting must not break the guard either.
    assert_eq!(
        Normalizer::all().normalize("__declspec(dllimport) ?h@@YAXH@Z"),
        "__declspec(dllimport) ?h@@YAXH@Z"
    );
}

#[test]
fn test_normalize_default_passes() {
    // Legacy Rust escapes.
    assert_eq!(normalize_symbol("foo..bar"), "foo::bar");
    assert_eq!(
        normalize_symbol("alloc..vec..Vec$LT$u8$GT$"),
        "alloc::vec::Vec<u8>"
    );
    assert_eq!(normalize_symbol("fmt$u20$Imp"), "fmt Imp");
    // Rust hash suffixes.
    assert_eq!(
        normalize_symbol("std::io::Read::read_to_end::hb85a0f6802e14499"),
        "std::io::Read::read_to_end"
    );
    // Import pointer decoration.
    assert_eq!(
        normalize_symbol("__imp_?h@@YAXH@Z"),
        "__declspec(dllimport) ?h@@YAXH@Z"
    );
    assert_eq!(
        normalize_symbol(".rdata$_Z1hic"),
        "__declspec(dllimport) _Z1hic"
    );
    // Pseudo-symbols.
    assert_eq!(normalize_symbol("__imp_anon.1234"), "anonymous");
    assert_eq!(normalize_symbol("anon."), "anonymous");
    assert_eq!(normalize_symbol(".L__unnamed_1234"), "anonymous");
    assert_eq!(normalize_symbol("GCC_except_table12"), "GCC_except_table");
    assert_eq!(normalize_symbol("@feat.00"), "SAFESEH");
    // Matching passes leave the input untouched.
    assert_eq!(normalize_symbol("hello"), "hello");
    assert_eq!(normalize_symbol("?h@@YAXH@Z"), "?h@@YAXH@Z");
    // The `.llvm.` suffix is a matching-only pass, off by default.
    assert_eq!(
        normalize_symbol("foo::bar.llvm.123456789012345"),
        "foo::bar.llvm.123456789012345"
    );
}

#[test]
fn test_normalize_all_passes() {
    let all = Normalizer::all();
    assert_eq!(all.normalize("foo@plt"), "foo");
    assert_eq!(all.normalize("foo@GOTPCREL"), "foo");
    assert_eq!(all.normalize("j___cxa_throw"), "__cxa_throw");
    assert_eq!(all.normalize("libc.so.6@GLIBC_2.2.5"), "libc.so.6");
    assert_eq!(all.normalize("_Z1hic@@GLIBC_2.2.5"), "_Z1hic");
    assert_eq!(all.normalize("foo::bar.llvm.123456789012345"), "foo::bar");
    // Passes compose.
    assert_eq!(
        all.normalize("__imp__ZN3foo3barEv"),
        "__declspec(dllimport) _ZN3foo3barEv"
    );
}

#[test]
fn test_normalize_idempotent() {
    let samples = [
        "foo..bar",
        "alloc..vec..Vec$LT$u8$GT$",
        "std::io::Read::read_to_end::hb85a0f6802e14499",
        "__imp_?h@@YAXH@Z",
        "__imp_anon.1234",
        "GCC_except_table12",
        "@feat.00",
        "foo@plt",
        "libc.so.6@GLIBC_2.2.5",
        "foo::bar.llvm.123456789012345",
        "?h@@YAXH@Z",
        "hello",
    ];
    for symbol in samples {
        let once = Normalizer::all().normalize(symbol).into_owned();
        let twice = Normalizer::all().normalize(&once).into_owned();
        assert_eq!(once, twice, "normalize is not idempotent for {symbol}");
    }
}

#[test]
fn test_demangle_options_normalize() {
    // Successful demangling is never normalized.
    assert_eq!(
        Name::from("_Z1hic").try_demangle(DemangleOptions::complete().normalize(true)),
        "h(int, char)"
    );
    // Failed demangling falls back to the normalized raw symbol (the default
    // passes: Rust hash trimming, escapes, import decoration, pseudo-symbols).
    assert_eq!(
        Name::from("foo::bar::hb85a0f6802e14499")
            .try_demangle(DemangleOptions::complete().normalize(true)),
        "foo::bar"
    );
    // Call stubs are a matching-only pass and stay untouched by the default
    // pass set.
    assert_eq!(
        Name::from("foo@plt").try_demangle(DemangleOptions::complete().normalize(true)),
        "foo@plt"
    );
    // Without the option, output is unchanged.
    assert_eq!(
        Name::from("foo::bar::hb85a0f6802e14499").try_demangle(DemangleOptions::complete()),
        "foo::bar::hb85a0f6802e14499"
    );
    // The `demangle` method itself never normalizes.
    assert_eq!(
        Name::from("foo::bar::hb85a0f6802e14499")
            .demangle(DemangleOptions::complete().normalize(true)),
        None
    );
}

#[test]
fn test_default_output_unchanged() {
    assert_eq!(
        Name::from("_Z1hic").demangle(DemangleOptions::complete()),
        Some("h(int, char)".to_string())
    );
    assert_eq!(
        Name::from("__ZN3std2io4Read11read_to_end17hb85a0f6802e14499E")
            .try_demangle(DemangleOptions::name_only()),
        "std::io::Read::read_to_end"
    );
}
