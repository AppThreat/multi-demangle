//! Kotlin/Native demangler integration tests.

use multi_demangle::{detect_language, Demangle, DemangleOptions};
use similar_asserts::assert_eq;
use symbolic_common::{Language, Name};

#[test]
fn test_demangle_kotlin_native_symbols() {
    for (symbol, full, name_only) in [
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
        (
            "_kfun:kotlin.io.println(kotlin.Any?)",
            "kotlin.io.println(Any?)",
            "kotlin.io.println",
        ),
        // Bare names: class initializers and top-level properties.
        (
            "_kfun:androidx.compose.material3.adaptive.WindowAdaptiveInfo",
            "androidx.compose.material3.adaptive.WindowAdaptiveInfo",
            "androidx.compose.material3.adaptive.WindowAdaptiveInfo",
        ),
        // Doubled platform underscore.
        (
            "__kfun:com.example.Counter.count(kotlin.Int)",
            "com.example.Counter.count(Int)",
            "com.example.Counter.count",
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
fn test_kotlin_native_return_type_suffix() {
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
    let name = Name::from("_kfun:kotlin.collections.Map.getOrDefault(kotlin.Any?;kotlin.Any?)");
    assert_eq!(
        name.demangle(DemangleOptions::complete()),
        Some("kotlin.collections.Map.getOrDefault(Any?, Any?)".to_string())
    );
}

#[test]
fn test_kotlin_rejects_non_kotlin() {
    for symbol in ["kfun", "_kfun:", "libc.so.6", "_kfun:foo(kotlin.Int"] {
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
    assert_eq!(detect_language("libc.so.6"), None);
}

#[test]
fn test_kotlin_structured() {
    let info = Name::from("_kfun:com.example.Foo.bar(kotlin.String;kotlin.Int)")
        .demangle_structured(DemangleOptions::complete())
        .expect("structured");
    assert_eq!(info.namespace, ["com", "example", "Foo"]);
    assert_eq!(info.name, "bar");
    assert_eq!(info.kind, multi_demangle::DemangledKind::Method);
    assert_eq!(
        info.parameters,
        Some(vec!["String".to_string(), "Int".to_string()])
    );

    // A plain function in a lowercase namespace stays a function.
    let info = Name::from("_kfun:kotlin.io.println(kotlin.Any?)")
        .demangle_structured(DemangleOptions::complete())
        .expect("structured");
    assert_eq!(info.kind, multi_demangle::DemangledKind::Function);
}
