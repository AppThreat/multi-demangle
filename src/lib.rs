//! Demangling support for various languages and compilers.
//!
//! Currently supported languages are:
//!
//! - C++ (Itanium, GNU v2, CodeWarrior, and MSVC) (`features = ["cpp", "gnuv2", "codewarrior", "msvc"]`)
//! - Rust (both `legacy` and `v0`) (`features = ["rust"]`)
//! - Scala Native via the unknown-language fallback (`features = ["scala-native"]`)
//! - Swift (up to Swift 6.3) (`features = ["swift"]`)
//! - ObjC (only symbol detection)
//!
//! As the demangling schemes for the languages are different, the supported demangling features are
//! inconsistent. For example, argument types were not encoded in legacy Rust mangling and thus not
//! available in demangled names.
//! The demangling results should not be considered stable, and may change over time as more
//! demangling features are added.
//!
//! # Examples
//!
//! ```rust
//! # #[cfg(feature = "rust")] {
//! use symbolic_common::{Language, Name};
//! use multi_demangle::{Demangle, DemangleOptions};
//!
//! let name = Name::from("__ZN3std2io4Read11read_to_end17hb85a0f6802e14499E");
//! assert_eq!(name.detect_language(), Language::Rust);
//! assert_eq!(
//!     name.try_demangle(DemangleOptions::complete()),
//!     "std::io::Read::read_to_end"
//! );
//! # }
//! ```
//!
//! On top of demangling, [`classify_symbol`], [`looks_mangled`], and
//! [`normalize_symbol`] provide cheap symbol hygiene: prefix-based mangling
//! detection, classification of linker decorations (`__imp_`, `@plt`,
//! `@GLIBC_2.2.5`, ...), and normalization of raw symbol names.
//!
//! For symbol tables and other bulk inputs, [`demangle_one`] and
//! [`demangle_iter`] expose the per-symbol and deduplicating batch pipelines;
//! the Python module mirrors them as `demangle_symbols`.
//!
//! [`Demangle::demangle_structured`] goes one step further than the string
//! APIs and extracts typed structure from the rendering — namespace path,
//! leaf name, entity [`kind`](DemangledKind), generics, parameters, return
//! type, and compiler hashes — as a [`DemangledInfo`]; the Python module
//! mirrors it as `demangle_symbol_structured`.

#![warn(missing_docs)]

use std::borrow::Cow;
use std::collections::HashMap;
#[cfg(feature = "swift")]
use std::ffi::{CStr, CString};
#[cfg(feature = "swift")]
use std::os::raw::{c_char, c_int};

use symbolic_common::{Language, Name, NameMangling};

mod hygiene;
mod structured;

pub use hygiene::{
    classify_symbol, detect_language, is_scala_native_symbol, language_name, looks_mangled,
    normalize_symbol, Decoration, Normalizer, SymbolStatus,
};
pub use structured::{DemangledInfo, DemangledKind};

// Feature flags forwarded over FFI to the vendored Swift demangler in `src/swiftdemangle.cpp`.
// The values must stay in sync with the `SYMBOLIC_SWIFT_FEATURE_*` defines in that file.
#[cfg(feature = "swift")]
const SYMBOLIC_SWIFT_FEATURE_RETURN_TYPE: c_int = 0x1;
#[cfg(feature = "swift")]
const SYMBOLIC_SWIFT_FEATURE_PARAMETERS: c_int = 0x2;

// Thin C ABI over the vendored Swift standard library demangler, compiled by `build.rs`.
// Both functions return 0 on failure and non-zero on success.
#[cfg(feature = "swift")]
extern "C" {
    /// Demangles a Swift symbol into the provided buffer, honoring the feature flags.
    fn multi_demangle_swift(
        sym: *const c_char,
        buf: *mut c_char,
        buf_len: usize,
        features: c_int,
    ) -> c_int;

    /// Checks whether the symbol is mangled in any known Swift scheme.
    fn multi_demangle_is_swift_symbol(sym: *const c_char) -> c_int;

    /// Writes the demangler's node-tree dump for the symbol into the buffer.
    fn multi_demangle_swift_dump(sym: *const c_char, buf: *mut c_char, buf_len: usize) -> c_int;
}

/// Options for [`Demangle::demangle`].
///
/// One can chose from complete, or name-only demangling, and toggle specific demangling features
/// explicitly.
///
/// The resulting output depends very much on the language of the mangled [`Name`], and may change
/// over time as more fine grained demangling options and features are added. Not all options are
/// fully supported by each language, and not every feature is mutually exclusive on all languages.
///
/// # Examples
///
/// ```
/// # #[cfg(feature = "swift")] {
/// use symbolic_common::{Name, NameMangling, Language};
/// use multi_demangle::{Demangle, DemangleOptions};
///
/// let symbol = Name::new("$s8mangling12GenericUnionO3FooyACyxGSicAEmlF", NameMangling::Mangled, Language::Swift);
///
/// let simple = symbol.demangle(DemangleOptions::name_only()).unwrap();
/// assert_eq!(&simple, "GenericUnion.Foo<A>");
///
/// let full = symbol.demangle(DemangleOptions::complete()).unwrap();
/// assert_eq!(&full, "mangling.GenericUnion.Foo<A>(mangling.GenericUnion<A>.Type) -> (Swift.Int) -> mangling.GenericUnion<A>");
/// # }
/// ```
///
/// [`Demangle::demangle`]: trait.Demangle.html#tymethod.demangle
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DemangleOptions {
    return_type: bool,
    parameters: bool,
}

impl DemangleOptions {
    /// DemangleOptions that output a complete verbose demangling.
    pub const fn complete() -> Self {
        Self {
            return_type: true,
            parameters: true,
        }
    }

    /// DemangleOptions that output the most simple (likely name-only) demangling.
    pub const fn name_only() -> Self {
        Self {
            return_type: false,
            parameters: false,
        }
    }

    /// Determines whether a functions return type should be demangled.
    pub const fn return_type(mut self, return_type: bool) -> Self {
        self.return_type = return_type;
        self
    }

    /// Determines whether function argument types should be demangled.
    pub const fn parameters(mut self, parameters: bool) -> Self {
        self.parameters = parameters;
        self
    }
}

/// Detects Objective-C method selectors, which look like `-[Class method]`
/// (instance methods) or `+[Class method]` (class methods).
pub(crate) fn is_maybe_objc(ident: &str) -> bool {
    (ident.starts_with("-[") || ident.starts_with("+[")) && ident.ends_with(']')
}

/// Detects C++ symbols mangled with the Itanium ABI (`_Z...`) as emitted by GCC and Clang.
/// The additional leading underscores cover platform prefixes added on macOS (`__Z`)
/// and Windows (`___Z`), plus symbols passed through an extra mangling pass (`____Z`).
fn is_maybe_cpp(ident: &str) -> bool {
    ident.starts_with("_Z")
        || ident.starts_with("__Z")
        || ident.starts_with("___Z")
        || ident.starts_with("____Z")
}

/// Detects symbols mangled by the Microsoft Visual C++ name mangling scheme.
fn is_maybe_msvc(ident: &str) -> bool {
    ident.starts_with('?') || ident.starts_with("@?")
}

/// An MD5 mangled name consists of the prefix "??@", 32 hex digits,
/// and the suffix "@".
pub(crate) fn is_maybe_md5(ident: &str) -> bool {
    if ident.len() != 36 {
        return false;
    }

    ident.starts_with("??@")
        && ident.ends_with('@')
        && ident[3..35].chars().all(|c| c.is_ascii_hexdigit())
}

/// Delegates Swift symbol detection to the vendored Swift demangler via FFI.
/// Symbols containing interior NUL bytes cannot be passed as C strings and are rejected.
#[cfg(feature = "swift")]
fn is_maybe_swift(ident: &str) -> bool {
    CString::new(ident)
        .map(|cstr| unsafe { multi_demangle_is_swift_symbol(cstr.as_ptr()) != 0 })
        .unwrap_or(false)
}

