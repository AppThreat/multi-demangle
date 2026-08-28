# Plan 01 — Symbol hygiene & language-detection API

**Tier:** 1 (next release) · **Effort:** S/M · **Value:** deletes ~120 lines of
Python-side heuristics from blint and removes the identity-check failure
detection anti-pattern.

## Motivation

`demangle_symbol()` today answers exactly one question: "what is the readable
form of this string?". Real consumers need two more operations that blint
currently hand-rolls in Python:

1. **Is this symbol mangled at all, and in which language?** blint guesses with
   prefix checks (`_ZN`, `_RN`, `_R`, `__Z`, `?`, or `"$LT$" in symbol` at
   `callgraph/canon.py:107-109`) — which misses Swift (`_$s…`, `$s…`, `_T0…`)
   and GNU v2 / CodeWarrior entirely, and sends unmangled strings (even NEEDED
   library names like `libc.so.6`, `binary.py:2401`) through the FFI call.
2. **Normalize pre-demangled / decorated names.** When demangling "fails"
   (detected by string identity!), `utils.py:73-118` applies: anonymous-symbol
   mapping, `GCC_except_table`, `@feat.00` → SAFESEH, `__imp_`/`.rdata$`/
   `.refptr.` dllimport decoration, and 16 chained `str.replace` calls decoding
   legacy Rust `$`-escapes. A *second*, divergent hash-trim lives in
   `canon.py` (`::h[0-9a-f]{8,}` regex) vs `utils.py:112-115`
   (last `::` segment of length 17). And `import_attribution.py:139-163`
   strips `@plt`/`@got`/`@GOTPCREL` suffixes, `__imp_`/`_imp_`/`j_` prefixes,
   and `@version` suffixes — more hygiene logic in a third file.

All of this belongs behind one tested Rust API so the rules exist once.

## Proposed API

### Rust

```rust
/// What the demangler knows about a raw symbol without demangling it.
pub enum SymbolStatus {
    /// Not mangled; nothing to do.
    Unmangled,
    /// Mangled in this language and demangling is available.
    Mangled(Language),
    /// Mangled in this language but the backend is disabled (feature flag).
    Unsupported(Language),
    /// Decoration on top of another symbol, e.g. `__imp_foo` or `foo@plt`.
    Decorated { decoration: Decoration, inner: Box<SymbolStatus> },
}

/// Classifications for linker/toolchain decorations found on real symbols.
pub enum Decoration {
    /// MSVC/PE import pointer: `__imp_`, `_imp_`, `.rdata$`, `.refptr.`
    ImportPointer,
    /// PLT/GOT references: `@plt`, `@got`, `@gotpcrel`, thunk prefix `j_`
    CallStub,
    /// ELF symbol version: `@GLIBC_2.2.5`, `@@GLIBC_2.2.5`
    Version(String),
    /// Linker hash suffix: `$<32 hex>` (already stripped for Itanium parsing)
    LinkerHash,
    /// GCC cold-section split: `foo.cold`
    ColdSection,
    /// SAFESEH COFF flag pseudo-symbol `@feat.00`
    SafeSeh,
    /// Anonymous/unnamable LLVM values: `anon.`, `__imp_anon.`, `.L__unnamed`
    Anonymous,
    /// GCC unwind landing pads: `GCC_except_table*`
    ExceptTable,
}

/// Builder selecting which hygiene passes to apply; defaults = blint's current
/// behavior. `Normalizer::new().all()` applies everything.
pub struct Normalizer { /* flags */ }

impl Normalizer {
    pub fn normalize(&self, symbol: &str) -> Cow<'_, str>;
}

/// One-shot shorthand for the default passes.
pub fn normalize_symbol(symbol: &str) -> Cow<'_, str>;
```

Hygiene passes (each a named, individually-tested function):

