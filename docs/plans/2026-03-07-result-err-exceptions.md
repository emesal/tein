# Result::Err → r7rs Exceptions Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** make `Result::Err` in `#[tein_fn]` raise proper r7rs exceptions instead of returning plain strings, so errors are catchable with `(guard ...)` and inspectable via `error-object?`/`error-object-message`.

**Architecture:** replace `sexp_c_str` with `ffi::make_error` (already exists, wraps `sexp_user_exception`) in all error codegen paths. `make_error` has the same `(ctx, msg, len) -> sexp` signature as `sexp_c_str`, so it's a drop-in replacement. on the rust side, `Value::from_raw` already converts exception sexps to `Err(Error::EvalError(...))`.

**Tech Stack:** rust proc macros (`tein-macros`), chibi-scheme FFI (`tein/src/ffi.rs`), C ABI vtable (`tein-ext`)

**Closes:** #135

---

### Task 1: create feature branch

**Step 1: create branch**

```bash
just bugfix result-err-exceptions-2603
```

**Step 2: commit design doc**

The design doc `docs/plans/2026-03-07-result-err-exceptions-design.md` is already committed on `dev`. Cherry-pick or it'll come along with the branch.

---

### Task 2: add `make_error` to ext vtable

**Files:**
- Modify: `tein-ext/src/lib.rs:39` (version bump)
- Modify: `tein-ext/src/lib.rs:211-241` (add field before sentinels section)

**Step 1: bump TEIN_EXT_API_VERSION**

In `tein-ext/src/lib.rs:39`, change:

```rust
pub const TEIN_EXT_API_VERSION: u32 = 2;
```

**Step 2: add `make_error` field to `TeinExtApi`**

In `tein-ext/src/lib.rs`, after the `sexp_make_bytes` field (~line 233) and before the sentinels section (~line 235), add:

```rust
    /// Create a scheme exception (error object) from a message string.
    /// Equivalent to `(error msg)` in scheme. Catchable by `guard`.
    /// `len` is byte length; pass -1 for null-terminated C strings.
    pub make_error:
        unsafe extern "C" fn(*mut OpaqueCtx, *const c_char, c_long) -> *mut OpaqueVal,
```

**Step 3: verify it compiles**

Run: `cargo build -p tein-ext`
Expected: PASS (struct definition only, no consumers yet)

**Step 4: commit**

```bash
git add tein-ext/src/lib.rs
git commit -m "feat(ext): add make_error to TeinExtApi vtable, bump to v2 (#135)"
```

---

### Task 3: populate vtable entry in context.rs

**Files:**
- Modify: `tein/src/context.rs:450-471` (add transmute before sentinels section)

**Step 1: add `make_error` transmute**

In `build_ext_api()`, after the `sexp_make_bytes` line (~line 452) and before the `// sentinels` comment (~line 454), add:

```rust
            // error constructor — same signature as sexp_c_str but returns exception
            make_error: std::mem::transmute::<
                unsafe fn(ffi::sexp, *const std::ffi::c_char, ffi::sexp_sint_t) -> ffi::sexp,
                unsafe extern "C" fn(
                    *mut OpaqueCtx,
                    *const std::ffi::c_char,
                    std::ffi::c_long,
                ) -> *mut OpaqueVal,
            >(ffi::make_error),
```

**Step 2: verify it compiles**

Run: `cargo build -p tein`
Expected: PASS

**Step 3: commit**

```bash
git add tein/src/context.rs
git commit -m "feat(context): populate make_error in ext vtable (#135)"
```

---

### Task 4: update macro codegen — standalone / internal `#[tein_fn]`

**Files:**
- Modify: `tein-macros/src/lib.rs:1882-1888` (Result::Err path)
- Modify: `tein-macros/src/lib.rs:1923-1927` (panic path)

**Step 1: change Result::Err codegen**

At lines 1882-1888, replace the block:

```rust
                        // Result::Err returns a scheme string, not an exception. callers
                        // receive Value::String(msg), not Err(...). (test-error ...) won't
                        // catch it; match on Value::String instead. same in ext mode.
                        Err(__tein_err) => {
                            let msg = __tein_err.to_string();
                            let c_msg = ::std::ffi::CString::new(msg.as_str()).unwrap_or_default();
                            tein::raw::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as tein::raw::sexp_sint_t)
                        }
```

with:

```rust
                        // Result::Err raises a scheme exception (error object).
                        // catchable with (guard ...), inspectable via error-object-message.
                        Err(__tein_err) => {
                            let msg = __tein_err.to_string();
                            let c_msg = ::std::ffi::CString::new(msg.as_str()).unwrap_or_default();
                            tein::raw::make_error(ctx, c_msg.as_ptr(), msg.len() as tein::raw::sexp_sint_t)
                        }
```

