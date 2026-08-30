//! D language demangler integration tests.
//!
//! The base cases mirror LLVM's `DLangDemangleTest.cpp` oracle; the function,
//! template, and type-grammar cases are derived from the D ABI specification
//! (<https://dlang.org/spec/abi.html#name_mangling>).

use multi_demangle::{detect_language, Demangle, DemangleOptions, DemangledKind};
use similar_asserts::assert_eq;
use symbolic_common::{Language, Name};

#[test]
fn test_dlang_detection() {
    assert_eq!(Name::from("_Dmain").detect_language(), Language::D);
    assert_eq!(
        Name::from("_D6module4funcFZv").detect_language(),
        Language::D
    );
    assert_eq!(detect_language("_Dmain"), Some("d"));
    // Near misses stay unknown.
    assert_eq!(Name::from("_DDD").detect_language(), Language::Unknown);
    assert_eq!(Name::from("_D88").detect_language(), Language::Unknown);
    assert_eq!(Name::from("_DEBUG").detect_language(), Language::Unknown);
}

#[test]
fn test_dlang_functions() {
    for (symbol, complete, name_only) in [
        ("_Dmain", "D main", "D main"),
        ("_D6module4funcFZv", "module.func()", "module.func"),
        ("_D6module4funcFiZv", "module.func(int)", "module.func"),
        (
            "_D6module4funcFikdZv",
            "module.func(int, uint, double)",
            "module.func",
        ),
        // Compound parameter types.
        (
            "_D6module4funcFPiaZv",
            "module.func(int*, char)",
            "module.func",
        ),
        ("_D6module4funcFAiZv", "module.func(int[])", "module.func"),
        ("_D6module4funcFGi3Zv", "module.func(int[3])", "module.func"),
        // Variadics.
        ("_D6module4funcFXv", "module.func(...)", "module.func"),
        ("_D6module4funcFiXv", "module.func(int, ...)", "module.func"),
        // Parameter storage classes.
        ("_D6module4funcFKiZv", "module.func(ref int)", "module.func"),
        (
            "_D6module4funcFLiZv",
            "module.func(lazy int)",
            "module.func",
        ),
        (
            "_D6module4funcFMKiZv",
            "module.func(scope ref int)",
            "module.func",
        ),
        (
            "_D6module4funcFNkiZv",
            "module.func(return int)",
            "module.func",
        ),
        // Member functions.
        (
            "_D6module4Test6methodMFiZi",
            "module.Test.method(int)",
            "module.Test.method",
        ),
        // Constructors and destructors.
        (
            "_D6module4Test6__ctorFZv",
            "module.Test.this()",
            "module.Test.this",
        ),
        (
            "_D6module4Test6__dtorFZv",
            "module.Test.~this()",
            "module.Test.~this",
        ),
    ] {
        let name = Name::from(symbol);
        assert_eq!(
            name.demangle(DemangleOptions::complete()),
            Some(complete.to_string()),
            "for {symbol}"
        );
        assert_eq!(
            name.demangle(DemangleOptions::name_only()),
            Some(name_only.to_string()),
            "for {symbol}"
        );
    }
}

#[test]
fn test_dlang_variables() {
    // A variable's type renders as a prefix with complete options and is
    // dropped by the individual toggles.
    let name = Name::from("_D6module7counteri");
    assert_eq!(
        name.demangle(DemangleOptions::complete()),
        Some("int module.counter".to_string())
    );
    assert_eq!(
        name.demangle(DemangleOptions::name_only()),
        Some("module.counter".to_string())
    );
    assert_eq!(
        name.demangle(DemangleOptions::complete().return_type(false)),
        Some("module.counter".to_string())
    );
    // typeof(null) renders nothing, so no prefix appears.
    assert_eq!(
        Name::from("_D6module3fooo")
            .demangle(DemangleOptions::complete())
            .as_deref(),
        Some("ifloat module.foo")
    );
}

