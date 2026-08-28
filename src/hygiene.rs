//! Symbol hygiene: cheap classification and normalization of raw symbol names.
//!
//! Demangling answers "what does this symbol mean". Hygiene answers the
//! supporting questions consumers face first: is this symbol mangled at all,
//! which language does it belong to, and which linker or toolchain decoration
//! (`__imp_`, `@plt`, `@GLIBC_2.2.5`, ...) sits on top of it? The
//! [`Normalizer`] applies the corresponding cleanup rules to raw names.
//!
//! The rules encoded here mirror the heuristics that consumers of this crate
//! (notably OWASP blint) previously had to re-implement on top of demangled
//! strings; the legacy Rust escape decoding matches `rustc_demangle`'s own
//! printer.
//!
//! # Examples
//!
//! ```
//! use multi_demangle::{classify_symbol, looks_mangled, normalize_symbol};
//!
//! // Cheap, prefix-based mangling check; never attempts a demangling pass.
//! assert!(looks_mangled("_$s8mangling12GenericUnionO3FooyACyxGSicAEmlF"));
//! assert!(!looks_mangled("libc.so.6"));
//!
//! // Classification without demangling.
//! let status = classify_symbol("__imp_?foo@bar@@YAXXZ");
//! assert!(matches!(status, multi_demangle::SymbolStatus::Decorated { .. }));
//!
//! // Normalization of a raw name.
//! assert_eq!(
//!     normalize_symbol("std::io::Read::read_to_end::hb85a0f6802e14499"),
//!     "std::io::Read::read_to_end"
//! );
//! ```

use std::borrow::Cow;

use symbolic_common::{Language, Name};

use crate::{Demangle, DemangleOptions};

/// A linker or toolchain decoration detected on a raw symbol.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decoration {
    /// MSVC/PE import pointer decoration: `__imp_`, `_imp_`, `.rdata$`,
    /// or `.refptr.` prefixes.
    ImportPointer,
    /// A PLT/GOT call stub: an `@plt`/`@got`/... suffix or a `j_` thunk prefix.
    CallStub,
    /// An ELF symbol version suffix (`foo@GLIBC_2.2.5`, `foo@@GLIBC_2.2.5`).
    Version(String),
    /// A linker hash (`$` followed by 32 hex digits) appended to the symbol.
    LinkerHash,
    /// An LLVM cold-section split of a function (`foo.cold`).
    ColdSection,
    /// The COFF `@feat.00` SAFESEH flag pseudo-symbol.
    SafeSeh,
    /// An anonymous/unnamable value (`anon.`, `__imp_anon.`, `.L__unnamed`).
    Anonymous,
    /// A GCC exception handling landing pad (`GCC_except_table*`).
    ExceptTable,
}

/// What the demangler knows about a raw symbol without demangling it.
///
/// See [`classify_symbol`] for the constructor.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SymbolStatus {
    /// Not mangled; nothing to do.
    Unmangled,
    /// Mangled in this language and demangling is available.
    Mangled(Language),
    /// Mangled in this language, but the demangler backend is compiled out
    /// (the corresponding cargo feature is disabled).
    Unsupported(Language),
    /// A decoration wrapped around another symbol: `decoration` is the
    /// outermost layer and `inner` the classification of what remains.
    Decorated {
        /// The outermost decoration layer.
        decoration: Decoration,
        /// The classification of the symbol below the decoration.
        inner: Box<SymbolStatus>,
    },
}

/// Selects which hygiene passes [`Normalizer::normalize`] applies.
///
/// There are two ready-made pass sets, keyed to the two ways consumers use
/// raw symbol names:
///
/// - [`Normalizer::display`] cleans a name for humans: it decodes legacy
///   Rust escapes, trims Rust hash suffixes, rewrites import pointers to
///   `__declspec(dllimport) ...`, and maps pseudo-symbols to readable
///   placeholders. This mirrors the upstream consumer's display behavior.
/// - [`Normalizer::matching`] prepares names for cross-symbol matching: it
///   adds the `.llvm.N` clone suffix, PLT/GOT call stub, and ELF version
///   passes, and *strips* import pointers instead of rewriting them, so the
///   result is the name that appears in the other binary's export table.
///
/// # Examples
///
/// ```
/// use multi_demangle::Normalizer;
///
/// assert_eq!(Normalizer::display().normalize("__imp_anon.1234"), "anonymous");
/// assert_eq!(
///     Normalizer::display().normalize("__imp_CreateFileW"),
///     "__declspec(dllimport) CreateFileW"
/// );
/// assert_eq!(Normalizer::matching().normalize("__imp_CreateFileW"), "CreateFileW");
/// assert_eq!(Normalizer::matching().normalize("memcpy@plt"), "memcpy");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Normalizer {
    legacy_rust_escapes: bool,
    rust_hash: bool,
    llvm_suffix: bool,
    import_pointer_rewrite: bool,
    import_pointer_strip: bool,
    call_stubs: bool,
    elf_version: bool,
    pseudo_symbols: bool,
}

