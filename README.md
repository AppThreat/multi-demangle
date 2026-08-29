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

The `cli` cargo feature (on by default) pulls in the argument parser for the
binary. Library-only consumers can drop it by re-enabling the backends
explicitly:

```toml
multi-demangle = { version = "...", default-features = false, features = [
  "cpp", "gnuv2", "codewarrior", "msvc", "rust", "scala-native", "swift",
] }
```

### Batch demangling

Symbol tables repeat the same symbol many times (dynsym, symtab, version
tables, and GOT/PLT maps), so the batch API demangles each distinct symbol at
most once and preserves input order:

```rust
use multi_demangle::{demangle_iter, DemangleOptions};

let symbols = ["_ZN3foo3barEv", "libc.so.6", "_ZN3foo3barEv"];
let demangled = demangle_iter(symbols, DemangleOptions::complete());
assert_eq!(&demangled[0], "foo::bar()");
assert_eq!(&demangled[1], "libc.so.6");
assert_eq!(&demangled[2], "foo::bar()");
```

`demangle_iter` computes the whole batch eagerly and returns a `Vec` (in
input order). `demangle_one` is the single-symbol pipeline the batch is built
on (it is what `multi_demangle::demangle` delegates to), exposed so consumers
can share it with their own batching. Enabling the `parallel` cargo feature
(off by default) demangles the distinct symbols on the rayon thread pool.

```toml
multi-demangle = { version = "...", features = ["parallel"] }
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

// Language detection (includes Scala Native, which has no Language variant).
assert_eq!(detect_language("_ZN3foo3barEv"), Some("cpp"));
assert_eq!(detect_language("libc.so.6"), None);

// Classification without demangling.
let status = classify_symbol("__imp_?foo@bar@@YAXXZ");
assert_eq!(
    status,
    SymbolStatus::Decorated {
        decoration: Decoration::ImportPointer,
        inner: Box::new(SymbolStatus::Mangled(symbolic_common::Language::Cpp)),
    }
);

// Display-oriented normalization: legacy Rust `$`-escapes, Rust hash
// suffixes, import pointer rewriting, and pseudo-symbol mapping.
assert_eq!(
    normalize_symbol("std::io::Read::read_to_end::hb85a0f6802e14499"),
    "std::io::Read::read_to_end"
);
assert_eq!(
    normalize_symbol("__imp__Z1fv"),
    "__declspec(dllimport) _Z1fv"
);
```

Two `Normalizer` pass sets are available: `Normalizer::display()` (the default
of `normalize_symbol`) cleans names for humans, while `Normalizer::matching()`
additionally strips `.llvm.` clone suffixes, PLT/GOT call stubs, and ELF
version suffixes — and strips import pointers instead of rewriting them — so
results match the other binary's export table:

```rust
use multi_demangle::Normalizer;

assert_eq!(
    Normalizer::matching().normalize("memcpy@plt"),
    "memcpy"
);
assert_eq!(
    Normalizer::matching().normalize("__imp_CreateFileW"),
    "CreateFileW"
);
```

[`Demangle::try_demangle_normalized`](crate::Demangle::try_demangle_normalized)
combines demangling with a normalizer fallback: a symbol that cannot be
demangled goes through the given passes instead of being returned unchanged
(successful demangled output is never normalized).

```rust
use symbolic_common::Name;
use multi_demangle::{Demangle, DemangleOptions, Normalizer};

assert_eq!(
    Name::from("__imp__ZN3foo3barEv")
        .try_demangle_normalized(DemangleOptions::complete(), &Normalizer::display()),
    "__declspec(dllimport) _ZN3foo3barEv"
);
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

### Batch demangling

`demangle_symbols` demangles a whole batch in one call, releasing the GIL for
the duration. It accepts any iterable of strings — lists, tuples, generators,
`map` objects — so hot loops can feed it without materializing first.
Duplicate symbols are demangled once by default and share a single string
object across their occurrences; results keep the input order and unmangled
symbols pass through unchanged:

```
>>> multi_demangle.demangle_symbols(["_ZN3foo3barEv", "libc.so.6", "_ZN3foo3barEv"])
['foo::bar()', 'libc.so.6', 'foo::bar()']

>>> # pass unique=False to demangle every position independently
>>> multi_demangle.demangle_symbols(["_Z1hic", "libc.so.6"], unique=False)
['h(int, char)', 'libc.so.6']

