# Plan 05 — Fuzzing, real-world corpus & llvm-cxxfilt parity

**Tier:** 2 · **Effort:** M · **Value:** multi-demangle parses **untrusted
input by definition** (symbols from possibly-malicious binaries); this plan
makes that posture provable and regressions visible.

## Motivation

Demanglers are a classic source of parser bugs:

- GNU libiberty's `rust-demangle.c` had CVE-2022-27943 (stack exhaustion);
- `cpp_demangle` historically shipped AFL targets after fuzzing found an
  integer-overflow panic overnight
  ([contributing guide](https://github.com/gimli-rs/cpp_demangle/blob/master/CONTRIBUTING.md));
- this crate already carries defenses — the `BoundedString` 4096-byte cap
  against "billion laughs" substitution expansion (`src/lib.rs:280-309`) and
  raised-but-bounded recursion limits (`src/lib.rs:521,529`) — showing the
  threat is understood, but nothing *systematically* tests it.

Two gaps make regressions likely as the crate grows:

1. **No fuzzing** of the crate's own dispatch layer (`detect_language` tries
   up to four backends per symbol; the Swift path crosses into vendored C++
   via FFI — the one place memory-unsafety is possible).
2. **No real-world corpus or parity harness.** Unit tests are hand-picked;
   there is no way to notice "output for 2% of Swift symbols changed" between
   releases — which matters because blint's cross-binary attribution depends
   on demangled-name *stability* (it matches export tables against retained
   raw names, `import_attribution.py:124-136`).

## Proposal

### 1. cargo-fuzz targets

```
fuzz/
  fuzz_targets/
    demangle.rs       # demangle(bytes) with all features — must not panic/abort
    detect.rs         # detect_language(bytes) — must not panic, must terminate
    normalize.rs      # Plan 01 passes — must be idempotent
    swift_ffi.rs      # full Swift FFI round-trip with ASan+UBSan (C++ code!)
```

Properties asserted on every input:

- no panic, no abort, no OOM (the existing `BoundedString` cap must hold);
- output byte length ≤ sane bound (e.g. 8 KiB) for any input;
- `normalize(normalize(x)) == normalize(x)` (idempotent hygiene);
- `detect_language` completes in bounded time (recursion guards stay in place).

The Swift target must run under ASan/UBSan since it exercises vendored C++
(`src/swiftdemangle.cpp`); the Rust-only targets run clean plus optionally
under ASan too. Consider an [OSS-Fuzz](https://oss-fuzz.com) project
registration once local targets are stable (Rust support is first-class).

### 2. Real-world symbol corpus

Assemble a new `tests/corpus/` directory (plain text, one symbol per line,
grouped per origin; today the crate only has hand-written table tests via the
`assert_demangle!` macro in `tests/utils/mod.rs`):

- Rust: `nm` dumps from release builds of a few crates (std-linked binary,
  a `cdylib`, a `staticlib`), plus v0-mangled test symbols;
- C++: Itanium symbols from glibc/libstdc++/LLVM tools; MSVC symbols from a
  Visual Studio build (blint's Windows test data can donate); GNU v2/CodeWarrior
  samples already in `tests/*.rs`;
- Swift: `nm` dumps from a macOS system dylib set + an iOS app binary per
  supported Swift version (the vendored demangler tracks upstream; see
  Plan 06);
- ObjC selectors, Scala Native `_SM…`, plus the hygiene fixtures from Plan 01
  (`__imp_*`, `@plt`, versioned, anonymous, `@feat.00`).

Run the corpus through `demangle` in CI (fast) and assert **snapshot
stability**: output changes require an explicit snapshot refresh commit, which
the release notes can then cite ("Swift 6.4 sync changed N outputs").

### 3. llvm-cxxfilt parity check (non-blocking)

For the Itanium/MSVC/Rust/D subset where
[llvm-cxxfilt](https://llvm.org/docs/CommandGuide/llvm-cxxfilt.html) is also
applicable, a CI job diffs our output against it on the shared corpus.
Differences are *recorded, not enforced* (rendering differences are expected —
e.g. `cpp_demangle` vs LLVM formatting) — the value is spotting functional
gaps ("LLVM demangles these 40 symbols we reject"). LLVM is preinstalled on
GitHub runners, so the job is cheap.

## Implementation steps

1. `cargo fuzz init` + the four targets; wire minimal seeds from existing
   `tests/*.rs` cases; add a nightly-fuzz workflow (scheduled, small budget)
   plus crash-artifact regression tests.
2. ASan/UBSan job for the Swift FFI path (also valuable for Plan 06 vendor
   syncs: run it as a sync acceptance gate).
3. Corpus collection script `scripts/collect-corpus.sh` documenting exactly how
   each file was produced (toolchain versions matter for Swift/Rust);
   snapshot test runner in Rust (`insta` or hand-rolled) + a Python variant
   exercising the wheel.
4. Parity job comparing against `llvm-cxxfilt --no-strip-underscore` where
   available; writes a diff artifact on change.
5. Document the threat model in README ("symbols are untrusted input; output is
   bounded; panics are bugs — please file with the fuzz input").

## Risks & mitigations

- **Corpus licensing/size:** symbol *names* from system libraries are fine to
  commit; keep files trimmed (top Nk symbols) and gzip if large.
- **Snapshot churn from dependency bumps** (`rustc-demangle`, `cpp_demangle`
   releases change rendering): that is precisely the visibility we want; the
   workflow makes the change explicit rather than silent.

## Acceptance criteria

- Fuzz targets merged and running on a schedule; any crash found converted to
  a regression test.
- Corpus snapshot tests part of `cargo test` and `pytest`.
- One full pass of the corpus under ASan/UBSan including the Swift FFI path.
- Parity artifact produced in CI with the current divergence count recorded.
