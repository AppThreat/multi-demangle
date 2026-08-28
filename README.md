# multi-demangle

[![CI](https://github.com/AppThreat/multi-demangle/actions/workflows/CI.yml/badge.svg)](https://github.com/AppThreat/multi-demangle/actions/workflows/CI.yml)

Demangling support for various languages and compilers, usable as a Rust crate or a
Python extension module. Fork of [symbolic-demangle](https://github.com/getsentry/symbolic/tree/10.2.1/symbolic-demangle).

Currently supported languages are:

| Language    | Mangling schemes / notes                                   | Cargo feature   |
| ----------- | ---------------------------------------------------------- | --------------- |
| C++         | Itanium ABI (GCC, Clang), GNU v2, CodeWarrior, and MSVC    | `cpp`, `gnuv2`, `codewarrior`, `msvc` |
| Rust        | Both `legacy` and `v0` schemes                             | `rust`          |
| Scala Native| Via the unknown-language fallback (symbols prefixed `_SM`) | `scala-native`  |
| Swift       | Up to Swift 6.3, using a vendored Swift demangler          | `swift`         |
| ObjC        | Symbol detection only (selectors are already readable)     | always on       |

All of the above features are enabled by default. Disabling them trims the
corresponding demangler (and, for `swift`, the vendored C++ sources) from the build.

As the demangling schemes for the languages are different, the supported demangling features are
inconsistent. For example, argument types were not encoded in legacy Rust mangling and thus not
available in demangled names.

## Rust usage

The crate exposes a `Demangle` trait on `symbolic_common::Name`, along with
`DemangleOptions` to control how verbose the output is:

```rust
use symbolic_common::{Language, Name};
use multi_demangle::{Demangle, DemangleOptions};

let name = Name::from("__ZN3std2io4Read11read_to_end17hb85a0f6802e14499E");

// Detect the language of a mangled symbol.
assert_eq!(name.detect_language(), Language::Rust);

// Demangle with a full, verbose signature.
assert_eq!(
    name.try_demangle(DemangleOptions::complete()),
    "std::io::Read::read_to_end"
);

// The shortcut free function demangles with complete options and
// falls back to the input if demangling fails.
assert_eq!(multi_demangle::demangle("_ZN3foo3barEv"), "foo::bar()");
```

### Symbol hygiene

On top of demangling, the crate provides cheap, prefix-based helpers for the
questions consumers face around demangling: is this symbol mangled, in which
language, and which linker decorations wrap it?

```rust
use multi_demangle::{
    classify_symbol, detect_language, looks_mangled, normalize_symbol, Decoration,
    SymbolStatus,
};

// Cheap mangling check; never attempts a demangling pass.
assert!(looks_mangled("_$s8mangling12GenericUnionO3FooyACyxGSicAEmlF"));
assert!(!looks_mangled("libc.so.6"));

// Language detection.
assert_eq!(detect_language("_ZN3foo3barEv"), symbolic_common::Language::Cpp);

// Classification without demangling.
let status = classify_symbol("__imp_?foo@bar@@YAXXZ");
assert_eq!(
    status,
    SymbolStatus::Decorated {
        decoration: Decoration::ImportPointer,
        inner: Box::new(SymbolStatus::Mangled(symbolic_common::Language::Cpp)),
    }
);

// Normalization: legacy Rust `$`-escapes, Rust hash suffixes, import pointer
// decoration, and pseudo-symbol mapping (see `Normalizer` for all passes).
assert_eq!(
    normalize_symbol("std::io::Read::read_to_end::hb85a0f6802e14499"),
    "std::io::Read::read_to_end"
);
assert_eq!(
    normalize_symbol("__imp__Z1fv"),
    "__declspec(dllimport) _Z1fv"
);
```

`DemangleOptions::complete().normalize(true)` applies the same default
hygiene passes to a symbol when demangling fails or does not apply; successful
demangled output is never modified.

## Python usage

Install the pypi package `multi-demangle`:

```
pip install multi-demangle
```

The module exposes `demangle_symbol` together with a `DemangleOptions` class:

```
>>> import multi_demangle
>>> print(multi_demangle.demangle_symbol("_ZN3foo3barEv"))
foo::bar()

>>> # name-only output, without parameters or return types
>>> opts = multi_demangle.DemangleOptions.name_only()
>>> print(multi_demangle.demangle_symbol("_ZN3foo3barEv", options=opts))
foo::bar

>>> # pick individual options via keyword arguments
>>> opts = multi_demangle.DemangleOptions(return_type=False, parameters=True)
>>> print(multi_demangle.demangle_symbol("__pl__FRC9CRelAngleRC9CRelAngle", options=opts))
operator+(CRelAngle const &, CRelAngle const &)
```

`demangle_symbol` returns the original string unchanged when the language cannot
be detected or demangling fails.

### Language detection and symbol hygiene

```
>>> multi_demangle.detect_language("_ZN3foo3barEv")
'cpp'
>>> multi_demangle.detect_language("libc.so.6") is None
True

>>> # cheap prefix-based check; never attempts a demangling pass
>>> multi_demangle.looks_mangled("_$s8mangling12GenericUnionO3FooyACyxGSicAEmlF")
True

>>> multi_demangle.normalize_symbol("std::io::Read::read_to_end::hb85a0f6802e14499")
'std::io::Read::read_to_end'

>>> info = multi_demangle.demangle_symbol_ex("__imp_?h@@YAXH@Z")
>>> info["status"], info["language"], info["decorations"]
('mangled', 'cpp', [{'kind': 'import-pointer'}])
```

`demangle_symbol_ex` returns a dict with `mangled`, `demangled`, `status`
(`"mangled"`, `"unmangled"`, or `"unsupported"`), `language`, and an
outermost-first `decorations` list. Passing
`multi_demangle.DemangleOptions(normalize=True)` applies the hygiene passes to
the fallback when demangling does not succeed.

## Development

Use `uv` package manager.

```
uv tool install maturin
maturin develop --all-features
```

Run the Rust test suite (includes the vendored Swift demangler build):

```
cargo test --all-features
```

Run the Python tests against the locally built module:

```
maturin develop --all-features
pytest python/tests
```

The Swift demangler is a minimal subset of the Swift standard library sources
vendored under `vendor/swift`; see [vendor/swift/README.md](vendor/swift/README.md)
for how it is maintained.

## License

MIT
