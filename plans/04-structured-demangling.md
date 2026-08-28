# Plan 04 — Structured demangling API

**Tier:** 2 · **Effort:** M/L (two phases) · **Value:** replaces all of blint's
post-demangling string re-parsing (`callgraph/canon.py`, ~270 lines) with typed
fields; foundation for stable cross-binary matching.

## Motivation

Consumers don't actually want a string — they want to know *which function*
it is. Today every consumer must re-parse the demangled text:

- blint `callgraph/canon.py` re-parses demangled Rust paths to build
  `CanonicalName(value, kind, raw, is_generic)`: strips the `::h<hash>`
  disambiguator and `.llvm.<N>` suffix, reduces `<Type as Trait>::method` →
  `Type::method`, strips generic/lifetime noise, and classifies into
  CLOSURE / GLUE / INTRINSIC / METHOD / FUNCTION. Classification quality
  "directly gates call-graph matching precision/recall" (blint
  `docs/CALLGRAPH_MATCH.md`).
- blint `binary.py guess_exe_type` (825-842) substring-matches symbol names to
  detect Go/Rust/.NET binaries.
- blint `analysis.py` fuzzable-name extraction regex-strips `*&()` from
  demangled C++/Rust names.
- blint keeps both `raw_name` and demangled `name` on every symbol because
  demangled strings can never match an export table — the pieces it actually
  needs (stable function identity) are trapped inside one string.

Ghidra demonstrates the model worth copying: its
[`DemangledObject`](https://ghidra.re/ghidra_docs/api/ghidra/app/util/demangler/DemangledObject.html)
hierarchy exposes `getName()`, `getNamespace()`, and typed subclasses
(`DemangledFunction`, `DemangledThunk`, …) that the analyzer consumes instead
of re-parsing text.

## Proposed API

### Rust

```rust
pub struct DemangledInfo {
    /// Detected language (`Language` from symbolic-common).
    pub language: Language,
    /// Full verbose rendering (what `demangle()` returns today).
    pub display: String,
    /// Name-only rendering.
    pub simple: String,
    /// Namespace/module/class path, outermost first: ["std", "vec", "Vec<T>"].
    pub namespace: Vec<String>,
    /// Leaf name: function/method name, or selector for ObjC.
    pub name: String,
    /// What kind of entity the symbol denotes.
    pub kind: DemangledKind,
    /// Parameter type renderings, when the scheme encodes them.
    pub parameters: Option<Vec<String>>,
    /// Return type rendering, when encoded.
    pub return_type: Option<String>,
    /// Rust legacy/v0 disambiguator (`h17hb85...` / `<hash>`), when present.
    pub hash: Option<String>,
    /// Generic/template argument renderings, when present.
    pub template_args: Option<Vec<String>>,
    /// True when the name carries generic/template arguments.
    pub is_generic: bool,
    /// The original mangled symbol.
    pub mangled: String,
}

pub enum DemangledKind {
    Function, Method, Closure, Glue, Intrinsic, MethodThunk,
    VirtualTable, TypeInfo, StaticVariable, ObjCMethod { class_method: bool },
    Other(String),
}

pub trait Demangle {
    // existing methods unchanged, plus:
    fn demangle_structured(&self, opts: DemangleOptions) -> Option<DemangledInfo>;
}
```

### Python

```python
info = multi_demangle.demangle_symbol_structured(
    "_ZN3std2io4Read11read_to_end17hb85a0f6802e14499E"
)
info.language        # "rust"
info.name           # "read_to_end"
info.namespace      # ["std", "io", "Read"]
info.kind           # "method"
info.hash           # "hb85a0f6802e14499"
info.parameters     # None for legacy rust (not encoded)
info.to_dict()      # plain dict for JSON serialization
```

## Phase 1 — text-derived structure (all languages)

Extract structure by parsing the demangled text in Rust, absorbing blint's
`canon.py` rules as the specification:

- hash/`.llvm.` suffix capture (instead of blind stripping) into `hash`;
- `<Type as Trait>::` and `<impl Trait for Type>::` reduction (keep both the
  reduced path and the original in `namespace`);
- `{{closure}}`/`closure_N` detection → `DemangledKind::Closure`;
- CRT-glue table (`_start`, `__rust_alloc`, `frame_dummy`, … — copy blint's
  `_CRT_GLUE` frozenset) → `DemangledKind::Glue`;
- leaf/namespace splitting on `::` (Rust/Scala Native) and `.` (Swift module
  paths, with `<` at depth 0 separating type from module);
- C++: reuse the existing `analyze_cpp_like_signature` machinery
  (`src/lib.rs:397-417`) to split return type / name / parameters;
- ObjC selectors: `-[Class sel:sel2:]` → `ObjCMethod { class_method: false }`,
  namespace = `[Class]`, name = selector — upgrading today's
  "detection only" support to structured support.

This phase alone lets blint delete `canon.py`'s parsing (keeping only its
graph-specific canonical string format) and stop guessing exe types from
substrings (`language` field answers it for Rust/Swift/Scala Native directly).

