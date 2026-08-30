# Fuzz targets (Plan 05 §1)

Four libFuzzer targets asserting the crate's safety and liveness
properties. They catch panics, aborts, unbounded expansion, and
non-termination — the *wrong-output* bug class needs the differential
generator in `contrib/scripts/gen_dlang_symbols.py` +
`contrib/collect-corpus.sh diff-fuzz`, which is where this crate's real
bugs have lived.

| target      | entry point                          | properties                                              |
| ----------- | ------------------------------------ | ------------------------------------------------------- |
| `demangle`  | full pipeline, both option sets      | no panic/abort/OOM; output ≤ 1 MiB (the documented cap)  |
| `detect`    | `detect_language` + classify         | no panic; terminates (libFuzzer `-timeout`)              |
| `normalize` | hygiene passes, both pass sets       | idempotent (`normalize²(x) == normalize(x)`, see scope)  |
| `swift_ffi` | Swift FFI round-trip incl. detection | as above; run under ASan — the one memory-unsafety site  |

The `normalize` idempotence property is scoped to the strip passes plus
`$`-free inputs: legacy Rust escape decoding is deliberately one-level
(mirroring `rustc_demangle`), which makes universal idempotence
impossible by construction. See `fuzz_targets/normalize.rs`.

## Running

```bash
cargo install cargo-fuzz
./seed-corpus.sh                      # seeds from tests/corpus/*_symbols.txt
cargo +nightly fuzz run demangle -- -max_total_time=180
cargo +nightly fuzz run swift_ffi -- -max_total_time=180   # ASan
```

The swift C++ can also be exercised under UBSan through the CLI (rustc
has no `-Zsanitizer=undefined`, so the instrumented-corpus pass goes via
a UBSan-built binary instead of the fuzz runner):

```bash
UBSAN_RT=$(find "$(xcrun -show-sdk-path)/.." -name libclang_rt.ubsan_osx_dynamic.dylib | head -1)
CXXFLAGS="-fsanitize=undefined -fno-sanitize-recover=undefined" \
RUSTFLAGS="-C link-arg=$UBSAN_RT" cargo build --bin multi-demangle
cat corpus/swift_ffi/*.sym ../tests/corpus/swift_symbols.txt |
  UBSAN_OPTIONS=halt_on_error=1 target/debug/multi-demangle > /dev/null
```

Findings become regression tests in `tests/` citing the artifact input;
several upstream crates needed vendored fixes along the way (see
`vendor/*/PATCH.md`).
