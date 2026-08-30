# Real-world symbol corpus

Raw (still-mangled) symbol dumps used by the `batch` criterion benchmark
(`benches/batch.rs`) and shared with the robustness/fuzzing work (Plan 05).
One symbol per line, deduplicated, capped at 5,000 lines per file, filtered
to the mangling scheme named by the file.

These are **symbol names only** — no code, no relocations, no data — extracted
with `nm` from binaries present on the collection machine. Regenerate with
`scripts/collect-corpus.sh` (override sources via `RUST_BINARIES`,
`SWIFT_DYLIB`, `CPP_BINARY`, `MAX_PER_FILE`).

## Per-toolchain Swift snapshots (`swift/`)

The `swift/` subdirectory holds the per-toolchain corpora managed by
`scripts/collect-swift-corpus.sh`: one `<version>/` directory per Swift
toolchain (`symbols.txt` collected from compiling
`scripts/swift-corpus-fixture.swift`, `provenance.txt` describing the
toolchain, and `expected.txt` snapshotting this project's rendering —
compared by `tests/test_swift_corpus.rs`). These back the README's
"up to Swift X" claim; see the main README for the update workflow.
They are not consumed by the benchmark, which reads the top-level files only.

## Provenance

Collected 2026-08-29 on macOS 26.6 (arm64); `nm` from the Xcode toolchain.

| File | Source | Notes |
| ---- | ------ | ----- |
| `rust_symbols.txt` | `nm` over `~/.cargo/bin/rust-analyzer` and `~/.cargo/bin/wasm-pack` (rustup-distributed release binaries) | legacy (`_ZN…E`, macOS `__ZN…`) and v0 (`_RN…`) Rust mangling |

### New-language corpora (toolchain-collected)

The D, Ada, Fortran, and Kotlin/Native symbols are collected by compiling
the fixtures in `contrib/fixtures/` inside pinned toolchain images and
dumping the resulting symbol tables:

```bash
WITH_KOTLIN=1 contrib/collect-corpus.sh build
WITH_KOTLIN=1 contrib/collect-corpus.sh collect
```

`new-languages-provenance.txt` records the exact compiler versions; the
symbol spelling is a compiler implementation detail, so the provenance is
part of the test data's meaning.

`tests/test_new_language_corpus.rs` checks each language's rendering against
two authority tiers, in separate `<lang>_golden.txt` / `<lang>_snapshot.txt`
files (`<symbol>\t<expected>` lines; `<rejected>` pins a symbol the pipeline
must not claim):

- **golden** — verified against GNU `c++filt` (D, Ada) or against the
  fixture's source declarations (Fortran round-trip, Kotlin/Native). A
  mismatch is a bug and fails CI. The D and Ada files regenerate via
  `contrib/scripts/update-corpus-expectations.sh`; the Fortran and
  Kotlin/Native ones are hand-curated from the fixtures.
- **snapshot** — merely stable: the documented deliberate divergences from
  the reference, and symbols it fails. A mismatch needs a deliberate refresh
  (`MULTI_DEMANGLE_UPDATE_SNAPSHOTS=1 cargo test --test
  test_new_language_corpus`) reviewed like any snapshot update.

Splitting the tiers is deliberate: one undifferentiated pile of snapshots
gets refreshes rubber-stamped, which is how a regression becomes an
"expected output update".

D and Ada are additionally under a CI ratchet: the
`new-language-differential` workflow job re-collects both corpora inside the
toolchain image, diffs against `c++filt`, and fails when the functional-gap
count (symbols the reference demangles and we reject) rises above the
baseline recorded in the workflow — 0 for D, 1 for Ada (`b.2`, the
deliberate rejection). Rendering differences never fail the job. The same
corpora feed the cross-backend invariants in `src/lib.rs`
(`corpus_invariants`): no symbol may be claimed by two backends, and Ada or
Fortran may never claim a Rust, C++, or Swift symbol.

| File | Source | Notes |
| ---- | ------ | ----- |
| `dlang_symbols.txt` | ldc2 1.30 + gdc 12.2 over `contrib/fixtures/dlang/corpus.d` (inside `contrib/docker/gnu-toolchains`) | the full D ABI type grammar; includes deliberately-unmangled C-linkage controls |
| `ada_symbols.txt` | gnat 12.2 over `contrib/fixtures/ada/` | packages, child packages, operators, overloads, task bodies, escapes; includes the rejected-with-intent `b.2` |
| `fortran_symbols.txt` | gfortran 12.2 over `contrib/fixtures/fortran/corpus.f90` | module procedures (incl. names ending in digits), submodules, g77 bare forms |
| `kotlin_symbols.txt` | kotlinc-native 2.0.21 over `contrib/fixtures/kotlin/corpus.kt` (inside `contrib/docker/kotlin-native`) | every `com.example` symbol plus a 1-in-8 runtime sample |

| `swift_symbols.txt` | `nm` over the `libswiftCore.dylib` bundled inside `/Applications/The Unarchiver.app` (Apple Swift runtime, current version 1001.0.82) | current (`_$s…`/`_$S…`) and pre-Swift-5 (`_T0`/`_Tt`) mangling; system Swift dylibs live in the dyld shared cache and carry no symbol table, hence an app-bundled copy |
| `cpp_symbols.txt` | `nm` over the Xcode `clang++` binary (Apple clang 21.0.0) | Itanium ABI (`_ZN…`/`__ZN…`) |

Filter per file (see the script for the exact pipelines):

- rust: `grep -E '^_{1,2}(ZN|RN)'`
- swift: `grep -E '^(_\$[sS]|\$[sS]|_T0|_Tt)'`
- cpp: `grep -E '^_{1,2}ZN'`

then `sort -u | awk -v max=5000 'NR <= max'`.

## Why committed

The benchmark's value is a stable, real-world mix of templates, generics,
overloads, and hash suffixes that synthetic generators miss. If this
directory grows (Plan 05 adds per-origin corpora), consider generating on
demand instead of committing — the script is deterministic for a fixed set
of source binaries.
