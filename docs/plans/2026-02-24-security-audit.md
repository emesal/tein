# security audit — full codebase pass

**date:** 2026-02-24
**scope:** full codebase security review across five domains: FFI/unsafe rust, sandbox/isolation,
input validation/integer safety, threading/custom ports, C shim/reader/macro hook attack surface.

---

## critical

### 1. sandbox escape via `eval` / `interaction-environment` / `primitive-environment`

**files:** `src/sandbox.rs`, `vendor/chibi-scheme/opcodes.c`

the sandbox preset system restricts bindings by allowlist — but `eval`, `compile`,
`interaction-environment`, `current-environment`, `set-current-environment!`,
`scheme-report-environment`, and `primitive-environment` are **not in `ALL_PRESETS`
and have no stubs**. a sandboxed program can call `(eval code (interaction-environment))`
to execute arbitrary code in the full unrestricted env, completely defeating the preset model.

```scheme
;; bypasses all presets:
(eval '(open-input-file "/etc/passwd") (interaction-environment))
```

**fix:** add stubs for all of the above to `ALL_PRESETS` that raise `SandboxViolation`.
also audit all modules in the VFS to ensure none re-export these primitives.

---

### 2. macro expansion hook: unbounded re-analysis loop (DoS)

**files:** `vendor/chibi-scheme/eval.c` (lines ~801–814, ~1122, ~1186, ~1221),
`vendor/chibi-scheme/tein_shim.c` (lines ~412–418)

when the hook returns a replacement form, eval.c does `goto loop` to re-analyse it —
but `depth` is **not incremented per loop iteration**, only on recursive `analyze()` entry.
`SEXP_MAX_ANALYZE_DEPTH` is therefore never triggered within a single `analyze()` call.

a hook that returns the unexpanded form (or chains macros cyclically) causes infinite
re-analysis, bypassing the depth limit. with fuel limits this eventually halts; without
them it's a stack overflow / DoS.

**fix:** add a per-loop-iteration counter in the `goto loop` path, or document that
callers must always set a step limit when using `set_macro_expand_hook!`.

---

## high

### 3. port trampoline buffer bounds not validated (memory safety)

**file:** `src/context.rs` (lines ~279–348)

in `port_read_trampoline` and `port_write_trampoline`, `start` and `end` are extracted
from scheme fixnums and cast directly to `usize` with no validation:

- if `end < start`, `len = end - start` wraps (integer underflow → huge allocation)
- if `start`/`end` are negative fixnums, they become huge `usize` values
- `buf_data.add(start)` and `copy_nonoverlapping` / `slice::from_raw_parts` then
  operate on out-of-bounds memory → heap corruption

a scheme program supplying negative or reversed indices to a custom port closure can
trigger this.

**fix:** validate `start <= end`, `end <= actual_buf_len`, and that both are non-negative
before any pointer arithmetic.

---

### 4. context thread death silently hangs caller

**files:** `src/managed.rs` (lines ~124–206), `src/timeout.rs` (lines ~94–133)

the dedicated threads in `ThreadLocalContext` / `TimeoutContext` have no panic handler.
if any code panics (including registered foreign fns), the thread dies silently, the
channel send never fires, and the caller blocks forever on `recv()`. no timeout, no error.

additionally, `self.rx.lock().unwrap()` is held across a blocking `recv()` — a poisoned
mutex (from a prior panic) causes all future calls to panic.

**fix:** wrap the thread body in `std::panic::catch_unwind` and send an error response
on panic. use `.lock().map_err(|_| ...)` instead of `.unwrap()`.

---

### 5. thread-local policy state race between sequential contexts

**files:** `src/context.rs` (~L951–955, ~L1053–1059), `src/sandbox.rs`

`MODULE_POLICY` and `FS_POLICY` are thread-locals set during `build()` and cleared in
`Drop`. two sequential contexts on the same thread with different policies can interfere:

```rust
let ctx1 = ContextBuilder::new().standard_env().preset(&ARITHMETIC).build()?;
// MODULE_POLICY = VfsOnly (from ctx1)
let ctx2 = ContextBuilder::new().standard_env().build()?;
// MODULE_POLICY still VfsOnly! ctx2 inherits ctx1's policy
drop(ctx1);
// MODULE_POLICY = Unrestricted — ctx2 now has wrong policy
ctx2.evaluate("(import (chibi process))")?; // may now succeed unexpectedly
```

