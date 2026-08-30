//! Structured demangling tests. The Rust expectations port the primary
//! consumer's (OWASP blint) `callgraph/canon.py` test oracle — hash capture,
//! `<Type as Trait>` reduction, closure/glue/intrinsic classification — and
//! extend it to every supported language.

#![cfg(all(
    feature = "cpp",
    feature = "rust",
    feature = "swift",
    feature = "scala-native"
))]
#![cfg(all(
    feature = "dlang",
    feature = "fortran",
    feature = "ada",
    feature = "kotlin-native"
))]

use multi_demangle::{Demangle, DemangleOptions, DemangledKind};
use similar_asserts::assert_eq;
use symbolic_common::Name;

fn structured(symbol: &str) -> multi_demangle::DemangledInfo {
    Name::from(symbol)
        .demangle_structured(DemangleOptions::complete())
        .expect("structured demangling succeeds")
}

// --- Rust: ported from blint's tests/test_callgraph_canon.py oracle ---

#[test]
fn rust_legacy_symbol_fields() {
    let info = structured("_ZN3std2io4Read11read_to_end17hb85a0f6802e14499E");
    assert_eq!(info.language, symbolic_common::Language::Rust);
    assert_eq!(info.namespace, ["std", "io", "Read"]);
    assert_eq!(info.name, "read_to_end");
    assert_eq!(info.kind, DemangledKind::Method);
    assert_eq!(info.hash.as_deref(), Some("hb85a0f6802e14499"));
    // Legacy Rust mangling does not encode parameter types.
    assert_eq!(info.parameters, None);
    assert!(!info.is_generic);
}

#[test]
fn rust_trait_qualified_self_is_reduced() {
    // <core::iter::adapters::map::Map<I,F> as core::iter::traits::iterator::Iterator>::next
    let info = structured("__ZN102_$LT$core..iter..adapters..map..Map$LT$I$C$F$GT$$u20$as$u20$core..iter..traits..iterator..Iterator$GT$4next17h588c4c3ad8f9f79aE");
    // The implementing type is what survives in the namespace, mirroring
    // `<SubType as core::fmt::Debug>::fmt` -> `SubType::fmt` in the oracle.
    assert_eq!(
        info.namespace,
        ["core", "iter", "adapters", "map", "Map<I,F>"]
    );
    assert_eq!(info.name, "next");
    assert_eq!(info.kind, DemangledKind::Method);
    assert_eq!(info.hash.as_deref(), Some("h588c4c3ad8f9f79a"));
    assert_eq!(
        info.template_args,
        Some(vec!["I".to_string(), "F".to_string(),])
    );
    assert!(info.is_generic);
}

#[test]
fn rust_hash_and_llvm_counter_captured() {
    // The oracle input "core::ptr::drop_in_place<alloc::vec::Vec<u8>>::
    // h41b828a7ca01b8c4.llvm.12153207245666130899" in mangled form.
    let info = structured("_ZN5tokio7runtime4task7harness20Harness$LT$T$C$S$GT$8complete17h79b950493dfd179dE.llvm.3144946739014404372");
    assert_eq!(
        info.display,
        "tokio::runtime::task::harness::Harness<T,S>::complete"
    );
    assert_eq!(info.name, "complete");
    assert_eq!(
        info.hash.as_deref(),
        Some("h79b950493dfd179d.llvm.3144946739014404372")
    );
    assert_eq!(
        info.template_args,
        Some(vec!["T".to_string(), "S".to_string(),])
    );
    assert!(info.is_generic);
    assert_eq!(info.kind, DemangledKind::Method);
}

#[test]
fn rust_impl_for_block_is_kept_and_named() {
    let info = structured("__ZN10hyper_util6client6legacy7connect4http112_$LT$impl$u20$hyper_util..client..legacy..connect..Connection$u20$for$u20$tokio..net..tcp..stream..TcpStream$GT$9connected17h5b8a4dc4058a3652E");
    // A mid-path impl block stays in the namespace as rendered (reduction
    // applies to the leading qualified-self prefix), and its contents are
    // not template arguments.
    assert_eq!(info.name, "connected");
    assert_eq!(info.namespace.last().map(String::as_str), Some(
        "<impl hyper_util::client::legacy::connect::Connection for tokio::net::tcp::stream::TcpStream>"
    ));
    assert_eq!(info.hash.as_deref(), Some("h5b8a4dc4058a3652"));
    assert_eq!(info.template_args, None);
    assert!(!info.is_generic);
}