>>> # options behave like demangle_symbol
>>> multi_demangle.demangle_symbols(["_ZN3foo3barEv"], options=multi_demangle.DemangleOptions.name_only())
['foo::bar']
```

This replaces the per-symbol calls in hot loops (full symbol tables, PE import
tables, Mach-O binding/stub maps) with a handful of batch calls. Type stubs
ship with the wheel (`multi_demangle.pyi`), so type checkers see the full API
including the keyword-only `unique` parameter.

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

>>> multi_demangle.classify_symbol("_Z1hic@GLIBC_2.2.5")
{'status': 'mangled', 'language': 'cpp', 'decorations': [{'kind': 'version', 'value': 'GLIBC_2.2.5'}]}

>>> info = multi_demangle.demangle_symbol_ex("__imp_?h@@YAXH@Z")
>>> info["status"], info["language"], info["decorations"]
('mangled', 'cpp', [{'kind': 'import-pointer'}])
```

`demangle_symbol_ex` returns a dict with `mangled`, `demangled`, `status`
(`"mangled"`, `"unmangled"`, or `"unsupported"`), `language`, and an
outermost-first `decorations` list; `classify_symbol` returns the same
classification without demangling. Passing
`multi_demangle.DemangleOptions(normalize=True)` applies the display hygiene
passes to the fallback when demangling does not succeed, and
`Normalizer.matching()` provides the pass set for cross-symbol matching
(`memcpy@plt` → `memcpy`, `__imp_CreateFileW` → `CreateFileW`).

## CLI

A `c++filt`-style command line tool ships with the crate. Install it with a
Rust toolchain (or use `cargo run --` from a checkout):

```
cargo install multi-demangle
```

With arguments, each argument is demangled to one output line. Without
arguments, the tool runs in **filter mode**: lines are read from stdin, every
whitespace-separated token that looks mangled is demangled, and everything
else passes through unchanged — so it composes with `nm` / `objdump`
pipelines:

```
$ multi-demangle _ZN3foo3barEv
foo::bar()

$ nm libfoo.so | multi-demangle
$ nm libfoo.so | sort | uniq -c | multi-demangle -n --normalize
```

Hyphen-prefixed symbols such as ObjC selectors are accepted as values, so the
obvious invocation just works (and `--` works too):

```
$ multi-demangle '-[Foo bar:blub:]'
-[Foo bar:blub:]
```

Options:

| Flag | Effect |
| ---- | ------ |
| `-n, --name-only` | names only, no parameters or return types |
| `--no-parameters` / `--no-return-type` | individual output toggles |
| `-l, --language <LANG>` | force a backend instead of auto-detecting (`cpp`, `rust`, `swift`, `objc`, `objcpp`, `scala-native`) |
| `--normalize` | apply the symbol hygiene passes (`__imp_`, `@plt`, ELF versions, Rust hash suffixes and `$`-escapes, `.llvm.` clone suffixes, pseudo-symbols) to symbols that cannot be demangled, then demangle the cleaned symbol when it succeeds |
| `-s, --structured` | print one JSON record per symbol with its status, language, and linker decorations |
| `--list-languages` | print the supported languages and the backends enabled in this build |
| `--color=auto/always/never` | colorize successfully demangled output (auto is the default) |

`multi-demangle --version` prints the crate version together with the enabled
backends. Exit code is `0` on success — including when nothing looked mangled
— and `1` on I/O errors.

```
$ multi-demangle -s "_Z1hic@GLIBC_2.2.5"
{"mangled":"_Z1hic@GLIBC_2.2.5","demangled":"_Z1hic@GLIBC_2.2.5","status":"mangled","language":"cpp","decorations":[{"kind":"version","value":"GLIBC_2.2.5"}]}
```

In filter mode with `--structured`, records are emitted only for tokens that
look like symbols or that the pipeline changed — under `--normalize`, a
cleaned token such as `bar.llvm.12345` is reported — while plain addresses,
type letters, and words are skipped.

`--normalize` never touches directly successful demangled output; the passes
run on the symbols the demanglers rejected, and the cleaned symbol is then
demangled once more — a version-suffixed `_Z1hic@GLIBC_2.2.5` comes out as
`h(int, char)`. Because `.llvm.` clone suffixes and legacy Rust `$`-escapes
appear on names that do not classify as mangled, filter mode processes every
token while `--normalize` is active:

```
$ multi-demangle --normalize bar.llvm.12345
bar
$ multi-demangle "_Z1hic@GLIBC_2.2.5"
_Z1hic@GLIBC_2.2.5
$ multi-demangle --normalize "_Z1hic@GLIBC_2.2.5"
h(int, char)
```

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

Run the criterion benchmarks for the batch pipeline (uses the real-symbol
dumps in `tests/corpus/`; regenerate them with
`scripts/collect-corpus.sh` when the producing toolchains change):

```
cargo bench
```

The Swift demangler is a minimal subset of the Swift standard library sources
vendored under `vendor/swift`; see [vendor/swift/README.md](vendor/swift/README.md)
for how it is maintained.

## License

MIT
