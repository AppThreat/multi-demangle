import pytest
import multi_demangle

swift_test_cases = [
    (
        "$S8mangling12any_protocolyyypF",
        "mangling.any_protocol(Any) -> ()",
        "any_protocol",
    ),
    (
        "$S8mangling12one_protocolyyAA3Foo_pF",
        "mangling.one_protocol(mangling.Foo) -> ()",
        "one_protocol",
    ),
    (
        "$S8mangling12GenericUnionO3FooyACyxGSicAEmlF",
        "mangling.GenericUnion.Foo<A>(mangling.GenericUnion<A>.Type) -> (Swift.Int) -> mangling.GenericUnion<A>",
        "GenericUnion.Foo<A>",
    ),
    (
        "$s8mangling12GenericUnionO3FooyACyxGSicAEmlF",
        "mangling.GenericUnion.Foo<A>(mangling.GenericUnion<A>.Type) -> (Swift.Int) -> mangling.GenericUnion<A>",
        "GenericUnion.Foo<A>",
    ),
    (
        "$s8mangling14varargsVsArray3arr1nySid_SStF",
        "mangling.varargsVsArray(arr: Swift.Int..., n: Swift.String) -> ()",
        "varargsVsArray",
    ),
    (
        "$s7example1fyyYaF",
        "example.f() async -> ()",
        "f"
    ),
    (
        "$s17distributed_thunk2DAC1fyyFTE",
        "distributed thunk distributed_thunk.DA.f() -> ()",
        "DA.f"
    ),
    (
        "$s4main20receiveInstantiationyySo34__CxxTemplateInst12MagicWrapperIiEVzF",
        "main.receiveInstantiation(inout __C.__CxxTemplateInst12MagicWrapperIiE) -> ()",
        "receiveInstantiation"
    ),
    (
        "$s4diff1hyyS2iYjlXEF",
        "diff.h(@differentiable(_linear) (Swift.Int) -> Swift.Int) -> ()",
        "h"
    )
]

rust_test_cases = [
    (
        "_ZN4core3ptr79drop_in_place$LT$alloc..vec..Vec$LT$wast..component..types..VariantCase$GT$$GT$17h41b828a7ca01b8c4E.llvm.12153207245666130899",
        "core::ptr::drop_in_place<alloc::vec::Vec<wast::component::types::VariantCase>>",
    ),
    (
        "_ZN5tokio7runtime4task7harness20Harness$LT$T$C$S$GT$8complete17h79b950493dfd179dE.llvm.3144946739014404372",
        "tokio::runtime::task::harness::Harness<T,S>::complete",
    ),
    (
        "core::ptr::drop_in_place<&core::option::Option<usize>>",
        "core::ptr::drop_in_place<&core::option::Option<usize>>",
    ),
    (
        "_ZN6anyhow5error31_$LT$impl$u20$anyhow..Error$GT$9construct17h41b87edbd45e0d86E.llvm.16823983138386609681",
        "anyhow::error::<impl anyhow::Error>::construct"
    ),
    (
        "_<alloc::string::String as core::ops::index::Index<core::ops::range::RangeFrom<usize>>>::index::h4be97e660083a1bb",
        "_<alloc::string::String as core::ops::index::Index<core::ops::range::RangeFrom<usize>>>::index::h4be97e660083a1bb"
    )
]