/// Without the `swift` feature no symbol can be classified as Swift.
#[cfg(not(feature = "swift"))]
fn is_maybe_swift(_ident: &str) -> bool {
    false
}

/// Demangles MSVC-mangled symbols, mapping [`DemangleOptions`] onto the
/// `msvc_demangler` crate's bitflags.
#[cfg(feature = "msvc")]
fn try_demangle_msvc(ident: &str, opts: DemangleOptions) -> Option<String> {
    use msvc_demangler::DemangleFlags as MsvcFlags;

    // the flags are bitflags
    let mut flags = MsvcFlags::COMPLETE
        | MsvcFlags::SPACE_AFTER_COMMA
        | MsvcFlags::HUG_TYPE
        | MsvcFlags::NO_MS_KEYWORDS
        | MsvcFlags::NO_CLASS_TYPE;
    if !opts.return_type {
        flags |= MsvcFlags::NO_FUNCTION_RETURNS;
    }
    if !opts.parameters {
        // a `NO_ARGUMENTS` flag is there in the code, but commented out
        flags |= MsvcFlags::NAME_ONLY;
    }

    msvc_demangler::demangle(ident, flags).ok()
}

#[cfg(not(feature = "msvc"))]
fn try_demangle_msvc(_ident: &str, _opts: DemangleOptions) -> Option<String> {
    None
}

/// Demangles symbols using the legacy GCC 2.x mangling scheme.
///
/// That crate has no output options, so the complete demangled name is post-processed by
/// [`normalize_cpp_like_output`] to honor the requested options.
#[cfg(feature = "gnuv2")]
fn try_demangle_gnuv2(ident: &str, opts: DemangleOptions) -> Option<String> {
    let config = gnuv2_demangle::DemangleConfig::new();
    gnuv2_demangle::demangle(ident, &config)
        .ok()
        .map(|demangled| normalize_cpp_like_output(&demangled, opts))
}

#[cfg(not(feature = "gnuv2"))]
fn try_demangle_gnuv2(_ident: &str, _opts: DemangleOptions) -> Option<String> {
    None
}

/// Demangles symbols produced by the Metrowerks CodeWarrior compilers.
///
/// `cwdemangle` always emits a full signature, so the result is post-processed by
/// [`normalize_cpp_like_output`] to honor the requested options.
#[cfg(feature = "codewarrior")]
fn try_demangle_codewarrior(ident: &str, opts: DemangleOptions) -> Option<String> {
    let options = cwdemangle::DemangleOptions::default();
    cwdemangle::demangle(ident, &options)
        .map(|demangled| normalize_cpp_like_output(&demangled, opts))
}

#[cfg(not(feature = "codewarrior"))]
fn try_demangle_codewarrior(_ident: &str, _opts: DemangleOptions) -> Option<String> {
    None
}

/// Demangles Scala Native symbols (prefixed with `_SM`).
///
/// This acts as the fallback for languages not detected by [`Demangle::detect_language`].
/// The result is post-processed by [`normalize_scala_native_output`] to honor the
/// requested options.
#[cfg(feature = "scala-native")]
fn try_demangle_scala_native(ident: &str, opts: DemangleOptions) -> Option<String> {
    scala_native_demangle::demangle_with_defaults(ident)
        .ok()
        .map(|demangled| normalize_scala_native_output(&demangled, opts))
}

#[cfg(not(feature = "scala-native"))]
fn try_demangle_scala_native(_ident: &str, _opts: DemangleOptions) -> Option<String> {
    None
}

/// Removes a suffix consisting of $ followed by 32 hex digits, if there is one,
/// otherwise returns its input.
pub(crate) fn strip_hash_suffix(ident: &str) -> &str {
    let len = ident.len();
    if len >= 33 {
        let mut char_iter = ident.char_indices();
        while let Some((pos, c)) = char_iter.next_back() {
            if (len - pos) == 33 && c == '$' {
                // If we have not yet returned we have a valid suffix to strip.  This is
                // safe because we know the current pos is on the start of the '$' char
                // boundary.
                return &ident[..pos];
            } else if (len - pos) > 33 || !c.is_ascii_hexdigit() {
                // If pos is more than 33 bytes from the end a multibyte char made us skip
                // pos 33, multibyte chars are not hexdigit or $ so nothing to strip.
                return ident;
            }
        }
    }
    ident
}

/// A [`std::fmt::Write`] adapter that fails once the accumulated string exceeds a fixed bound.
///
/// Used to cap demangled output: maliciously crafted symbols can encode a huge number of
/// substitutions, and without a bound the expanded output could exhaust memory.
struct BoundedString {
    str: String,
    bound: usize,
}

impl BoundedString {
    /// Creates an empty buffer that rejects writes beyond `bound` bytes.
    fn new(bound: usize) -> Self {
        Self {
            str: String::new(),
            bound,
        }
    }

    /// Consumes the buffer and returns the accumulated string.
    pub fn into_inner(self) -> String {
        self.str
    }
}

impl std::fmt::Write for BoundedString {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        // saturating_add guards against a length overflow on huge inputs,
        // treating it the same as exceeding the bound.
        if self.str.len().saturating_add(s.len()) > self.bound {
            return Err(std::fmt::Error);
        }
        self.str.write_str(s)
    }
}

// -----------------------------------------------------------------------------
// C++-like signature post-processing
//
// The GNU v2 and CodeWarrior demanglers always emit full signatures and offer no
// output options, so `DemangleOptions` is honored by stripping the return type
// and/or parameters from their textual output. These helpers parse the demangled
// string structurally (angle brackets for templates, parentheses for parameters)
// rather than with a full grammar.
// -----------------------------------------------------------------------------

/// Finds the byte index of the first `(` that opens a parameter list at template
/// depth zero, i.e. not inside `<...>`. Returns `None` if there is no such paren.
pub(crate) fn signature_prefix_end(demangled: &str) -> Option<usize> {
    let mut angle_depth = 0usize;
    for (idx, ch) in demangled.char_indices() {
        match ch {
            '<' => angle_depth = angle_depth.saturating_add(1),
            '>' => angle_depth = angle_depth.saturating_sub(1),
            '(' if angle_depth == 0 => return Some(idx),
            _ => {}
        }
    }
    None
}

/// C++ type and qualifier keywords that can precede `(` in a demangled string,
/// used to tell a leading return type apart from a function name.
fn is_cpp_like_type_keyword(candidate: &str) -> bool {
    matches!(
        candidate,
        "void"
            | "bool"
            | "char"
            | "short"
            | "int"
            | "long"
            | "float"
            | "double"
            | "wchar_t"
            | "signed"
            | "unsigned"
            | "const"
            | "volatile"
    )
}

/// Finds the byte index just past the `)` matching the `(` at `open_idx`.
/// Returns `None` if the parentheses are unbalanced.
pub(crate) fn matching_paren_end(demangled: &str, open_idx: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (idx, ch) in demangled[open_idx..].char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(open_idx + idx + ch.len_utf8());
                }
            }
            _ => {}
        }
    }
    None
}

/// Advances past trailing ` const` / ` volatile` qualifiers that follow a
/// parameter list, returning the byte index where the signature ends.
fn cpp_like_qualifier_end(demangled: &str, mut idx: usize) -> usize {
    while let Some(rest) = demangled.get(idx..) {
        if let Some(next) = rest.strip_prefix(" const") {
            idx = demangled.len() - next.len();
        } else if let Some(next) = rest.strip_prefix(" volatile") {
            idx = demangled.len() - next.len();
        } else {
            break;
        }
    }
    idx
}

