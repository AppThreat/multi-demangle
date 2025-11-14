# multi-demangle

Demangling support for various languages and compilers. Fork of symbolic-demangle.

Currently supported languages are:

- C++ (GCC-style compilers and MSVC)
- Rust (both `legacy` and `v0`)
- Swift (up to Swift 6.2.1)
- ObjC (only symbol detection)

As the demangling schemes for the languages are different, the supported demangling features are
inconsistent. For example, argument types were not encoded in legacy Rust mangling and thus not
available in demangled names.

## Development

Use `uv` package manager.

```
uv tool install maturin
maturin develop --all-features
```

```
>>> import multi_demangle
>>> print(multi_demangle.demangle_symbol("_ZN3foo3barEv"))
foo::bar()
```

## License

MIT
