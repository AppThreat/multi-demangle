# multi-demangle feature plans

This directory contains researched, prioritized plans for evolving `multi-demangle`
(the Rust crate + Python extension) with its primary consumer —
[OWASP blint](https://github.com/owasp-dep-scan/blint) — as the anchor customer.

Written: 2026-08-26. Each plan is self-contained and can be implemented
independently unless noted. Plans are informed by:

- a code audit of how blint consumes the module (see "blint findings" below),
- a survey of the demangler ecosystem (LLVM `Demangle`, Ghidra, `llvm-cxxfilt`,
  radare2, `swift-demangle`, Kotlin/Native, gfortran/Intel Fortran mangling),
- the crate's current architecture (`src/lib.rs`, vendored Swift subset,
  PyO3/maturin packaging, CI).

## The one-paragraph summary

blint calls `demangle_symbol()` symbol-by-symbol in hot loops and then **re-parses
the demangled strings in Python** to strip Rust hashes, decode legacy Rust
`$`-escapes, reduce `<Type as Trait>::` qualifiers, classify names into
closure/glue/intrinsic/method, and clean Windows `__imp_` decorations. All of
that logic belongs in this crate — in Rust, tested once, and exposed as
(a) a symbol-hygiene/normalization API, (b) a structured demangling API,
(c) a batch API for hot loops. On top of that foundation, the highest-leverage
growth features are: a `cxxfilt`-style CLI, automated Swift vendor sync, fuzzing
with a real-world symbol corpus, and new language backends that no other Rust
demangler covers (D, Fortran, Kotlin/Native).

## Plan index and roadmap

Priority is driven by value to blint and effort. Tiers are suggestions for
release planning, not hard dependencies.

| Plan | Title | Tier | Effort | Value to blint |
| ---- | ----- | ---- | ------ | -------------- |
| [01](01-symbol-hygiene-api.md) | Symbol hygiene & language-detection API | 1 — next release | S/M | High — deletes ~120 lines of Python heuristics |
| [02](02-batch-performance.md) | Batch demangling & performance | 1 — next release | S | High — hot loops over full symtabs |
| [03](03-cli-tool.md) | `multi-demangle` CLI (cxxfilt-style) | 1 — next release | S | Medium — debugging, pipelines, parity checks |
| [04](04-structured-demangling.md) | Structured demangling API | 2 | M/L | High — replaces blint's `canon.py` string re-parsing |
| [05](05-robustness-fuzzing.md) | Fuzzing, corpus & llvm-cxxfilt parity CI | 2 | M | Medium — stability on untrusted symbols |
| [06](06-swift-vendor-sync.md) | Swift vendor sync automation & fidelity | 2 | M | Medium — keeps Swift support current |
| [07](07-new-languages.md) | New language backends (D, Fortran, Kotlin/Native, Ada, ObjC structured) | 3 | M/L each | Medium — coverage gaps no other Rust demangler fills |

S ≈ days, M ≈ 1–3 weeks, L ≈ 4+ weeks for one contributor.

## Key blint findings (the evidence base)

From auditing `/Users/prabhu/work/owasp/blint` (dependency:
`multi-demangle>=1.0.3`):

1. **Per-symbol FFI calls everywhere, no batch API, no caching.** ~20 call
   sites; hot loops include `binary.py parse_symbols` (full ELF symtab),
   `parse_pe_imports` (which demangles the same name *twice* per entry,
   binary.py:1124-1125), Mach-O binding/stub maps in `disassembler.py:1266-1304`
   (heavy on Swift apps), and per-node canonicalization in
   `callgraph/model.py`.
2. **Python-side heuristics layered on top** (`utils.py:73-118`
   `demangle_symbolic_name`): anonymous-symbol mapping, `GCC_except_table`,
   `@feat.00` → SAFESEH, `__imp_`/`.rdata$`/`.refptr.` dllimport decoration,
   and 16 chained `str.replace` calls decoding legacy Rust `$`-escapes
   (`$LT$`, `$u5b$`, `$SP$`, …).