/// Locates the function name and signature span in a demangled C++-like string.
///
/// Scans for a `(` at angle-bracket depth zero — a `(` inside `<...>`
/// belongs to a template argument's type (`function_ref<void ()>`), not to
/// the signature — that is preceded by something that looks like a function
/// name (not a bare type keyword, not empty). On success returns the
/// function name, the byte index of the opening `(`, and the byte index
/// just past the closing `)` including any trailing qualifiers.
pub(crate) fn analyze_cpp_like_signature(demangled: &str) -> Option<(String, usize, usize)> {
    let mut angle_depth = 0usize;
    for (idx, ch) in demangled.char_indices() {
        match ch {
            '<' => angle_depth = angle_depth.saturating_add(1),
            '>' => angle_depth = angle_depth.saturating_sub(1),
            '(' if angle_depth == 0 => {
                // The call operator renders its own empty pair as part of
                // the name: `operator()(args)` — the parameter list is the
                // second pair.
                let idx = if demangled[idx..].starts_with("()(") {
                    idx + 2
                } else {
                    idx
                };

                let prefix = demangled[..idx].trim_end();
                let candidate = trim_cpp_like_name_prefix(prefix);
                // Skip candidate "names" that are actually return types (e.g. "void") or
                // artifacts of operator/template syntax without a proper identifier.
                if candidate.is_empty()
                    || is_cpp_like_type_keyword(&candidate)
                    || !prefix.ends_with(&candidate)
                {
                    continue;
                }

                let params_end = matching_paren_end(demangled, idx)?;
                let signature_end = cpp_like_qualifier_end(demangled, params_end);
                return Some((candidate, idx, signature_end));
            }
            _ => {}
        }
    }

    None
}

/// Extracts the function name from a signature prefix: everything after the
/// last space that sits outside any `<...>` group. Spaces inside template
/// arguments (`function_ref<void ()>`) do not separate a return type from
/// the name, and `operator ...` names are kept verbatim since they
/// legitimately contain spaces.
fn trim_cpp_like_name_prefix(prefix: &str) -> String {
    let prefix = prefix.trim();
    if prefix.starts_with("operator ") {
        return prefix.to_string();
    }

    let mut depth = 0usize;
    let mut last_top_level_space = None;
    for (idx, ch) in prefix.char_indices() {
        match ch {
            '<' => depth = depth.saturating_add(1),
            '>' => depth = depth.saturating_sub(1),
            ' ' if depth == 0 => last_top_level_space = Some(idx),
            _ => {}
        }
    }

    match last_top_level_space {
        Some(pos) => prefix[pos + 1..].to_string(),
        None => prefix.to_string(),
    }
}

/// Removes a leading return type from a demangled C++-like string, keeping the
/// name, parameters, and trailing qualifiers. Falls back to progressively more
/// heuristic trimming when no analyzable signature is found.
fn strip_cpp_like_return_type(demangled: &str) -> String {
    if let Some((name, params_start, signature_end)) = analyze_cpp_like_signature(demangled) {
        return format!("{name}{}", &demangled[params_start..signature_end]);
    }

    // No parameter list: drop everything before the last token (the return type).
    let Some(sig_start) = signature_prefix_end(demangled) else {
        return trim_cpp_like_name_prefix(demangled);
    };

    let prefix = trim_cpp_like_name_prefix(&demangled[..sig_start]);
    format!("{prefix}{}", &demangled[sig_start..])
}

/// Removes the parameter list (and any trailing qualifiers) from a demangled
/// C++-like string, leaving just the function name. Falls back to trimming the
/// whole string when no analyzable signature is found.
fn strip_cpp_like_parameters(demangled: &str) -> String {
    if let Some((name, _, _)) = analyze_cpp_like_signature(demangled) {
        return name;
    }

    let Some(sig_start) = signature_prefix_end(demangled) else {
        return demangled.trim().to_string();
    };

    demangled[..sig_start].trim().to_string()
}

/// Applies [`DemangleOptions`] to the full output of demanglers that cannot
/// limit their own output (GNU v2, CodeWarrior) by stripping the return type
/// and/or parameters as requested.
fn normalize_cpp_like_output(demangled: &str, opts: DemangleOptions) -> String {
    let mut normalized = demangled.trim().to_string();

    if !opts.return_type {
        normalized = strip_cpp_like_return_type(&normalized);
    }
    if !opts.parameters {
        normalized = strip_cpp_like_parameters(&normalized);
    }

    normalized
}

/// Applies [`DemangleOptions`] to the full output of the Scala Native demangler.
/// Its signatures look like `pkg.Class.method(Types): ReturnType`, so the return
/// type is everything after the last `": "` and parameters start at the first `(`.
fn normalize_scala_native_output(demangled: &str, opts: DemangleOptions) -> String {
    let mut normalized = demangled.trim().to_string();

    if !opts.return_type {
        if let Some((prefix, _)) = normalized.rsplit_once(": ") {
            normalized = prefix.to_string();
        }
    }
    if !opts.parameters {
        if let Some(sig_start) = normalized.find('(') {
            normalized = normalized[..sig_start].to_string();
        }
    }

    normalized
}

/// Demangles C++ symbols, dispatching on the mangling scheme.
///
/// MSVC symbols go to `msvc_demangler`; Itanium ABI symbols go to `cpp_demangle`, with
/// GNU v2 and CodeWarrior attempted as fallbacks for symbols that Itanium parsing
/// rejects. Cargo features disable individual backends, in which case those attempts
/// simply return `None`.
fn try_demangle_cpp(ident: &str, opts: DemangleOptions) -> Option<String> {
    if is_maybe_msvc(ident) {
        return try_demangle_msvc(ident, opts);
    }

    #[cfg(feature = "cpp")]
    if is_maybe_cpp(ident) {
        use cpp_demangle::{DemangleOptions as CppOptions, ParseOptions, Symbol as CppSymbol};

        // Some linkers append a `$` + 32 hex digits hash after the mangled name,
        // which the parser does not accept.
        let stripped = strip_hash_suffix(ident);

        let parse_options = ParseOptions::default().recursion_limit(160); // default is 96
        let symbol = match CppSymbol::new_with_options(stripped, &parse_options) {
            Ok(symbol) => symbol,
            // Not a valid Itanium symbol; maybe it uses the older GNU v2
            // or CodeWarrior scheme instead.
            Err(_) => {
                return try_demangle_gnuv2(ident, opts)
                    .or_else(|| try_demangle_codewarrior(ident, opts))
            }
        };

        let mut cpp_options = CppOptions::new().recursion_limit(192); // default is 128
        if !opts.parameters {
            cpp_options = cpp_options.no_params();
        }
        if !opts.return_type {
            cpp_options = cpp_options.no_return_type();
        }

        // Bound the maximum output string, as a huge number of substitutions could potentially
        // lead to a "Billion laughs attack".
        let mut buf = BoundedString::new(4096);

        return symbol
            .structured_demangle(&mut buf, &cpp_options)
            .ok()
            .map(|_| buf.into_inner());
    }
    #[cfg(not(feature = "cpp"))]
    let _ = opts;

    // The symbol did not start with a `_Z`-style prefix; it can still be GNU v2
    // or CodeWarrior mangled, since those schemes use different prefixes.
    try_demangle_gnuv2(ident, opts).or_else(|| try_demangle_codewarrior(ident, opts))
}

/// Demangles Rust symbols in both the legacy (`_ZN...17h<hash>E`) and v0
/// (`_R...`) schemes via `rustc_demangle`.
///
/// `{:#}` strips the trailing `::h<hash>` from legacy symbols. The options are
/// ignored: legacy mangling does not encode argument types, and `rustc_demangle`
/// exposes no output controls.
#[cfg(feature = "rust")]
fn try_demangle_rust(ident: &str, _opts: DemangleOptions) -> Option<String> {
    match rustc_demangle::try_demangle(ident) {
        Ok(demangled) => Some(format!("{demangled:#}")),
        Err(_) => None,
    }
}

