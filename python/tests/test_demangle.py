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
