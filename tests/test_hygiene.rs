//! Symbol hygiene tests: mangling detection, classification, normalization.
//!
//! Includes compatibility vectors ported from the primary consumer's (OWASP
//! blint) test suite so behavior drift shows up here first.

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
    assert!(looks_mangled("__RNvNtCs1234_7mycrate3foo3bar"));
    // Swift, with and without underscore prefix, and the old scheme.
    assert!(looks_mangled("$s8mangling6curry1yyF"));
    assert!(looks_mangled(
        "_$s8mangling12GenericUnionO3FooyACyxGSicAEmlF"
    ));
    assert!(looks_mangled("_T08mangling3barSiyKF"));
    // ObjC selectors, metadata symbols, and Scala Native.
    assert!(looks_mangled("-[Foo bar:blub:]"));
    assert!(looks_mangled("_OBJC_CLASS_$_Foo"));
    assert!(looks_mangled("l_OBJC_SELECTOR_REFERENCES_12"));
    assert!(looks_mangled("_SM17java.lang.IntegerD7compareiiiEo"));
    // D, Kotlin/Native, Ada, and Fortran module symbols.
    assert!(looks_mangled("_D6module4funcFZv"));
    assert!(looks_mangled("_kfun:com.example.Foo.bar(kotlin.String)"));
    assert!(looks_mangled("ada__exceptions__raiseXn"));
    assert!(looks_mangled("__my_module_MOD_my_proc"));
    assert!(looks_mangled("my_module_mp_my_proc_"));
    // Partial legacy Rust escapes (the upstream consumer gates on `$LT$`).
    assert!(looks_mangled("impl$LT$T$GT$display"));

    // Rust v0 requires an uppercase start byte after `_R`; plain C symbols
    // must not be flagged.
    assert!(!looks_mangled("_Reset"));
    assert!(!looks_mangled("_RtlMoveMemory"));
    assert!(!looks_mangled("_Rb_tree_insert_and_rebalance"));
    assert!(!looks_mangled("_R"));

    assert!(!looks_mangled(""));
    assert!(!looks_mangled("hello"));
    assert!(!looks_mangled("libc.so.6"));
    assert!(!looks_mangled("GCC_except_table0"));
    // GNU v2 and CodeWarrior still have no stable prefix of their own, but
    // names carrying `__` separators now trip the Ada heuristic (an
    // intentional over-approximation: looks-mangled makes no correctness
    // promise).
    assert!(looks_mangled("do_thing__C6StupidRC6StupidT1"));
}

#[test]
fn test_detect_language() {
    assert_eq!(detect_language("_Z1hic"), Some("cpp"));
    assert_eq!(detect_language("?h@@YAXH@Z"), Some("cpp"));
    assert_eq!(
        detect_language("_RNvNtCs1234_7mycrate3foo3bar"),
        Some("rust")
    );
    // Scala Native has no Language variant but is still reported.
    assert_eq!(
        detect_language("_SM17java.lang.IntegerD7compareiiiEo"),
        Some("scala-native")
    );
    // The same holds for D, Kotlin/Native, Ada, and Fortran.
    assert_eq!(detect_language("_D6module4funcFZv"), Some("d"));
    assert_eq!(detect_language("_Dmain"), Some("d"));
    assert_eq!(
        detect_language("_kfun:com.example.Foo.bar(kotlin.String)"),
        Some("kotlin-native")
    );
    assert_eq!(
        detect_language("ada__exceptions__last_chance_handlerXn"),
        Some("ada")
    );
    assert_eq!(detect_language("__my_module_MOD_my_proc"), Some("fortran"));
    assert_eq!(detect_language("my_module_mp_my_proc_"), Some("fortran"));
    // The plain g77 form is not detected (it collides with C symbols).
    assert_eq!(detect_language("init_"), None);
    assert_eq!(detect_language("libc.so.6"), None);
    assert_eq!(detect_language("hello"), None);
}