**fix:** bind policy ownership to the `Context` instance and restore the *previous* value
on drop rather than unconditionally clearing.

---

## medium

### 6. missing stubs for many chibi primitives

**file:** `src/sandbox.rs`

`ALL_PRESETS` covers ~50 primitives. chibi defines hundreds more in `opcodes.c`. any
primitive not in `ALL_PRESETS` is simply absent from restricted envs — no stub, no
`SandboxViolation`, just "undefined variable". this makes the restriction surface
unpredictable.

**fix:** audit `opcodes.c` for all dangerous primitives and add stubs (not just omissions)
for anything that should be explicitly forbidden.

---

### 7. non-UTF8 exception messages silently mangled in policy detection

**file:** `src/value.rs` (~L359)

exception messages are extracted with `from_utf8_lossy`, replacing invalid UTF-8 with
U+FFFD. since error strings are used in policy-violation detection (substring matching
on error messages), an attacker could embed invalid UTF-8 bytes to corrupt the match
string and potentially bypass detection.

**fix:** use `from_utf8` and propagate an error on invalid sequences, or document the
lossy behaviour and ensure policy detection doesn't rely on exact string content.

---

### 8. port / foreign handle IDs are forgeable

**files:** `src/port.rs`, `src/foreign.rs`

port and foreign handle IDs are plain monotonically-increasing `u64` values exposed to
scheme as fixnums. a scheme program can craft fixnums to guess valid IDs and access
ports or foreign objects it shouldn't.

**fix:** for security-sensitive deployments, use opaque or randomly-generated IDs
(e.g. via `rand::random::<u64>()`) so handles cannot be guessed.

---

### 9. `env_copy_named` parent-chain walk has no cycle detection

**file:** `vendor/chibi-scheme/tein_shim.c` (~L284–308)

the while loop walks `sexp_env_parent(env)` until `!sexp_envp(env)`. if a corrupted
environment has a cyclic parent chain, the loop runs forever. chibi shouldn't produce
cycles normally, but it's a defence-in-depth gap.

**fix:** add an iteration limit or visited-pointer check.

---

## low / informational

### 10. `sexp_vector_set` has no bounds check

**file:** `vendor/chibi-scheme/tein_shim.c` (~L95–97)

direct array write with no bounds check. the Rust caller is trusted today, but an
incorrect index would be a heap write OOB.

### 11. `u64` overflow in `ForeignStore::next_id` / `PortStore::next_id`

**files:** `src/foreign.rs`, `src/port.rs`

IDs wrap to 0 after 2^64 insertions with no saturation or panic, causing handle
collisions. practically unreachable, but worth a `checked_add` + panic.

### 12. reader dispatch table is ASCII-only (128 entries), undocumented

**file:** `vendor/chibi-scheme/tein_shim.c` (~L333), `src/context.rs` (~L1540)

chars > 127 are silently discarded. the public API docstring doesn't document this
limitation. fix: document it, or extend the table.

### 13. function pointer transmute in `define_fn_variadic` is not compile-time enforced

**file:** `src/context.rs` (~L1397)

safety relies on chibi's variadic calling convention. no compile-time check prevents
registering a fn with the wrong signature.

### 14. `CString` not used consistently in sandbox error path

**file:** `src/context.rs` (~L630)

one site casts a string literal directly to `*const c_char` instead of going through
`CString::new()`. safe today (rust string literals are null-terminated in practice),
but inconsistent.

### 15. unbounded channel in `ThreadLocalContext`

**file:** `src/managed.rs` (~L121)

`mpsc::channel()` is unbounded. a flood of `evaluate()` calls before responses arrive
causes unbounded memory growth. consider `sync_channel(N)`.

---

## summary by priority

| # | issue | severity |
|---|-------|----------|
| 1 | sandbox escape via `eval`/`interaction-environment` | **critical** |
| 2 | macro hook unbounded re-analysis loop | **critical** |
| 3 | port trampoline buffer bounds not validated | **high** |
| 4 | context thread death silently hangs caller | **high** |
| 5 | thread-local policy state race (sequential contexts) | **high** |
| 6 | missing stubs for many chibi primitives | medium |
| 7 | non-UTF8 exception messages mangled in policy detection | medium |
| 8 | port/foreign handle IDs are forgeable | medium |
| 9 | `env_copy_named` no cycle detection | medium |
| 10 | `sexp_vector_set` no bounds check | low |
| 11 | `u64` ID overflow, no saturation | low |
| 12 | reader dispatch ASCII-only, undocumented | low |
| 13 | fn pointer transmute not compile-time enforced | low |
| 14 | `CString` inconsistency in error path | low |
| 15 | unbounded channel in `ThreadLocalContext` | low |

