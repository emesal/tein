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
