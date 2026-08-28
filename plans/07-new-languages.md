# Plan 07 — New language backends

**Tier:** 3 · **Effort:** M per backend (D), S (Fortran, Ada, Kotlin/Native) ·
**Value:** multi-demangle's reason to exist is "one demangler for a binary
analysis pipeline". Every language below appears in real binaries blint scans,
and — critically — **no other Rust crate demangles any of them** (verified
against crates.io/lib.rs; existing crates cover only C++/Rust/Swift).

Priority order below is recommended. Each backend ships behind its own cargo
feature (`dlang`, `fortran`, `kotlin-native`, `ada`), is detected in
`detect_language`, integrated into `SymbolStatus` (Plan 01), and covered by the
Plan 05 corpus.

## 1. Fortran (gfortran + Intel) — recommended first

**Why first:** zero tooling exists anywhere (binutils `nm -C`/`llvm-cxxfilt`
demangle only C++-family schemes, and LLVM has no Fortran demangler), the
mangling is trivially pattern-based, and HPC/scientific binaries are a real
blint audience.

Schemes ([overview](https://stackoverflow.com/questions/52741473/naming-of-symbols-in-fortran-shared-library-intel-vs-gcc),
[CMake's detector](https://cmake.org/cmake/help/latest/module/FortranCInterface.html)
is the canonical reference for the variants):

| Compiler | Pattern | Demangled form |
| -------- | ------- | -------------- |
| gfortran | `__<module>_MOD_<proc>` | `<module>::<proc>` |
| gfortran (module) | `<module>_MOD_<proc>` (no leading `__` in some ABIs) | `<module>::<proc>` |
| Intel ifort/ifx | `<module>_mp_<proc>_` | `<module>::<proc>` |
| any, plain | `<name>_` (trailing underscore, `name` contains no underscore) | `<name>` |
| gfortran, renamed | `__<module>_MOD_<proc>_<len>` with length suffixes | strip length suffixes |

Notes: plain `name_` detection must be opt-in/conservative (collides with any C
symbol ending in `_`); module patterns are unambiguous. Also handle array
binding/BIND(C) forms opportunistically and expose `DemangledKind::Function`
plus module as namespace (feeds Plan 04 naturally).

## 2. D language

**Why:** LLVM upstream demangles D (`_D…` prefix, added in
[LLVM D110576](https://reviews.llvm.org/D110576), a port of libiberty's
`dlang_demangle`), but **no Rust crate exists** — this would be the only Rust
D demangler. D binaries (e.g. tools built with LDC/GDC, some trading/embedded
software) otherwise show as garbage symbols in analysis output.

Approach: port LLVM's `DLangDemangle.cpp` to Rust (Apache-2.0/LLVM-exception;
same licensing approach as `vendor/swift` — attribution header + vendored
license file). ~1-2k lines including type grammar. Detection: starts with `_D`
followed by a valid D symbol body (`_Dmain` special-cased); no collision with
Rust (`_ZN`/`_R`) or C++ Itanium (`_Z`). Note there is also a
[`swift-demangler`-style precedent](https://lib.rs/crates/swift-demangler)
for vendoring via `cc` if a straight C++ compile of the LLVM source is easier
than a port — but a Rust port keeps the crate C++-toolchain-free outside the
`swift` feature and benefits from Plan 05's memory-safe fuzzing.

## 3. Kotlin/Native

**Why:** Kotlin Multiplatform `.framework`/`.klib` binaries are increasingly
shipped inside iOS/Android apps (a core blint scanning target), and their
symbols use a scheme no standalone demangler handles
([upstream discussion](https://github.com/JetBrains/kotlin-native/issues/755),
[mangling overview](https://en.wikipedia.org/wiki/Name_mangling)).

Scheme: symbols begin `_kfun:` and carry a mostly-readable qualified name plus
a type signature, e.g. `_kfun:com.example.Foo.bar(kotlin.String;kotlin.Int)`.
So this is closer to a *parser/pretty-printer* than a classic demangler:
split qualified name, format parameters, mark closures (`lambda` segments) and
objective-C interop bridging. Demangled form:
`com.example.Foo.bar(String, Int)` with `kotlin.` prefix elision optional.
Effort is small; value shows up as readable Swift/ObjC call targets in
disassembly maps (blint `disassembler.py` import maps).

## 4. Ada / GNAT

**Why:** LLVM ships an Ada (GNAT) demangler; Ada appears in aerospace/defense
and some embedded malware. Scheme is encoded-entity names
(`pkg__subprogram` with package prefixes, plus suffixes for bodies/specs).
Port LLVM's `AdaDemangle.cpp` (small, string-transformation level) — cheapest
of the "real grammar" backends.

## 5. ObjC structured support (completing "detection only")

Not a new backend: today ObjC selectors pass through unchanged
(`src/lib.rs:609-621`). Plan 04 Phase 1 already adds structured parsing
(`-[Class sel:]` → class, selector, class-vs-instance method). Additionally
recognize runtime metadata symbols: `_OBJC_CLASS_$_Foo`,
`_OBJC_METCLASS_$_Foo`, `_OBJC_IVAR_$_…`, and emitted-selector symbols
(`l_OBJC_SELECTOR…` / `OBJC_SELECTOR_REFERENCES…`), mapping them to typed kinds
— these dominate non-Swift Mach-O symbol tables and currently confuse blint's
exe-type heuristics.

## Considered and deliberately deferred

- **Go** — not mangled (symbols are `pkg.Fn` with unicode separators); what's
  needed is *normalization* (middle-dot handling), which belongs in Plan 01
  hygiene, not a demangler backend.
- **Delphi/FreePascal (Borland scheme `@Unit@Class@method$qqri`)** — Ghidra
  supports it; demand for blint's audience is low. Revisit on user request.
- **Zig, Java/GraalVM native-image** — symbols are not reversibly mangled in a
  way that recovers more than hygiene passes give.
- **CUDA/NVCC, HP-UX aCC, Watcom** — Itanium-adjacent or effectively extinct;
  not worth backend surface until real corpus samples arrive via Plan 05.

## Implementation steps (per backend)

1. Detection predicate + feature gate + `Language` mapping (coordinate with
   `symbolic-common`'s `Language` enum; unknown variants map to
   `Language::Unknown` with a `DemangledInfo.language` string override).
2. Demangler implementation with unit tests derived from the upstream test
   suites (LLVM's `llvm/test/Demangle/*.test` are excellent oracles and can be
   parsed into table-driven tests).
3. Integrate: dispatch in `demangle()`/`detect_language`, hygiene rules
   (Plan 01), structured fields (Plan 04), corpus entries (Plan 05), CLI
   `--language` value (Plan 03), README table row.
4. Fuzz target extension (Plan 05) — non-negotiable for the D backend at least,
   since it implements a full grammar over untrusted input.

## Acceptance criteria

- Each backend: feature-gated, detected, demangled, structured fields
  populated, corpus-tested, fuzzed (grammar backends), documented in the
  README language table.
- Fortran and Kotlin/Native land first (small effort, unmet need); D next
  (only Rust implementation in existence); Ada last.
