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