#[test]
fn rust_crt_glue_and_closures() {
    let info = structured("__RNvCsdBezzDwma51_7___rustc12___rust_alloc");
    assert_eq!(info.kind, DemangledKind::Glue);
    assert_eq!(info.name, "__rust_alloc");

    let closure = structured("_ZN10wasm_smith4core15closure_2202_7717h0123456789abcdefE");
    assert_eq!(closure.kind, DemangledKind::Closure);

    let drop_glue = structured("_ZN4core3ptr13drop_in_place17h588c4c3ad8f9f79aE");
    assert_eq!(drop_glue.kind, DemangledKind::Glue);
    assert_eq!(drop_glue.name, "drop_in_place");
}

#[test]
fn rust_v0_symbol_fields() {
    let info = structured("__RNvCsdBezzDwma51_7___rustc10rust_panic");
    assert_eq!(info.namespace, ["__rustc"]);
    assert_eq!(info.name, "rust_panic");
}

// --- C++ ---

#[test]
fn cpp_function_fields() {
    let info = structured("_ZN3foo3barEv");
    assert_eq!(info.language, symbolic_common::Language::Cpp);
    assert_eq!(info.namespace, ["foo"]);
    assert_eq!(info.name, "bar");
    assert_eq!(info.kind, DemangledKind::Function);
    assert_eq!(info.parameters, Some(Vec::new()));
    assert_eq!(info.return_type, None);
    assert_eq!(info.simple, "foo::bar");
}

#[test]
fn cpp_msvc_fields() {
    let info = structured("?h@@YAXH@Z");
    assert_eq!(info.display, "void h(int)");
    assert_eq!(info.name, "h");
    assert_eq!(info.parameters, Some(vec!["int".to_string()]));
    assert_eq!(info.return_type.as_deref(), Some("void"));
}

#[test]
fn cpp_template_function() {
    let info = structured("_Z3addIiET_S0_S0_");
    assert_eq!(info.display, "int add<int>(int, int)");
    assert_eq!(info.name, "add");
    assert_eq!(info.template_args, Some(vec!["int".to_string()]));
    assert_eq!(
        info.parameters,
        Some(vec!["int".to_string(), "int".to_string(),])
    );
    assert_eq!(info.return_type.as_deref(), Some("int"));
    assert!(info.is_generic);
    assert_eq!(info.kind, DemangledKind::Function);
}

#[test]
fn cpp_template_arg_with_function_type() {
    // The call operator of llvm::function_ref<void ()>: the ( inside the
    // template argument must not be taken for the signature.
    let info = structured("_ZNK4llvm12function_refIFvvEEclEv");
    assert_eq!(
        info.display,
        "llvm::function_ref<void ()>::operator()() const"
    );
    assert_eq!(info.namespace, ["llvm", "function_ref<void ()>"]);
    assert_eq!(info.name, "operator()");
    assert_eq!(info.kind, DemangledKind::Method);
    assert_eq!(info.parameters, Some(Vec::new()));
    assert_eq!(info.return_type, None);
}

#[test]
fn cpp_shift_and_comparison_operators() {
    // Regression guard: the angle-depth scanner must treat operator tokens
    // (`<<`, `<`) as name text, not brackets — `operator<<` carries two
    // unmatched `<` that would otherwise poison the depth count.
    let shift = structured("_ZN4llvm5APIntlsEi");
    assert_eq!(shift.display, "llvm::APInt::operator<<(int)");
    assert_eq!(shift.namespace, ["llvm", "APInt"]);
    assert_eq!(shift.name, "operator<<");
    assert_eq!(shift.parameters, Some(vec!["int".to_string()]));
    assert_eq!(shift.kind, DemangledKind::Method);

    // A template comparison operator: the operator's own `<` and the
    // template group are distinct, and the leaf stays bare.
    let template_lt = structured("_ZN3fooltIiEEbT_");
    assert_eq!(template_lt.display, "bool foo::operator< <int>(int)");
    assert_eq!(template_lt.namespace, ["foo"]);
    assert_eq!(template_lt.name, "operator<");
    assert_eq!(template_lt.parameters, Some(vec!["int".to_string()]));
    assert_eq!(template_lt.return_type.as_deref(), Some("bool"));
    assert_eq!(template_lt.template_args, Some(vec!["int".to_string()]));
}