## Phase 2 — AST-backed structure (higher fidelity, per backend)

Where a backend already parses into an AST, derive fields from it instead of
text:

- **C++ Itanium:** `cpp_demangle`'s public `ast` module already models the
  symbol (function vs variable vs vtable, name components, substitution
  structure). Walk it to fill `kind`, `namespace`, `template_args`,
  `parameters`, `return_type` exactly. `msvc_demangler` similarly exposes
  parsed structures for MSVC symbols.
- **Swift:** the vendored subset already ships `NodeDumper.cpp`
  (`vendor/swift/MANIFEST.txt:51`). Add an FFI entry point
  `multi_demangle_swift_dump(sym, buf, len)` returning the node dump, and map
  demangler node kinds (Global, Function, Class, Method, Constructor, …) to
  `DemangledKind`/`namespace` — this is exactly the data the Swift
  demangler already understands, no text guessing required.
- **Rust:** `rustc_demangle` is display-only; long-term option is a vendored
  v0 parser, but Phase 1 text rules cover blint's needs (v0's structure is
  largely recoverable from its deterministic rendering).

## Implementation steps

1. Define `DemangledInfo`/`DemangledKind` in a new `src/structured.rs` with
   conversions to/from the text pipeline; unit tests per language.
2. Phase 1 extractors + port blint `canon.py` test cases as the oracle.
3. PyO3 exposure (`demangle_symbol_structured` + pyclass with getters and
   `to_dict`); JSON snapshot tests in `python/tests`.
4. Phase 2a: `cpp_demangle`/`msvc_demangler` AST walks (feature-gated with the
   existing `cpp`/`msvc` features).
5. Phase 2b: Swift node-dump FFI + mapping table.
6. blint follow-up: `canonicalize()` consumes structured fields; `guess_exe_type`
   uses `language`; report demangled names via `namespace + name` rather than
   string surgery.

## Risks & mitigations

- **Text-parsing brittleness** is the status quo being *replaced*, not
  introduced; Phase 2 systematically removes it per backend.
- **API stability:** mark `DemangledInfo` fields non-exhaustive
  (`#[non_exhaustive]`) so adding fields later is not a breaking change.
- **Cost in hot paths:** structured demangling allocates more; blint should use
   it where identity matters (callgraph, attribution) and the batch string API
   (Plan 02) for bulk symbol tables. Both can share one pass later via
   `demangle_symbols(structured=True)`.

## Acceptance criteria

- `demangle_structured` (Rust) and `demangle_symbol_structured` (Python) cover
  all supported languages with snapshot-tested output.
- blint's `canon.py` no longer re-parses demangled strings for hash/qualifier/
  closure handling; its callgraph matching tests stay green.
- Swift structured output derived from the node dump matches a curated set of
  real iOS-app symbols.