impl Normalizer {
    /// Creates a normalizer with the display-oriented passes enabled; see
    /// [`Normalizer::display`].
    pub const fn new() -> Self {
        Self::display()
    }

    /// Creates a normalizer with the display-oriented passes: legacy Rust
    /// escape decoding, Rust hash suffix trimming, import pointer rewriting,
    /// and pseudo-symbol mapping.
    pub const fn display() -> Self {
        Self {
            legacy_rust_escapes: true,
            rust_hash: true,
            llvm_suffix: false,
            import_pointer_rewrite: true,
            import_pointer_strip: false,
            call_stubs: false,
            elf_version: false,
            pseudo_symbols: true,
        }
    }

    /// Creates a normalizer with the matching-oriented passes: everything
    /// [`Normalizer::display`] does, plus `.llvm.N` clone suffixes, PLT/GOT
    /// call stubs, and ELF version suffixes, with import pointers stripped
    /// rather than rewritten.
    pub const fn matching() -> Self {
        Self {
            legacy_rust_escapes: true,
            rust_hash: true,
            llvm_suffix: true,
            import_pointer_rewrite: false,
            import_pointer_strip: true,
            call_stubs: true,
            elf_version: true,
            pseudo_symbols: true,
        }
    }

    /// Toggles decoding of legacy Rust `$`-escapes (`$LT$`, `$u5b$`, `..`, ...).
    pub const fn legacy_rust_escapes(mut self, on: bool) -> Self {
        self.legacy_rust_escapes = on;
        self
    }

    /// Toggles trimming of trailing Rust hash suffixes (`::h<8+ hex>`).
    pub const fn rust_hash(mut self, on: bool) -> Self {
        self.rust_hash = on;
        self
    }

    /// Toggles trimming of LLVM clone suffixes (`foo.llvm.123456`).
    pub const fn llvm_suffix(mut self, on: bool) -> Self {
        self.llvm_suffix = on;
        self
    }

    /// Toggles rewriting of import pointer decoration
    /// (`__imp_foo` becomes `__declspec(dllimport) foo`).
    pub const fn import_pointer_rewrite(mut self, on: bool) -> Self {
        self.import_pointer_rewrite = on;
        self
    }

    /// Toggles stripping of import pointer decoration
    /// (`__imp_foo` becomes `foo`).
    pub const fn import_pointer_strip(mut self, on: bool) -> Self {
        self.import_pointer_strip = on;
        self
    }

    /// Toggles stripping of PLT/GOT call stubs (`foo@plt`, `j_foo`).
    pub const fn call_stubs(mut self, on: bool) -> Self {
        self.call_stubs = on;
        self
    }

    /// Toggles stripping of ELF symbol version suffixes
    /// (`foo@GLIBC_2.2.5` becomes `foo`).
    pub const fn elf_version(mut self, on: bool) -> Self {
        self.elf_version = on;
        self
    }

    /// Toggles mapping of pseudo-symbols to readable placeholders
    /// (`GCC_except_table*` becomes `GCC_except_table`, `@feat.00` becomes
    /// `SAFESEH`, anonymous values become `anonymous`).
    pub const fn pseudo_symbols(mut self, on: bool) -> Self {
        self.pseudo_symbols = on;
        self
    }