#[test]
fn cpp_anonymous_namespace_components() {
    // `(anonymous namespace)` is one path component, not a parameter list
    // and not a space-separated return type: the spaces inside it are not
    // top level, and the parenthesized group is not a signature.
    let anon = structured("__ZN12_GLOBAL__N_113compEnumNamesIhEEbRKN4llvm9EnumEntryIT_EES6_");
    assert_eq!(anon.namespace, ["(anonymous namespace)"]);
    assert_eq!(anon.name, "compEnumNames");
    assert_eq!(anon.return_type.as_deref(), Some("bool"));
    assert_eq!(anon.template_args, Some(vec!["unsigned char".to_string()]));

    // Nested anonymous namespaces: a candidate name ending in `::` is a
    // path prefix, so neither group is mistaken for the parameter list.
    let nested = structured(
        "__ZN12_GLOBAL__N_112_GLOBAL__N_119ProtocolMethodLists3getEPKN5clang16ObjCProtocolDeclE",
    );
    assert_eq!(
        nested.namespace,
        [
            "(anonymous namespace)",
            "(anonymous namespace)",
            "ProtocolMethodLists"
        ]
    );
    assert_eq!(nested.name, "get");
    assert_eq!(nested.kind, DemangledKind::Method);
    assert_eq!(
        nested.parameters,
        Some(vec!["clang::ObjCProtocolDecl const*".to_string()])
    );
}

#[test]
fn cpp_free_operators_are_not_methods() {
    // A free operator in a namespace: the owner is not a type, so the
    // owner-shape predicate must not fire on the leaf's "operator" prefix.
    let info = structured("_ZN3ns2eqERKNS_1AES2_");
    assert_eq!(info.namespace, ["ns2"]);
    assert_eq!(info.name, "operator==");
    assert_eq!(info.kind, DemangledKind::Function);
}

#[test]
fn cpp_lowercase_typed_methods() {
    // Template-shaped owners are types by construction, so methods on
    // lowercase-typed classes (the entire std:: surface) are methods even
    // though the CamelCase heuristic cannot see it.
    let info = structured("_ZNSt6vectorIiSaIiEE9push_backERKi");
    assert_eq!(info.namespace, ["std", "vector<int, std::allocator<int> >"]);
    assert_eq!(info.name, "push_back");
    assert_eq!(info.kind, DemangledKind::Method);
}

#[test]
fn cpp_constructor_and_method() {
    let ctor = structured("_ZN3FooC1Ev");
    assert_eq!(ctor.name, "Foo");
    assert_eq!(ctor.namespace, ["Foo"]);
    assert_eq!(ctor.kind, DemangledKind::Method);

    let getter = structured("_ZNK3Foo3getEv");
    assert_eq!(getter.name, "get");
    assert_eq!(getter.namespace, ["Foo"]);
    assert_eq!(getter.kind, DemangledKind::Method);
    assert_eq!(getter.parameters, Some(Vec::new()));
}

#[test]
fn cpp_vtable_typeinfo_and_thunks() {
    let vtable = structured("_ZTVN10__cxxabiv117__class_type_infoE");
    assert_eq!(vtable.kind, DemangledKind::VirtualTable);
    assert_eq!(vtable.namespace, ["__cxxabiv1"]);
    assert_eq!(vtable.name, "__class_type_info");

    let thunk = structured("_ZThn8_N3foo3barEv");
    assert_eq!(thunk.kind, DemangledKind::MethodThunk);
    // Known limitation: without a reachable Itanium AST the thunk's target
    // identity is not extracted — the name stays the full brace rendering
    // ({virtual override thunk(...)}), only the kind is classified.
    assert_eq!(
        thunk.name,
        "{virtual override thunk({offset(-8)}, foo::bar())}"
    );

    // Typeinfo symbols do not demangle in this cpp_demangle version, so the
    // structured API declines them just like `demangle` does.
    assert_eq!(
        Name::from("_ZTI4Foo").demangle_structured(DemangleOptions::complete()),
        None
    );
}

#[test]
fn cpp_builtin_is_intrinsic() {
    let info = structured("_Z14__builtin_testv");
    assert_eq!(info.name, "__builtin_test");
    assert_eq!(info.kind, DemangledKind::Intrinsic);
}

// --- Swift ---

