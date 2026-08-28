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
//! strings.
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
///
/// Decorations wrap another symbol; [`classify_symbol`] reports them
/// outermost-first via [`SymbolStatus::Decorated`].
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
/// [`Normalizer::new`] enables the display-oriented passes (the behavior of
/// upstream consumers): legacy Rust escape decoding, Rust hash suffix
/// trimming, import pointer decoration, and pseudo-symbol mapping.
/// [`Normalizer::all`] additionally enables the passes used for cross-symbol
/// matching: `.llvm.N` suffixes, PLT/GOT call stubs, and ELF version suffixes.
///
/// # Examples
///
/// ```
/// use multi_demangle::Normalizer;
///
/// assert_eq!(Normalizer::new().normalize("__imp_anon.1234"), "anonymous");
/// assert_eq!(Normalizer::new().normalize("foo@plt"), "foo@plt");
/// assert_eq!(Normalizer::all().normalize("foo@plt"), "foo");
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Normalizer {
    legacy_rust_escapes: bool,
    rust_hash: bool,
    llvm_suffix: bool,
    import_pointer: bool,
    call_stubs: bool,
    elf_version: bool,
    pseudo_symbols: bool,
}

impl Normalizer {
    /// Creates a normalizer with the default (display-oriented) passes
    /// enabled: legacy Rust escapes, Rust hash suffixes, import pointer
    /// decoration, and pseudo-symbol mapping.
    pub const fn new() -> Self {
        Self {
            legacy_rust_escapes: true,
            rust_hash: true,
            llvm_suffix: false,
            import_pointer: true,
            call_stubs: false,
            elf_version: false,
            pseudo_symbols: true,
        }
    }

