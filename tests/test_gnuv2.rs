//! GNU v2 demangler integration tests derived from upstream crate docs.

use multi_demangle::{Demangle, DemangleOptions};
use similar_asserts::assert_eq;
use symbolic_common::{Language, Name, NameMangling};

fn assert_demangle_variants(symbol: &str, full: &str, no_return: &str, name_only: &str) {
    let name = Name::new(symbol, NameMangling::Unknown, Language::Cpp);
    assert_eq!(
        name.demangle(DemangleOptions::complete()),
        Some(full.to_string())
    );
    assert_eq!(
        name.demangle(DemangleOptions::complete().return_type(false)),
        Some(no_return.to_string())
    );
    assert_eq!(
        name.demangle(DemangleOptions::name_only()),
        Some(name_only.to_string())
    );
}

#[test]
fn test_demangle_gnuv2_symbols() {
    for (symbol, full, no_return, name_only) in [
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
    ] {
        assert_demangle_variants(symbol, full, no_return, name_only);
    }
}
