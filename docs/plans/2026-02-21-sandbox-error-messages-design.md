# sandbox-aware error messages

**date**: 2026-02-21
**branch**: feature/additional-value-types
**status**: approved

## motivation

tein is intended as a substrate for LLM-driven tool synthesis: an LLM synthesises scheme code, tein evaluates it in a sandboxed context, and the LLM iterates based on results. for this loop to work, the LLM must be able to distinguish two fundamentally different failure modes:

- **code bugs** — wrong logic, bad syntax, type errors
- **sandbox walls** — attempted something the host has explicitly restricted

currently, two of three sandbox violation cases produce errors indistinguishable from ordinary scheme errors:

| case | current message | problem |
|------|----------------|---------|
| file IO denied | `"access denied: /etc/passwd"` (as `EvalError`) | structurally same as a code error |
| module import blocked | `"couldn't find file in module path: ..."` | sounds like a missing dependency |
| stripped binding called | `"unbound variable: eval"` | sounds like a typo |

## design

### `Error::SandboxViolation`

add one new variant to the public `Error` enum:

```rust
/// evaluation was blocked by sandbox policy (not a code bug)
///
/// indicates the scheme code attempted something explicitly restricted
/// by the context's configuration: a blocked module import, denied
/// file access, or use of a primitive not included in the active presets.
SandboxViolation(String),
```

display: `"sandbox violation: {msg}"`

this is the only public api change — purely additive, no existing variants modified.

### detection: sentinel prefixes + `extract_exception_message`

all three cases funnel through `Value::extract_exception_message`. change its return type from `String` to `Error`, so it can emit either `EvalError` or `SandboxViolation` directly. detection uses sentinel prefixes in the chibi exception message string:

- `"[sandbox:file]"` → `SandboxViolation("file access denied: {path}")`
- `"[sandbox:module]"` → `SandboxViolation("module import blocked: {module}")`
- `"[sandbox:binding]"` → `SandboxViolation("'{name}' is not available in this sandbox")`

the module case can also be detected without a C patch by checking `MODULE_POLICY` thread-local inside `extract_exception_message`: if `VfsOnly` is active and the message is `"couldn't find file in module path"`, it's a policy block. this avoids any eval.c changes for that case.

### three implementation sites

**1. file IO** (`context.rs` — `check_and_delegate`)

change the denied-access error message from `"access denied: {path}"` to `"[sandbox:file] {path}"`. no other change needed — already goes through chibi exception → `extract_exception_message`.

**2. module import** (`context.rs` — `extract_exception_message`)

read `MODULE_POLICY` thread-local. if `VfsOnly` and message is `"couldn't find file in module path"`, return `SandboxViolation("module import blocked: {irritant}")`. no C changes required.

**3. stripped binding stubs** (`context.rs` — `sandbox_build`)

after copying allowed bindings into the restricted env, for each preset primitive that was *not* allowed, register a stub foreign fn under that name:

```rust
unsafe extern "C" fn sandbox_stub(
    ctx: sexp, self_: sexp, _n: sexp_sint_t, _args: sexp,
) -> sexp {
    // extract name from the opcode's name slot (set by sexp_define_foreign)
    let name = sexp_opcode_name(self_); // → scheme string
    let name_str = /* extract C string from scheme string */;
    let msg = format!("[sandbox:binding] '{}' is not available in this sandbox", name_str);
    make_string_error(ctx, &msg)
}
```

one function, registered N times under different names. chibi sets `sexp_opcode_name` when `sexp_define_foreign` is called, so the stub self-identifies at call time with no thread-locals or closures needed.

note: stubs only fire on *call*. referencing the proc (e.g. `(procedure? eval)`) returns `#t` — correct and useful, the proc exists but will error on call.

### data flow

```
scheme code hits sandbox wall
  → chibi exception with "[sandbox:*]" prefix (or module policy match)
  → evaluate() sees sexp_exceptionp
  → Value::from_raw() → extract_exception_message()
  → detects prefix / policy state
  → returns Error::SandboxViolation(...)
  → caller receives Err(SandboxViolation(...))
```

## error messages

| case | `SandboxViolation` message |
|------|---------------------------|
| file read denied | `"file access denied: /etc/passwd (read not permitted)"` |
| file write denied | `"file access denied: /tmp/out.txt (write not permitted)"` |
| module import blocked | `"module import blocked: (chibi process) (not available in this sandbox)"` |
| stripped binding | `"'eval' is not available in this sandbox"` |

## testing

update existing sandbox tests to assert `Error::SandboxViolation` (not just `is_err()`). add new tests:

```rust
// file IO
let err = ctx.evaluate("(open-input-file \"/etc/passwd\")").unwrap_err();
assert!(matches!(err, Error::SandboxViolation(_)));

// module import
let err = ctx.evaluate("(import (chibi process))").unwrap_err();
assert!(matches!(err, Error::SandboxViolation(_)));
assert!(err.to_string().contains("module import blocked"));

// stripped binding stub
let err = ctx.evaluate("(eval '(+ 1 2) (the-environment))").unwrap_err();
assert!(matches!(err, Error::SandboxViolation(_)));
assert!(err.to_string().contains("not available in this sandbox"));
```

tighten `test_module_policy_blocks_filesystem_import` to assert `SandboxViolation` (currently only asserts `is_err()`).

## files to modify

- `tein/src/error.rs` — add `SandboxViolation` variant + display
- `tein/src/value.rs` — `extract_exception_message` returns `Error` instead of `String`; detection logic
- `tein/src/context.rs` — sentinel prefix in IO denial; stub registration in sandbox_build; update tests
- `tein/src/lib.rs` — re-export `SandboxViolation` (already re-exports `Error`)

no C changes required.