extra_demangler_cases = [
    (
        "do_thing__C6StupidRC6StupidT1",
        "Stupid::do_thing(Stupid const &, Stupid const &) const",
        "Stupid::do_thing(Stupid const &, Stupid const &) const",
        "Stupid::do_thing",
    ),
    (
        "_$_5tName",
        "tName::~tName(void)",
        "tName::~tName(void)",
        "tName::~tName",
    ),
    (
        "a_function__Q35silly8my_thing17another_namespacefffi",
        "silly::my_thing::another_namespace::a_function(float, float, float, int)",
        "silly::my_thing::another_namespace::a_function(float, float, float, int)",
        "silly::my_thing::another_namespace::a_function",
    ),
    (
        "Printf__7ConsolePce",
        "Console::Printf(char *, ...)",
        "Console::Printf(char *, ...)",
        "Console::Printf",
    ),
    (
        "_GLOBAL_$F$__default_terminate",
        "global frames keyed to __default_terminate",
        "__default_terminate",
        "__default_terminate",
    ),
    (
        "BuildLight__9CGuiLightCFv",
        "CGuiLight::BuildLight() const",
        "CGuiLight::BuildLight() const",
        "CGuiLight::BuildLight",
    ),
    (
        "__pl__FRC9CRelAngleRC9CRelAngle",
        "operator+(CRelAngle const &, CRelAngle const &)",
        "operator+(CRelAngle const &, CRelAngle const &)",
        "operator+",
    ),
    (
        "__dt__6CActorFv",
        "CActor::~CActor()",
        "CActor::~CActor()",
        "CActor::~CActor",
    ),
    (
        "BareFn__FPFPCcPv_v_v",
        "void BareFn(void (*)(const char*, void*))",
        "BareFn(void (*)(const char*, void*))",
        "BareFn",
    ),
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
]


@pytest.mark.parametrize("mangled, demangled_full, demangled_simple", swift_test_cases)
def test_demangle_swift_symbols_with_options(mangled, demangled_full, demangled_simple):
    """Tests Swift demangling with different DemangleOptions."""
    options_complete = multi_demangle.DemangleOptions.complete()
    assert multi_demangle.demangle_symbol(mangled, options=options_complete) == demangled_full
    assert multi_demangle.demangle_symbol(mangled) == demangled_full

    options_name_only = multi_demangle.DemangleOptions.name_only()
    assert multi_demangle.demangle_symbol(mangled, options=options_name_only) == demangled_simple


@pytest.mark.parametrize("mangled, expected_demangled", rust_test_cases)
def test_demangle_rust_symbols(mangled, expected_demangled):
    """Tests Rust demangling with default (complete) options."""
    assert multi_demangle.demangle_symbol(mangled) == expected_demangled


@pytest.mark.parametrize(
    "mangled, demangled_full, demangled_no_return, demangled_simple",
    extra_demangler_cases,
)
def test_demangle_additional_symbols(
    mangled, demangled_full, demangled_no_return, demangled_simple
):
    """Tests GNU v2, CodeWarrior, and Scala Native demangling with complete, no-return, and name-only options."""
    assert multi_demangle.demangle_symbol(mangled) == demangled_full
    assert multi_demangle.demangle_symbol(
        mangled, options=multi_demangle.DemangleOptions(return_type=False, parameters=True)
    ) == demangled_no_return
    assert multi_demangle.demangle_symbol(
        mangled, options=multi_demangle.DemangleOptions.name_only()
    ) == demangled_simple


def test_demangle_symbols():
    """Tests batch demangling: order preservation, dedup, and fallback parity."""
    symbols = [
        "_ZN3foo3barEv",
        "libc.so.6",
        "_Z1hic",
        "_ZN3foo3barEv",  # duplicate, interleaved
        "$s8mangling6curry1yyF",
        "_Z1hic",  # duplicate
        "memcpy@plt",
        "?h@@YAXH@Z",
    ]
    result = multi_demangle.demangle_symbols(symbols)
    assert len(result) == len(symbols)
    assert result == [multi_demangle.demangle_symbol(s) for s in symbols]
    assert result[0] == result[3] == "foo::bar()"
    assert result[1] == "libc.so.6"
    assert result[2] == result[5] == "h(int, char)"
    # Accepts any sequence of strings, not just lists.
    assert multi_demangle.demangle_symbols(tuple(symbols)) == result