#[test]
fn swift_function_fields() {
    let info = structured("$s8mangling6curry1yyF");
    assert_eq!(info.language, symbolic_common::Language::Swift);
    assert_eq!(info.namespace, ["mangling"]);
    assert_eq!(info.name, "curry1");
    assert_eq!(info.kind, DemangledKind::Function);
    assert_eq!(info.parameters, Some(Vec::new()));
    assert_eq!(info.return_type.as_deref(), Some("()"));
}

#[test]
fn swift_generic_method_fields() {
    let info = structured("$s8mangling12GenericUnionO3FooyACyxGSicAEmlF");
    assert_eq!(info.display, "mangling.GenericUnion.Foo<A>(mangling.GenericUnion<A>.Type) -> (Swift.Int) -> mangling.GenericUnion<A>");
    assert_eq!(info.simple, "GenericUnion.Foo<A>(_:)");
    // Swift module paths split on `.`, with generic groups kept together.
    assert_eq!(info.namespace, ["mangling", "GenericUnion"]);
    assert_eq!(info.name, "Foo");
    assert_eq!(info.template_args, Some(vec!["A".to_string()]));
    assert!(info.is_generic);
    assert_eq!(info.kind, DemangledKind::Method);
    assert_eq!(
        info.parameters,
        Some(vec!["mangling.GenericUnion<A>.Type".to_string()])
    );
    assert_eq!(
        info.return_type.as_deref(),
        Some("(Swift.Int) -> mangling.GenericUnion<A>")
    );
}

// --- Scala Native ---

#[test]
fn scala_native_method_fields() {
    let info = structured("_SM17java.lang.IntegerD7compareiiiEo");
    assert_eq!(info.namespace, ["java", "lang", "Integer"]);
    assert_eq!(info.name, "compare");
    assert_eq!(info.kind, DemangledKind::Method);
    assert_eq!(
        info.parameters,
        Some(vec!["Int".to_string(), "Int".to_string(),])
    );
    assert_eq!(info.return_type.as_deref(), Some("Int"));
}

// --- Phase 2: AST-backed / mangling-grammar fidelity ---

#[test]
fn swift_accessor_kind_from_node_dump() {
    let getter = structured("$s8mangling24InstanceAndClassPropertyV8propertySivg");
    assert_eq!(getter.kind, DemangledKind::Method);
    assert_eq!(getter.namespace, ["mangling", "InstanceAndClassProperty"]);
    assert_eq!(getter.name, "property");

    let static_getter = structured("$s8mangling24InstanceAndClassPropertyV8propertySivgZ");
    assert_eq!(static_getter.kind, DemangledKind::Method);
    assert_eq!(static_getter.name, "property");
}

#[test]
fn swift_closure_kind_from_node_dump() {
    let closure = structured("$s8mangling10HasVarInitV5stateSbvpZfiSbyKXKfu_");
    assert_eq!(closure.kind, DemangledKind::Closure);
    assert_eq!(closure.name, "implicit closure #1 : @autoclosure");
}

#[test]
fn swift_method_kind_from_node_dump() {
    // The demangler's tree has the enum context as a sibling of the
    // function's identifier; the walker must classify Foo as a method.
    let info = structured("$s8mangling12GenericUnionO3FooyACyxGSicAEmlF");
    assert_eq!(info.kind, DemangledKind::Method);
    assert_eq!(info.namespace, ["mangling", "GenericUnion"]);
    assert_eq!(info.name, "Foo");
    // Signature fields still come from the rendering.
    assert_eq!(info.template_args, Some(vec!["A".to_string()]));
}

#[test]
fn swift_metadata_descriptor_is_other_kind() {
    // Real iOS-corpus symbol: associated type descriptors are metadata
    // artifacts, and the node dump keeps them out of the function/method
    // heuristics that the rendered text would suggest.
    let descriptor = structured("_$s11MaskStorages4SIMDPTl");
    assert_eq!(
        descriptor.kind,
        DemangledKind::Other("AssociatedTypeDescriptor".to_string())
    );
    assert_eq!(descriptor.name, "MaskStorage");
}

#[test]
fn msvc_static_variable_vs_function() {
    // A data symbol has no parameter list; only the parse tree knows it is
    // a variable, not a nullary function.
    let value = structured("?value@ns@@3HA");
    assert_eq!(value.kind, DemangledKind::StaticVariable);
    assert_eq!(value.namespace, ["ns"]);
    assert_eq!(value.name, "value");
    assert_eq!(value.parameters, None);
    assert_eq!(value.display, "int ns::value");
}