**Step 2: change panic handler codegen**

At lines 1923-1927, replace:

```rust
                    // panic → return error string as scheme value
                    let msg = concat!("rust panic in ", stringify!(#fn_name));
                    let c_msg = ::std::ffi::CString::new(msg).unwrap();
                    tein::raw::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as tein::raw::sexp_sint_t)
```

with:

```rust
                    // panic → raise scheme exception
                    let msg = concat!("rust panic in ", stringify!(#fn_name));
                    let c_msg = ::std::ffi::CString::new(msg).unwrap();
                    tein::raw::make_error(ctx, c_msg.as_ptr(), msg.len() as tein::raw::sexp_sint_t)
```

**Step 3: verify it compiles**

Run: `cargo build -p tein-macros`
Expected: PASS (macro crate only)

**Step 4: commit**

```bash
git add tein-macros/src/lib.rs
git commit -m "feat(macros): Result::Err raises exception in standalone/internal mode (#135)"
```

---

### Task 5: update macro codegen — ext `#[tein_fn]` and ext `#[tein_methods]`

**Files:**
- Modify: `tein-macros/src/lib.rs:1059-1067` (ext fn Result::Err)
- Modify: `tein-macros/src/lib.rs:916-924` (ext fn panic)
- Modify: `tein-macros/src/lib.rs:1150-1156` (ext methods Result::Err)
- Modify: `tein-macros/src/lib.rs:836-841` (ext methods panic)

**Step 1: change ext fn Result::Err codegen**

At lines 1059-1067, replace `sexp_c_str` with `make_error`:

```rust
                        // Result::Err raises a scheme exception (error object).
                        Err(__tein_err) => {
                            let msg = __tein_err.to_string();
                            let c_msg = ::std::ffi::CString::new(msg.as_str()).unwrap_or_default();
                            ((*__tein_api).make_error)(
                                ctx as *mut tein_ext::OpaqueCtx,
                                c_msg.as_ptr(), msg.len() as ::std::ffi::c_long,
                            )
                        }
```

**Step 2: change ext fn panic handler**

At lines 916-924, replace `sexp_c_str` with `make_error`:

```rust
                Err(_) => unsafe {
                    let __tein_api = __TEIN_API.with(|cell| cell.get());
                    let msg = concat!("rust panic in ", stringify!(#fn_name));
                    let c_msg = ::std::ffi::CString::new(msg).unwrap();
                    ((*__tein_api).make_error)(
                        ctx as *mut tein_ext::OpaqueCtx,
                        c_msg.as_ptr(),
                        msg.len() as ::std::ffi::c_long,
                    )
```

**Step 3: change ext methods Result::Err codegen**

At lines 1150-1156, replace `sexp_c_str` with `make_error`:

```rust
                        Err(__tein_err) => {
                            let msg = __tein_err.to_string();
                            let c_msg = ::std::ffi::CString::new(msg.as_str()).unwrap_or_default();
                            ((*#api).make_error)(
                                ctx, c_msg.as_ptr(), msg.len() as ::std::ffi::c_long,
                            )
                        }
```

**Step 4: change ext methods panic handler**

At lines 836-841, replace `sexp_c_str` with `make_error`:

```rust
                Err(_) => unsafe {
                    let msg = concat!("rust panic in method ", stringify!(#method_ident));
                    let c_msg = ::std::ffi::CString::new(msg).unwrap();
                    ((*api).make_error)(
                        ctx, c_msg.as_ptr(), msg.len() as ::std::ffi::c_long,
                    )
                }
```

**Step 5: verify it compiles**

Run: `cargo build -p tein-macros`
Expected: PASS

**Step 6: commit**

```bash
git add tein-macros/src/lib.rs
git commit -m "feat(macros): Result::Err raises exception in ext mode (#135)"
```

---

### Task 6: update hand-written trampolines

**Files:**
- Modify: `tein/src/context.rs` — json_parse_trampoline (~L822-858), json_stringify_trampoline (~L890-893), toml_parse_trampoline (~L920-946), toml_stringify_trampoline (~L978-981)
- Modify: `tein/src/http.rs` — http_request_trampoline (~L224,276-278)

In each trampoline, every `ffi::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)` on an error path becomes `ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)`.