---

## resolution status (2026-02-25)

| issue | description | status | commit |
|-------|-------------|--------|--------|
| 1 | sandbox escape via eval/interaction-environment | resolved | 9565414 |
| 2 | macro hook unbounded re-analysis loop | resolved | 6c0b6a0 |
| 3 | port trampoline buffer bounds not validated | resolved | da36d24 |
| 4 | context thread death silently hangs caller | resolved | aab2f30 |
| 5 | thread-local policy state race between sequential contexts | resolved | 2f8d252 |
| 6 | missing stubs for dangerous chibi primitives | resolved | a18342d |
| 7 | non-UTF8 exception messages mangled in policy detection | resolved | 6f24097 |
| 8 | port/foreign handle IDs are forgeable | resolved | 271d7f7 |
| 9 | env_copy_named no cycle detection | resolved | 1643f37 |
| 10 | sexp_vector_set no bounds check | resolved | fe6b339 |
| 11 | u64 ID overflow, no saturation | resolved | 3bbb0f3 (counter replaced by PRNG in 271d7f7) |
| 12 | reader dispatch ASCII-only, undocumented | resolved | 7e8f5ec |
| 13 | fn pointer transmute not compile-time enforced | deferred | low severity; no safe API exposure; tracked in TODO.md |
| 14 | CString inconsistency in error path | resolved | c319731 |
| 15 | unbounded channel in ThreadLocalContext | resolved | ef65526 |

---

## appendix: issue #13 implementation plan — eliminate fn pointer transmute

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Eliminate all `mem::transmute` fn-pointer casts in the codebase by making the C shim accept the correct 4-argument variadic signature, so the Rust FFI binding and every call site are fully type-correct without casts.

**Architecture:** The root cause is that chibi's `sexp_proc1` typedef is declared as a 3-argument function pointer even though the variadic calling convention passes a 4th `args` argument at runtime. The shim wrapper `tein_sexp_define_foreign_proc` inherits this lie. The fix is to introduce a new `tein_variadic_proc` typedef in the shim that has the correct 4-argument signature, update the shim function to cast internally (one `(sexp_proc1)` cast in C, clearly annotated), and update the Rust FFI binding + all call sites to use the correct type — eliminating every `mem::transmute`. All existing extern "C" fn definitions already have the correct 4-argument signature; only the binding and the transmutes change.

**Tech Stack:** Rust (edition 2024), C (chibi-scheme shim), `cargo test`, `cargo clippy`.

**Background for implementors:**

- Source lives under `tein/` (not the repo root). All `cargo` commands run from `tein/`.
- Tests are inline in `tein/src/context.rs` in the `#[cfg(test)]` block. External tests in `tein/tests/scheme_fn.rs`.
- `sexp_proc1` is chibi's C typedef: `typedef sexp (*sexp_proc1)(sexp, sexp, sexp_sint_t)` — 3 args. This is the lie.
- The variadic calling convention passes `(ctx, self, nargs, args)` — 4 args — regardless of the type.
- All `unsafe extern "C" fn` definitions in `context.rs` already use the correct 4-arg signature. The transmutes only exist to satisfy the 3-arg FFI binding.
- There are **5 transmute sites** in `context.rs` to eliminate:
  1. `define_fn_variadic` (~line 1447) — public API
  2. `register_protocol_fns` (~line 554) — internal registration loop
  3. IO wrapper registration in `ContextBuilder::build()` (~line 1065) — IO policy wrappers
  4. Sandbox stub registration (~line 1093) — `sandbox_stub` fn pointer
  5. ALWAYS_STUB registration (~line 1093) — same `stub_fn`, same transmute

- The shim file is `tein/vendor/chibi-scheme/tein_shim.c` (~line 71).
- The FFI binding is `tein/src/ffi.rs` (~line 121 for extern decl, ~line 434 for safe wrapper).
- No changes needed to chibi's own headers or C files.

---