    /// Applies the selected hygiene passes to a raw symbol.
    ///
    /// Returns the input unchanged (borrowed) when no pass matches, so the
    /// function is safe to apply to a table of mixed symbols. Applying the
    /// normalizer twice yields the same result as applying it once.
    ///
    /// The passes run in a fixed order: pseudo-symbol mapping, import
    /// pointers, `.llvm.` suffixes, Rust hash suffixes, legacy Rust escapes,
    /// call stubs, ELF versions. The `.llvm.` pass runs before the Rust hash
    /// pass so combined suffixes like `::h<hash>.llvm.<n>` strip cleanly.
    pub fn normalize<'a>(&self, symbol: &'a str) -> Cow<'a, str> {
        if symbol.is_empty() {
            return Cow::Borrowed(symbol);
        }

        if self.pseudo_symbols {
            if is_anonymous_symbol(symbol) {
                return Cow::Owned("anonymous".to_string());
            }
            if is_except_table_symbol(symbol) {
                return Cow::Owned("GCC_except_table".to_string());
            }
            if is_safe_seh_symbol(symbol) {
                return Cow::Owned("SAFESEH".to_string());
            }
        }

        // Strip offsets are computed against the current value first and
        // applied through `to_mut()` afterwards, so at most one allocation
        // happens no matter how many passes match.
        let mut current = Cow::Borrowed(symbol);

        if self.import_pointer_rewrite {
            if let Some(prefix_len) = import_pointer_prefix_len(&current) {
                current
                    .to_mut()
                    .replace_range(0..prefix_len, "__declspec(dllimport) ");
            }
        } else if self.import_pointer_strip {
            if let Some(prefix_len) = import_pointer_prefix_len(&current) {
                current.to_mut().replace_range(0..prefix_len, "");
            }
        }

        if self.llvm_suffix {
            if let Some(end) = llvm_suffix_end(&current) {
                current.to_mut().truncate(end);
            }
        }

        if self.rust_hash {
            if let Some(end) = rust_hash_end(&current) {
                current.to_mut().truncate(end);
            }
        }

        // The legacy escape pass only runs on symbols that actually look
        // like legacy Rust (a rust mangling prefix, or any `$` escape);
        // decoding `..` into `::` on arbitrary strings would corrupt normal
        // text such as `x...y`.
        if self.legacy_rust_escapes && (is_rust_prefix(&current) || current.contains('$')) {
            if let Cow::Owned(decoded) = decode_legacy_rust_escapes(&current) {
                current = Cow::Owned(decoded);
            }
        }

        if self.call_stubs {
            if let Some((start, at)) = call_stub_kept_range(&current) {
                let s = current.to_mut();
                s.replace_range(at..s.len(), "");
                s.replace_range(0..start, "");
            }
        }

        if self.elf_version {
            if let Some((base_end, _version)) = split_version(&current) {
                current.to_mut().truncate(base_end);
            }
        }

        current
    }
}

impl Default for Normalizer {
    fn default() -> Self {
        Self::new()
    }
}

/// Normalizes a raw symbol with the display-oriented hygiene passes.
///
/// Shorthand for [`Normalizer::display`]: decodes legacy Rust `$`-escapes,
/// trims Rust hash suffixes, rewrites import pointer decoration, and maps
/// pseudo-symbols to readable placeholders. Returns the input unchanged when
/// no pass matches.
///
/// # Examples
///
/// ```
/// use multi_demangle::normalize_symbol;
///
/// assert_eq!(
///     normalize_symbol("_ZN4core..vec..Vec$LT$u8$GT$17h0123abcdef456789E"),
///     "_ZN4core::vec::Vec<u8>17h0123abcdef456789E"
/// );
/// assert_eq!(
///     normalize_symbol("__imp__Z1fv"),
///     "__declspec(dllimport) _Z1fv"
/// );
/// ```
pub fn normalize_symbol(symbol: &str) -> Cow<'_, str> {
    Normalizer::display().normalize(symbol)
}