#[test]
fn test_dlang_templates() {
    for (symbol, expected) in [
        ("_D6module9__T4tempZ4funcFZv", "module.temp!().func()"),
        (
            "_D6module13__T4tempTiTkZ4funcFZv",
            "module.temp!(int, uint).func()",
        ),
        (
            "_D6module14__T4tempVii42Z4funcFZv",
            "module.temp!(42).func()",
        ),
        (
            "_D6module14__T4tempViN10Z4funcFZv",
            "module.temp!(-10).func()",
        ),
        (
            "_D6module24__T4tempVaa5_68656c6c6fZ4funcFZv",
            "module.temp!(\"hello\").func()",
        ),
        (
            "_D6module22__T4tempS6module4TestZ4funcFZv",
            "module.temp!(module.Test).func()",
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
    // A template block whose length prefix disagrees fails.
    assert_eq!(
        Name::from("_D6module12__T4tempTiTkZ4funcFZv").demangle(DemangleOptions::complete()),
        None
    );
}

#[test]
fn test_dlang_magic_symbols() {
    assert_eq!(
        Name::from("_D8demangle4test6__initZ")
            .demangle(DemangleOptions::complete())
            .as_deref(),
        Some("initializer for demangle.test")
    );
    assert_eq!(
        Name::from("_D8demangle4test6__vtblZ")
            .demangle(DemangleOptions::complete())
            .as_deref(),
        Some("vtable for demangle.test")
    );
    assert_eq!(
        Name::from("_D8demangle4test7__ClassZ")
            .demangle(DemangleOptions::complete())
            .as_deref(),
        Some("ClassInfo for demangle.test")
    );
    assert_eq!(
        Name::from("_D8demangle4test12__ModuleInfoZ")
            .demangle(DemangleOptions::complete())
            .as_deref(),
        Some("ModuleInfo for demangle.test")
    );
}

#[test]
fn test_dlang_back_references() {
    // Symbol back reference: `Qe` points back at `3ABC`.
    assert_eq!(
        Name::from("_D8demangle3ABCQe1ai")
            .demangle(DemangleOptions::complete())
            .as_deref(),
        Some("int demangle.ABC.ABC.a")
    );
    // Type back reference: `Qd` points back at `i`.
    assert_eq!(
        Name::from("_D8demangle4ABCi1aQd")
            .demangle(DemangleOptions::complete())
            .as_deref(),
        Some("int demangle.ABCi.a")
    );
    // Recursive and out-of-range back references fail.
    assert_eq!(
        Name::from("_D8demangle3ABCQa1ai").demangle(DemangleOptions::complete()),
        None
    );
    assert_eq!(
        Name::from("_D8demangle5recurQa").demangle(DemangleOptions::complete()),
        None
    );
}

#[test]
fn test_dlang_structured() {
    let info = Name::from("_D6module4Test6methodMFiZi")
        .demangle_structured(DemangleOptions::complete())
        .unwrap();
    assert_eq!(info.language, Language::D);
    assert_eq!(info.namespace, ["module", "Test"]);
    assert_eq!(info.name, "method");
    assert_eq!(info.kind, DemangledKind::Method);
    assert_eq!(info.parameters, Some(vec!["int".to_string()]));

    // Variables are static data.
    let info = Name::from("_D6module7counteri")
        .demangle_structured(DemangleOptions::complete())
        .unwrap();
    assert_eq!(info.kind, DemangledKind::StaticVariable);
    assert_eq!(info.name, "counter");

    // Template arguments of the leaf are captured.
    let info = Name::from("_D6module11__T4tempTiZ4funcFZv")
        .demangle_structured(DemangleOptions::complete())
        .unwrap();
    assert_eq!(info.name, "func");
    assert_eq!(info.namespace, ["module", "temp!(int)"]);
    assert!(info.is_generic);

    // Vtable symbols keep their kind.
    let info = Name::from("_D8demangle4test6__vtblZ")
        .demangle_structured(DemangleOptions::complete())
        .unwrap();
    assert_eq!(info.kind, DemangledKind::VirtualTable);
}

#[test]
fn test_dlang_rejects_garbage() {
    for symbol in [
        "_D",
        "_D8",
        "_DDD",
        "_D8demangle3foo",
        "_D8demangle3fooinvalidtypeseq",
        "_D8demangleQDXXXXXXXXXXXXx",
        "_D8demangle3fooQXXXx",
    ] {
        assert_eq!(
            Name::from(symbol).demangle(DemangleOptions::complete()),
            None,
            "for {symbol}"
        );
    }
}