### Task 1: Update tein_shim.c to accept the correct 4-arg variadic signature

**Files:**
- Modify: `tein/vendor/chibi-scheme/tein_shim.c` (~line 71)

**Background:** The shim currently accepts `sexp_proc1` (3-arg). We introduce `tein_variadic_proc` (4-arg) and cast internally. This is the one place where the lie is acknowledged and contained.

**Step 1: Read the current shim function**

In `tein/vendor/chibi-scheme/tein_shim.c`, find:

```c
sexp tein_sexp_define_foreign_proc(sexp ctx, sexp env, const char* name,
                                   int num_args, int flags,
                                   const char* fname, sexp_proc1 f) {
    return sexp_define_foreign_proc_aux(ctx, env, name, num_args, flags, fname, f, NULL);
}
```

**Step 2: Replace with the corrected version**

Add a typedef and update the function signature. The cast from `tein_variadic_proc` to `sexp_proc1` is the one intentional cast — kept in C where chibi's own type lives:

```c
/* correct 4-argument signature for variadic foreign functions.
 * chibi's sexp_proc1 is declared as 3-arg, but the variadic calling
 * convention always passes (ctx, self, nargs, args) — 4 args. this
 * typedef matches the actual ABI; the cast to sexp_proc1 below is the
 * single intentional shim between chibi's type and the real signature. */
typedef sexp (*tein_variadic_proc)(sexp, sexp, sexp_sint_t, sexp);

sexp tein_sexp_define_foreign_proc(sexp ctx, sexp env, const char* name,
                                   int num_args, int flags,
                                   const char* fname, tein_variadic_proc f) {
    return sexp_define_foreign_proc_aux(ctx, env, name, num_args, flags, fname,
                                        (sexp_proc1)f, NULL);
}
```

**Step 3: Build to confirm no C errors**

```bash
cd tein && cargo build 2>&1 | grep -E "error|warning"
```

Expected: clean build (or only pre-existing warnings unrelated to the shim).

**Step 4: Commit**

```bash
git add tein/vendor/chibi-scheme/tein_shim.c
git commit -m "fix: introduce tein_variadic_proc typedef in shim to match real 4-arg variadic ABI"
```

---

### Task 2: Update the FFI binding to use the correct 4-arg signature

**Files:**
- Modify: `tein/src/ffi.rs` (~line 121 and ~line 434)

**Background:** Two places declare the 3-arg signature: the `extern "C"` block entry and the safe wrapper. Both need updating to `(sexp, sexp, sexp_sint_t, sexp) -> sexp`.

**Step 1: Update the extern block declaration (~line 121)**

Find:

```rust
    pub fn tein_sexp_define_foreign_proc(
        ctx: sexp,
        env: sexp,
        name: *const c_char,
        num_args: c_int,
        flags: c_int,
        fname: *const c_char,
        f: Option<unsafe extern "C" fn(sexp, sexp, sexp_sint_t) -> sexp>,
    ) -> sexp;
```

Replace with:

```rust
    pub fn tein_sexp_define_foreign_proc(
        ctx: sexp,
        env: sexp,
        name: *const c_char,
        num_args: c_int,
        flags: c_int,
        fname: *const c_char,
        f: Option<unsafe extern "C" fn(sexp, sexp, sexp_sint_t, sexp) -> sexp>,
    ) -> sexp;
```

**Step 2: Update the safe wrapper (~line 434)**

Find:

```rust
pub unsafe fn sexp_define_foreign_proc(
    ctx: sexp,
    env: sexp,
    name: *const c_char,
    num_args: c_int,
    flags: c_int,
    fname: *const c_char,
    f: Option<unsafe extern "C" fn(sexp, sexp, sexp_sint_t) -> sexp>,
) -> sexp {
    unsafe { tein_sexp_define_foreign_proc(ctx, env, name, num_args, flags, fname, f) }
}
```

Replace with:

```rust
pub unsafe fn sexp_define_foreign_proc(
    ctx: sexp,
    env: sexp,
    name: *const c_char,
    num_args: c_int,
    flags: c_int,
    fname: *const c_char,
    f: Option<unsafe extern "C" fn(sexp, sexp, sexp_sint_t, sexp) -> sexp>,
) -> sexp {
    unsafe { tein_sexp_define_foreign_proc(ctx, env, name, num_args, flags, fname, f) }
}
```