/// Cheaply checks whether a symbol looks mangled in any known scheme.
///
/// This is a deliberate over-approximation based on mangling prefixes only:
/// it never attempts a demangling pass, works regardless of which cargo
/// features are enabled, and is safe to use as a gate before demangling. A
/// `true` result does not guarantee that demangling will succeed, and schemes
/// without a stable prefix (GNU v2, CodeWarrior) are not detected; use
/// [`detect_language`] for full detection.
///
/// # Examples
///
/// ```
/// use multi_demangle::looks_mangled;
///
/// assert!(looks_mangled("__ZN3std2io4Read11read_to_end17hb85a0f6802e14499E"));
/// assert!(looks_mangled("_$s8mangling12GenericUnionO3FooyACyxGSicAEmlF"));
/// assert!(!looks_mangled("libc.so.6"));
/// // Rust v0 requires an uppercase start byte after `_R`, so plain C
/// // symbols like `_Reset` or `_RtlMoveMemory` are not flagged.
/// assert!(!looks_mangled("_Reset"));
/// ```
pub fn looks_mangled(symbol: &str) -> bool {
    crate::is_maybe_objc(symbol)
        || crate::is_maybe_cpp(symbol)
        || crate::is_maybe_msvc(symbol)
        || is_rust_prefix(symbol)
        || is_swift_prefix(symbol)
        || is_scala_native_prefix(symbol)
        // Legacy Rust symbols that only partially survived a previous
        // demangling pass keep their `$LT$`-style escapes; upstream
        // consumers gate on this too.
        || symbol.contains("$LT$")
}

/// Returns the short name of the language a symbol is mangled in
/// (`"cpp"`, `"rust"`, `"swift"`, `"objc"`, `"scala-native"`, ...), or
/// `None` when the symbol is unmangled or its scheme is unrecognized.
///
/// This is the single detection entry point shared by the Rust and Python
/// APIs. It carries two fallbacks beyond what [`Demangle::detect_language`]
/// reports: Swift symbols are recognized by their mangling prefix even when
/// the `swift` backend is compiled out, and Scala Native — which has no
/// [`Language`] variant — is recognized when its demangler accepts the
/// symbol.
///
/// # Examples
///
/// ```
/// use multi_demangle::detect_language;
///
/// assert_eq!(detect_language("_Z1hic"), Some("cpp"));
/// assert_eq!(detect_language("libc.so.6"), None);
/// ```
pub fn detect_language(symbol: &str) -> Option<&'static str> {
    match detected_language(symbol) {
        Some(language) => language_name(language),
        None if is_scala_native_symbol(symbol) => Some("scala-native"),
        None => None,
    }
}

/// Renders a [`Language`] as the short lowercase name used across the crate's
/// APIs (`"cpp"`, `"rust"`, `"objc"`, ...), or `None` for
/// [`Language::Unknown`].
pub fn language_name(language: Language) -> Option<&'static str> {
    match language {
        Language::Unknown => None,
        language => Some(language.name()),
    }
}

/// Checks whether the Scala Native demangler accepts this symbol.
///
/// Scala Native has no `symbolic_common::Language` variant and is recognized
/// only by demangling, mirroring the fallback in [`Demangle::demangle`].
pub fn is_scala_native_symbol(symbol: &str) -> bool {
    is_scala_native_symbol_impl(symbol)
}

/// Classifies a raw symbol without demangling it.
///
/// The returned [`SymbolStatus`] reports whether the symbol is mangled, in
/// which language, and which decorations (`__imp_`, `@plt`, ELF versions,
/// ...) wrap it, outermost-first. Classification is prefix- and pattern-based
/// and never attempts a demangling pass beyond what
/// [`Demangle::detect_language`] already performs.
///
/// # Examples
///
/// ```
/// use multi_demangle::{classify_symbol, Decoration, SymbolStatus};
/// use symbolic_common::Language;
///
/// assert_eq!(classify_symbol("libc.so.6"), SymbolStatus::Unmangled);
/// assert_eq!(
///     classify_symbol("__imp_?foo@bar@@YAXXZ"),
///     SymbolStatus::Decorated {
///         decoration: Decoration::ImportPointer,
///         inner: Box::new(SymbolStatus::Mangled(Language::Cpp)),
///     }
/// );
/// ```
pub fn classify_symbol(symbol: &str) -> SymbolStatus {
    // Pseudo-symbols are placeholders rather than real names; they never
    // wrap anything.
    if is_safe_seh_symbol(symbol) {
        return decorated(Decoration::SafeSeh, SymbolStatus::Unmangled);
    }
    if is_anonymous_symbol(symbol) {
        return decorated(Decoration::Anonymous, SymbolStatus::Unmangled);
    }
    if is_except_table_symbol(symbol) {
        return decorated(Decoration::ExceptTable, SymbolStatus::Unmangled);
    }

    if let Some(prefix_len) = import_pointer_prefix_len(symbol) {
        return decorated(
            Decoration::ImportPointer,
            classify_symbol(&symbol[prefix_len..]),
        );
    }

    if let Some((start, at)) = call_stub_kept_range(symbol) {
        return decorated(Decoration::CallStub, classify_symbol(&symbol[start..at]));
    }

    if let Some(base) = strip_cold_section(symbol) {
        return decorated(Decoration::ColdSection, classify_symbol(base));
    }

    if let Some(base) = strip_linker_hash(symbol) {
        return decorated(Decoration::LinkerHash, classify_symbol(base));
    }

    if let Some((base_end, version)) = split_version(symbol) {
        return decorated(
            Decoration::Version(version.to_string()),
            classify_symbol(&symbol[..base_end]),
        );
    }

    // Scala Native has no `Language` variant; prefix-matching symbols its
    // demangler accepts are reported as mangled in an unnamed language.
    if is_scala_native_symbol(symbol) {
        return SymbolStatus::Mangled(Language::Unknown);
    }

    match detected_language(symbol) {
        Some(language) if is_language_supported(language) => SymbolStatus::Mangled(language),
        Some(language) => SymbolStatus::Unsupported(language),
        None => SymbolStatus::Unmangled,
    }
}

