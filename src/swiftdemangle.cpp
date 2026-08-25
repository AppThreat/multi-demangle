// C ABI shim around the vendored Swift demangler (see vendor/swift/README.md).
// Compiled by build.rs and declared on the Rust side in src/lib.rs; the
// SYMBOLIC_SWIFT_FEATURE_* values must stay in sync with the constants there.
#include "swift/Demangling/Demangle.h"

#define SYMBOLIC_SWIFT_FEATURE_RETURN_TYPE 0x1
#define SYMBOLIC_SWIFT_FEATURE_PARAMETERS 0x2
#define SYMBOLIC_SWIFT_FEATURE_ALL 0x3

extern "C" int multi_demangle_swift(const char *symbol,
                                       char *buffer,
                                       size_t buffer_length,
                                       int features) {
    swift::Demangle::DemangleOptions opts;

    // With all features requested, keep the default (fully verbose) options.
    // Otherwise start from the simplified profile and re-enable only the
    // requested pieces (return type / argument types).
    if (features < SYMBOLIC_SWIFT_FEATURE_ALL) {
        opts = swift::Demangle::DemangleOptions::SimplifiedUIDemangleOptions();
        bool return_type = features & SYMBOLIC_SWIFT_FEATURE_RETURN_TYPE;
        bool argument_types = features & SYMBOLIC_SWIFT_FEATURE_PARAMETERS;

        opts.DisplayEntityTypes = return_type;
        opts.ShowFunctionArgumentTypes = argument_types;
    }

    std::string demangled =
        swift::Demangle::demangleSymbolAsString(llvm::StringRef(symbol), opts);

    // Reject empty results (not a Swift symbol) and results that do not fit the
    // caller's buffer (plus the NUL terminator) instead of truncating.
    if (demangled.size() == 0 || demangled.size() >= buffer_length) {
        return false;
    }

    memcpy(buffer, demangled.c_str(), demangled.size());
    buffer[demangled.size()] = '\0';
    return true;
}

extern "C" int multi_demangle_is_swift_symbol(const char *symbol) {
    return swift::Demangle::isSwiftSymbol(symbol);
}