**Step 3: Build — expect errors at all transmute call sites (that's the point)**

```bash
cd tein && cargo build 2>&1 | grep "error"
```

Expected: type mismatch errors at every transmute site in `context.rs` — these are the 5 sites we'll fix in Task 3.

**Step 4: Commit (even with errors — the binding is now correct)**

```bash
git add tein/src/ffi.rs
git commit -m "fix: update sexp_define_foreign_proc binding to correct 4-arg variadic signature"
```

---

### Task 3: Remove all transmutes in context.rs — site 1: define_fn_variadic

**Files:**
- Modify: `tein/src/context.rs` (`define_fn_variadic`, ~line 1436)

**Background:** `define_fn_variadic` takes the correct 4-arg fn type from callers, then transmutes down to 3-arg to satisfy the old binding. Now that the binding is fixed, the transmute and intermediate local variable are gone.

**Step 1: Find the transmute in define_fn_variadic**

Current code (~line 1444):

```rust
        unsafe {
            let env = ffi::sexp_context_env(self.ctx);
            let f_typed: Option<
                unsafe extern "C" fn(ffi::sexp, ffi::sexp, ffi::sexp_sint_t) -> ffi::sexp,
            > = std::mem::transmute::<*const std::ffi::c_void, _>(f as *const std::ffi::c_void);
            let result = ffi::sexp_define_foreign_proc(
                self.ctx,
                env,
                c_name.as_ptr(),
                0,
                ffi::SEXP_PROC_VARIADIC,
                c_name.as_ptr(),
                f_typed,
            );
```

**Step 2: Replace — pass f directly**

```rust
        unsafe {
            let env = ffi::sexp_context_env(self.ctx);
            let result = ffi::sexp_define_foreign_proc(
                self.ctx,
                env,
                c_name.as_ptr(),
                0,
                ffi::SEXP_PROC_VARIADIC,
                c_name.as_ptr(),
                Some(f),
            );
```

**Step 3: Build to check this site**

```bash
cd tein && cargo build 2>&1 | grep "error" | head -20
```

Expected: this error gone, remaining errors at the other 4 sites.

**Step 4: Commit**

```bash
git add tein/src/context.rs
git commit -m "fix: remove transmute in define_fn_variadic — pass fn ptr directly to corrected binding"
```

---

### Task 4: Remove transmute — site 2: register_protocol_fns loop

**Files:**
- Modify: `tein/src/context.rs` (`register_protocol_fns`, ~line 527)

**Background:** `register_protocol_fns` builds a slice of `(&str, 4-arg-fn)` pairs and transmutes each fn to 3-arg in the loop body.

**Step 1: Find the transmute in register_protocol_fns (~line 554)**

Current loop body:

```rust
        for (name, f) in protocol_fns {
            let env = ffi::sexp_context_env(ctx);
            let c_name = CString::new(*name).unwrap();
            let f_typed: Option<
                unsafe extern "C" fn(ffi::sexp, ffi::sexp, ffi::sexp_sint_t) -> ffi::sexp,
            > = std::mem::transmute::<*const std::ffi::c_void, _>(*f as *const std::ffi::c_void);
            ffi::sexp_define_foreign_proc(
                ctx,
                env,
                c_name.as_ptr(),
                0,
                ffi::SEXP_PROC_VARIADIC,
                c_name.as_ptr(),
                f_typed,
            );
        }
```

**Step 2: Replace — pass f directly**

```rust
        for (name, f) in protocol_fns {
            let env = ffi::sexp_context_env(ctx);
            let c_name = CString::new(*name).unwrap();
            ffi::sexp_define_foreign_proc(
                ctx,
                env,
                c_name.as_ptr(),
                0,
                ffi::SEXP_PROC_VARIADIC,
                c_name.as_ptr(),
                Some(*f),
            );
        }
```

**Step 3: Build to confirm**

```bash
cd tein && cargo build 2>&1 | grep "error" | head -20
```

**Step 4: Commit**

```bash
git add tein/src/context.rs
git commit -m "fix: remove transmute in register_protocol_fns loop"
```

---

### Task 5: Remove transmute — site 3: IO wrapper registration in ContextBuilder::build

**Files:**
- Modify: `tein/src/context.rs` (IO wrapper registration in `ContextBuilder::build`, ~line 1058)

**Background:** `wrapper_fn_for(op)` returns a 4-arg fn; `build()` currently transmutes it to 3-arg before passing to `sexp_define_foreign_proc`.

**Step 1: Find the transmute (~line 1058)**

```rust
                        let wrapper = wrapper_fn_for(op);
                        // transmute to match the 3-arg signature ffi expects
                        let f_typed: Option<
                            unsafe extern "C" fn(
                                ffi::sexp,
                                ffi::sexp,
                                ffi::sexp_sint_t,
                            ) -> ffi::sexp,
                        > = std::mem::transmute::<*const std::ffi::c_void, _>(
                            wrapper as *const std::ffi::c_void,
                        );
                        ffi::sexp_define_foreign_proc(
                            ctx,
                            null_env,
                            c_name.as_ptr(),
                            0,
                            ffi::SEXP_PROC_VARIADIC,
                            c_name.as_ptr(),
                            f_typed,
                        );
```

**Step 2: Replace**

```rust
                        let wrapper = wrapper_fn_for(op);
                        ffi::sexp_define_foreign_proc(
                            ctx,
                            null_env,
                            c_name.as_ptr(),
                            0,
                            ffi::SEXP_PROC_VARIADIC,
                            c_name.as_ptr(),
                            Some(wrapper),
                        );
```

**Step 3: Build to confirm**

```bash
cd tein && cargo build 2>&1 | grep "error" | head -20
```

**Step 4: Commit**

```bash
git add tein/src/context.rs
git commit -m "fix: remove transmute in IO wrapper registration"
```

---

### Task 6: Remove transmute — sites 4 & 5: sandbox stub registration

**Files:**
- Modify: `tein/src/context.rs` (sandbox stub registration in `ContextBuilder::build`, ~line 1091)

**Background:** `sandbox_stub` is a 4-arg fn. It's currently transmuted to 3-arg and stored in `stub_fn` which is then passed to two registration loops (ALL_PRESETS stubs and ALWAYS_STUB stubs).

**Step 1: Find the stub_fn transmute (~line 1091)**

```rust
                let stub_fn: Option<
                    unsafe extern "C" fn(ffi::sexp, ffi::sexp, ffi::sexp_sint_t) -> ffi::sexp,
                > = std::mem::transmute::<*const std::ffi::c_void, _>(
                    sandbox_stub as *const std::ffi::c_void,
                );
```

**Step 2: Replace — use the correct type directly**

```rust
                let stub_fn: Option<
                    unsafe extern "C" fn(ffi::sexp, ffi::sexp, ffi::sexp_sint_t, ffi::sexp) -> ffi::sexp,
                > = Some(sandbox_stub);
```

The two loops that use `stub_fn` below this point require no changes since `stub_fn`'s type now matches `sexp_define_foreign_proc`'s parameter type.

**Step 3: Build — expect clean**

```bash
cd tein && cargo build 2>&1 | grep -E "^error"
```

Expected: no errors.

**Step 4: Run clippy to confirm no transmutes remain**

```bash
cd tein && cargo clippy 2>&1 | grep -i "transmute\|warning\|error"
```

Expected: no transmute warnings, clean clippy.

**Step 5: Confirm no transmutes remain in context.rs**

```bash
grep -n "transmute" tein/src/context.rs
```

Expected: no matches.

**Step 6: Commit**

```bash
git add tein/src/context.rs
git commit -m "fix: remove transmute in sandbox stub registration — sandbox_stub now passed directly"
```

---

### Task 7: Run full test suite and verify

**Step 1: Run all tests**

```bash
cd tein && cargo test 2>&1 | grep "test result"
```

Expected: all test suites pass (205+ lib tests, 12 scheme_fn tests, 21 doc tests, etc.).

**Step 2: Run clippy**

```bash
cd tein && cargo clippy 2>&1 | grep -v "^$\|Checking\|Compiling\|Finished"
```

Expected: clean.

**Step 3: Confirm zero transmutes remain anywhere in tein/src/**

```bash
grep -rn "mem::transmute" tein/src/
```

Expected: no matches.

**Step 4: Update resolution status in the audit doc**

In `docs/plans/2026-02-24-security-audit.md`, change issue #13's status from `deferred` to `resolved` and add the final commit hash.

**Step 5: Commit**

```bash
git add docs/plans/2026-02-24-security-audit.md
git commit -m "docs: mark issue #13 (fn pointer transmute) resolved"
```