fn decorated(decoration: Decoration, inner: SymbolStatus) -> SymbolStatus {
    SymbolStatus::Decorated {
        decoration,
        inner: Box::new(inner),
    }
}

/// The language of a symbol, including a cheap Swift prefix fallback for
/// builds with the `swift` backend compiled out.
fn detected_language(symbol: &str) -> Option<Language> {
    let language = Name::from(symbol).detect_language();
    if language != Language::Unknown {
        return Some(language);
    }
    #[cfg(not(feature = "swift"))]
    if is_swift_prefix(symbol) {
        return Some(Language::Swift);
    }
    None
}

/// Whether demangling is actually available for the language in this build.
fn is_language_supported(language: Language) -> bool {
    match language {
        Language::Rust => cfg!(feature = "rust"),
        Language::Swift => cfg!(feature = "swift"),
        // C++ dispatches to one of four backends; the language is demanglable
        // when any of them is compiled in. ObjC selectors pass through
        // unchanged and are always supported.
        Language::Cpp | Language::ObjCpp => {
            cfg!(feature = "cpp")
                || cfg!(feature = "msvc")
                || cfg!(feature = "gnuv2")
                || cfg!(feature = "codewarrior")
        }
        _ => true,
    }
}

/// Legacy Rust symbols start with `_ZN` (`__ZN` with a macOS prefix); v0
/// symbols start with `_R` (`__R`) followed by an uppercase byte, mirroring
/// `rustc_demangle`'s own cheap validation ("paths always start with
/// uppercase characters"). This keeps plain C symbols such as `_Reset` or
/// `_RtlMoveMemory` from being flagged as Rust.
fn is_rust_prefix(symbol: &str) -> bool {
    if symbol.starts_with("_ZN") || symbol.starts_with("__ZN") {
        return true;
    }
    for prefix in ["_R", "__R"] {
        if let Some(rest) = symbol.strip_prefix(prefix) {
            return rest
                .as_bytes()
                .first()
                .is_some_and(|b| b.is_ascii_uppercase());
        }
    }
    false
}

/// Swift symbols use `$s`/`$S` (with an optional platform underscore prefix)
/// in the current mangling, and `_T0`/`_Tt` in the pre-Swift-5 schemes.
fn is_swift_prefix(symbol: &str) -> bool {
    symbol.starts_with("$s")
        || symbol.starts_with("$S")
        || symbol.starts_with("_$s")
        || symbol.starts_with("_$S")
        || symbol.starts_with("_T0")
        || symbol.starts_with("_Tt")
}

/// Scala Native symbols are prefixed with `_SM`.
fn is_scala_native_prefix(symbol: &str) -> bool {
    symbol.starts_with("_SM")
}

#[cfg(feature = "scala-native")]
fn is_scala_native_symbol_impl(symbol: &str) -> bool {
    is_scala_native_prefix(symbol)
        && Name::from(symbol)
            .demangle(DemangleOptions::name_only())
            .is_some()
}

