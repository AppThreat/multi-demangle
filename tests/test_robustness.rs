//! Robustness tests for the grammar-based backends (D, Ada, Fortran,
//! Kotlin/Native).
//!
//! The D backend implements a full recursive grammar over untrusted input,
//! and the other new backends parse pattern-based encodings, so crafted
//! symbols must never panic, loop forever, or allocate unboundedly. Real
//! fuzzing (cargo-fuzz, Plan 05) builds on these seeds; this suite runs the
//! deterministic mutation subset on every `cargo test`.

use multi_demangle::{demangle_as, Demangle, DemangleOptions};

/// Valid seed symbols, one or more per backend.
const SEEDS: &[&str] = &[
    // D
    "_Dmain",
    "_D6module4funcFZv",
    "_D6module4Test6methodMFiZi",
    "_D8demangle3ABCQe1ai",
    "_D6module13__T4tempTiTkZ4funcFZv",
    "_D8demangle4test6__initZ",
    "_D6module2fpPFiZi",
    "_D6module3fooFDFiZvZv",
    // Ada
    "ada__exceptions__last_chance_handlerXn",
    "_ada_main",
    "module__square__2",
    "Oeq",
    // Fortran
    "__my_module_MOD_my_proc",
    "my_module_mp_my_proc_",
    // Kotlin/Native — real compiler shapes from tests/corpus/kotlin_symbols.txt
    // (kotlinc-native 2.0.21): `#` visibility/static markers, `{}` body
    // blocks, `<init>`/property-accessor names. The pre-Plan-08 dotted
    // spelling (`_kfun:com.example.Foo.bar(kotlin.String)`) parsed as a bare
    // name and exercised none of the grammar the compiler actually emits.
    "kfun:com.example.Color#<init>(kotlin.String;kotlin.Int){}",
    "kfun:com.example.Color#$getEnumAt#static(kotlin.Int){}com.example.Color",
    "kfun:com.example.Color.$init_global#internal",
    "kfun:com.example.Counter.Companion#create(){}com.example.Counter",
    "kfun:com.example.Shape#area(){}kotlin.Double-trampoline",
    "kfun:com.example.Counter#<get-$companion>#static(){}com.example.Counter.Companion",
    "kfun:main(kotlin.Array<kotlin.String>){}",
    // ObjC metadata
    "_OBJC_CLASS_$_Foo",
    "_OBJC_IVAR_$_MyObject._count",
];

/// Every mutation of every seed must demangle (or decline) without panicking.
#[test]
fn mutations_do_not_panic() {
    let mut count = 0;
    for &seed in SEEDS {
        // Byte-level mutations: truncations, byte substitutions, repeats.
        for len in 0..=seed.len() {
            // Truncation.
            let truncated = &seed[..len];
            let _ = Name::from(truncated).demangle(DemangleOptions::complete());
            count += 1;
        }
        for (idx, byte) in seed.bytes().enumerate() {
            // Substitution with grammar-significant bytes.
            for &replacement in b"_Q0ZNF9" {
                let mut mangled = seed.as_bytes().to_vec();
                mangled[idx] = replacement;
                // All seeds are ASCII, so the mutation stays valid UTF-8.
                let mangled = String::from_utf8(mangled).expect("ascii mutation");
                let _ = Name::from(&mangled).demangle(DemangleOptions::complete());
                count += 1;
            }
            let _ = byte;
        }
        // Duplication (nested templates, repeated back references).
        let doubled = format!("{seed}{seed}");
        let _ = Name::from(&doubled).demangle(DemangleOptions::complete());
        // Repetition to stress recursion guards.
        for _ in 0..64 {
            let _ = Name::from(seed).demangle(DemangleOptions::complete());
        }
        count += 3;
    }
    assert!(count > 1000, "mutation loop ran {count} cases");
}

/// Pathological inputs aimed at the D grammar's recursive productions.
#[test]
fn dlang_pathological_inputs() {
    let opts = DemangleOptions::complete();
    let cases = [
        // Deeply nested dynamic arrays.
        format!("_D6module3fooF{}Zv", "A".repeat(10_000)),
        // Deeply nested pointers.
        format!("_D6module3fooF{}iZv", "P".repeat(10_000)),
        // Repeated anonymous symbol skips.
        format!("_D{}3fooZ", "0".repeat(10_000)),
        // Repeated template opens without closes.
        format!("_D6module{}", "__T4tempT".repeat(1_000)),
        // Back reference storms.
        format!("_D8demangle3abc{}Z", "Qi".repeat(1_000)),
        // Huge identifier lengths.
        "_D4294967295xZ".to_string(),
        "_D18446744073709551615xZ".to_string(),
        // Huge template argument counts.
        "_D6module8__T4tempA4294967295iZ4funcFZv".to_string(),
        // Huge string literal counts.
        "_D6module8__T4tempVa4294967295_61ZZ4funcFZv".to_string(),
    ];
    for case in &cases {
        let _ = Name::from(case).demangle(opts);
    }
}

/// Pathological inputs aimed at the Ada component walk.
#[test]
fn ada_pathological_inputs() {
    let cases = [
        // Deep component nesting.
        "a".to_string() + &"__a".repeat(5_000),
        // Deep nesting ending in a marker suffix.
        "a".to_string() + &"__a".repeat(5_000) + "Xn",
        // Long anonymous blocks.
        format!("module__B_{}", "1".repeat(10_000)),
        // Long escape runs.
        format!("module__{}x__proc", "U41".repeat(3_000)),
        // Truncated escapes.
        "module__U4".to_string(),
        "module__W00".to_string(),
        // Multiple body suffixes.
        "module__square__2__3__4".to_string(),
    ];
    for case in &cases {
        let _ = Name::from(case).demangle(DemangleOptions::complete());
    }
}

/// The explicit-request entry point accepts the same abuse without panicking.
#[test]
fn demangle_as_handles_mutations() {
    for &seed in SEEDS {
        for backend in ["fortran", "kotlin-native", "ada", "d"] {
            let _ = demangle_as(backend, seed, DemangleOptions::complete());
            let truncated = &seed[..seed.len() / 2];
            let _ = demangle_as(backend, truncated, DemangleOptions::complete());
        }
    }
}

use symbolic_common::Name;
