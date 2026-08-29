// Minimal stand-in for the CMake-generated llvm/Config/abi-breaking.h.
//
// Upstream configures this file from abi-breaking.h.cmake during the LLVM
// build; no plain source version exists in either the swift or llvm-project
// repositories. The vendored demangler subset includes it for the
// LLVM_ENABLE_ABI_BREAKING_CHECKS switch only, which we pin off (matching the
// LLVM_DISABLE_ABI_BREAKING_CHECKS_ENFORCING=1 the cc build sets).
//
// Do not try to sync this file from upstream — scripts/sync-swift.sh skips it.
#ifndef LLVM_CONFIG_ABI_BREAKING_H
#define LLVM_CONFIG_ABI_BREAKING_H

#ifndef LLVM_ENABLE_ABI_BREAKING_CHECKS
#define LLVM_ENABLE_ABI_BREAKING_CHECKS 0
#endif

#endif // LLVM_CONFIG_ABI_BREAKING_H
