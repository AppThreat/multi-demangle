# Vendored patch: gnuv2_demangle 0.4.0

This directory is the crate source of
[`gnuv2_demangle` 0.4.0](https://crates.io/crates/gnuv2_demangle)
(the latest release at the time of writing) with one local fix, wired in via
`[patch.crates-io]` in the root `Cargo.toml` and mirrored in `fuzz/Cargo.toml`.
The vendored `Cargo.toml` drops the upstream snapshot tests and their
dev-dependencies (`insta`, `pretty_assertions`) — the vendored subset is
build-time only; upstream's test suite runs against the registry crate.

## Why

`demangle_argument_list_impl` looped over an argument list with no progress
check: when `demangle_argument` returned success without consuming input, the
loop pushed the same argument forever, growing the argument `Vec` until the
allocator gave up. The detect fuzz target found it on its first run with the
input

```
00000L<e0000000e00__2cZN1255555555555555_00]0eeeee0Nf`>_Z__0v0
```

reached through `detect_language`, which attempts a full GNU v2 demangling
pass on every symbol that the C++ prefix predicates decline (the GNU v2 scheme
has no cheap prefix gate — its symbols do not start with a fixed marker).
libFuzzer's malloc hook intercepted a single 3 GiB reallocation.

Two fixes, both rejecting the symbol instead of misbehaving:

- `DemangledArg::Repeat` expanded `N<count><index>` by pushing `count - 1`
  entries into the argument vec with `count` taken verbatim from the symbol —
  the input carries a count of 1.25 quadrillion, so the vec tried to grow
  until the allocator gave up. Counts beyond 65535 (past any plausible real
  signature) now fail with `InvalidRepeatingArgument`.
- The argument-list loop had no progress check: an argument parse that
  consumed no input would loop forever. That now fails with the new
  `DemangleError::NoProgressOnArgument` variant.
- The array-length fixup added 1 to the parsed length unchecked; a symbol
  declaring `18446744073709551615` (found by the swift_ffi fuzz target
  through the detection path) overflowed. It now rejects.

Nothing else is changed.

## Maintenance

- Same policy as `vendor/scala-native-demangle/PATCH.md`: minimal fixes only;
  continued fuzzing is the check for other reachable bugs. Delete this
  directory and both `[patch.crates-io]` sections when upstream publishes a
  release with the progress guard.
