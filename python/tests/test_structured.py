# -*- coding: utf-8 -*-
"""Structured demangling tests: typed fields across all supported languages.

The Rust-side expectations port the canonicalization oracle from
``blint/lib/callgraph/canon.py``; these tests pin the same behavior through
the Python API, using ``to_dict()`` as the JSON snapshot.
"""

import pytest

import multi_demangle


def _dict(symbol: str) -> dict:
    info = multi_demangle.demangle_symbol_structured(symbol)
    assert info is not None, symbol
    return info.to_dict()


def test_legacy_rust_symbol_fields():
    d = _dict("_ZN3std2io4Read11read_to_end17hb85a0f6802e14499E")
    assert d["language"] == "rust"
    assert d["namespace"] == ["std", "io", "Read"]
    assert d["name"] == "read_to_end"
    assert d["kind"] == "method"
    assert d["hash"] == "hb85a0f6802e14499"
    # Legacy Rust mangling does not encode parameter types.
    assert d["parameters"] is None
    assert d["is_generic"] is False
    assert d["demangled"] == "std::io::Read::read_to_end"


def test_trait_qualified_self_reduction():
    d = _dict(
        "__ZN102_$LT$core..iter..adapters..map..Map$LT$I$C$F$GT$$u20$as$u20$"
        "core..iter..traits..iterator..Iterator$GT$4next17h588c4c3ad8f9f79aE"
    )
    assert d["namespace"][-1] == "Map<I,F>"
    assert d["name"] == "next"
    assert d["kind"] == "method"
    assert d["template_args"] == ["I", "F"]
    assert d["is_generic"] is True


def test_hash_and_llvm_counter():
    d = _dict(
        "_ZN5tokio7runtime4task7harness20Harness$LT$T$C$S$GT$8complete"
        "17h79b950493dfd179dE.llvm.3144946739014404372"
    )
    assert d["hash"] == "h79b950493dfd179d.llvm.3144946739014404372"
    assert d["template_args"] == ["T", "S"]
    assert d["name"] == "complete"


def test_cpp_function():
    d = _dict("_ZN3foo3barEv")
    assert d["language"] == "cpp"
    assert d["namespace"] == ["foo"]
    assert d["name"] == "bar"
    assert d["kind"] == "function"
    assert d["parameters"] == []
    assert d["return_type"] is None


def test_cpp_template_function():
    d = _dict("_Z3addIiET_S0_S0_")
    assert d["name"] == "add"
    assert d["template_args"] == ["int"]
    assert d["parameters"] == ["int", "int"]
    assert d["return_type"] == "int"
    assert d["is_generic"] is True


def test_cpp_vtable_kind():
    d = _dict("_ZTVN10__cxxabiv117__class_type_infoE")
    assert d["kind"] == "virtual_table"
    assert d["namespace"] == ["__cxxabiv1"]
    assert d["name"] == "__class_type_info"


def test_swift_generic_method():
    d = _dict("$s8mangling12GenericUnionO3FooyACyxGSicAEmlF")
    assert d["language"] == "swift"
    assert d["namespace"] == ["mangling", "GenericUnion"]
    assert d["name"] == "Foo"
    assert d["kind"] == "method"
    assert d["template_args"] == ["A"]
    assert d["parameters"] == ["mangling.GenericUnion<A>.Type"]
    assert d["return_type"] == "(Swift.Int) -> mangling.GenericUnion<A>"


def test_scala_native_method():
    d = _dict("_SM17java.lang.IntegerD7compareiiiEo")
    assert d["language"] == "scala-native"
    assert d["namespace"] == ["java", "lang", "Integer"]
    assert d["name"] == "compare"
    assert d["kind"] == "method"
    assert d["parameters"] == ["Int", "Int"]
    assert d["return_type"] == "Int"


def test_objc_selectors():
    instance = _dict("-[Foo bar:blub:]")
    assert instance["language"] == "objc"
    assert instance["namespace"] == ["Foo"]
    assert instance["name"] == "bar:blub:"
    assert instance["kind"] == "objc_method"
    assert instance["class_method"] is False

    class_method = _dict("+[Foo bar:]")
    assert class_method["class_method"] is True


def test_phase2_msvc_static_variable():
    d = _dict("?value@ns@@3HA")
    assert d["kind"] == "static_variable"
    assert d["namespace"] == ["ns"]
    assert d["name"] == "value"
    assert d["parameters"] is None


def test_phase2_swift_getter_kind():
    d = _dict("$s8mangling24InstanceAndClassPropertyV8propertySivg")
    assert d["kind"] == "method"
    assert d["namespace"] == ["mangling", "InstanceAndClassProperty"]
    assert d["name"] == "property"


def test_phase2_swift_closure_kind():
    d = _dict("$s8mangling10HasVarInitV5stateSbvpZfiSbyKXKfu_")
    assert d["kind"] == "closure"


def test_phase2_itanium_guard_variable():
    d = _dict("_ZGVZN3foo3barEvE5mutex")
    assert d["kind"] == "static_variable"
    assert d["name"] == "mutex"


def test_unmangled_input_returns_none():
    assert multi_demangle.demangle_symbol_structured("libc.so.6") is None


def test_getters_match_to_dict():
    info = multi_demangle.demangle_symbol_structured("_ZN3std2io4Read11read_to_end17hb85a0f6802e14499E")
    assert info is not None
    d = info.to_dict()
    assert info.language == d["language"]
    assert info.display == d["demangled"]
    assert info.simple == d["simple"]
    assert info.namespace == d["namespace"]
    assert info.name == d["name"]
    assert info.kind == d["kind"]
    assert info.hash == d["hash"]
    assert info.is_generic == d["is_generic"]
    assert info.mangled == d["mangled"]


def test_options_argument_is_honored():
    name_only = multi_demangle.demangle_symbol_structured(
        "_SM17java.lang.IntegerD7compareiiiEo",
        options=multi_demangle.DemangleOptions.name_only(),
    )
    assert name_only is not None
    assert name_only.display == "java.lang.Integer.compare"
    assert name_only.display == name_only.simple


if __name__ == "__main__":
    pytest.main([__file__])