| Pass | Rule | Source of truth |
| ---- | ---- | --------------- |
| Legacy Rust escapes | `..`→`::`, `$SP$`→`@`, `$BP$`→`*`, `$LT$`→`<`, `$GT$`→`>`, `$u5b$`→`[`, `$u5d$`→`]`, `$u7b$`→`{`, `$u7d$`→`}`, `$u3b$`→`;`, `$u20$`→space, `$u27$`→`'`, `$RF$`→`&`, `$LP$`→`(`, `$RP$`→`)`, `$C$`→`,` | blint `utils.py:93-110`; complete escape table exists in `rustc_demangle`'s legacy handling — sync with it |
| Rust hash trim | strip trailing `::h[0-9a-f]{8,}` (adopt the *regex* version; it subsumes the length-17 heuristic and covers v0) | blint `canon.py:35` vs `utils.py:112-115` (divergent today) |
| `.llvm.<N>` suffix | strip trailing `\.llvm\.\d+$` | blint `canon.py:36` |
| Import decoration | `__imp_`/`_imp_`/`.rdata$`/`.refptr.` → `__declspec(dllimport) <cleaned>` | blint `utils.py:87-92`, `import_attribution.py:139-163` |
| Call stubs | strip `@plt`, `@got`, `@GOTPCREL`(+rel), `j_` thunk prefix | blint `import_attribution.py:139-163` |
| ELF versions | strip `@`/`@@` version suffix, expose it as `Decoration::Version` | blint `import_attribution.py`, `binary.py:2354` |
| Pseudo-symbols | `@feat.00`→SAFESEH, `anon.*`/`__imp_anon.`/`.L__unnamed*`→anonymous, `GCC_except_table*` | blint `utils.py:80-86` |

### Python

```python
import multi_demangle

multi_demangle.detect_language("_ZN3foo3barEv")   # "cpp" | "rust" | "swift" | "objc" | "scala-native" | None
multi_demangle.looks_mangled("_$s8mangling...")   # True (works for Swift, unlike blint's heuristic)
multi_demangle.normalize_symbol("impl$u20$Trait$LP$$RP$::method")
```

`detect_language` already exists in Rust (`Demangle::detect_language`,
`src/lib.rs:646`); it only needs exposing through PyO3. Add
`demangle_symbol_ex(symbol, options=None)` returning a small result object
(`{demangled, language, status}`) so consumers can distinguish
*unmangled* / *unsupported* / *failed* instead of diffing strings. Keep
`demangle_symbol` as the friendly shorthand.

## Implementation steps

1. Port the escape/hash/decoration tables from blint into
   `src/hygiene.rs` with unit tests generated from blint's existing behavior
   (reuse blint's test cases verbatim as the compatibility oracle).
2. Implement `SymbolStatus` classification on top of the existing
   `is_maybe_*` predicates (`src/lib.rs:131-175`), extending `detect_language`
   where needed (it must remain cheap — no full demangle attempts beyond what
   it already does for GNU v2/CodeWarrior).
3. Wire `Normalizer` + free functions; integrate into `demangle()`/`try_demangle`
   only as opt-in (`DemangleOptions::normalize(bool)` or a wrapper function) so
   default output stays byte-identical.
4. Expose `detect_language`, `looks_mangled`, `normalize_symbol`,
   `demangle_symbol_ex` in the pymodule; update `python/tests`.
5. Update README (Rust + Python sections); bump minor version.
6. Follow-up PR in blint: replace `utils.demangle_symbolic_name` body and
   `canon._looks_mangled` with calls into the new API; delete the duplicated
   hash-trim.

## Risks & mitigations

- **Behavior drift for blint output.** Mitigate by porting blint's exact rules
  first, running blint's test suite against the new build, and only then
  unifying divergences (the two hash-trims) behind a flagged pass.
- **Legacy Rust escape table completeness.** blint's list is known-incomplete
  (no `$x5f_`-style underscore escapes). Sync the table with
  `rustc_demangle`/rustc's legacy mangling docs while porting.

## Acceptance criteria

- `normalize_symbol` and `detect_language` available from both Rust and Python.
- blint's `demangle_symbolic_name` and `_looks_mangled` reduce to thin calls
  into this crate, with blint's test suite green.
- New unit tests cover every hygiene pass, including Swift symbols that
  blint's prefix heuristic currently misses.
