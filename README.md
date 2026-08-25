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