#[cfg(not(feature = "rust"))]
fn try_demangle_rust(_ident: &str, _opts: DemangleOptions) -> Option<String> {
    None
}

/// Demangles Swift symbols through the vendored Swift demangler (see
/// `src/swiftdemangle.cpp`), passing the requested options as feature flags.
///
/// The output is written into a fixed 4 KiB buffer; symbols whose demangled form
/// does not fit are rejected rather than truncated.
#[cfg(feature = "swift")]
fn try_demangle_swift(ident: &str, opts: DemangleOptions) -> Option<String> {
    let mut buf = vec![0; 4096];
    let sym = match CString::new(ident) {
        Ok(sym) => sym,
        Err(_) => return None,
    };

    let mut features = 0;
    if opts.return_type {
        features |= SYMBOLIC_SWIFT_FEATURE_RETURN_TYPE;
    }
    if opts.parameters {
        features |= SYMBOLIC_SWIFT_FEATURE_PARAMETERS;
    }

    unsafe {
        match multi_demangle_swift(sym.as_ptr(), buf.as_mut_ptr(), buf.len(), features) {
            0 => None,
            _ => Some(CStr::from_ptr(buf.as_ptr()).to_string_lossy().to_string()),
        }
    }
}

#[cfg(not(feature = "swift"))]
fn try_demangle_swift(_ident: &str, _opts: DemangleOptions) -> Option<String> {
    None
}

/// Returns the vendored Swift demangler's node-tree dump for `sym`, which
/// exposes declaration structure (node kinds, modules, type contexts) that
/// the string rendering does not.
///
/// The dump buffer is a per-thread scratch allocation reserved once and
/// reused across calls, so structured mode never re-zeroes 64 KiB per
/// symbol. A dump that does not fit is rejected (`None`), which callers
/// treat as the signal to fall back to text-derived extraction.
#[cfg(feature = "swift")]
pub(crate) fn try_dump_swift(sym: &str) -> Option<String> {
    use std::cell::RefCell;

    thread_local! {
        /// Scratch buffer for node dumps; capacity is reserved without
        /// zero-filling and the C side always writes a NUL terminator.
        static DUMP_BUFFER: RefCell<Vec<c_char>> = const { RefCell::new(Vec::new()) };
    }

    let cstr = CString::new(sym).ok()?;
    DUMP_BUFFER.with(|cell| {
        let mut buffer = cell.borrow_mut();
        if buffer.capacity() == 0 {
            buffer.reserve(64 * 1024);
        }
        let ok = unsafe {
            multi_demangle_swift_dump(cstr.as_ptr(), buffer.as_mut_ptr(), buffer.capacity())
        };
        if ok == 0 {
            return None;
        }
        let dumped = unsafe { CStr::from_ptr(buffer.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        Some(dumped)
    })
}

/// Objective-C selectors are their own readable name, so "demangling" returns
/// the selector unchanged.
fn demangle_objc(ident: &str, _opts: DemangleOptions) -> String {
    ident.to_string()
}

/// Handles Objective-C++ symbols: selectors demangle to themselves, anything
/// else falls through to the C++ demanglers.
fn try_demangle_objcpp(ident: &str, opts: DemangleOptions) -> Option<String> {
    if is_maybe_objc(ident) {
        Some(demangle_objc(ident, opts))
    } else {
        try_demangle_cpp(ident, opts)
    }
}

/// An extension trait on `Name` for demangling names.
///
/// See the [module level documentation] for a list of supported languages.
///
/// [module level documentation]: index.html
pub trait Demangle {
    /// Infers the language of a mangled name.
    ///
    /// In case the symbol is not mangled or its language is unknown, the return value will be
    /// `Language::Unknown`. If the language of the symbol was specified explicitly, this is
    /// returned instead. For a list of supported languages, see the [module level documentation].
    ///
    /// # Examples
    ///
    /// ```
    /// use symbolic_common::{Language, Name};
    /// use multi_demangle::{Demangle, DemangleOptions};
    ///
    /// assert_eq!(Name::from("_ZN3foo3barEv").detect_language(), Language::Cpp);
    /// assert_eq!(Name::from("unknown").detect_language(), Language::Unknown);
    /// ```
    ///
    /// [module level documentation]: index.html
    fn detect_language(&self) -> Language;

    /// Demangles the name with the given options.
    ///
    /// Returns `None` in one of the following cases:
    ///  1. The language cannot be detected.
    ///  2. The language is not supported.
    ///  3. Demangling of the name failed.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "cpp")] {
    /// use symbolic_common::Name;
    /// use multi_demangle::{Demangle, DemangleOptions};
    ///
    /// assert_eq!(
    ///     Name::from("_ZN3foo3barEv").demangle(DemangleOptions::name_only()),
    ///     Some("foo::bar".to_string())
    /// );
    /// assert_eq!(
    ///     Name::from("unknown").demangle(DemangleOptions::name_only()),
    ///     None
    /// );
    /// # }
    /// ```
    fn demangle(&self, opts: DemangleOptions) -> Option<String>;

    /// Tries to demangle the name and falls back to the original name.
    ///
    /// Similar to [`demangle`], except that it returns a borrowed instance of the original name if
    /// the name cannot be demangled.
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "cpp")] {
    /// use symbolic_common::Name;
    /// use multi_demangle::{Demangle, DemangleOptions};
    ///
    /// assert_eq!(
    ///     Name::from("_ZN3foo3barEv").try_demangle(DemangleOptions::name_only()),
    ///     "foo::bar"
    /// );
    /// assert_eq!(
    ///     Name::from("unknown").try_demangle(DemangleOptions::name_only()),
    ///     "unknown"
    /// );
    /// # }
    /// ```
    ///
    /// [`demangle`]: trait.Demangle.html#tymethod.try_demangle
    fn try_demangle(&self, opts: DemangleOptions) -> Cow<'_, str>;

    /// Tries to demangle the name with the given options, falling back to
    /// symbol hygiene instead of the raw name.
    ///
    /// Like [`Demangle::try_demangle`], except that a name that cannot be
    /// demangled (or is not mangled at all) goes through the given
    /// [`Normalizer`] rather than being returned unchanged: legacy Rust
    /// `$`-escapes are decoded, Rust hash suffixes are trimmed, import
    /// pointer decoration is rewritten, and pseudo-symbols are mapped to
    /// readable placeholders. Successful demangled output is never
    /// normalized, since the passes could corrupt legitimate notation (for
    /// example Swift's `...` variadics).
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(feature = "cpp")] {
    /// use symbolic_common::Name;
    /// use multi_demangle::{Demangle, DemangleOptions, Normalizer};
    ///
    /// let name = Name::from("__imp__ZN3foo3barEv");
    /// assert_eq!(
    ///     name.try_demangle_normalized(DemangleOptions::complete(), &Normalizer::display()),
    ///     "__declspec(dllimport) _ZN3foo3barEv"
    /// );
    /// # }
    /// ```
    fn try_demangle_normalized(
        &self,
        opts: DemangleOptions,
        normalizer: &Normalizer,
    ) -> Cow<'_, str>;

    /// Demangles the name and extracts typed structure from the rendering.
    ///
    /// Returns a [`DemangledInfo`] with the namespace path, leaf name,
    /// entity kind, generic arguments, parameter and return types, and any
    /// compiler disambiguation hash, so consumers do not have to re-parse
    /// demangled text. The `display` field carries the rendering of `opts`;
    /// `simple` always carries a name-only rendering.
    ///
    /// Returns `None` when the name is not mangled in any known scheme or
    /// is an opaque MD5 name, mirroring [`Demangle::demangle`].
    ///
    /// # Examples
    ///
    /// ```
    /// # #[cfg(all(feature = "cpp", feature = "rust"))] {
    /// use symbolic_common::Name;
    /// use multi_demangle::{Demangle, DemangleOptions, DemangledKind};
    ///
    /// let info = Name::from("_ZN3std2io4Read11read_to_end17hb85a0f6802e14499E")
    ///     .demangle_structured(DemangleOptions::complete())
    ///     .unwrap();
    /// assert_eq!(info.namespace, ["std", "io", "Read"]);
    /// assert_eq!(info.name, "read_to_end");
    /// assert_eq!(info.kind, DemangledKind::Method);
    ///
    /// let cpp = Name::from("_ZN3foo3barEv")
    ///     .demangle_structured(DemangleOptions::complete())
    ///     .unwrap();
    /// assert_eq!(cpp.parameters, Some(Vec::new()));
    /// # }
    /// ```
    fn demangle_structured(&self, opts: DemangleOptions) -> Option<DemangledInfo>;
}

impl Demangle for Name<'_> {
    fn detect_language(&self) -> Language {
        // An explicitly assigned language always wins over heuristics.
        if self.language() != Language::Unknown {
            return self.language();
        }

        if is_maybe_objc(self.as_str()) {
            return Language::ObjC;
        }

        #[cfg(feature = "rust")]
        {
            // `rustc_demangle` accepts both legacy and v0 symbols, and its parser
            // is strict enough to avoid false positives on other schemes.
            if rustc_demangle::try_demangle(self.as_str()).is_ok() {
                return Language::Rust;
            }
        }

        // C++ covers several mangling schemes: Itanium and MSVC are detected by
        // prefix, while GNU v2 and CodeWarrior require an actual demangling pass.
        if is_maybe_cpp(self.as_str())
            || is_maybe_msvc(self.as_str())
            || try_demangle_gnuv2(self.as_str(), DemangleOptions::name_only()).is_some()
            || try_demangle_codewarrior(self.as_str(), DemangleOptions::name_only()).is_some()
        {
            return Language::Cpp;
        }

        if is_maybe_swift(self.as_str()) {
            return Language::Swift;
        }

        Language::Unknown
    }

    fn demangle(&self, opts: DemangleOptions) -> Option<String> {
        // Names known to be unmangled, as well as MD5-mangled names (which are
        // already opaque), are returned as-is.
        if matches!(self.mangling(), NameMangling::Unmangled) || is_maybe_md5(self.as_str()) {
            return Some(self.to_string());
        }

        match self.detect_language() {
            Language::ObjC => Some(demangle_objc(self.as_str(), opts)),
            Language::ObjCpp => try_demangle_objcpp(self.as_str(), opts),
            Language::Rust => try_demangle_rust(self.as_str(), opts),
            Language::Cpp => try_demangle_cpp(self.as_str(), opts),
            Language::Swift => try_demangle_swift(self.as_str(), opts),
            // Unknown languages may still be Scala Native, which is only
            // recognizable by attempting to demangle.
            _ => try_demangle_scala_native(self.as_str(), opts),
        }
    }

    fn try_demangle(&self, opts: DemangleOptions) -> Cow<'_, str> {
        if matches!(self.mangling(), NameMangling::Unmangled) {
            return Cow::Borrowed(self.as_str());
        }
        match self.demangle(opts) {
            Some(demangled) => Cow::Owned(demangled),
            None => Cow::Borrowed(self.as_str()),
        }
    }

    fn try_demangle_normalized(
        &self,
        opts: DemangleOptions,
        normalizer: &Normalizer,
    ) -> Cow<'_, str> {
        if matches!(self.mangling(), NameMangling::Unmangled) {
            return normalize_or_borrow(normalizer, self.as_str());
        }
        match self.demangle(opts) {
            Some(demangled) => Cow::Owned(demangled),
            None => normalize_or_borrow(normalizer, self.as_str()),
        }
    }

    fn demangle_structured(&self, opts: DemangleOptions) -> Option<DemangledInfo> {
        structured::demangle_structured(self, opts)
    }
}