#[test]
fn msvc_vftable_and_rtti_kinds() {
    let vftable = structured("??_7Bar@@6B@");
    assert_eq!(vftable.kind, DemangledKind::VirtualTable);
    assert_eq!(vftable.namespace, ["Bar"]);
    assert_eq!(vftable.name, "`vftable'");

    let rtti = structured("??_R0?AVBar@@@8");
    assert_eq!(rtti.kind, DemangledKind::TypeInfo);
    // The qualified type moves to the namespace; the leaf is the descriptor.
    assert_eq!(rtti.namespace, ["Bar"]);
    assert_eq!(rtti.name, "`RTTI Type Descriptor'");
}

#[test]
fn msvc_ctor_dtor_identity() {
    let ctor = structured("??0Bar@@QEAA@XZ");
    assert_eq!(ctor.kind, DemangledKind::Method);
    assert_eq!(ctor.namespace, ["Bar"]);
    assert_eq!(ctor.name, "Bar");
    // Access specifiers are not return types, and a lone `void` parameter
    // list normalizes to the same empty shape Itanium produces.
    assert_eq!(ctor.return_type, None);
    assert_eq!(ctor.parameters, Some(Vec::new()));

    let dtor = structured("??1Bar@@QEAA@XZ");
    assert_eq!(dtor.kind, DemangledKind::Method);
    assert_eq!(dtor.namespace, ["Bar"]);
    assert_eq!(dtor.name, "~Bar");
    assert_eq!(dtor.return_type, None);
}

#[test]
fn msvc_thunk_signature_split() {
    // The entity name contains spaces; the AST-guided split keeps the
    // return type clean instead of truncating at the last space.
    let thunk = structured("??_EBar@@W3EAAXXZ");
    assert_eq!(thunk.kind, DemangledKind::MethodThunk);
    assert_eq!(thunk.namespace, ["Bar"]);
    assert_eq!(thunk.return_type.as_deref(), Some("void"));
    assert_eq!(thunk.parameters, Some(Vec::new()));
}

#[test]
fn msvc_template_member() {
    let info = structured("?fn@?$Vec@H@@QEAAXXZ");
    assert_eq!(info.kind, DemangledKind::Method);
    assert_eq!(info.namespace, ["Vec<int>"]);
    assert_eq!(info.name, "fn");
    // The scope template sets is_generic, and the rendered path keeps the
    // argument rendering in template_args.
    assert!(info.is_generic);
    assert_eq!(info.template_args, Some(vec!["int".to_string()]));
    assert_eq!(info.return_type.as_deref(), Some("void"));
}

#[test]
fn itanium_guard_variable_kind() {
    let guard = structured("_ZGVZN3foo3barEvE5mutex");
    assert_eq!(guard.kind, DemangledKind::StaticVariable);
    assert_eq!(guard.namespace, ["foo", "bar()"]);
    assert_eq!(guard.name, "mutex");
    // The enclosing function's parameter list is not this entity's.
    assert_eq!(guard.parameters, None);
}

// --- D ---

#[test]
fn dlang_member_function_fields() {
    let info = structured("_D6module4Test6methodMFiZi");
    assert_eq!(info.language, symbolic_common::Language::D);
    assert_eq!(info.namespace, ["module", "Test"]);
    assert_eq!(info.name, "method");
    assert_eq!(info.kind, DemangledKind::Method);
    assert_eq!(info.parameters, Some(vec!["int".to_string()]));
    assert_eq!(info.return_type, None);
}

#[test]
fn dlang_variable_fields() {
    let info = structured("_D6module7counteri");
    assert_eq!(info.namespace, ["module"]);
    assert_eq!(info.name, "counter");
    assert_eq!(info.kind, DemangledKind::StaticVariable);
    // The variable type renders in display but is not a return type.
    assert_eq!(info.display, "int module.counter");
    assert_eq!(info.return_type, None);
}

#[test]
fn dlang_template_fields() {
    let info = structured("_D6module13__T4tempTiTkZ4funcFZv");
    assert_eq!(info.namespace, ["module", "temp!(int, uint)"]);
    assert_eq!(info.name, "func");
    assert!(info.is_generic);
    // The template lives in the path, and its arguments are captured.
    assert_eq!(
        info.template_args,
        Some(vec!["int".to_string(), "uint".to_string()])
    );
}