def test_demangle_symbols_unique_false():
    """Tests that unique=False yields the same results without deduplication."""
    symbols = ["_Z1hic", "libc.so.6", "_Z1hic", "_Z1hic"]
    assert multi_demangle.demangle_symbols(symbols, unique=False) == [
        "h(int, char)",
        "libc.so.6",
        "h(int, char)",
        "h(int, char)",
    ]
    assert multi_demangle.demangle_symbols(symbols) == multi_demangle.demangle_symbols(
        symbols, unique=False
    )


def test_demangle_symbols_empty():
    """Tests the empty-input edge case."""
    assert multi_demangle.demangle_symbols([]) == []


def test_demangle_symbols_rejects_non_sequences():
    """Tests that a bare string is rejected instead of demangled per character."""
    with pytest.raises(TypeError):
        multi_demangle.demangle_symbols("_Z1hic")
    with pytest.raises(TypeError):
        multi_demangle.demangle_symbols([1, 2, 3])


def test_demangle_symbols_options():
    """Tests that DemangleOptions are honored for the whole batch."""
    symbols = ["_ZN3foo3barEv", "_Z1hic"]
    assert multi_demangle.demangle_symbols(
        symbols, options=multi_demangle.DemangleOptions.name_only()
    ) == ["foo::bar", "h"]
    assert multi_demangle.demangle_symbols(
        symbols, options=multi_demangle.DemangleOptions(return_type=False, parameters=True)
    ) == ["foo::bar()", "h(int, char)"]


def test_demangle_symbols_normalize_option():
    """Tests that the normalize fallback policy applies to the batch."""
    symbols = ["__imp_?h@@YAXH@Z", "std::io::Read::read_to_end::hb85a0f6802e14499"]
    assert multi_demangle.demangle_symbols(symbols) == [
        "__imp_?h@@YAXH@Z",
        "std::io::Read::read_to_end::hb85a0f6802e14499",
    ]
    options = multi_demangle.DemangleOptions(normalize=True)
    assert multi_demangle.demangle_symbols(symbols, options=options) == [
        "__declspec(dllimport) ?h@@YAXH@Z",
        "std::io::Read::read_to_end",
    ]


def test_demangle_symbols_duplicates_large():
    """Tests a duplicate-heavy batch the size of a small symbol table."""
    uniques = ["_Z1hic", "?h@@YAXH@Z", "libc.so.6", "main"]
    symbols = [uniques[i % len(uniques)] for i in range(10_000)]
    result = multi_demangle.demangle_symbols(symbols)
    assert len(result) == 10_000
    assert all(result[i] == multi_demangle.demangle_symbol(symbols[i]) for i in range(0, 10_000, 97))


def test_detect_language():
    """Tests language detection for raw symbols."""
    assert multi_demangle.detect_language("_Z1hic") == "cpp"
    assert multi_demangle.detect_language("?h@@YAXH@Z") == "cpp"
    assert multi_demangle.detect_language("_RNvNtCs1234_7mycrate3foo3bar") == "rust"
    assert multi_demangle.detect_language("$s8mangling6curry1yyF") == "swift"
    assert multi_demangle.detect_language("_SM17java.lang.IntegerD7compareiiiEo") == "scala-native"
    assert multi_demangle.detect_language("libc.so.6") is None
    assert multi_demangle.detect_language("hello") is None


def test_looks_mangled():
    """Tests the cheap prefix-based mangling check."""
    assert multi_demangle.looks_mangled("_ZN3foo3barEv") is True
    # Swift symbols, which blint's own prefix heuristic misses.
    assert multi_demangle.looks_mangled("_$s8mangling12GenericUnionO3FooyACyxGSicAEmlF") is True
    assert multi_demangle.looks_mangled("?h@@YAXH@Z") is True
    assert multi_demangle.looks_mangled("libc.so.6") is False
    assert multi_demangle.looks_mangled("hello") is False


