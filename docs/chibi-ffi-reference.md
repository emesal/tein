# chibi FFI reference

## numeric tower shim functions

these wrap chibi internals for bignum/ratio/complex — needed in `value.rs` `to_raw()`.

- `sexp_bignum_to_string(ctx, x)` — opens a string port, writes bignum in decimal, returns string sexp (allocates)
- `sexp_string_to_number(ctx, str, base)` — parses a scheme string as a number (used for `Bignum::to_raw`)
- `sexp_make_ratio(ctx, num, den)` — ratio constructor; first arg must be GC-rooted before calling (allocates)
- `sexp_make_complex(ctx, real, imag)` — complex constructor; first arg must be GC-rooted before calling (allocates)

## chibi feature flags (linux)

- `SEXP_USE_GREEN_THREADS=1` (linux default) — `threads` cond-expand feature active; affects which VFS files load (e.g. `srfi/39/syntax.scm` vs `syntax-no-threads.scm`)
- `full-unicode` always enabled — affects `scheme/char.sld` path selection