/// Applies `normalizer` to `symbol`, borrowing when the passes left it
/// unchanged.
fn normalize_or_borrow<'a>(normalizer: &Normalizer, symbol: &'a str) -> Cow<'a, str> {
    match normalizer.normalize(symbol) {
        Cow::Borrowed(_) => Cow::Borrowed(symbol),
        Cow::Owned(owned) => Cow::Owned(owned),
    }
}

/// Demangles a single identifier with the given options and falls back to the
/// original symbol.
///
/// This is the per-symbol pipeline shared by [`demangle`] and the batch APIs
/// ([`demangle_iter`] and the Python `demangle_symbols`): the language is
/// auto-detected, the matching backend demangles the symbol, and unmangled or
/// unsupported inputs are returned unchanged (borrowed, without an
/// allocation).
///
/// # Examples
///
/// ```
/// # #[cfg(feature = "cpp")] {
/// use multi_demangle::{demangle_one, DemangleOptions};
///
/// assert_eq!(demangle_one("_ZN3foo3barEv", DemangleOptions::complete()), "foo::bar()");
/// assert_eq!(demangle_one("libc.so.6", DemangleOptions::complete()), "libc.so.6");
/// # }
/// ```
pub fn demangle_one(sym: &str, opts: DemangleOptions) -> Cow<'_, str> {
    match Name::from(sym).demangle(opts) {
        Some(demangled) => Cow::Owned(demangled),
        None => Cow::Borrowed(sym),
    }
}

/// Demangles a batch of symbols with a shared per-batch memo table.
///
/// Each distinct symbol is demangled at most once — symbol tables contain
/// heavy duplication (the same import appears in `dynsym`, `symtab`, version
/// tables, and GOT/PLT maps) — and the results are returned in input order,
/// falling back to the original symbol where demangling fails. The batch is
/// computed eagerly, so all demangling happens before this function returns.
///
/// The `parallel` cargo feature demangles the distinct symbols on the rayon
/// thread pool; detection and demangling are CPU-bound and use no shared
/// mutable state, so the batch is safe to run concurrently.
///
/// # Examples
///
/// ```
/// # #[cfg(all(feature = "cpp", feature = "rust"))] {
/// use multi_demangle::{demangle_iter, DemangleOptions};
///
/// let symbols = ["_ZN3foo3barEv", "libc.so.6", "_ZN3foo3barEv"];
/// let demangled = demangle_iter(symbols, DemangleOptions::complete());
/// assert_eq!(&demangled[0], "foo::bar()");
/// assert_eq!(&demangled[1], "libc.so.6");
/// assert_eq!(&demangled[2], "foo::bar()");
/// # }
/// ```
pub fn demangle_iter<'a, I>(symbols: I, opts: DemangleOptions) -> Vec<Cow<'a, str>>
where
    I: IntoIterator<Item = &'a str>,
{
    let symbols: Vec<&'a str> = symbols.into_iter().collect();
    demangle_batch_with(&symbols, true, |sym| demangle_one(sym, opts))
}

/// Groups `symbols` into its distinct values, demangles each once, and
/// returns the demangled uniques together with `assignment`, which maps every
/// input position to the index of its unique result.
///
/// The mapping lets callers share one instance of a result across all its
/// duplicates (the Python module builds a single string object per unique
/// symbol this way).
fn demangle_unique_with<'a, F>(
    symbols: &[&'a str],
    demangle_fn: &F,
) -> (Vec<Cow<'a, str>>, Vec<usize>)
where
    F: Fn(&'a str) -> Cow<'a, str> + Sync,
{
    let mut first_index: HashMap<&'a str, usize> = HashMap::with_capacity(symbols.len());
    let mut uniques: Vec<&'a str> = Vec::with_capacity(symbols.len());
    let mut assignment: Vec<usize> = Vec::with_capacity(symbols.len());
    for &sym in symbols {
        let idx = match first_index.entry(sym) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                let idx = uniques.len();
                entry.insert(idx);
                uniques.push(sym);
                idx
            }
            std::collections::hash_map::Entry::Occupied(entry) => *entry.get(),
        };
        assignment.push(idx);
    }

    (demangle_many(&uniques, demangle_fn), assignment)
}

