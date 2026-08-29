// C ABI shim around the vendored Swift demangler (see vendor/swift/README.md).
// Compiled by build.rs and declared on the Rust side in src/lib.rs; the
// SYMBOLIC_SWIFT_FEATURE_* values must stay in sync with the constants there.
//
// Return protocol shared by both entry points below:
//   0            failure (not a Swift symbol, demangling failed, empty output)
//   > 0          success; number of bytes written including the NUL terminator
//   < 0          output did not fit; the negated required buffer size
//                (including the NUL) so the caller can retry with a larger
//                allocation instead of silently dropping the symbol
#include "swift/Demangling/Demangle.h"

#include <cstdint>
#include <cstring>
#include <string>

#define SYMBOLIC_SWIFT_FEATURE_RETURN_TYPE 0x1
#define SYMBOLIC_SWIFT_FEATURE_PARAMETERS 0x2
#define SYMBOLIC_SWIFT_FEATURE_ALL 0x3

namespace {
// Copies `output` into `buffer` as a C string. Returns 0 when the output is
// empty or too large to describe in the return value, the negated required
// size when it does not fit `buffer_length`, and otherwise the number of
// bytes written (including the NUL terminator).
int copy_to_buffer(const std::string &output, char *buffer, size_t buffer_length) {
    const size_t required = output.size() + 1;
    if (output.empty() || required > static_cast<size_t>(INT32_MAX)) {
        return 0;
    }
    if (required > buffer_length) {
        return -static_cast<int>(required);
    }
    std::memcpy(buffer, output.data(), output.size());
    buffer[output.size()] = '\0';
    return static_cast<int>(required);
}
} // namespace

extern "C" int multi_demangle_swift(const char *symbol,
                                       char *buffer,
                                       size_t buffer_length,
                                       int features) {
    swift::Demangle::DemangleOptions opts;

    // With all features requested, keep the default (fully verbose) options.
    // Otherwise start from the simplified profile and re-enable only the
    // requested pieces (return type / argument types). Upstream removed
    // ShowFunctionReturnType at Swift 6.3: return types now print whenever
    // ShowFunctionArgumentTypes is on, and DisplayEntityTypes no longer
    // suppresses them, so return_type=false + parameters=true renders the
    // return type anyway.
    if (features < SYMBOLIC_SWIFT_FEATURE_ALL) {
        opts = swift::Demangle::DemangleOptions::SimplifiedUIDemangleOptions();
        bool return_type = features & SYMBOLIC_SWIFT_FEATURE_RETURN_TYPE;
        bool argument_types = features & SYMBOLIC_SWIFT_FEATURE_PARAMETERS;

        opts.DisplayEntityTypes = return_type;
        opts.ShowFunctionArgumentTypes = argument_types;
    }

    std::string demangled =
        swift::Demangle::demangleSymbolAsString(llvm::StringRef(symbol), opts);

    // Reject empty results (not a Swift symbol); report results that do not
    // fit the caller's buffer (plus the NUL terminator) instead of truncating.
    return copy_to_buffer(demangled, buffer, buffer_length);
}

extern "C" int multi_demangle_is_swift_symbol(const char *symbol) {
    return swift::Demangle::isSwiftSymbol(symbol);
}

// Returns the demangler's node-tree dump for a mangled Swift symbol, which
// exposes structure (node kinds, declaration names, modules) that the plain
// string rendering does not. Uses the shared return protocol above.
extern "C" int multi_demangle_swift_dump(const char *symbol,
                                         char *buffer,
                                         size_t buffer_length) {
    swift::Demangle::Context context;
    swift::Demangle::NodePointer root =
        context.demangleSymbolAsNode(llvm::StringRef(symbol));
    if (root == nullptr) {
        return 0;
    }

    std::string dump = swift::Demangle::getNodeTreeAsString(root);
    return copy_to_buffer(dump, buffer, buffer_length);
}
