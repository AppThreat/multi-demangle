//! Tests for the batch demangling pipeline: `demangle_one`, `demangle_iter`,
//! and the order/dedup guarantees shared with the Python `demangle_symbols`.

use std::borrow::Cow;

use multi_demangle::{demangle, demangle_iter, demangle_one, DemangleOptions};

/// A duplicate-heavy mix of languages, unmangled names, and decorated
/// symbols, as found in real symbol tables.
fn mixed_symbols() -> Vec<String> {
    let uniques = [
        "_ZN3foo3barEv",
        "_ZN4core3ptr79drop_in_place$LT$alloc..vec..Vec$LT$u8$GT$$GT$17h41b828a7ca01b8c4E",
        "_RNvNtCs1234_7mycrate3foo3bar",
        "_Z1hic",
        "?h@@YAXH@Z",
        "$s8mangling12GenericUnionO3FooyACyxGSicAEmlF",
        "_SM17java.lang.IntegerD7compareiiiEo",
        "-[Class method]",
        "libc.so.6",
        "main",
        "memcpy@plt",
        "__imp_?h@@YAXH@Z",
    ];
    let mut symbols = Vec::new();
    for sym in uniques {
        // Three occurrences each, interleaved the way dynsym, symtab, and
        // PLT/GOT views repeat the same import.
        for _ in 0..3 {
            symbols.push(sym.to_string());
        }
    }
    symbols
}

#[test]
fn demangle_one_matches_demangle() {
    for sym in [
        "_ZN3foo3barEv",
        "_Z1hic",
        "$s8mangling6curry1yyF",
        "libc.so.6",
        "",
    ] {
        assert_eq!(demangle_one(sym, DemangleOptions::complete()), demangle(sym));
    }
}

#[test]
fn demangle_one_honors_options() {
    assert_eq!(
        demangle_one("_ZN3foo3barEv", DemangleOptions::name_only()),
        "foo::bar"
    );
    assert_eq!(
        demangle_one("_ZN3foo3barEv", DemangleOptions::complete()),
        "foo::bar()"
    );
}

#[test]
fn batch_preserves_order_and_dedupes() {
    let symbols = mixed_symbols();
    let expected: Vec<String> = symbols
        .iter()
        .map(|sym| demangle_one(sym, DemangleOptions::complete()).into_owned())
        .collect();

    let actual: Vec<String> = demangle_iter(symbols.iter().map(String::as_str), DemangleOptions::complete())
        .map(Cow::into_owned)
        .collect();

    assert_eq!(actual, expected);
    // The mix has exactly three occurrences of every symbol; all three
    // positions must agree.
    for chunk in actual.chunks(3) {
        assert_eq!(chunk[0], chunk[1]);
        assert_eq!(chunk[1], chunk[2]);
    }
}

#[test]
fn batch_matches_per_symbol_on_corpus_mix() {
    // Same guarantee with every third symbol removed, so the scatter has to
    // handle irregular duplicate runs.
    let symbols: Vec<String> = mixed_symbols()
        .into_iter()
        .enumerate()
        .filter(|(i, _)| i % 3 != 1)
        .map(|(_, sym)| sym)
        .collect();
    let expected: Vec<String> = symbols
        .iter()
        .map(|sym| demangle_one(sym, DemangleOptions::complete()).into_owned())
        .collect();
    let actual: Vec<String> = demangle_iter(symbols.iter().map(String::as_str), DemangleOptions::complete())
        .map(Cow::into_owned)
        .collect();
    assert_eq!(actual, expected);
}

#[test]
fn batch_all_identical() {
    let demangled: Vec<String> = demangle_iter(
        std::iter::repeat_n("_Z1hic", 1000),
        DemangleOptions::complete(),
    )
    .map(Cow::into_owned)
    .collect();
    assert_eq!(demangled.len(), 1000);
    assert!(demangled.iter().all(|s| s == "h(int, char)"));
}

#[test]
fn batch_empty_input() {
    let demangled: Vec<_> = demangle_iter(Vec::new(), DemangleOptions::complete()).collect();
    assert!(demangled.is_empty());
}

#[test]
fn batch_unmangled_stay_borrowed() {
    let demangled: Vec<_> = demangle_iter(["libc.so.6", "main"], DemangleOptions::complete()).collect();
    assert_eq!(&demangled[0], "libc.so.6");
    assert_eq!(&demangled[1], "main");
    // Unmangled symbols borrow rather than allocate.
    assert!(matches!(demangled[0], Cow::Borrowed(_)));
}

#[test]
fn batch_accepts_any_into_iterator() {
    // Owned strings, references, and chained iterators all work.
    let owned = ["_Z1hic".to_string(), "libc.so.6".to_string()];
    let demangled: Vec<String> = demangle_iter(owned.iter().map(String::as_str), DemangleOptions::complete())
        .map(Cow::into_owned)
        .collect();
    assert_eq!(demangled, vec!["h(int, char)".to_string(), "libc.so.6".to_string()]);
}