#[cfg(not(feature = "scala-native"))]
fn is_scala_native_symbol_impl(_symbol: &str) -> bool {
    false
}

fn is_safe_seh_symbol(symbol: &str) -> bool {
    symbol.starts_with("@feat.00")
}

fn is_anonymous_symbol(symbol: &str) -> bool {
    symbol.starts_with("__imp_anon.")
        || symbol.starts_with("anon.")
        || symbol.starts_with(".L__unnamed")
}

fn is_except_table_symbol(symbol: &str) -> bool {
    symbol.starts_with("GCC_except_table")
}

/// MSVC-decorated symbols always begin with `?` (plain) or `@?` (import
/// thunks); their `@` and `?` characters are never suffix material. Checking
/// the prefix is both tighter and cheaper than scanning the whole name.
fn is_msvc_prefixed(symbol: &str) -> bool {
    symbol.starts_with('?') || symbol.starts_with("@?")
}

/// Byte length of the outermost import pointer prefix, if any.
fn import_pointer_prefix_len(symbol: &str) -> Option<usize> {
    for prefix in ["__imp_", "_imp_", ".rdata$", ".refptr."] {
        if let Some(inner) = symbol.strip_prefix(prefix) {
            if !inner.is_empty() {
                return Some(prefix.len());
            }
        }
    }
    None
}

/// Byte index where the symbol ends after removing an LLVM clone suffix
/// (`foo.llvm.123456`), if present.
fn llvm_suffix_end(symbol: &str) -> Option<usize> {
    let idx = symbol.rfind(".llvm.")?;
    if idx == 0 {
        return None;
    }
    let counter = &symbol[idx + 6..];
    if !counter.is_empty() && counter.bytes().all(|b| b.is_ascii_digit()) {
        Some(idx)
    } else {
        None
    }
}

/// Byte index where the symbol ends after removing a trailing Rust hash
/// suffix (`::h` + at least 8 lowercase hex digits), if present.
fn rust_hash_end(symbol: &str) -> Option<usize> {
    let idx = symbol.rfind("::h")?;
    if idx == 0 {
        return None;
    }
    let hash = &symbol[idx + 3..];
    if hash.len() >= 8 && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        Some(idx)
    } else {
        None
    }
}

/// Byte range of the symbol that remains after removing `j_` jump-thunk
/// prefixes from the front and trailing call stub suffixes (`@plt`,
/// `@GOTPCREL`, ...) from the back. The kept part is always contiguous.
fn call_stub_kept_range(symbol: &str) -> Option<(usize, usize)> {
    if is_msvc_prefixed(symbol) {
        return None;
    }
    let mut start = 0;
    while symbol[start..].starts_with("j_") {
        start += 2;
    }
    let mut at = symbol.len();
    loop {
        let window = &symbol[start..at];
        let Some(idx) = window.rfind('@') else {
            break;
        };
        if idx == 0 || !is_call_stub_token(&window[idx + 1..]) {
            break;
        }
        at = start + idx;
    }
    if start > 0 || at < symbol.len() {
        Some((start, at))
    } else {
        None
    }
}

/// Whether `token` names a known PLT/GOT call stub suffix.
fn is_call_stub_token(token: &str) -> bool {
    matches!(
        token.to_ascii_lowercase().as_str(),
        "plt" | "got" | "gotpcrel" | "gotoff"
    )
}

/// Strips a trailing `.cold` cold-section suffix, if present.
fn strip_cold_section(symbol: &str) -> Option<&str> {
    let base = symbol.strip_suffix(".cold")?;
    if base.is_empty() || base.ends_with('.') {
        None
    } else {
        Some(base)
    }
}

/// Strips a linker hash suffix (`$` + 32 hex digits), if present.
fn strip_linker_hash(symbol: &str) -> Option<&str> {
    let stripped = crate::strip_hash_suffix(symbol);
    if stripped.len() < symbol.len() {
        Some(stripped)
    } else {
        None
    }
}

