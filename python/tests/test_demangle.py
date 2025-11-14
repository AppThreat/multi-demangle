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