/// Core batch pipeline shared by [`demangle_iter`] and the Python
/// `demangle_symbols`.
///
/// With `unique`, duplicate symbols are demangled once and their results are
/// scattered back over every occurrence; otherwise every position is
/// demangled independently. `demangle_fn` is called at most once per distinct
/// symbol (or once per position when `unique` is off).
fn demangle_batch_with<'a, F>(
    symbols: &[&'a str],
    unique: bool,
    demangle_fn: F,
) -> Vec<Cow<'a, str>>
where
    F: Fn(&'a str) -> Cow<'a, str> + Sync,
{
    let (mut results, assignment) = if unique {
        demangle_unique_with(symbols, &demangle_fn)
    } else {
        let results = demangle_many(symbols, &demangle_fn);
        let assignment: Vec<usize> = (0..results.len()).collect();
        (results, assignment)
    };

    // Scatter: every output position owns its string without re-demangling —
    // repeated occurrences clone the demangled value, and the last occurrence
    // of each symbol moves it out of the results table.
    let mut remaining = vec![0usize; results.len()];
    for &idx in &assignment {
        remaining[idx] += 1;
    }
    let mut out = Vec::with_capacity(assignment.len());
    for &idx in &assignment {
        if remaining[idx] > 1 {
            remaining[idx] -= 1;
            out.push(Cow::clone(&results[idx]));
        } else {
            out.push(std::mem::replace(&mut results[idx], Cow::Borrowed("")));
        }
    }
    out
}

/// Demangles every symbol in `symbols`, sequentially or — with the `parallel`
/// feature — on the rayon thread pool.
fn demangle_many<'a, F>(symbols: &[&'a str], demangle_fn: &F) -> Vec<Cow<'a, str>>
where
    F: Fn(&'a str) -> Cow<'a, str> + Sync,
{
    #[cfg(feature = "parallel")]
    {
        use rayon::prelude::*;

        symbols.par_iter().map(|&sym| demangle_fn(sym)).collect()
    }
    #[cfg(not(feature = "parallel"))]
    {
        symbols.iter().map(|&sym| demangle_fn(sym)).collect()
    }
}

/// Demangles an identifier and falls back to the original symbol.
///
/// This is a shortcut for [`Demangle::try_demangle`] with complete demangling.
///
/// # Examples
///
/// ```
/// # #[cfg(feature = "cpp")] {
/// assert_eq!(multi_demangle::demangle("_ZN3foo3barEv"), "foo::bar()");
/// # }
/// ```
///
/// [`Demangle::try_demangle`]: trait.Demangle.html#tymethod.try_demangle
pub fn demangle(ident: &str) -> Cow<'_, str> {
    demangle_one(ident, DemangleOptions::complete())
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn simple_md5() {
        let md5_mangled = "??@8ba8d245c9eca390356129098dbe9f73@";
        assert_eq!(
            Name::from(md5_mangled)
                .demangle(DemangleOptions::name_only())
                .unwrap(),
            md5_mangled
        );
    }

    #[test]
    fn test_strip_hash_suffix() {
        assert_eq!(
            strip_hash_suffix("hello$0123456789abcdef0123456789abcdef"),
            "hello"
        );
        assert_eq!(
            strip_hash_suffix("hello_0123456789abcdef0123456789abcdef"),
            "hello_0123456789abcdef0123456789abcdef",
        );
        assert_eq!(
            strip_hash_suffix("hello\u{1000}0123456789abcdef0123456789abcdef"),
            "hello\u{1000}0123456789abcdef0123456789abcdef"
        );
        assert_eq!(
            strip_hash_suffix("hello$0123456789abcdef0123456789abcdxx"),
            "hello$0123456789abcdef0123456789abcdxx"
        );
        assert_eq!(
            strip_hash_suffix("hello$\u{1000}0123456789abcdef0123456789abcde"),
            "hello$\u{1000}0123456789abcdef0123456789abcde"
        );
    }
}

#[cfg(feature = "extension-module")]
use pyo3::prelude::*;

/// A Python module for demangling symbols, implemented in Rust.
///
/// Exposed API (built with the `extension-module` feature via maturin):
/// - `DemangleOptions` — options class with `complete()` / `name_only()` constructors
///   and `return_type` / `parameters` / `normalize` keyword arguments.
/// - `Normalizer` — hygiene pass set with `display()` / `matching()` constructors.
/// - `demangle_symbol(mangled, options=None)` — demangle a symbol, falling back to
///   the original string if the language is unsupported or demangling fails.
/// - `demangle_symbols(symbols, options=None, *, unique=True)` — demangle a
///   batch (any iterable of strings) in one call with the GIL released;
///   duplicates are demangled once by default and share one string object,
///   and results keep the input order.
/// - `demangle_symbol_ex(mangled, options=None)` — demangle and return a dict
///   with the result plus language, status, and decoration classification.
/// - `demangle_symbol_structured(mangled, options=None)` — demangle and
///   return a `DemangledInfo` with namespace, name, kind, parameters,
///   return type, generics, and hash fields plus `to_dict()`; `None` when
///   the symbol is not mangled.
/// - `classify_symbol(mangled)` — the classification dict without demangling.
/// - `detect_language(mangled)` — the short language name, or `None`.
/// - `looks_mangled(mangled)` — cheap prefix-based mangling check.
/// - `normalize_symbol(mangled, normalizer=None)` — apply hygiene passes.
#[cfg(feature = "extension-module")]
#[pymodule]
mod multi_demangle {
    // Import necessary types from the parent `lib.rs` module
    use super::{
        classify_symbol as classify_symbol_impl, demangle_many, demangle_one, demangle_unique_with,
        detect_language as detect_language_impl, language_name,
        looks_mangled as looks_mangled_impl, normalize_or_borrow, Decoration, Demangle,
        DemangleOptions, DemangledInfo as StructuredInfo, Name, Normalizer, SymbolStatus,
    };
    use pyo3::exceptions::{PyTypeError, PyValueError};
    use pyo3::prelude::*;
    use pyo3::types::{PyAny, PyDict, PyList, PyString};
    use std::borrow::Cow;