#[test]
fn test_classify_unmangled() {
    assert_eq!(classify_symbol("hello"), SymbolStatus::Unmangled);
    assert_eq!(classify_symbol("libc.so.6"), SymbolStatus::Unmangled);
    assert_eq!(classify_symbol(""), SymbolStatus::Unmangled);
    // `@`-containing names that are not ELF versions stay untouched.
    assert_eq!(classify_symbol("foo@bar"), SymbolStatus::Unmangled);
    assert_eq!(classify_symbol("main.init@v2"), SymbolStatus::Unmangled);
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
    // Scala Native is mangled in an unnamed language.
    assert_eq!(
        classify_symbol("_SM17java.lang.IntegerD7compareiiiEo"),
        SymbolStatus::Mangled(Language::Unknown)
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
    assert_eq!(Normalizer::matching().normalize("?h@@YAXH@Z"), "?h@@YAXH@Z");
    // The dllimport decoration rewriting must not break the guard either.
    assert_eq!(
        Normalizer::matching().normalize("__declspec(dllimport) ?h@@YAXH@Z"),
        "__declspec(dllimport) ?h@@YAXH@Z"
    );
}

#[test]
fn test_normalize_display_passes() {
    // Legacy Rust escapes (blint's display-name parity).
    assert_eq!(
        normalize_symbol("alloc..vec..Vec$LT$u8$GT$"),
        "alloc::vec::Vec<u8>"
    );
    assert_eq!(normalize_symbol("fmt$u20$Imp"), "fmt Imp");
    assert_eq!(
        normalize_symbol("_ZN4core..vec..Vec$LT$u8$GT$17h0123abcdef456789E"),
        "_ZN4core::vec::Vec<u8>17h0123abcdef456789E"
    );
    // Generic `$u<hex>$` escapes, matching rustc_demangle's printer.
    assert_eq!(normalize_symbol("$u5f$x"), "_x");
    assert_eq!(normalize_symbol("$u1f980$crab"), "\u{1f980}crab");
    assert_eq!(normalize_symbol("$u2b$plus"), "+plus");
    // Unknown or malformed escapes stay verbatim.
    assert_eq!(normalize_symbol("$u5F$x"), "$u5F$x");
    assert_eq!(normalize_symbol("$u$"), "$u$");
    assert_eq!(normalize_symbol("$trailing"), "$trailing");
    // Rust hash suffixes.
    assert_eq!(
        normalize_symbol("std::io::Read::read_to_end::hb85a0f6802e14499"),
        "std::io::Read::read_to_end"
    );
    // Import pointer decoration is rewritten for display.
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
    // The escape pass is gated on the symbol looking like legacy Rust, so
    // ordinary dots are never rewritten.
    assert_eq!(normalize_symbol("foo..bar"), "foo..bar");
    assert_eq!(normalize_symbol("x...y"), "x...y");
    // Matching-only passes stay off for display.
    assert_eq!(
        normalize_symbol("foo::bar.llvm.123456789012345"),
        "foo::bar.llvm.123456789012345"
    );
    assert_eq!(normalize_symbol("foo@plt"), "foo@plt");
    assert_eq!(
        normalize_symbol("libc.so.6@GLIBC_2.2.5"),
        "libc.so.6@GLIBC_2.2.5"
    );
    // Unmatched input is returned unchanged.
    assert_eq!(normalize_symbol("hello"), "hello");
    assert_eq!(normalize_symbol("?h@@YAXH@Z"), "?h@@YAXH@Z");
}

#[test]
fn test_normalize_matching_passes() {
    let matching = Normalizer::matching();
    assert_eq!(matching.normalize("foo@plt"), "foo");
    assert_eq!(matching.normalize("foo@GOTPCREL"), "foo");
    assert_eq!(matching.normalize("j___cxa_throw"), "__cxa_throw");
    assert_eq!(matching.normalize("libc.so.6@GLIBC_2.2.5"), "libc.so.6");
    assert_eq!(matching.normalize("_Z1hic@@GLIBC_2.2.5"), "_Z1hic");
    // Import pointers are stripped for matching, not rewritten.
    assert_eq!(matching.normalize("__imp_CreateFileW"), "CreateFileW");
    // Combined Rust hash + LLVM clone suffixes strip cleanly because the
    // `.llvm.` pass runs first.
    assert_eq!(
        matching.normalize("core::ptr::drop_in_place::h41b828a7ca01b8c4.llvm.12153207245666130899"),
        "core::ptr::drop_in_place"
    );
}

#[test]
fn test_normalize_version_tokens_are_strict() {
    let matching = Normalizer::matching();
    // Real ELF versions strip; arbitrary `@`-containing names do not.
    assert_eq!(matching.normalize("_Z1hic@GLIBC_2.2.5"), "_Z1hic");
    assert_eq!(matching.normalize("foo@@GLIBC_2.2.5"), "foo");
    assert_eq!(matching.normalize("foo@1.2.3"), "foo");
    assert_eq!(matching.normalize("foo@bar"), "foo@bar");
    assert_eq!(matching.normalize("main.init@v2"), "main.init@v2");
    assert_eq!(matching.normalize("foo@12345"), "foo@12345");
}

#[test]
fn test_normalize_idempotent() {
    let samples = [
        "foo..bar",
        "alloc..vec..Vec$LT$u8$GT$",
        "$u5f$x",
        "$u24$",
        "x...y",
        "std::io::Read::read_to_end::hb85a0f6802e14499",
        "__imp_?h@@YAXH@Z",
        "__imp_CreateFileW",
        "__imp_anon.1234",
        "GCC_except_table12",
        "@feat.00",
        "foo@plt",
        "libc.so.6@GLIBC_2.2.5",
        "foo::bar.llvm.123456789012345",
        "core::ptr::drop_in_place::h41b828a7ca01b8c4.llvm.12153207245666130899",
        "main.init@v2",
        "?h@@YAXH@Z",
        "hello",
    ];
    for passes in [Normalizer::display(), Normalizer::matching()] {
        for symbol in samples {
            let once = passes.normalize(symbol).into_owned();
            let twice = passes.normalize(&once).into_owned();
            assert_eq!(once, twice, "normalize is not idempotent for {symbol}");
        }
    }
}

#[test]
fn test_try_demangle_normalized() {
    let normalizer = Normalizer::display();
    // Successful demangling is never normalized.
    assert_eq!(
        Name::from("_Z1hic").try_demangle_normalized(DemangleOptions::complete(), &normalizer),
        "h(int, char)"
    );
    // Failed demangling falls back to the normalized raw symbol.
    assert_eq!(
        Name::from("__imp__ZN3foo3barEv")
            .try_demangle_normalized(DemangleOptions::complete(), &normalizer),
        "__declspec(dllimport) _ZN3foo3barEv"
    );
    assert_eq!(
        Name::from("std::io::Read::read_to_end::hb85a0f6802e14499")
            .try_demangle_normalized(DemangleOptions::complete(), &normalizer),
        "std::io::Read::read_to_end"
    );
    // Plain try_demangle is never affected.
    assert_eq!(
        Name::from("__imp__ZN3foo3barEv").try_demangle(DemangleOptions::complete()),
        "__imp__ZN3foo3barEv"
    );
    // The `demangle` method never normalizes.
    assert_eq!(
        Name::from("__imp__ZN3foo3barEv").demangle(DemangleOptions::complete()),
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

/// Compatibility vectors ported verbatim from blint's test suite
/// (`tests/test_import_attribution.py`, `tests/test_utils.py`) and from the
/// heuristics in its `demangle_symbolic_name` and `canon.py`.
#[test]
fn test_blint_compatibility_oracle() {
    // blint's `normalize_call_target` (matching-oriented).
    let matching = Normalizer::matching();
    assert_eq!(matching.normalize("memcpy@plt"), "memcpy");
    assert_eq!(matching.normalize("__imp_CreateFileW"), "CreateFileW");
    assert_eq!(matching.normalize("memcpy@GLIBC_2.14"), "memcpy");
    // Namespaced names are left intact.
    assert_eq!(
        matching.normalize("APT::Container::begin"),
        "APT::Container::begin"
    );
    assert_eq!(
        matching.normalize("/usr/lib/libSystem.B.dylib::printf"),
        "/usr/lib/libSystem.B.dylib::printf"
    );

    // blint's `demangle_symbolic_name` fallbacks (display-oriented).
    let display = Normalizer::display();
    // plain C symbols pass through
    assert_eq!(display.normalize("CCCryptorCreate"), "CCCryptorCreate");
    // `__imp_` becomes a dllimport decoration, as in blint's display path
    assert_eq!(
        display.normalize("__imp_CreateFileW"),
        "__declspec(dllimport) CreateFileW"
    );
    // legacy Rust `$`-escapes decode exactly like blint's replace chain
    assert_eq!(
        display.normalize("_ZN4core3ptr79drop_in_place$LT$alloc..vec..Vec$GT$17h41b828a7ca01b8c4E"),
        "_ZN4core3ptr79drop_in_place<alloc::vec::Vec>17h41b828a7ca01b8c4E"
    );
    // pseudo-symbols
    assert_eq!(display.normalize("GCC_except_table0"), "GCC_except_table");
    assert_eq!(display.normalize("@feat.00"), "SAFESEH");
    // anonymous values behind an import pointer
    assert_eq!(display.normalize("__imp_anon.12345"), "anonymous");

    // blint's two divergent hash trims now agree in one implementation.
    // utils.py trims a 17-char trailing `::h...` segment; canon.py uses the
    // `::h[0-9a-f]{8,}` regex; both vectors normalize identically here.
    assert_eq!(
        display.normalize("std::io::Read::read_to_end::hb85a0f6802e14499"),
        "std::io::Read::read_to_end"
    );
    assert_eq!(
        matching.normalize(
            "core::ptr::drop_in_place<alloc::vec::Vec<wast::component::types::VariantCase>>::h41b828a7ca01b8c4.llvm.12153207245666130899"
        ),
        "core::ptr::drop_in_place<alloc::vec::Vec<wast::component::types::VariantCase>>"
    );
}
