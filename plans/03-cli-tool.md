# Plan 03 — `multi-demangle` CLI (cxxfilt-style)

**Tier:** 1 (next release) · **Effort:** S · **Value:** first-class debugging
tool for blint users and contributors; replaces ad-hoc `llvm-cxxfilt` /
`swift-demangle` round-trips; great for shell pipelines over `nm`/`objdump`
output.

## Motivation

Every demangler ecosystem has a filter CLI: `llvm-cxxfilt`/GNU `c++filt` for
C++, `swift-demangle` for Swift, `rustfilt` for Rust. multi-demangle supports
*all* these languages behind one detector, but is only reachable as a library —
so debugging "why didn't blint demangle symbol X" today means writing a Python
one-liner or hopping between three vendor-specific tools.

A single CLI that auto-detects C++ (Itanium/GNU v2/CodeWarrior/MSVC), Rust,
Swift, Scala Native, and ObjC selectors — plus the hygiene passes from
Plan 01 — is both a developer convenience and a support tool: blint issue
reports can include `nm foo | multi-demangle` output directly.

## Proposed interface

```
multi-demangle [options] [mangled...]
```

- With no arguments: **filter mode** — read lines from stdin, demangle every
  whitespace-separated token that looks mangled, pass the rest through
  unchanged (this is `c++filt`'s model and composes with `nm | sort | uniq -c`).
- With arguments: demangle each argument, one result per line.

Options:

| Flag | Effect |
| ---- | ------ |
| `-n, --name-only` | `DemangleOptions::name_only()` (no params/return type) |
| `--no-parameters` / `--no-return-type` | individual toggles |
| `-l, --language LANG` | force a backend instead of auto-detection (`cpp`, `rust`, `swift`, `objc`, `scala-native`, …) |
| `--normalize` | apply the Plan 01 hygiene passes (hash trim, `$`-escapes, `__imp_`, `@plt`, versions) |
| `-s, --structured` | print JSON records (wired up with Plan 04's structured API) |
| `--list-languages` | print supported languages and enabled features |
| `--version` | crate version + enabled backends |
| `--color=auto/always/never` | colorize demangled output (nice-to-have) |

Exit codes: 0 on success (including "nothing looked mangled"), 1 on I/O error.
Unmangled tokens pass through verbatim — same contract as the library's
`try_demangle`.

## Implementation steps

1. Add `src/main.rs` (or `src/bin/multi-demangle.rs`) gated so it does not
   affect the cdylib build; use `clap` with `derive` (or hand-rolled arg
   parsing to keep the dependency tree tiny — prefer clap, it is standard).
2. Filter mode: `BufRead` lines → split on whitespace → per token: detect,
   demangle, print. Handle invalid UTF-8 with lossy conversion, mirroring
   what a symbol table can contain.
3. Compose with Plans 01 (`--normalize`, `--list-languages`) and 04
   (`--structured`) — stub the flags until those land.
4. Tests: integration test driving the binary against the existing
   `tests/*.rs` corpora expectations; add a couple of shell-level tests in CI
   (pipe a fixture `nm` dump through filter mode).
5. Distribution:
   - `cargo install multi-demangle` works immediately (add `[[bin]]`).
   - PyPI: add a second maturin project/wheel (`multi-demangle-cli`) using
     `bindings = "bin"` so `pipx install multi-demangle-cli` gives the tool
     to blint users without a Rust toolchain. Keep it out of the main wheel
     to avoid bloating blint's dependency closure.
6. README: add a "CLI" section with `nm`/`objdump` pipeline examples.

## Risks & mitigations

- **Scope creep into a general symbol tool.** Keep the CLI a thin shell over
  the library; anything smarter (reading ELF directly, etc.) belongs in blint.
- **Windows console quoting/UTF-8 quirks** — covered by CI matrix already
  running Windows (see `.github/workflows/CI.yml`); add one PowerShell-style
  test invocation.

## Acceptance criteria

- `cargo run -- _ZN3foo3barEv` → `foo::bar()`; stdin filter mode verified with
  a mixed `nm` fixture.
- `--name-only`, `--language`, `--normalize`, `--version` all tested.
- Binary published on crates.io (`cargo install`) and, optionally, a
  `multi-demangle-cli` wheel on PyPI.
