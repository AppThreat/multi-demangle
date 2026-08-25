/// Asserts that a list of mangled symbols demangle to the expected strings.
///
/// Every entry is demangled with the given language and [`DemangleOptions`]; a
/// failed demangling shows up as `<demangling failed>`. All mismatches are
/// collected and reported in a single assertion so one bad symbol does not hide
/// the rest. Test files opt in with `#[macro_use] mod utils;`.
///
/// ```ignore
/// use symbolic_common::Language;
/// use multi_demangle::DemangleOptions;
///
/// assert_demangle!(Language::Cpp, DemangleOptions::name_only(), {
///     "_ZN3foo3barEv" => "foo::bar",
///     "unknown" => "<demangling failed>",
/// });
/// ```
#[macro_export]
macro_rules! assert_demangle {
    ($l:expr, $o:expr, { $($m:expr => $d:expr),* }) => {{
        let mut __failures: Vec<String> = Vec::new();

        $({
            use multi_demangle::Demangle;

            let __mangled = $m;
            let __demangled = ::symbolic_common::Name::new(__mangled, ::symbolic_common::NameMangling::Unknown, $l).demangle($o);
            let __demangled = __demangled.as_ref().map(String::as_str).unwrap_or("<demangling failed>");

            if __demangled != $d {
                __failures.push(format!(
                    "{}\n   expected: {}\n   actual:   {}",
                    __mangled,
                    $d,
                    __demangled
                ));
            }
        })*

        assert!(__failures.is_empty(), "demangling failed: \n\n{}\n", __failures.join("\n\n"));
    }};
    ($l:expr, $o:expr, { $($m:expr => $d:expr,)* }) => {
        assert_demangle!($l, $o, { $($m => $d),* })
    };
}
