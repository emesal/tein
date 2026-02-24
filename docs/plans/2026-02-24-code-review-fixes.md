# code review fixes — dev vs milestone-2

date: 2026-02-24
branch: bugfix/mvp-code-review-2602
reviewed range: 14c8839..9b12749

---

## critical

### 1. `reader_unset_wrapper` missing null check — UB

**file:** `src/context.rs:420`

`reader_unset_wrapper` calls `sexp_car(args)` without checking whether `args` is the null
list. `reader_set_wrapper` (line 368) has the guard; `unset` is missing it. calling
`(unset-reader!)` with no arguments from scheme causes undefined behaviour (likely crash).

**fix:** add the same `sexp_nullp(args) != 0` guard as `reader_set_wrapper`.

---

### 2. `tein_make_error` discards allocation and ignores `len` — tein_shim.c

**file:** `vendor/chibi-scheme/tein_shim.c:181`

```c
sexp tein_make_error(sexp ctx, const char* msg, sexp_sint_t len) {
    sexp s = sexp_c_string(ctx, msg, len);      // allocates into s
    return sexp_user_exception(ctx, SEXP_FALSE, msg, SEXP_NULL);  // uses msg, not s
}
```

`s` is allocated and immediately abandoned. `sexp_user_exception` receives the raw `msg`
pointer and creates its own string internally. the `len` parameter is silently unused.
no visible behaviour change since all call sites pass static nul-terminated strings, but
the function signature lies and the allocation is waste.

**fix:** either pass `s` to `sexp_user_exception`, or remove `s` and the `len` parameter
and simplify the call.

---

## important

### 3. `macro_expand_hook()` uses `sexp_booleanp` instead of false-check

**file:** `src/context.rs:1593`

`sexp_booleanp` matches both `SEXP_TRUE` and `SEXP_FALSE`. the sentinel for "no hook" is
`SEXP_FALSE`. using the broader predicate is imprecise — returns `None` for `SEXP_TRUE`
too, which cannot currently be set but is still wrong.

**fix:** use a `sexp_falsep` equivalent or direct `== ffi::get_false()` comparison.

---

### 4. `register_reader` accepts `char` but table is ASCII-only

**file:** `src/context.rs:1544`

the dispatch table has 128 slots. `register_reader` accepts `char` (full unicode scalar),
casts to `c_int`, and returns a confusing "character out of ASCII range" error for values
> 127. the type signature implies unicode support that doesn't exist.

**fix:** change the parameter type to `u8` (or a dedicated `AsciiChar`/`char` with an
explicit ascii check) so the constraint is expressed at the type boundary. add a test for
the out-of-range path.

---

### 5. `PortStore` never removes entries — handle/object lifetime leak

**file:** `src/port.rs`

`PortStore` grows monotonically. `open_input_port` / `open_output_port` permanently insert
entries; there is no remove or close API. backing `Box<dyn Read>` / `Box<dyn Write>`
objects (file handles, sockets, etc.) are held until the entire `Context` is dropped.

**fix (minimal):** add a warning to the `open_input_port` / `open_output_port` rustdoc
that the backing object lives for the lifetime of the context, and add a `// TODO: add
close_port API` comment. a full fix would add explicit handle removal.

---

### 6. redundant `PORT_STORE_PTR` guard in `open_input_port` / `open_output_port`

**file:** `src/context.rs:1632` (input), `1694` (output)

```rust
PORT_STORE_PTR.with(|c| c.set(&self.port_store as *const _));
let _guard = PortStoreGuard;
let read_proc_val = self.evaluate(&closure_code)?;
```

`evaluate()` unconditionally sets and clears `PORT_STORE_PTR` itself via its own
`PortStoreGuard`. the outer set is overwritten; the outer guard drops a pointer that
`evaluate()`'s guard already nulled. dead code that misleads about invariants.

**fix:** remove the outer `PORT_STORE_PTR` setup and `_guard` from both functions.
remove the misleading comment "need PORT_STORE_PTR set for the evaluate call".

---

### 7. `CString::new().unwrap_or_default()` with mismatched length (13 occurrences)

**file:** `src/context.rs` (13 call sites, e.g. line 126)

```rust
let c_msg = CString::new(msg).unwrap_or_default();
ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t)
```

if `CString::new` fails (interior nul in `msg`), `unwrap_or_default()` returns a
single-byte `\0` CString, but `msg.len()` is still the original length. `tein_make_error`
would then pass that length to `sexp_c_string`, reading past the single byte — UB.

safe in practice (all messages are static literals / `format!` strings, never contain
nul), but the fallback is wrong and there is no comment explaining why it is safe.

**fix:** add a comment at the pattern explaining the invariant, or change the fallback to
something that preserves the length invariant (e.g. `unwrap_or_else(|_| CString::new("[nul in error message]").unwrap())`).

---

## suggestions

### 8. `ForeignStore` two-phase borrow — future split opportunity

**file:** `src/foreign.rs:297`

the immutable borrow for method lookup drops before the mutable borrow for `get_mut`.
correct, and the comment helps. a future refactor splitting `ForeignStore` into separate
`types` and `instances` `RefCell`s would eliminate the dance. not a bug.

---

### 9. no concurrent `ThreadLocalContext` test

**file:** `src/context.rs`

`ThreadLocalContext` is `Send + Sync` with a `Mutex<Receiver<Response>>` serialising
concurrent callers. there is no test exercising concurrent `evaluate()` calls from
multiple threads.

**fix:** add a test spawning 2+ threads calling `evaluate()` concurrently on a shared
`Arc<ThreadLocalContext>`.

---

### 10. `# Example` capitalisation — style violation (critical per project principles)

**files:** `src/managed.rs:22`, `src/error.rs:20`, `src/timeout.rs:17`

three module-level doc blocks use `# Example` (title case). the rest of the codebase uses
`# examples` (lowercase). per project convention and the sentence-case style, should be
`# examples`. missing/incorrect docs are critical bugs per project principles.

**fix:** s/`# Example`/`# examples`/ in all three files.

---

### 11. `transmute` in `register_protocol_fns` missing safety comment

**file:** `src/context.rs:528`

the transmute from 4-arg handler to 3-arg signature is intentional and mirrors the
documented pattern in `define_fn_variadic`, but the comment in `register_protocol_fns`
is less explicit than in the sandbox path.

**fix:** add a comment matching the clarity of the existing one in `define_fn_variadic`.

---

### 12. `foreign_ref` borrow conflict with mutable dispatch not documented

**file:** `src/context.rs:1485`

holding a `Ref<T>` from `foreign_ref()` while calling back into the context (e.g. via
`ctx.call()`) causes a `RefCell` panic in `dispatch_foreign_call`'s `borrow_mut()`.
the public API rustdoc does not warn of this.

**fix:** add a `# panics` section to `foreign_ref` documenting that holding the returned
ref across any evaluation or call will panic.
