# Plan 02 — Batch demangling & performance

**Tier:** 1 (next release) · **Effort:** S · **Value:** removes per-symbol FFI
overhead from blint's hottest loops (full symtabs, PE import tables, Mach-O
binding/stub maps).

## Motivation

blint demangles symbol-by-symbol in every hot loop it has:

- `binary.py parse_symbols` (~737-794): once per entry of the full ELF
  symtab/dynsym — tens of thousands of calls on real binaries, *and* it
  demangles strings LIEF already demangled (`symbol.demangled_name` at 751-754,
  re-fed through `demangle_symbolic_name`).
- `binary.py parse_pe_imports` (1118-1133): demangles the same `entry.name`
  **twice** per import (lines 1124-1125).
- `disassembler.py build_macho_import_address_map` (1266-1304): per dyld
  binding and per `__stubs` entry — the dominant cost on Swift iOS apps.
- `callgraph/model.py`: `canonicalize(name)` per graph node, each potentially
  triggering a demangle.
- `binary.py:2401`: even NEEDED entries (`libc.so.6`) go through the call.

Each call pays Python→PyO3 argument conversion, `Name::from` allocation, and —
biggest cost — a full `detect_language()` that may *attempt demangling* up to
four backends before deciding. Symbol tables also contain heavy duplication
(the same import appears in dynsym, symtab, version tables, and GOT/PLT maps),
so the same work repeats many times per binary.

## Proposed API

### Python

```python
multi_demangle.demangle_symbols(
    symbols: Sequence[str],
    options: DemangleOptions | None = None,
    *,
    unique: bool = True,     # dedupe before demangling, map results back
) -> list[str]
```

- Releases the GIL for the whole batch (`pyo3::Python::allow_threads`) so blint
  stays responsive and threads can overlap.
- `unique=True` (default) demangles each distinct string once — measured
  duplication in real symbol tables is significant — then fans results back out
  positionally.
- An `iterator=` variant can be added later if blint wants streaming; start
  with the list version which covers all current call sites.

### Rust

```rust
/// Demangles an iterator of symbols with a shared per-batch memo table.
/// Also usable without Python; `parallel` feature adds rayon-based
/// parallelism (detection/demangling is CPU-bound and lock-free).
pub fn demangle_iter<'a, I>(symbols: I, opts: DemangleOptions) -> impl Iterator<Item = Cow<'a, str>>
```

Optional `parallel` cargo feature (rayon). Off by default to keep the build
lean; blint's binaries are large enough that they can enable it in their wheel
only if benchmarks justify it.

## Implementation steps

1. Extract the current per-symbol pipeline into `fn demangle_one(sym: &str, opts) -> Cow<str>`
   shared by `demangle` and the batch path (no behavior change).
2. Implement `demangle_symbols` in the pymodule: collect input, build a
   `HashMap<&str, usize>` of unique symbols, demangle each once inside
   `allow_threads`, scatter results back. Return a list.
3. Benchmark with `criterion`: (a) synthetic 100k-symbol mixes (Rust/C++/Swift/
   unmangled, realistic ratios), (b) real symbol dumps — export symtabs from a
   release Rust binary and a Swift macOS dylib via `nm` and check the files
   into a new `tests/corpus/` directory (shared with Plan 05; today only the
   `assert_demangle!` macro helper exists under `tests/utils/`).
4. Parallel variant behind the `parallel` feature; chunk by unique index.
5. Document in README; blint follow-up PRs:
   - `parse_symbols` / `parse_pe_imports` / Mach-O maps: batch demangle once,
     index results by raw name; fix the double-demangle at `binary.py:1124-1125`;
   - skip demangling for strings LIEF reports as already-demangled, or pass a
     `pre_demangled=True` path that runs only hygiene (Plan 01) on them;
   - gate calls on `looks_mangled()` (Plan 01) so `libc.so.6`-style entries
     never reach the FFI.

## Expected wins (to be validated by the benchmarks)

- FFI crossing overhead: ~20 per-symbol call sites → a handful of batch calls.
- Duplicate elimination: same import demangled once instead of 3-5× across
  dynsym/symtab/versions/PLT.
- GIL release lets blint's Rich console/progress rendering overlap with work.

## Risks & mitigations

- **Memory**: batch returns one String per input; for 500k symbols that is fine
  (tens of MB worst case). blint can chunk with `itertools.islice` if needed.
- **Order preservation** is guaranteed by the scatter step; tested with
  duplicate-heavy inputs.

## Acceptance criteria

- `demangle_symbols` exposed with order-preserving, dedupe-by-default behavior.
- Criterion benchmarks merged under `benches/` with baseline numbers recorded
  in the PR; ≥2× throughput on the mixed real-world corpus vs the naive loop.
- blint hot paths migrated; `pytest` green.
