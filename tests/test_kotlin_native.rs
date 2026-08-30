//! Kotlin/Native demangler integration tests.
//!
//! The modern spellings here are verbatim `kotlinc-native` 2.0.21 output for
//! `contrib/fixtures/kotlin/corpus.kt`; the legacy dotted forms are kept
//! because old frameworks in the wild still carry them.

use multi_demangle::{detect_language, Demangle, DemangleOptions};
use similar_asserts::assert_eq;
use symbolic_common::{Language, Name};

#[test]
fn test_demangle_kotlin_native_symbols() {
    for (symbol, full, name_only) in [
        // Modern member function: `#` separates the class from the method,
        // the braced type-parameter block is empty, the return type follows.
        (
            "_kfun:com.example.Counter#increment(kotlin.Int){}kotlin.Int",
            "com.example.Counter.increment(Int): Int",
            "com.example.Counter.increment",
        ),
        // Top-level function with no return value.
        (
            "_kfun:com.example#main(kotlin.Array<kotlin.String>){}",
            "com.example.main(Array<String>)",
            "com.example.main",
        ),
        // Legacy dotted spelling, seen in pre-2020 binaries.
        (
            "_kfun:com.example.Foo.bar(kotlin.String;kotlin.Int)",
            "com.example.Foo.bar(String, Int)",
            "com.example.Foo.bar",
        ),
        (
            "_kfun:main(kotlin.Array<kotlin.String>)",
            "main(Array<String>)",
            "main",
        ),
        // Bare names: class initializers and top-level properties.
        (
            "_kfun:androidx.compose.material3.adaptive.WindowAdaptiveInfo",
            "androidx.compose.material3.adaptive.WindowAdaptiveInfo",
            "androidx.compose.material3.adaptive.WindowAdaptiveInfo",
        ),
        // Doubled platform underscore.
        (
            "__kfun:com.example.Counter#reset(){}",
            "com.example.Counter.reset()",
            "com.example.Counter.reset",
        ),
    ] {
        let name = Name::from(symbol);
        assert_eq!(name.detect_language(), Language::Unknown);
        assert_eq!(
            name.demangle(DemangleOptions::complete()),
            Some(full.to_string()),
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
fn test_kotlin_native_compiler_markers_and_thunks() {
    // `#static` / `#internal` are declaration markers, so they render as
    // trailing tags rather than as name segments — attaching them to the
    // name would make `static` the leaf of `$getEnumAt`.
    assert_eq!(
        Name::from("_kfun:com.example.Color#$getEnumAt#static(kotlin.Int){}com.example.Color")
            .demangle(DemangleOptions::complete()),
        Some("com.example.Color.$getEnumAt(Int): com.example.Color [static]".to_string())
    );
    assert_eq!(
        Name::from("_kfun:com.example.Color#$getEnumAt#static(kotlin.Int){}com.example.Color")
            .demangle(DemangleOptions::name_only()),
        Some("com.example.Color.$getEnumAt".to_string())
    );
    assert_eq!(
        Name::from("_kfun:com.example.Color.$init_global#internal")
            .demangle(DemangleOptions::complete()),
        Some("com.example.Color.$init_global [internal]".to_string())
    );
    // Trampolines stay distinguishable from the function they dispatch to.
    assert_eq!(
        Name::from("_kfun:com.example.Rect#area(){}kotlin.Double-trampoline")
            .demangle(DemangleOptions::complete()),
        Some("com.example.Rect.area(): Double [trampoline]".to_string())
    );
}

#[test]
fn test_kotlin_native_return_type_suffix() {
    // The legacy `:Ret` suffix, kept for old binaries.
    let name = Name::from("_kfun:com.example.Foo.size():kotlin.Int");
    assert_eq!(
        name.demangle(DemangleOptions::complete()),
        Some("com.example.Foo.size(): Int".to_string())
    );
    assert_eq!(
        name.demangle(DemangleOptions::complete().return_type(false)),
        Some("com.example.Foo.size()".to_string())
    );
    assert_eq!(
        name.demangle(DemangleOptions::name_only()),
        Some("com.example.Foo.size".to_string())
    );
}

#[test]
fn test_kotlin_nested_generics_survive() {
    let name = Name::from(
        "_kfun:kotlin.collections.AbstractList.Companion#orderedHashCode(kotlin.collections.Collection<*>){}kotlin.Int",
    );
    assert_eq!(
        name.demangle(DemangleOptions::complete()),
        Some(
            "kotlin.collections.AbstractList.Companion.orderedHashCode(collections.Collection<*>): Int"
                .to_string()
        )
    );
}

#[test]
fn test_kotlin_generic_type_parameters() {
    // Type-parameter references (`0:0`) are unnamed in the mangling and
    // render verbatim; the braced bounds block is parsed but not rendered.
    let name = Name::from("_kfun:com.example#genericIdentity(0:0){0§<kotlin.Any?>}0:0");
    assert_eq!(
        name.demangle(DemangleOptions::complete()),
        Some("com.example.genericIdentity(0:0): 0:0".to_string())
    );
}

#[test]
fn test_kotlin_rejects_non_kotlin() {
    for symbol in [
        "kfun",
        "_kfun:",
        "libc.so.6",
        "_kfun:foo(kotlin.Int",
        // A trailing part after the signature with no braced block is not
        // demangled.
        "_kfun:com.example.Counter#reset()junk",
    ] {
        assert_eq!(
            Name::from(symbol).demangle(DemangleOptions::complete()),
            None,
            "for {symbol}"
        );
    }
    // C++ symbols are claimed by the C++ backend; the Kotlin/Native backend
    // itself rejects them.
    assert_eq!(
        multi_demangle::demangle_as(
            "kotlin-native",
            "_ZN3foo3barEv",
            DemangleOptions::complete()
        ),
        None
    );
}

#[test]
fn test_kotlin_detection() {
    assert_eq!(detect_language("_kfun:main"), Some("kotlin-native"));
    assert_eq!(detect_language("__kfun:main"), Some("kotlin-native"));
    assert_eq!(detect_language("kfun:main"), Some("kotlin-native"));
    assert_eq!(detect_language("libc.so.6"), None);
}

#[test]
fn test_kotlin_structured() {
    let info = Name::from("_kfun:com.example.Counter#increment(kotlin.Int){}kotlin.Int")
        .demangle_structured(DemangleOptions::complete())
        .expect("structured");
    assert_eq!(info.namespace, ["com", "example", "Counter"]);
    assert_eq!(info.name, "increment");
    assert_eq!(info.kind, multi_demangle::DemangledKind::Method);
    assert_eq!(info.parameters, Some(vec!["Int".to_string()]));

    // A plain function in a lowercase namespace stays a function.
    let info = Name::from("_kfun:com.example#describe(kotlin.Int;kotlin.String){}kotlin.String")
        .demangle_structured(DemangleOptions::complete())
        .expect("structured");
    assert_eq!(info.namespace, ["com", "example"]);
    assert_eq!(info.kind, multi_demangle::DemangledKind::Function);
}
