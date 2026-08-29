// Minimal stand-in for the upstream-generated llvm/Config/llvm-config.h.
//
// Upstream LLVM generates this file with CMake during its build; no source
// version exists in either the swift or llvm-project repositories. The
// vendored demangler subset only observes a handful of feature switches, so
// this stub pins them to the configuration the subset was validated with
// (matches building the old headers, where these macros were simply absent
// and every `#if` took the zero branch).
//
// Do not try to sync this file from upstream — scripts/sync-swift.sh skips it.
#ifndef LLVM_CONFIG_LLVM_CONFIG_H
#define LLVM_CONFIG_LLVM_CONFIG_H

// Asserts in the vendored LLVM headers (STLExtras.h) stay compiled out; the
// cc build also defines LLVM_DISABLE_ABI_BREAKING_CHECKS_ENFORCING=1.
#ifndef LLVM_ENABLE_ABI_BREAKING_CHECKS
#define LLVM_ENABLE_ABI_BREAKING_CHECKS 0
#endif

// Nothing in the vendored subset spawns threads; keep the header's threading
// shims compiled out.
#ifndef LLVM_ENABLE_THREADS
#define LLVM_ENABLE_THREADS 0
#endif

#endif // LLVM_CONFIG_LLVM_CONFIG_H
