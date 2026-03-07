# design: `Result::Err` → r7rs exceptions (#135)

## problem

when a `#[tein_fn]` returns `Result::Err(msg)`, the macro codegen emits
`sexp_c_str(ctx, msg)` — a plain scheme string. this string is:

- **not catchable** by `guard` / `with-exception-handler`
- **indistinguishable** from a normal string return value
- **inconsistent** — some hand-written trampolines (json/toml arity checks)
  already use `ffi::make_error` which IS catchable

## solution

replace `sexp_c_str` with `ffi::make_error` (already exists, wraps
`sexp_user_exception`) on all `Err` paths. the resulting exception:

- `error-object?` → `#t`
- `error-object-message` → the error string
- `error-object-irritants` → `'()`
- catchable by `guard` and `with-exception-handler`
- on the rust side, `Value::from_raw` converts it to `Err(Error::EvalError(msg))`

### why `sexp_user_exception` (kind `user`)

in chibi, `error-object?` is aliased to `exception?` — a pure type predicate
on `SEXP_EXCEPTION`. it does not check the kind symbol. both `user` and `error`
kinds pass. `user` is what chibi uses for application-level errors throughout.
keeping `user` means no change to `tein_make_error`.

the kind symbol only matters for `read-error?` (kind `read`) and `file-error?`
(kind `file`). a future `(tein error)` module (#136) could define subtype
predicates if needed.

### r7rs portability

callers use standard r7rs primitives from `(scheme base)`:

```scheme
(guard (exn
        ((error-object? exn)
         (error-object-message exn)))
  (json-parse "not json"))
```

this works identically on any r7rs scheme. no SRFIs required.

## changes

### macro codegen (`tein-macros/src/lib.rs`)

4 paths where `Result::Err` currently emits `sexp_c_str`:

| path | location | change |
|------|----------|--------|
| standalone `#[tein_fn]` | ~L1885 | `tein::raw::sexp_c_str` → `tein::raw::make_error` |
| module internal `#[tein_fn]` | ~L1059 | `((*__tein_api).sexp_c_str)` → `((*__tein_api).make_error)` |
| ext `#[tein_fn]` | ~L1064 | `((*__tein_api).sexp_c_str)` → `((*__tein_api).make_error)` |
| ext `#[tein_methods]` | ~L1150 | `((*#api).sexp_c_str)` → `((*#api).make_error)` |

internal `#[tein_methods]` already returns `Err(tein::Error::EvalError(...))` —
no change needed.

### ext vtable (`tein-ext/src/lib.rs`)

- add `make_error` fn pointer to `TeinExtApi`
- bump `TEIN_EXT_API_VERSION` to 2

### vtable population (`tein/src/context.rs`)

- populate the new `make_error` vtable entry with `ffi::make_error` (transmuted)

### hand-written trampolines (`tein/src/context.rs`, `tein/src/http.rs`)

replace remaining `sexp_c_str` error returns with `ffi::make_error` in:

- `json_parse_trampoline` (type mismatch, utf-8, parse error, conversion error)
- `json_stringify_trampoline` (conversion error)
- `toml_parse_trampoline` (same pattern as json)
- `toml_stringify_trampoline` (same pattern as toml)
- `http_request_trampoline` (type validation, transport error, conversion error)

note: json/toml arity checks already use `make_error` — no change there.

### tests (~15 tests)

tests currently asserting `Ok(Value::String(...))` for error returns need
updating to `Err(Error::EvalError(...))`:

- `tein/tests/tein_fn.rs`: `test_tein_fn_result_err`, `test_tein_fn_wrong_arg_type`, `test_tein_fn_panic_safety`
- `tein/tests/tein_fn_value_return.rs`: `value_return_result_err`
- `tein/tests/http_tests.rs`: 3 tests expecting `Value::String`
- `tein/tests/ext_loading.rs`: `test_ext_free_fn_result_err`
- `tein/src/context.rs` inline tests: ~6 json/toml error tests
- `tein/tests/scheme/safe_regexp.scm`: change `(string? (regexp "["))` to use `guard`

### docs

- AGENTS.md: update "Result::Err returns a scheme string" gotcha
- reference.md: update if it mentions the string convention

## unchanged

- `ffi::make_error` / `tein_make_error` in chibi shim (already correct)
- `Value::from_raw` exception handling (already converts exceptions to `Err`)
- the REPL (already handles `Err(e)` correctly)
- `#[tein_methods]` internal mode (already returns rust `Error`)

## future

`(tein error)` (#136) — pure scheme ergonomic wrappers over `guard` /
`error-object?`. separate issue, not blocking.