#[test]
fn dlang_magic_kinds() {
    let info = structured("_D8demangle4test6__vtblZ");
    assert_eq!(info.kind, DemangledKind::VirtualTable);
    let info = structured("_D8demangle4test7__ClassZ");
    assert_eq!(info.kind, DemangledKind::TypeInfo);
    let info = structured("_D8demangle4test6__initZ");
    assert_eq!(info.kind, DemangledKind::Function);
}

// --- Fortran ---

#[test]
fn fortran_has_no_structured_view() {
    // As with Ada: explicit-request-only, and a structured view has no
    // language parameter to request it through.
    assert!(Name::from("__my_module_MOD_my_proc")
        .demangle_structured(DemangleOptions::complete())
        .is_none());
}

// --- Kotlin/Native ---

#[test]
fn kotlin_native_fields() {
    let info = structured("_kfun:com.example.Foo.bar(kotlin.String;kotlin.Int)");
    assert_eq!(info.namespace, ["com", "example", "Foo"]);
    assert_eq!(info.name, "bar");
    assert_eq!(info.kind, DemangledKind::Method);
    assert_eq!(
        info.parameters,
        Some(vec!["String".to_string(), "Int".to_string()])
    );
}

// --- Ada ---

#[test]
fn ada_has_no_structured_view() {
    // Ada is explicit-request-only and a structured view has no language
    // parameter to request it through; `demangle_as` returns the full name.
    assert!(Name::from("ada__exceptions__last_chance_handlerXn")
        .demangle_structured(DemangleOptions::complete())
        .is_none());
}

// --- ObjC metadata ---

#[test]
fn objc_selectors() {
    let instance = structured("-[Foo bar:blub:]");
    assert_eq!(instance.language, symbolic_common::Language::ObjC);
    assert_eq!(instance.namespace, ["Foo"]);
    assert_eq!(instance.name, "bar:blub:");
    assert_eq!(
        instance.kind,
        DemangledKind::ObjCMethod {
            class_method: false
        }
    );

    let class_method = structured("+[Foo bar:]");
    assert_eq!(
        class_method.kind,
        DemangledKind::ObjCMethod { class_method: true }
    );
    assert_eq!(class_method.name, "bar:");
}

// --- ObjC runtime metadata symbols ---

#[test]
fn objc_metadata_kinds() {
    let class = structured("_OBJC_CLASS_$_MyViewController");
    assert_eq!(class.language, symbolic_common::Language::ObjC);
    assert_eq!(class.name, "MyViewController");
    assert_eq!(class.kind, DemangledKind::ObjCClass);

    let metaclass = structured("_OBJC_METACLASS_$_MyViewController");
    assert_eq!(metaclass.kind, DemangledKind::ObjCMetaclass);

    let ivar = structured("_OBJC_IVAR_$_MyObject._count");
    assert_eq!(ivar.namespace, ["MyObject"]);
    assert_eq!(ivar.name, "_count");
    assert_eq!(ivar.kind, DemangledKind::ObjCIvar);

    // Emitted selector references are compiler glue.
    let selector = structured("l_OBJC_SELECTOR_REFERENCES_12");
    assert_eq!(selector.kind, DemangledKind::Glue);
    let selector = structured("OBJC_SELECTOR_REFERENCES_34");
    assert_eq!(selector.kind, DemangledKind::Glue);
}

// --- Contracts shared with the string API ---

#[test]
fn unmangled_and_md5_inputs_are_none() {
    assert_eq!(
        Name::from("libc.so.6").demangle_structured(DemangleOptions::complete()),
        None
    );
    assert_eq!(
        Name::from("??@8ba8d245c9eca390356129098dbe9f73@")
            .demangle_structured(DemangleOptions::complete()),
        None
    );
}

#[test]
fn simple_is_always_name_only() {
    let info = structured("_SM17java.lang.IntegerD7compareiiiEo");
    assert_eq!(info.simple, "java.lang.Integer.compare");
    assert_eq!(info.display, "java.lang.Integer.compare(Int,Int): Int");

    let name_only = Name::from("_SM17java.lang.IntegerD7compareiiiEo")
        .demangle_structured(DemangleOptions::name_only())
        .unwrap();
    // With name-only options the display itself is the simple rendering.
    assert_eq!(name_only.display, name_only.simple);
}