    /// Creates a normalizer with every hygiene pass enabled.
    pub const fn all() -> Self {
        Self {
            legacy_rust_escapes: true,
            rust_hash: true,
            llvm_suffix: true,
            import_pointer: true,
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
    pub const fn import_pointer(mut self, on: bool) -> Self {
        self.import_pointer = on;
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

        let mut current = Cow::Borrowed(symbol);

        if self.import_pointer {
            if let Some(inner) = strip_import_pointer(&current) {
                let owned = format!("__declspec(dllimport) {inner}");
                current = Cow::Owned(owned);
            }
        }

        if self.legacy_rust_escapes {
            if let Cow::Owned(decoded) = decode_legacy_rust_escapes(&current) {
                current = Cow::Owned(decoded);
            }
        }

        if self.rust_hash {
            if let Some(stripped) = strip_rust_hash_suffix(&current) {
                let owned = stripped.to_string();
                current = Cow::Owned(owned);
            }
        }

        if self.llvm_suffix {
            if let Some(stripped) = strip_llvm_suffix(&current) {
                let owned = stripped.to_string();
                current = Cow::Owned(owned);
            }
        }

        if self.call_stubs {
            let stripped = strip_thunk_prefixes(&current);
            if stripped.len() < current.len() {
                let owned = stripped.to_string();
                current = Cow::Owned(owned);
            }
            let stripped = strip_call_stub_suffixes(&current);
            if stripped.len() < current.len() {
                let owned = stripped.to_string();
                current = Cow::Owned(owned);
            }
        }

        if self.elf_version {
            if let Some((base, _version)) = split_version_suffix(&current) {
                let owned = base.to_string();
                current = Cow::Owned(owned);
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

/// Normalizes a raw symbol with the default hygiene passes.
///
/// Shorthand for [`Normalizer::new`]: decodes legacy Rust `$`-escapes, trims
/// Rust hash suffixes, rewrites import pointer decoration, and maps
/// pseudo-symbols to readable placeholders. Returns the input unchanged when
/// no pass matches.
///
/// # Examples
///
/// ```
/// use multi_demangle::normalize_symbol;
///
/// assert_eq!(normalize_symbol("foo..bar"), "foo::bar");
/// assert_eq!(
///     normalize_symbol("__imp__Z1fv"),
///     "__declspec(dllimport) _Z1fv"
/// );
/// ```
pub fn normalize_symbol(symbol: &str) -> Cow<'_, str> {
    Normalizer::new().normalize(symbol)
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
/// ```
pub fn looks_mangled(symbol: &str) -> bool {
    crate::is_maybe_objc(symbol)
        || crate::is_maybe_cpp(symbol)
        || crate::is_maybe_msvc(symbol)
        || is_rust_prefix(symbol)
        || is_swift_prefix(symbol)
        || is_scala_native_prefix(symbol)
}

/// Returns the detected [`Language`] of a possibly-mangled symbol, or
/// [`Language::Unknown`] when the symbol is unmangled or its scheme is not
/// recognized.
///
/// # Examples
///
/// ```
/// use multi_demangle::detect_language;
/// use symbolic_common::Language;
///
/// assert_eq!(detect_language("_Z1hic"), Language::Cpp);
/// assert_eq!(detect_language("libc.so.6"), Language::Unknown);
/// ```
pub fn detect_language(symbol: &str) -> Language {
    Name::from(symbol).detect_language()
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

    if let Some(inner) = strip_import_pointer(symbol) {
        return decorated(Decoration::ImportPointer, classify_symbol(inner));
    }

    if let Some(inner) = strip_thunk_prefixes_once(symbol) {
        return decorated(Decoration::CallStub, classify_symbol(inner));
    }

    if let Some(base) = strip_cold_section(symbol) {
        return decorated(Decoration::ColdSection, classify_symbol(base));
    }

    if let Some(base) = strip_linker_hash(symbol) {
        return decorated(Decoration::LinkerHash, classify_symbol(base));
    }

    // MSVC-mangled names carry `@` and `?` characters of their own; the
    // suffix strippers below refuse symbols containing `?`.
    if let Some(base) = strip_call_stub_suffix(symbol) {
        return decorated(Decoration::CallStub, classify_symbol(base));
    }
    if let Some((base, version)) = split_version_suffix(symbol) {
        return decorated(Decoration::Version(version), classify_symbol(base));
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
/// symbols start with `_R` (`__R`).
fn is_rust_prefix(symbol: &str) -> bool {
    symbol.starts_with("_ZN")
        || symbol.starts_with("__ZN")
        || symbol.starts_with("_R")
        || symbol.starts_with("__R")
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

/// Strips the outermost import pointer prefix, if any.
fn strip_import_pointer(symbol: &str) -> Option<&str> {
    for prefix in ["__imp_", "_imp_", ".rdata$", ".refptr."] {
        if let Some(inner) = symbol.strip_prefix(prefix) {
            if !inner.is_empty() {
                return Some(inner);
            }
        }
    }
    None
}

/// Strips one `j_` jump-thunk prefix, if present.
fn strip_thunk_prefixes_once(symbol: &str) -> Option<&str> {
    let inner = symbol.strip_prefix("j_")?;
    if inner.is_empty() {
        None
    } else {
        Some(inner)
    }
}

/// Strips all `j_` jump-thunk prefixes.
fn strip_thunk_prefixes(symbol: &str) -> &str {
    let mut symbol = symbol;
    while let Some(stripped) = strip_thunk_prefixes_once(symbol) {
        symbol = stripped;
    }
    symbol
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

/// Whether `token` names a known PLT/GOT call stub suffix.
fn is_call_stub_token(token: &str) -> bool {
    matches!(
        token.to_ascii_lowercase().as_str(),
        "plt" | "got" | "gotpcrel" | "gotoff"
    )
}

/// Strips one trailing call stub suffix (`@plt`, `@GOTPCREL`, ...), if any.
///
/// MSVC-mangled names carry `@` and `?` characters of their own, so symbols
/// containing `?` are never stripped.
fn strip_call_stub_suffix(symbol: &str) -> Option<&str> {
    if symbol.contains('?') {
        return None;
    }
    let idx = symbol.rfind('@')?;
    if idx == 0 {
        return None;
    }
    let token = &symbol[idx + 1..];
    if is_call_stub_token(token) {
        Some(&symbol[..idx])
    } else {
        None
    }
}

/// Strips all trailing call stub suffixes.
fn strip_call_stub_suffixes(symbol: &str) -> &str {
    let mut symbol = symbol;
    while let Some(stripped) = strip_call_stub_suffix(symbol) {
        symbol = stripped;
    }
    symbol
}

/// Splits an ELF symbol version suffix (`foo@GLIBC_2.2.5`, with `@@` marking
/// the default version definition), returning the base name and version.
///
/// MSVC-mangled names carry `@` and `?` characters of their own, so symbols
/// containing `?` are never split.
fn split_version_suffix(symbol: &str) -> Option<(&str, String)> {
    if symbol.contains('?') {
        return None;
    }
    let idx = symbol.rfind('@')?;
    if idx == 0 {
        return None;
    }
    let version = &symbol[idx + 1..];
    if version.is_empty()
        || !version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_+".contains(&b))
        || is_call_stub_token(version)
    {
        return None;
    }
    // A double `@` marks the default version definition.
    let base_end = if symbol[..idx].ends_with('@') {
        idx - 1
    } else {
        idx
    };
    if base_end == 0 {
        return None;
    }
    Some((&symbol[..base_end], version.to_string()))
}

/// Strips a trailing Rust hash suffix (`::h` + at least 8 lowercase hex
/// digits), if present.
fn strip_rust_hash_suffix(symbol: &str) -> Option<&str> {
    let idx = symbol.rfind("::h")?;
    if idx == 0 {
        return None;
    }
    let hash = &symbol[idx + 3..];
    if hash.len() >= 8 && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
        Some(&symbol[..idx])
    } else {
        None
    }
}

/// Strips a trailing LLVM clone suffix (`foo.llvm.123456`), if present.
fn strip_llvm_suffix(symbol: &str) -> Option<&str> {
    let idx = symbol.rfind(".llvm.")?;
    if idx == 0 {
        return None;
    }
    let counter = &symbol[idx + 6..];
    if !counter.is_empty() && counter.bytes().all(|b| b.is_ascii_digit()) {
        Some(&symbol[..idx])
    } else {
        None
    }
}

/// The legacy Rust mangling escapes special characters in demangled identifiers
/// as `$...$` sequences (and `..` for paths). The table is ordered so that the
/// path separator is decoded first, mirroring upstream consumers.
const RUST_LEGACY_ESCAPES: &[(&str, &str)] = &[
    ("..", "::"),
    ("$SP$", "@"),
    ("$BP$", "*"),
    ("$LT$", "<"),
    ("$GT$", ">"),
    ("$LP$", "("),
    ("$RP$", ")"),
    ("$RF$", "&"),
    ("$C$", ","),
    ("$u5b$", "["),
    ("$u5d$", "]"),
    ("$u7b$", "{"),
    ("$u7d$", "}"),
    ("$u3b$", ";"),
    ("$u20$", " "),
    ("$u27$", "'"),
];

/// Decodes legacy Rust `$`-escapes in a single pass. Borrows the input when
/// nothing needs decoding.
fn decode_legacy_rust_escapes(symbol: &str) -> Cow<'_, str> {
    if !(symbol.contains('$') || symbol.contains("..")) {
        return Cow::Borrowed(symbol);
    }

    let mut out: Option<String> = None;
    let mut idx = 0;
    while idx < symbol.len() {
        let rest = &symbol[idx..];
        let mut matched = false;
        for (pattern, replacement) in RUST_LEGACY_ESCAPES {
            if rest.starts_with(pattern) {
                out.get_or_insert_with(|| String::from(&symbol[..idx]))
                    .push_str(replacement);
                idx += pattern.len();
                matched = true;
                break;
            }
        }
        if !matched {
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