3. **Rust name canonicalization reimplemented in Python**
   (`callgraph/canon.py`): `_looks_mangled` prefix filter (which misses Swift
   `_$s…`/`_T0…`), two *divergent* hash-trim implementations (last-`::`-segment
   length-17 in utils.py vs the `::h[0-9a-f]{8,}` regex in canon.py),
   `.llvm.<N>` suffix stripping, `<Type as Trait>::` → `Type::` reduction,
   generics stripping, and a `NameKind` classification (CLOSURE / GLUE /
   INTRINSIC / METHOD / FUNCTION) that directly gates call-graph matching
   quality.
4. **Failure detection by string identity** (`symbol == demangled_symbol`)
   because the API cannot distinguish "not mangled" from "demangling failed" —
   even `NEEDED` library names like `libc.so.6` round-trip through the FFI call.
5. **Cross-binary import attribution depends on raw linkage names** being
   retained whenever demangling changed the name
   (`import_attribution.py:124-136`), so demangler output stability is
   load-bearing for blint's SBOM quality.
6. blint's git history shows recurring demangler fixes ("More demangling for
   rust", "Demangle more names with fallback", the migration from `symbolic`
   to `multi-demangle`), confirming this is an active pain axis.

## Guiding principles

- **`demangle_symbol()` stays stable.** blint and other consumers depend on it;
  everything new is additive.
- **Absorb blint's heuristics upstream.** Any string post-processing blint does
  in Python around demangling is a candidate to move into this crate (in Rust,
  with tests), then deleted downstream.
- **Feature-gate new backends** like the existing ones (`cpp`, `swift`, …), so
  build size and compile time stay controllable.
- **Every new API ships in both Rust and Python** unless there is a concrete
  reason not to; the Python surface stays small and typed.
- **Test against real-world symbols**, not just unit tests: corpora from real
  Swift/Rust/MSVC binaries (blint's test data is a good source) and, where
  applicable, parity with `llvm-cxxfilt`.

## Research sources

- LLVM `Demangle` library (Itanium, MSVC, Rust, D, Ada):
  <https://github.com/llvm/llvm-project/blob/main/llvm/lib/Demangle/Demangle.cpp>
- `llvm-cxxfilt` command guide: <https://llvm.org/docs/CommandGuide/llvm-cxxfilt.html>
- LLVM D demangler addition (D110576): <https://reviews.llvm.org/D110576>
- Ghidra structured `DemangledObject` API:
  <https://ghidra.re/ghidra_docs/api/ghidra/app/util/demangler/DemangledObject.html>
- Kotlin/Native `_kfun:` mangling and the lack of a standalone demangler:
  <https://github.com/JetBrains/kotlin-native/issues/755>,
  <https://en.wikipedia.org/wiki/Name_mangling>
- gfortran `__mod_MOD_proc` / Intel `mod_mp_proc_` mangling (no demangler in
  binutils/LLVM): <https://stackoverflow.com/questions/52741473/naming-of-symbols-in-fortran-shared-library-intel-vs-gcc>,
  <https://cmake.org/cmake/help/latest/module/FortranCInterface.html>
- Swift demangler sources and runtime-demangle pitch:
  <https://github.com/apple/swift/blob/main/tools/swift-demangle/swift-demangle.cpp>,
  <https://forums.swift.org/t/pitch-expose-demangle-function-in-runtime-module/82605>
- `swift-demangler` Rust crate (vendored runtime demangler):
  <https://lib.rs/crates/swift-demangler>
- Demangler fuzzing practice (`cpp_demangle` AFL, cargo-fuzz, historical
  panics; libiberty CVE-2022-27943 stack exhaustion):
  <https://github.com/gimli-rs/cpp_demangle/blob/master/CONTRIBUTING.md>,
  <https://github.com/rust-fuzz/cargo-fuzz>,
  <https://rust-fuzz.github.io/book/cargo-fuzz/guide.html>