    /// The status name, the innermost language name, and the outermost-first
    /// decoration `(kind, value)` list of a classification.
    type SymbolDescription = (
        &'static str,
        Option<&'static str>,
        Vec<(&'static str, Option<String>)>,
    );

    /// Python-visible wrapper around the Rust [`DemangleOptions`] plus the
    /// normalization fallback policy. `from_py_object` lets instances be
    /// passed as `options=` arguments directly.
    #[pyclass(name = "DemangleOptions", from_py_object)]
    #[derive(Clone, Copy, Debug)]
    struct PyDemangleOptions {
        opts: DemangleOptions,
        normalize: bool,
    }

    #[pymethods]
    impl PyDemangleOptions {
        /// Constructs options from three keyword arguments, defaulting to a
        /// complete demangling without normalization.
        #[new]
        #[pyo3(signature = (*, return_type=true, parameters=true, normalize=false))]
        fn new(return_type: bool, parameters: bool, normalize: bool) -> Self {
            Self {
                opts: DemangleOptions::complete()
                    .return_type(return_type)
                    .parameters(parameters),
                normalize,
            }
        }

        /// Creates DemangleOptions for a complete, verbose demangling.
        #[staticmethod]
        fn complete() -> Self {
            Self {
                opts: DemangleOptions::complete(),
                normalize: false,
            }
        }

        /// Creates DemangleOptions for a simple, name-only demangling.
        #[staticmethod]
        fn name_only() -> Self {
            Self {
                opts: DemangleOptions::name_only(),
                normalize: false,
            }
        }
    }

    /// Python-visible wrapper around the Rust [`Normalizer`] pass set.
    #[pyclass(name = "Normalizer", from_py_object)]
    #[derive(Clone, Copy, Debug)]
    struct PyNormalizer {
        inner: Normalizer,
    }

    #[pymethods]
    impl PyNormalizer {
        /// Constructs the default (display-oriented) normalizer.
        #[new]
        fn new() -> Self {
            Self {
                inner: Normalizer::display(),
            }
        }

        /// Display-oriented hygiene passes: legacy Rust escapes, Rust hash
        /// suffixes, import pointer decoration rewriting, and pseudo-symbol
        /// mapping.
        #[staticmethod]
        fn display() -> Self {
            Self {
                inner: Normalizer::display(),
            }
        }

        /// Matching-oriented hygiene passes: everything `display` does, plus
        /// `.llvm.` clone suffixes, PLT/GOT call stubs, and ELF version
        /// suffixes, with import pointers stripped instead of rewritten so
        /// results match the other binary's export table.
        #[staticmethod]
        fn matching() -> Self {
            Self {
                inner: Normalizer::matching(),
            }
        }

        /// Normalizes a raw symbol with the selected passes.
        fn normalize(&self, mangled: &str) -> String {
            self.inner.normalize(mangled).into_owned()
        }
    }

    /// Resolves the (options, normalize-fallback) pair for a call.
    fn resolve_options(options: Option<PyDemangleOptions>) -> (DemangleOptions, bool) {
        options.map_or((DemangleOptions::complete(), false), |o| {
            (o.opts, o.normalize)
        })
    }

    /// One-symbol pipeline for the Python batch: demangle with `opts`, and
    /// when `normalize` is set fall back to the display hygiene passes
    /// (mirroring `Name::try_demangle_normalized` for auto-detected names,
    /// but tying the fallback to the input's lifetime).
    fn demangle_py_one<'a>(
        sym: &'a str,
        opts: DemangleOptions,
        normalize: bool,
        normalizer: &Normalizer,
    ) -> Cow<'a, str> {
        if normalize {
            match Name::from(sym).demangle(opts) {
                Some(demangled) => Cow::Owned(demangled),
                None => normalize_or_borrow(normalizer, sym),
            }
        } else {
            demangle_one(sym, opts)
        }
    }

    /// Demangles an identifier and falls back to the original symbol.
    ///
    /// This function automatically detects the language of the mangled symbol
    /// and attempts to demangle it. With `normalize=True` on the options, a
    /// symbol that cannot be demangled goes through the display hygiene
    /// passes instead of being returned unchanged.
    #[pyfunction]
    #[pyo3(signature = (mangled, options = None))]
    fn demangle_symbol(mangled: &str, options: Option<PyDemangleOptions>) -> String {
        let (opts, normalize) = resolve_options(options);
        let name = Name::from(mangled);
        if normalize {
            name.try_demangle_normalized(opts, &Normalizer::display())
                .into_owned()
        } else {
            name.try_demangle(opts).into_owned()
        }
    }

    /// Demangles a batch of symbols in a single call.
    ///
    /// Takes any iterable of strings (list, tuple, generator, `map` object,
    /// ...) and returns a list of the same length with the results in input
    /// order; symbols that cannot be demangled keep their original string,
    /// exactly like `demangle_symbol`.
    ///
    /// The whole batch runs with the GIL released (`Python::detach`), so other
    /// Python threads can overlap their work with the demangling (which also
    /// runs on the rayon pool when the `parallel` cargo feature is enabled).
    ///
    /// With `unique=True` (the default) every distinct symbol is demangled at
    /// most once and the result is fanned back out over its duplicates — real
    /// symbol tables repeat the same import across dynsym, symtab, version
    /// tables, and GOT/PLT maps, so this removes most of the work. Duplicate
    /// positions share one string object, keeping the dedup win all the way
    /// to the caller. Pass `unique=False` to demangle every position
    /// independently.
    #[pyfunction]
    #[pyo3(signature = (symbols, options = None, *, unique = true))]
    fn demangle_symbols<'py>(
        py: Python<'py>,
        symbols: &Bound<'py, PyAny>,
        options: Option<PyDemangleOptions>,
        unique: bool,
    ) -> PyResult<Bound<'py, PyList>> {
        // A bare string would iterate per character; reject it explicitly.
        if symbols.is_instance_of::<PyString>() {
            return Err(PyTypeError::new_err(
                "demangle_symbols expects a sequence of strings, not a single string",
            ));
        }
        // Owned copies are required for soundness of the GIL release below:
        // borrowed string data could be freed by another thread mutating or
        // exhausting the iterable while the GIL is released. The copy is a
        // memcpy per symbol, trivial next to the demangling work.
        let mut collected: Vec<String> = Vec::new();
        for item in symbols.try_iter()? {
            collected.push(item?.extract::<String>()?);
        }
        let refs: Vec<&str> = collected.iter().map(String::as_str).collect();
        let (opts, normalize) = resolve_options(options);
        let normalizer = Normalizer::display();
        let (results, assignment) = py.detach(move || {
            let demangle_fn = |sym| demangle_py_one(sym, opts, normalize, &normalizer);
            if unique {
                demangle_unique_with(&refs, &demangle_fn)
            } else {
                let results = demangle_many(&refs, &demangle_fn);
                let assignment: Vec<usize> = (0..results.len()).collect();
                (results, assignment)
            }
        });

        // One string object per distinct result; duplicate positions share it
        // by reference instead of re-encoding the same bytes.
        let shared: Vec<Py<PyString>> = results
            .iter()
            .map(|result| PyString::new(py, result.as_ref()).unbind())
            .collect();
        let list = PyList::empty(py);
        for idx in assignment {
            list.append(shared[idx].clone_ref(py))?;
        }
        Ok(list)
    }

    /// Detects the language of a mangled symbol.
    ///
    /// Returns the short language name (`"cpp"`, `"rust"`, `"swift"`, ...),
    /// or `None` when the symbol is not mangled or its scheme is unknown.
    #[pyfunction]
    fn detect_language(mangled: &str) -> Option<&'static str> {
        detect_language_impl(mangled)
    }

    /// Cheaply checks whether a symbol looks mangled in any known scheme.
    ///
    /// Prefix-based over-approximation; never attempts a demangling pass.
    #[pyfunction]
    fn looks_mangled(mangled: &str) -> bool {
        looks_mangled_impl(mangled)
    }

    /// Normalizes a raw symbol with the display hygiene passes, or with the
    /// passes of a given `normalizer` (`Normalizer.display()` or
    /// `Normalizer.matching()`).
    #[pyfunction]
    #[pyo3(signature = (mangled, normalizer = None))]
    fn normalize_symbol(mangled: &str, normalizer: Option<PyNormalizer>) -> String {
        let normalizer = normalizer.map_or(Normalizer::display(), |n| n.inner);
        normalizer.normalize(mangled).into_owned()
    }

    /// Classifies a raw symbol without demangling it.
    ///
    /// Returns a dict with the same `status`, `language`, and `decorations`
    /// fields as `demangle_symbol_ex`, without paying for a demangle —
    /// suitable for deciding whether a symbol is worth demangling at all.
    #[pyfunction]
    fn classify_symbol<'py>(py: Python<'py>, mangled: &'py str) -> PyResult<Bound<'py, PyDict>> {
        let dict = PyDict::new(py);
        fill_classification(&dict, mangled)?;
        Ok(dict)
    }

    /// Demangles a symbol and returns rich classification information.
    ///
    /// The returned dict contains:
    ///
    /// - `mangled`: the original symbol
    /// - `demangled`: the demangled form (falls back to the raw symbol)
    /// - `status`: `"mangled"`, `"unmangled"`, or `"unsupported"` (mangled in
    ///   a language whose demangler backend is compiled out)
    /// - `language`: the short language name of the innermost symbol, or `None`
    /// - `decorations`: outermost-first list of `{"kind": ..., "value": ...}`
    ///   dicts; kinds are `import-pointer`, `call-stub`, `version`,
    ///   `linker-hash`, `cold-section`, `safe-seh`, `anonymous`,
    ///   `except-table`
    #[pyfunction]
    #[pyo3(signature = (mangled, options = None))]
    fn demangle_symbol_ex<'py>(
        py: Python<'py>,
        mangled: &'py str,
        options: Option<PyDemangleOptions>,
    ) -> PyResult<Bound<'py, PyDict>> {
        let (opts, normalize) = resolve_options(options);
        let demangled = if normalize {
            Name::from(mangled)
                .try_demangle_normalized(opts, &Normalizer::display())
                .into_owned()
        } else {
            Name::from(mangled).try_demangle(opts).into_owned()
        };

        let dict = PyDict::new(py);
        dict.set_item("mangled", mangled)?;
        dict.set_item("demangled", demangled)?;
        fill_classification(&dict, mangled)?;
        Ok(dict)
    }

    /// Sets the `status`, `language`, and `decorations` items on `dict` from
    /// the classification of `mangled`.
    fn fill_classification(dict: &Bound<'_, PyDict>, mangled: &str) -> PyResult<()> {
        let (status_name, language, decorations) = describe_status(&classify_symbol_impl(mangled));
        dict.set_item("status", status_name)?;
        dict.set_item("language", language)?;
        let decoration_dicts = decorations
            .into_iter()
            .map(|(kind, value)| {
                let item = PyDict::new(dict.py());
                item.set_item("kind", kind)?;
                if let Some(value) = value {
                    item.set_item("value", value)?;
                }
                Ok(item)
            })
            .collect::<PyResult<Vec<Bound<'_, PyDict>>>>()?;
        dict.set_item("decorations", decoration_dicts)?;
        Ok(())
    }

    /// Flattens a [`SymbolStatus`] into Python-friendly primitives: the
    /// status name, the innermost language name, and the outermost-first
    /// decoration list.
    fn describe_status(status: &SymbolStatus) -> SymbolDescription {
        let mut decorations = Vec::new();
        let mut current = status;
        while let SymbolStatus::Decorated { decoration, inner } = current {
            let entry = match decoration {
                Decoration::ImportPointer => ("import-pointer", None),
                Decoration::CallStub => ("call-stub", None),
                Decoration::Version(version) => ("version", Some(version.clone())),
                Decoration::LinkerHash => ("linker-hash", None),
                Decoration::ColdSection => ("cold-section", None),
                Decoration::SafeSeh => ("safe-seh", None),
                Decoration::Anonymous => ("anonymous", None),
                Decoration::ExceptTable => ("except-table", None),
            };
            decorations.push(entry);
            current = inner;
        }
        let (status_name, language) = match current {
            SymbolStatus::Mangled(language) => ("mangled", language_name(*language)),
            SymbolStatus::Unsupported(language) => ("unsupported", language_name(*language)),
            SymbolStatus::Unmangled => ("unmangled", None),
            SymbolStatus::Decorated { .. } => unreachable!("decorations are unwrapped above"),
        };
        (status_name, language, decorations)
    }

    /// Python-visible structured demangling result with read-only getters
    /// and a `to_dict` serialization.
    #[pyclass(name = "DemangledInfo")]
    struct PyDemangledInfo {
        info: StructuredInfo,
        language: Option<&'static str>,
    }

    #[pymethods]
    impl PyDemangledInfo {
        /// The short language name (`"cpp"`, `"rust"`, `"scala-native"`, ...),
        /// or `None` when unknown.
        #[getter]
        fn language(&self) -> Option<&'static str> {
            self.language
        }

        /// The full verbose rendering.
        #[getter]
        fn display(&self) -> String {
            self.info.display.clone()
        }

        /// The name-only rendering.
        #[getter]
        fn simple(&self) -> String {
            self.info.simple.clone()
        }

        /// The namespace/module/class path, outermost first.
        #[getter]
        fn namespace(&self) -> Vec<String> {
            self.info.namespace.clone()
        }

        /// The leaf name (function/method name, or the ObjC selector).
        #[getter]
        fn name(&self) -> String {
            self.info.name.clone()
        }

        /// The lowercase kind name (`"function"`, `"method"`, `"closure"`, ...).
        #[getter]
        fn kind(&self) -> String {
            self.info.kind.kind_name().to_string()
        }

        /// Whether this is an Objective-C class (`+`) method.
        #[getter]
        fn class_method(&self) -> Option<bool> {
            self.info.kind.class_method()
        }

        /// Parameter type renderings, when the scheme encodes them.
        #[getter]
        fn parameters(&self) -> Option<Vec<String>> {
            self.info.parameters.clone()
        }

        /// The return type rendering, when encoded.
        #[getter]
        fn return_type(&self) -> Option<String> {
            self.info.return_type.clone()
        }

        /// The captured disambiguation hash (and/or `.llvm.` clone counter).
        #[getter]
        fn hash(&self) -> Option<String> {
            self.info.hash.clone()
        }

        /// Generic/template argument renderings from the path, in path order.
        #[getter]
        fn template_args(&self) -> Option<Vec<String>> {
            self.info.template_args.clone()
        }

        /// Whether the name carries generic/template arguments.
        #[getter]
        fn is_generic(&self) -> bool {
            self.info.is_generic
        }

        /// The original mangled symbol.
        #[getter]
        fn mangled(&self) -> String {
            self.info.mangled.clone()
        }

        /// Serializes the structured info into a plain dict for JSON.
        fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
            let dict = PyDict::new(py);
            dict.set_item("mangled", &self.info.mangled)?;
            dict.set_item("demangled", &self.info.display)?;
            dict.set_item("simple", &self.info.simple)?;
            dict.set_item("language", self.language)?;
            dict.set_item("namespace", self.info.namespace.clone())?;
            dict.set_item("name", &self.info.name)?;
            dict.set_item("kind", self.info.kind.kind_name())?;
            if let Some(class_method) = self.info.kind.class_method() {
                dict.set_item("class_method", class_method)?;
            }
            dict.set_item("parameters", self.info.parameters.clone())?;
            dict.set_item("return_type", self.info.return_type.clone())?;
            dict.set_item("hash", self.info.hash.clone())?;
            dict.set_item("template_args", self.info.template_args.clone())?;
            dict.set_item("is_generic", self.info.is_generic)?;
            Ok(dict)
        }
    }

    /// Demangles a symbol and returns structured fields: namespace path,
    /// leaf name, kind, parameters, return type, generics, and hash.
    ///
    /// Returns `None` when the symbol is not mangled in any known scheme.
    /// The optional `options` argument behaves like `demangle_symbol`'s,
    /// except `normalize=True` is rejected: normalization only applies when
    /// demangling fails, and a failed demangling has no structure to
    /// return — use `demangle_symbol` for normalized strings.
    #[pyfunction]
    #[pyo3(signature = (mangled, options = None))]
    fn demangle_symbol_structured(
        mangled: &str,
        options: Option<PyDemangleOptions>,
    ) -> PyResult<Option<PyDemangledInfo>> {
        let (opts, normalize) = resolve_options(options);
        if normalize {
            return Err(PyValueError::new_err(
                "normalize is not supported by demangle_symbol_structured; use demangle_symbol for normalized string output",
            ));
        }
        match Name::from(mangled).demangle_structured(opts) {
            Some(info) => Ok(Some(PyDemangledInfo {
                info,
                language: detect_language_impl(mangled),
            })),
            None => Ok(None),
        }
    }
}