def test_normalize_symbol():
    """Tests the display symbol hygiene passes."""
    assert (
        multi_demangle.normalize_symbol("alloc..vec..Vec$LT$u8$GT$")
        == "alloc::vec::Vec<u8>"
    )
    assert (
        multi_demangle.normalize_symbol("std::io::Read::read_to_end::hb85a0f6802e14499")
        == "std::io::Read::read_to_end"
    )
    assert (
        multi_demangle.normalize_symbol("__imp_?h@@YAXH@Z")
        == "__declspec(dllimport) ?h@@YAXH@Z"
    )
    assert multi_demangle.normalize_symbol("__imp_anon.1234") == "anonymous"
    assert multi_demangle.normalize_symbol("GCC_except_table12") == "GCC_except_table"
    assert multi_demangle.normalize_symbol("@feat.00") == "SAFESEH"
    # Generic rustc legacy escapes decode; ordinary dots are never rewritten.
    assert multi_demangle.normalize_symbol("$u5f$x") == "_x"
    assert multi_demangle.normalize_symbol("x...y") == "x...y"
    # Unmatched input is returned unchanged.
    assert multi_demangle.normalize_symbol("hello") == "hello"


def test_normalize_symbol_matching_normalizer():
    """Tests the matching-oriented passes against blint's normalize_call_target vectors."""
    matching = multi_demangle.Normalizer.matching()
    assert matching.normalize("memcpy@plt") == "memcpy"
    assert matching.normalize("memcpy@GLIBC_2.14") == "memcpy"
    assert matching.normalize("__imp_CreateFileW") == "CreateFileW"
    # Namespaced names are left intact.
    assert matching.normalize("APT::Container::begin") == "APT::Container::begin"
    # Display normalizer rewrites import pointers instead of stripping them.
    display = multi_demangle.Normalizer.display()
    assert display.normalize("__imp_CreateFileW") == "__declspec(dllimport) CreateFileW"


def test_classify_symbol():
    """Tests classification without demangling."""
    info = multi_demangle.classify_symbol("_Z1hic")
    assert info["status"] == "mangled"
    assert info["language"] == "cpp"
    assert info["decorations"] == []

    info = multi_demangle.classify_symbol("_Z1hic@GLIBC_2.2.5")
    assert info["status"] == "mangled"
    assert info["language"] == "cpp"
    assert info["decorations"] == [{"kind": "version", "value": "GLIBC_2.2.5"}]

    # `@`-containing names that are not ELF versions stay untouched.
    info = multi_demangle.classify_symbol("foo@bar")
    assert info["status"] == "unmangled"
    assert info["decorations"] == []


def test_demangle_symbol_ex():
    """Tests demangling with structured classification info."""
    info = multi_demangle.demangle_symbol_ex("_Z1hic")
    assert info["mangled"] == "_Z1hic"
    assert info["demangled"] == "h(int, char)"
    assert info["status"] == "mangled"
    assert info["language"] == "cpp"
    assert info["decorations"] == []

    # Decorated MSVC import: classified, with the raw fallback demangling.
    info = multi_demangle.demangle_symbol_ex("__imp_?h@@YAXH@Z")
    assert info["status"] == "mangled"
    assert info["language"] == "cpp"
    assert info["decorations"] == [{"kind": "import-pointer"}]
    assert info["demangled"] == "__imp_?h@@YAXH@Z"

    # With normalize=True the fallback goes through the hygiene passes.
    options = multi_demangle.DemangleOptions(normalize=True)
    info = multi_demangle.demangle_symbol_ex("__imp_?h@@YAXH@Z", options=options)
    assert info["demangled"] == "__declspec(dllimport) ?h@@YAXH@Z"

    # Versioned library names are unmangled with a version decoration.
    info = multi_demangle.demangle_symbol_ex("libc.so.6@@GLIBC_2.2.5")
    assert info["status"] == "unmangled"
    assert info["language"] is None
    assert info["decorations"] == [{"kind": "version", "value": "GLIBC_2.2.5"}]