/// Splits an ELF symbol version suffix, returning the byte index where the
/// base name ends together with the version token.
///
/// Real ELF versions are `@`/`@@` + a `NAME_1.2`-style token, so arbitrary
/// `@`-containing names (`foo@bar`, `main.init@v2`) are left intact. The
/// `@@` default-version form is accepted permissively; the single-`@` form
/// additionally requires an underscore or a dotted number.
fn split_version(symbol: &str) -> Option<(usize, &str)> {
    if is_msvc_prefixed(symbol) {
        return None;
    }
    let idx = symbol.rfind('@')?;
    if idx == 0 {
        return None;
    }
    let version = &symbol[idx + 1..];
    let default_form = symbol[..idx].ends_with('@');
    if !is_version_token(version, default_form) {
        return None;
    }
    let base_end = if default_form { idx - 1 } else { idx };
    if base_end == 0 {
        return None;
    }
    Some((base_end, version))
}

/// Whether `version` looks like a real ELF version token.
fn is_version_token(version: &str, default_form: bool) -> bool {
    if version.is_empty() || is_call_stub_token(version) {
        return false;
    }
    if !version
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b".-_+".contains(&b))
    {
        return false;
    }
    if default_form {
        return true;
    }
    version.contains('_') || (version.contains('.') && version.bytes().any(|b| b.is_ascii_digit()))
}

/// The named legacy Rust escapes, per `rustc_demangle`'s printer (which maps
/// `SP` to `@`); every other character is encoded as `$u<hex>$`.
const RUST_LEGACY_NAMED_ESCAPES: &[(&str, &str)] = &[
    ("SP", "@"),
    ("BP", "*"),
    ("RF", "&"),
    ("LT", "<"),
    ("GT", ">"),
    ("LP", "("),
    ("RP", ")"),
    ("C", ","),
];

/// A decoded legacy Rust escape: a fixed replacement or a `$u<hex>$` code
/// point.
enum LegacyEscape {
    Text(&'static str),
    Char(char),
}

/// Decodes the escape at the start of `rest`, returning the decoded value and
/// the number of bytes consumed. Returns `None` when `rest` does not start
/// with a valid escape.
fn decode_legacy_escape(rest: &str) -> Option<(LegacyEscape, usize)> {
    if !rest.starts_with('$') {
        return None;
    }
    let end = rest[1..].find('$')? + 2;
    if end == 2 {
        return None;
    }
    let escape = &rest[1..end - 1];
    for (name, replacement) in RUST_LEGACY_NAMED_ESCAPES {
        if escape == *name {
            return Some((LegacyEscape::Text(replacement), end));
        }
    }
    let digits = escape.strip_prefix('u')?;
    // rustc_demangle only accepts lowercase hex and refuses control code
    // points.
    let all_lower_hex = digits
        .bytes()
        .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    let code = u32::from_str_radix(digits, 16).ok()?;
    let ch = char::from_u32(code)?;
    if !all_lower_hex || ch.is_control() {
        return None;
    }
    Some((LegacyEscape::Char(ch), end))
}

/// Decodes legacy Rust `$`-escapes and `..` path separators, mirroring
/// `rustc_demangle`'s printer. Unknown escapes are kept verbatim (rather than
/// aborting the decode) so arbitrary consumer strings survive round-trips.
/// Borrows the input when nothing needs decoding.
fn decode_legacy_rust_escapes(symbol: &str) -> Cow<'_, str> {
    if !(symbol.contains('$') || symbol.contains("..")) {
        return Cow::Borrowed(symbol);
    }

    let mut out: Option<String> = None;
    let mut idx = 0;
    while idx < symbol.len() {
        let rest = &symbol[idx..];
        if rest.starts_with("..") {
            out.get_or_insert_with(|| String::from(&symbol[..idx]))
                .push_str("::");
            idx += 2;
        } else if rest.starts_with('.') {
            if let Some(out) = out.as_mut() {
                out.push('.');
            }
            idx += 1;
        } else if let Some((decoded, consumed)) = decode_legacy_escape(rest) {
            let out = out.get_or_insert_with(|| String::from(&symbol[..idx]));
            match decoded {
                LegacyEscape::Text(text) => out.push_str(text),
                LegacyEscape::Char(ch) => out.push(ch),
            }
            idx += consumed;
        } else {
            let ch = rest.chars().next().expect("non-empty remainder");
            if let Some(out) = out.as_mut() {
                out.push(ch);
            }
            idx += ch.len_utf8();
        }
    }

    match out {
        Some(out) => Cow::Owned(out),
        None => Cow::Borrowed(symbol),
    }
}