**important:** do NOT change the successful return paths (e.g. `json_stringify_trampoline` returns `sexp_c_str` for the JSON string result — that's a value, not an error). only change the `Err` / error branches.

**Step 1: update json_parse_trampoline**

In `tein/src/context.rs`, the json_parse_trampoline has 4 error paths that use `sexp_c_str` (lines 832, 841, 850, 856). Change all 4 to `ffi::make_error`. Note: line 826 already uses `make_error` (arity check) — leave it.

**Step 2: update json_stringify_trampoline**

Line 893 (`Err(e)` path): change `sexp_c_str` → `make_error`. Leave line 888 alone (that's the success path returning a JSON string).

**Step 3: update toml_parse_trampoline**

Lines 922, 931, 940, 946: change `sexp_c_str` → `make_error`. Line 916 already uses `make_error` — leave it.

**Step 4: update toml_stringify_trampoline**

Line 981 (`Err(e)` path): change `sexp_c_str` → `make_error`. Leave line 976 alone (success path).

**Step 5: update http_request_trampoline**

In `tein/src/http.rs`:
- Line 224: type validation error `scheme_str(...)` → use `ffi::make_error` instead. Note: this uses `ffi::scheme_str`, not `sexp_c_str`. Check if `scheme_str` wraps `sexp_c_str`:

The trampoline uses `ffi::scheme_str(ctx, "msg")` as a convenience. We need to replace these with `make_error`. Since `make_error` takes `(ctx, *const c_char, len)`, we need `CString` for each. Replace each `ffi::scheme_str(ctx, msg_literal)` with:

```rust
let c_msg = CString::new(msg_literal).unwrap();
return ffi::make_error(ctx, c_msg.as_ptr(), msg_literal.len() as ffi::sexp_sint_t);
```

There are 4 `scheme_str` calls on error paths (lines 224, 231-232, 276, plus the `Err(msg)` at line 278). And one `sexp_c_str`-via-format for the conversion error at line 276.

Actually, look more carefully at http.rs: it uses `ffi::scheme_str` which is a helper. Let's check what it does and decide whether to add a parallel `ffi::scheme_error` helper or just inline the `make_error` calls.

For consistency with json/toml trampolines, inline the `CString` + `make_error` calls. Add `use std::ffi::CString;` to http.rs if not already present.

**Step 6: update HTTP_SCM doc comment**

In `tein/src/http.rs:33-35`, replace:

```
;;; on transport error, returns a plain string (not an exception). callers
;;; should check (pair? result) before accessing fields. see gh #135.
```

with:

```
;;; on transport error, raises a scheme exception (error object).
;;; use (guard ...) to catch errors. closes gh #135.
```

**Step 7: verify it compiles**

Run: `cargo build -p tein --features http`
Expected: PASS

**Step 8: commit**

```bash
git add tein/src/context.rs tein/src/http.rs
git commit -m "feat(trampolines): error paths raise exceptions instead of returning strings (#135)"
```

---

### Task 7: update tests — `#[tein_fn]` tests

**Files:**
- Modify: `tein/tests/tein_fn.rs:123-135` (test_tein_fn_result_err)
- Modify: `tein/tests/tein_fn.rs:137-151` (test_tein_fn_wrong_arg_type)
- Modify: `tein/tests/tein_fn.rs:162-178` (test_tein_fn_panic_safety)
- Modify: `tein/tests/tein_fn_value_return.rs:47-55` (value_return_result_err)

**Step 1: update test_tein_fn_result_err**

Replace lines 123-135 with:

```rust
#[test]
fn test_tein_fn_result_err() {
    let ctx = Context::new().expect("create context");
    ctx.define_fn_variadic("safe-div", __tein_safe_div)
        .expect("define");
    // division by zero raises a scheme exception → Err(EvalError(...))
    let result = ctx.evaluate("(safe-div 10 0)");
    match result {
        Err(e) => {
            let msg = e.to_string();
            assert!(msg.contains("division by zero"), "got: {msg}");
        }
        Ok(v) => panic!("expected error, got {v:?}"),
    }
}
```

**Step 2: update test_tein_fn_wrong_arg_type**

Replace lines 137-151 with:

```rust
#[test]
fn test_tein_fn_wrong_arg_type() {
    let ctx = Context::new().expect("create context");
    ctx.define_fn_variadic("add", __tein_add).expect("define");
    // pass string where integer expected → scheme exception
    let result = ctx.evaluate(r#"(add "hello" 1)"#);
    match result {
        Err(e) => {
            let msg = e.to_string();
            assert!(
                msg.contains("expected i64"),
                "expected type error message, got: {msg}",
            );
        }
        Ok(v) => panic!("expected error, got {v:?}"),
    }
}
```

**Step 3: update test_tein_fn_panic_safety**

Replace lines 162-178 with:

```rust
#[test]
fn test_tein_fn_panic_safety() {
    #[tein_fn]
    fn panicker() -> i64 {
        panic!("oh no!");
    }

    let ctx = Context::new().expect("create context");
    ctx.define_fn_variadic("panicker", __tein_panicker)
        .expect("define");
    // should not crash — panic is caught, raises scheme exception
    let result = ctx.evaluate("(panicker)");
    match result {
        Err(e) => {
            let msg = e.to_string();
            assert!(msg.contains("panic"), "expected panic message, got: {msg}");
        }
        Ok(v) => panic!("expected error from panic, got {v:?}"),
    }
}
```

**Step 4: update value_return_result_err**

Replace lines 47-55 in `tein/tests/tein_fn_value_return.rs` with:

```rust
#[test]
fn value_return_result_err() {
    let ctx = Context::new_standard().unwrap();
    valret::register_module_valret(&ctx).unwrap();
    let result = ctx.evaluate("(import (tein valret)) (maybe-vec -1)");
    match result {
        Err(e) => {
            let msg = e.to_string();
            assert!(msg.contains("negative"), "got: {msg}");
        }
        Ok(v) => panic!("expected error, got {v:?}"),
    }
}
```

**Step 5: run the updated tests**

Run: `cargo test -p tein --test tein_fn --test tein_fn_value_return`
Expected: all PASS

**Step 6: commit**

```bash
git add tein/tests/tein_fn.rs tein/tests/tein_fn_value_return.rs
git commit -m "test: update #[tein_fn] tests for exception error returns (#135)"
```

---

### Task 8: update tests — json/toml/http inline tests

**Files:**
- Modify: `tein/src/context.rs` — 10 tests (~L9076-9457)
- Modify: `tein/tests/http_tests.rs` — 3 tests (~L23-58)

**Step 1: update json error tests**

All these tests follow the same pattern: change `.unwrap()` to expect `Err`, and assert the error message contains the function name. Here's the pattern for each:

`test_json_parse_invalid` (L9076-9085) — change:
```rust
    fn test_json_parse_invalid() {
        let ctx = Context::new_standard().expect("context");
        ctx.evaluate("(import (tein json))").expect("import");
        let result = ctx.evaluate("(json-parse \"not json\")");
        match result {
            Err(e) => assert!(e.to_string().contains("json-parse")),
            Ok(v) => panic!("expected error, got {v:?}"),
        }
    }
```

Apply the same pattern to:
- `test_json_parse_wrong_type_integer` (L9317)
- `test_json_parse_wrong_type_boolean` (L9330)
- `test_json_parse_wrong_type_list` (L9342)
- `test_json_parse_wrong_type_lambda` (L9354)
- `test_json_stringify_lambda_arg` (L9377) — assert `result.is_err()`
- `test_toml_parse_wrong_type_integer` (L9401)
- `test_toml_parse_wrong_type_boolean` (L9413)
- `test_toml_parse_wrong_type_list` (L9425)
- `test_toml_stringify_integer_arg` (L9448) — assert `result.is_err()`

**Step 2: update http tests**

In `tein/tests/http_tests.rs`, update 3 tests:

`http_get_bad_url_returns_error_string` (L23-32) — rename and change:
```rust
    #[test]
    fn http_get_bad_url_raises_error() {
        let ctx = ctx();
        ctx.evaluate("(import (tein http))").unwrap();
        let result = ctx.evaluate("(http-get \"not-a-url\" '())");
        assert!(result.is_err(), "expected error, got {result:?}");
    }
```

`http_request_bad_url_returns_error_string` (L34-45) — same pattern, rename to `http_request_bad_url_raises_error`.

`http_request_with_timeout` (L47-58) — same pattern, assert `is_err()`.

**Step 3: run the tests**

Run: `cargo test -p tein test_json_parse_invalid test_json_parse_wrong test_json_stringify_lambda test_toml_parse_wrong test_toml_stringify_integer -- --nocapture`

Run: `cargo test -p tein --test http_tests`

Expected: all PASS

**Step 4: commit**

```bash
git add tein/src/context.rs tein/tests/http_tests.rs
git commit -m "test: update json/toml/http tests for exception error returns (#135)"
```

---

### Task 9: update tests — ext loading and scheme test

**Files:**
- Modify: `tein/tests/ext_loading.rs:115-132` (test_ext_free_fn_result_err)
- Modify: `tein/tests/scheme/safe_regexp.scm:10-11`

**Step 1: rebuild test extension**

The test extension (`tein-test-ext`) needs rebuilding against the new `tein-ext` API v2.

Run: `cargo build -p tein-test-ext`
Expected: PASS (the extension code itself doesn't change — it still returns `Result::Err`)

**Step 2: update ext_loading test**

Replace lines 115-132 in `tein/tests/ext_loading.rs`:

```rust
#[test]
fn test_ext_free_fn_result_err() {
    // Result::Err raises a scheme exception — same as internal mode.
    let ctx = Context::new_standard().expect("context");
    ctx.load_extension(ext_lib_path()).expect("load");
    ctx.evaluate("(import (tein testext))").expect("import");
    let result = ctx.evaluate("(testext-safe-div 10 0)");
    match result {
        Err(e) => assert!(
            e.to_string().contains("division by zero"),
            "expected 'division by zero' in error, got: {e}"
        ),
        Ok(v) => panic!("expected error from Result::Err, got {v:?}"),
    }
}
```

**Step 3: update scheme test**

Replace lines 10-11 in `tein/tests/scheme/safe_regexp.scm`:

```scheme
;; invalid pattern raises an error (exception), not a string
(test-error "safe-regexp/invalid-pattern-raises-error"
  (regexp "["))
```

Note: check that `(test-error ...)` is available in the scheme test harness. It should be — it's from `(chibi test)`. `test-error` checks that evaluating the body raises an exception.

**Step 4: run tests**

Run: `cargo test -p tein --test ext_loading test_ext_free_fn_result_err`

Run: `cargo test -p tein --test scheme_tests safe_regexp`

Expected: all PASS

**Step 5: commit**

```bash
git add tein/tests/ext_loading.rs tein/tests/scheme/safe_regexp.scm
git commit -m "test: update ext and scheme tests for exception error returns (#135)"
```

---

### Task 10: update docs

**Files:**
- Modify: `AGENTS.md` — "Result::Err returns a scheme string" section
- Modify: `tein/src/http.rs:9` — module doc comment referencing the old convention

**Step 1: update AGENTS.md**

Find the gotcha that reads:

> **Result::Err returns a scheme string**: `fn foo() -> Result<i64, String>` — the `Err` path returns `sexp_c_str(msg)` which becomes `Value::String(msg)` in rust. it's not an exception; `(test-error ...)` won't catch it. match on `Value::String` instead. same in internal and ext mode.

Replace with:

> **Result::Err raises a scheme exception**: `fn foo() -> Result<i64, String>` — the `Err` path calls `make_error(msg)` which creates a proper r7rs error object. in scheme, catch with `(guard (exn ((error-object? exn) (error-object-message exn))) ...)`. in rust, `evaluate()` returns `Err(Error::EvalError(msg))`. same in internal and ext mode.

**Step 2: grep for any other references to the old convention**

Run: `grep -rn "error string" tein/src/ AGENTS.md docs/ --include="*.rs" --include="*.md" | grep -v target | grep -v plans`

Update any stale comments found (e.g. trampoline doc comments that say "returns a scheme string with the error message").

Specifically, update these doc comments:
- `tein/src/context.rs` json_stringify_trampoline (~L870-871): "On conversion error, returns a scheme string" → "On conversion error, raises a scheme exception"
- `tein/src/context.rs` toml_parse_trampoline (~L904-905): same
- `tein/src/context.rs` toml_stringify_trampoline (~L958-959): same
- `tein/src/http.rs:206`: "returns response alist on success, error string on transport failure" → "returns response alist on success, raises exception on transport failure"

**Step 3: run full test suite**

Run: `just test`
Expected: all tests PASS

**Step 4: lint**

Run: `just lint`

**Step 5: commit**

```bash
git add AGENTS.md tein/src/context.rs tein/src/http.rs
git commit -m "docs: update error handling convention — exceptions not strings (#135)

closes #135"
```

---

### Task 11: final verification and cleanup

**Step 1: run full test suite one more time**

Run: `just test`
Expected: all PASS

**Step 2: grep for any remaining `sexp_c_str` on error paths**

Run: `grep -n 'sexp_c_str' tein-macros/src/lib.rs` — should have NO hits in Result::Err or panic branches. remaining hits should only be in type-extraction error messages (arg parsing — those are type errors at the FFI boundary, arguably should also be exceptions, but those use a different codegen path and can be a follow-up).

Run: `grep -n 'sexp_c_str' tein/src/context.rs` — remaining hits should only be in non-error paths (e.g. building scheme strings for values).

Run: `grep -n 'scheme_str' tein/src/http.rs` — should have zero hits (all replaced with make_error).

**Step 3: collect AGENTS.md notes**

Review any caveats discovered during implementation and add to AGENTS.md if needed. The main update (Result::Err convention) was done in task 10.

**Step 4: halt for review**

Do NOT push. Wait for code review before pushing.
