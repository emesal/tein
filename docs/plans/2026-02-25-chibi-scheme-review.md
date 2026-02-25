# chibi-scheme upstream code review — security & correctness

> **scope:** upstream chibi-scheme C code (not tein patches). focus: memory safety,
> GC correctness, buffer handling, integer overflow, undefined behaviour, error propagation.

> **methodology:** verify the bug → verify the solution → implement → update plan → commit.

**date:** 2026-02-25
**branch:** bugfix/mvp-code-review-2602
**source:** ~/forks/chibi-scheme (emesal/chibi-scheme, branch emesal-tein)
**reviewed by:** 6 parallel sonnet agents, consolidated by opus

## prior art

- `2026-02-25-c-code-review.md` — reviewed tein patches + rust↔c boundary (14 findings, all resolved)
- `2026-02-24-security-audit.md` — full codebase security audit (15 findings, all resolved)

this review covers the **upstream chibi code we ship** — code written by alex shinn,
not our patches. we're responsible for shipping it, so we need to understand its quality.

## cross-reference with upstream master

our fork (branch `emesal-tein`) is 250 commits behind `master` and 2 commits ahead (our
tein patches). of those 250 master commits, only **8 touch reviewed C files** — mostly
snow-fort tooling, float conversion fixes, and one critical security fix:

| master commit | files | fixes finding? |
|---|---|---|
| `72ec53ca` — "more thorough checks for SEXP_MIN_FIXNUM/-1" | vm.c, sexp.c, eval.c, sexp.h | **yes → M15** (quotient UB) |
| `558e1a89` — "bind stack result to local var before casting" | vm.c | partial precursor to M15 fix |
| `cfbe7cdf` — "use sexp_read_incomplete_error" | sexp.c | no (error subtyping, not a safety fix) |
| `a8448545` — "don't allow mixing rational/float syntax" | sexp.c | no (input validation improvement) |
| `d4028f95` — "manually encoding non-finite f16 values" | sexp.c | no (half-float NaN/Inf handling) |
| `b1a370b2` — "add link to paper for float conversions" | sexp.c | no (comment only) |
| `9d6af60e` — "add nan/inf macros for Plan9" | sexp.h | no (Plan9 only) |
| `243fd41a` — "conform to 1/0 style" | sexp.c | no (style) |

**only M15 is fixed on master.** all other findings (including both criticals) remain
unfixed on master as of 2026-02-25. the `72ec53ca` fix should be cherry-picked into our
fork when we next rebase.

## subsystems reviewed

| # | subsystem | files | ~lines |
|---|-----------|-------|--------|
| 1 | GC | gc.c, gc_heap.c, gc_heap.h | 1650 |
| 2 | VM | vm.c, opcodes.c | 2700 |
| 3 | reader/sexp | sexp.c, sexp.h | 6100 |
| 4 | evaluator/compiler | eval.c, eval.h | 3100 |
| 5 | bignum | bignum.c, bignum.h | 2400 |
| 6 | config | features.h | 1100 |

---

## summary

| severity | count | resolved | subsystems |
|----------|-------|----------|------------|
| critical | 2 | **2** | evaluator |
| high | 6 | **6** | evaluator(2), reader(2), bignum(1), config(1) |
| medium | 25 | **16** | all subsystems |
| low | 17 | 0 | all subsystems |

---

## critical

### CR1. `analyze_app` — use-after-exception reads garbage car (eval.c:753) — **fixed**

`analyze_list()` can return an exception sexp on OOM. the very next line calls `sexp_car(res)`
unconditionally. if `res` is an exception, `sexp_car` reads a field at a fixed offset in the
exception struct, then `sexp_lambdap` dereferences that garbage value.

**impact:** memory corruption / UB on any allocation failure inside `analyze_list`. reachable
from user scheme code under memory pressure.

**fix:** add `if (sexp_exceptionp(res)) return res;` before line 756.

**resolved:** `43d4bb72` in emesal/chibi-scheme (emesal-tein branch).

### CR2. `sexp_extend_synclo_env` — unchecked OOM in env chain construction (eval.c:250) — **fixed**

~~`e2` is unrooted across allocation points~~ — **correction:** chibi uses mark-sweep (non-moving)
GC, so the "unrooted" framing is misleading. `e2` is always reachable from the GC-rooted `e`
through the env parent chain, so the collector will mark it and never sweep it. the GC rooting
concern does not apply to non-moving collectors.

the **real bug** is unchecked OOM: `sexp_alloc_type` can return the global OOM exception sexp.
if this happens, the exception is written into as if it were an env struct (via
`sexp_env_bindings`, `sexp_env_syntactic_p`, `sexp_env_parent`), corrupting the global OOM error
object and producing a bogus env chain. additionally, the existing early-return on line 265
(`if (!e2)`) leaks the GC root (missing `sexp_gc_release1`).

**impact:** heap corruption of the global OOM error object under memory pressure during
hygienic macro expansion. requires OOM to trigger — not reachable in normal operation.

**fix:** add `sexp_exceptionp` checks after both `sexp_alloc_type` calls. break out the
dense ternary into explicit if/else for clarity. fix the `!e2` early-return to release GC root.

**resolved:** `43d4bb72` in emesal/chibi-scheme (emesal-tein branch).

---

## high

### ~~H1.~~ → M19. GC finaliser resurrection — swept after finalisation (gc.c:566) — **mitigated**

`sexp_finalize` runs on unmarked objects, then `sexp_sweep` reclaims them. if a finaliser
saves a reference to the dying object (object resurrection), sweep still reclaims the memory.
any subsequent access through the saved reference is use-after-free.

**impact:** dangling pointer from any finaliser that resurrects. built-in finalisers (port, DL)
don't resurrect, but user-defined types can. affects tein if foreign type finalisers ever
reference scheme-managed fields of the dying object.

**mitigation:** not exploitable in tein. `SEXP_USE_DL=0` prevents scheme code from registering
custom types with finalisers. built-in finalisers (port, fileno) don't resurrect. tein's
`ForeignType` protocol stores objects rust-side in `ForeignStore`, bypassing chibi finalisers
entirely. defensive comments added in `build.rs` and `foreign.rs`. downgraded from high.

### ~~H2.~~ → M20. GC re-entrancy from allocating finalisers (gc.c:566) — **mitigated**

a finaliser can call `sexp_apply` or any allocating function, re-entering `sexp_gc`. the inner
GC runs mark+sweep. the outer finaliser loop has a stale heap cursor `p` and `h` pointer.
after the inner GC potentially reorganises/shrinks the heap, the outer loop iterates from
stale positions — heap walk corruption.

**impact:** high if any finaliser allocates, which is common in scheme.

**mitigation:** not exploitable in tein. built-in finalisers (`sexp_finalize_port`,
`sexp_finalize_fileno`) only call POSIX `close()`/`fclose()` — no scheme allocations.
`SEXP_USE_DL=0` prevents user-registered finalisers. downgraded from high.

### ~~H3.~~ → M21. GC finaliser sees half-collected referenced objects (gc.c:420) — **mitigated**

`sexp_finalize` scans all unmarked objects and calls their finaliser. but the dying object's
slots may point to *other* unmarked objects that have already been finalised or whose memory
content is no longer valid. no ordering by reachability (except the DL two-pass).

**impact:** finalisers that access fields of the dying object may read garbage.

**mitigation:** not exploitable in tein. `sexp_finalize_port` accesses `sexp_port_fd(port)`
(a File-Descriptor slot), but only reads primitive integer fields (`sexp_fileno_openp`,
`sexp_fileno_count`) — valid regardless of the fileno's mark state. the DL two-pass handles
the DL dependency case (moot with `SEXP_USE_DL=0`). downgraded from high.

### ~~H4.~~ → M22. heap growth integer overflow — `sexp_heap_pad_size` (gc.c:628) — **mitigated**

`new_size` near `SIZE_MAX` causes `sexp_heap_pad_size(size)` (= `sizeof(heap_t) + size + align`)
to overflow. `malloc`/`mmap` gets a tiny size, returns success, but the heap is initialised as
if it were `new_size` bytes — immediate OOB writes.

**impact:** real integer overflow vulnerability. reachable if heap exhaustion drives many
growth cycles (unlikely in practice but possible in constrained embedded deployments).

**mitigation:** on 64-bit systems (tein's target), `SIZE_MAX` is 16 EiB — unreachable via
heap growth. `sexp_grow_heap` (gc.c:628) uses `ceil(SEXP_GROW_HEAP_FACTOR * cur_size)`, so
overflow requires the process to already hold near-`SIZE_MAX` memory. further mitigated once
H13's `heap_limit()` API is added, which bounds `total_size` via `max_size` check in
`sexp_alloc` (gc.c:728). downgraded from high.

### ~~H5.~~ → M23. image loading: unbounded `fread` from untrusted file (gc_heap.c:652) — **mitigated**

`header.size` comes from the image file with no upper bound validation. if `header.size +
heap_free_size` overflows the allocation arithmetic, `fread` writes past the heap buffer.

**impact:** heap buffer overflow from malicious image files. only relevant if
`SEXP_USE_IMAGE_LOADING=1` and application loads user-provided images.

**mitigation:** `SEXP_USE_IMAGE_LOADING` defaults to `SEXP_USE_DL && ...` (features.h:909).
since tein sets `SEXP_USE_DL=0`, image loading is compiled out entirely. downgraded from high.

### ~~H6.~~ → M24. image loading: packed heap size overflow (gc_heap.c:256) — **mitigated**

`packed_size + free_size + sexp_free_chunk_size + 128` — all `size_t` additions, no overflow
check. overflow wraps to tiny allocation, subsequent copy writes past it.

**mitigation:** same as M23 — image loading compiled out via `SEXP_USE_DL=0`. downgraded
from high.

### H7. `sexp_env_import_op` — env chain corruption on OOM (eval.c:2697) — **fixed**

env chain is mutated in-place *before* allocations that can fail. if `sexp_make_env` returns
an exception, that exception is spliced into the env parent chain, permanently corrupting it.

**fix:** add `sexp_exceptionp` checks after both `sexp_make_env` calls. on OOM, release GC
roots and return the exception immediately before mutating the env chain.

**resolved:** `c373db43` in emesal/chibi-scheme (emesal-tein branch).

### H8. duplicate parameter check silently disabled for >= 100 params (eval.c:887) — **fixed**

`verify_duplicates_p = sexp_length_unboxed(sexp_cadr(x)) < 100` — lambdas with 100+ params
skip duplicate checking entirely. both params get env entries; second shadows first; generated
bytecode accesses wrong stack slot.

**impact:** incorrect code generation, spec violation.

**fix:** removed the 100-param threshold entirely. `sexp_memq` is O(n) per param making
the check O(n²), but nobody has 100+ params in scheme — the performance hack was a
correctness trap.

**resolved:** `c373db43` in emesal/chibi-scheme (emesal-tein branch).

### ~~H9.~~ → M25. `sexp_load_standard_env` — stack buffer + unchecked version (eval.c:2612) — **mitigated**

`init_file[128]` — version byte written without range check (`version + '0'` overflows for
version >= 10). no check that path fits in buffer.

**mitigation:** tein hardcodes `sexp_make_fixnum(7)` as the version parameter (r7rs).
the resulting path `"init-7.scm"` is 11 bytes, well under the 128-byte buffer. defensive
comment added at the call site in `context.rs`. downgraded from high.

### H10. reader label `c2` integer overflow — OOB array access (sexp.c:3617) — **fixed**

`c2` is `int`, accumulated via `c2 = c2 * 10 + digit_value(c1)` with no overflow check.
signed overflow is UB in C. after overflow, `c2` is used directly as an array index into
the shares vector without bounds checking — OOB read/write.

**impact:** heap corruption from crafted reader input like `#2147483648=...`.

**fix:** add `c2 > (INT_MAX - 9) / 10` guard before the multiply. on overflow, emit
`sexp_read_error` and break out of the accumulation loop; the exception propagates via
the existing `sexp_exceptionp(res)` check.

**resolved:** `9d722d26` in emesal/chibi-scheme (emesal-tein branch).

### H11. `sexp_decode_utf8_char` — wrong 4-byte decode formula (sexp.c:3104) — **fixed**

~~the formula uses mask `0x0F` instead of `0x07` for the leading byte~~ — **correction:** mask
`0x0F` is incidentally correct because the input range `0xF0-0xF7` constrains bit 3 to always
be 0, so `0x0F` and `0x07` produce identical results. the actual bugs are the **shifts**: the
leading byte was shifted `<<16` instead of `<<18`, and byte 1 was shifted `<<6` instead of
`<<12`. any supplementary-plane character (> U+FFFF) decodes to garbage.

**impact:** wrong data for 4-byte UTF-8 character literals (`#\` path). verified with U+1F600:
buggy decode produces U+0DC0, correct decode produces U+1F600.

**fix:** change shifts to `<<18` and `<<12` respectively.

**resolved:** `9d722d26` in emesal/chibi-scheme (emesal-tein branch).

### H12. `sexp_bignum_fxdiv` — no divide-by-zero guard (bignum.c:268) — **fixed**

public API function divides by `b` unconditionally. `b == 0` → integer division UB on native
path, silent wrong result on custom-long-longs path. callers check before calling, but the API
itself is a trap.

**fix:** add `if (b == 0) return 0;` guard before the division loop.

**resolved:** `051abfc1` in emesal/chibi-scheme (emesal-tein branch).

### H13. config: no heap size limit by default (features.h) — **already resolved**

`SEXP_MAXIMUM_HEAP_SIZE=0` — heap grows without bound. a scheme program can exhaust host
process memory. `TimeoutContext` limits wall-clock time but not memory.

**recommendation:** expose `heap_limit(usize)` on `ContextBuilder`, threading through to
`sexp_make_context`'s `max_size` parameter. (runtime, not compile-time.)

**resolved:** `ContextBuilder::heap_max(usize)` already exists with a default of 128 MiB,
passed through to `sexp_make_eval_context`'s `max_size` parameter. chibi's `sexp_alloc`
(gc.c:728) checks `(!h->max_size) || (total_size < h->max_size)` before growing the heap.
this also strengthens the mitigation for M22 (heap growth overflow).

---

## medium

### M1. reader: UTF-8 escape writes before buffer expansion check (sexp.c:2623) — **fixed**

`sexp_utf8_encode_char` writes up to 4 bytes into `buf + i` *before* `maybe_expand` checks
buffer space. heap overflow of up to 3 bytes at buffer boundary.

**fix:** inline buffer expansion check (`i + len >= size`) before `sexp_utf8_encode_char` call,
using the exact `len` from `sexp_utf8_char_byte_count`. the post-encode `goto maybe_expand`
is retained for the next iteration's headroom.

### M2. reader: `sexp_push_char` unsigned underflow (sexp.h:1662) — **fixed**

`--sexp_port_offset(p)` when offset is 0 wraps to `SIZE_MAX`. subsequent `buf[SIZE_MAX] = c`
is a wild write. latent — all 32 callers read before pushing back.

**fix:** added `sexp_port_offset(p) > 0` guard in the macro. silently drops the push on
underflow (returns 0 like the EOF case). defensive — no current caller triggers it.

### M3. reader: label lookup uses raw `c2` without range check (sexp.c:3622) — **fixed**

even when the shares vector exists, `sexp_vector_data(*shares)[c2]` uses the raw (potentially
overflowed) `c2` without checking `0 <= c2 < vector_length - 1`. the out-of-order check (+16
max gap) and vector doubling indirectly prevent OOB in practice, but the invariant is fragile.

**fix:** added explicit `c2 >= vector_length - 1` bounds check in the reference path (`#N#`).
changed definition path (`#N=`) vector growth from single `if` to `while` loop (doubles until
`c2` fits), with OOM check on each allocation and max-label slot preservation.

### M4. evaluator: `res` not GC-rooted across hook arg construction (eval.c:792) — **fixed**

in `analyze_macro_once`, between macro application and hook argument construction, `res`
holds the expansion result as a raw local. the `sexp_cons` calls for hook_args can trigger GC.
upstream `res` was never rooted — safe pre-patch because no allocation happened, but the hook
changes the allocation pattern.

**fix:** changed `sexp_gc_var2(tmp, hook_args)` → `sexp_gc_var3(res, tmp, hook_args)` with
matching preserve3/release3. this is our bug — introduced by the macro hook patch (D).

### M5. evaluator: `sexp_env_cell_define` — key/value not rooted (eval.c:153) — **mitigated**

`sexp_env_push` → `sexp_cons(key, value)` can GC. neither `key` nor `value` is in the root
set. callers usually hold roots, but the function's own contract is fragile.

**mitigation:** safe in practice — all callers root their arguments, and common values
(symbols, SEXP_VOID, SEXP_UNDEF) are immediates. added defensive comment documenting the
caller-rooting convention.

### M6. evaluator: `sexp_env_define` — no GC preservation at all (eval.c:190) — **fixed**

same pattern as M5 but worse — no `sexp_gc_var` / `sexp_gc_preserve` at all. `key`, `value`,
`env` can all be moved. additionally, `sexp_env_push` can return an exception on OOM via
`tmp`, which was never checked — the exception gets spliced into the binding chain.

**fix:** added `sexp_exceptionp(tmp)` checks after both `sexp_env_push` calls. propagates
OOM instead of corrupting the env binding chain.

### M7. evaluator: unsafe env binding splice (eval.c:2657) — **fixed**

manually inserts cons cell into env binding chain. assumes `sexp_env_bindings(e)` is non-null.
`SEXP_NULL` is an immediate (0x2e), not a valid pointer — dereferencing it as a pair is UB.

**fix:** added `sexp_pairp(sexp_env_bindings(e))` guard. if bindings are empty, sets `tmp`
as the first binding with `SEXP_NULL` next pointer.

### M8. evaluator: `to == from` aliasing in import (eval.c:2697) — **fixed**

self-import destructively empties `to`'s bindings into a new frame, then looks up names in
`from` (= the now-empty `to`). silently imports nothing.

**fix:** added `if (to == from) return SEXP_VOID;` early return after parameter validation.

### M9. GC: image version check bug (gc_heap.c:586)

`header.major < SEXP_IMAGE_MINOR_VERSION` — compares major against minor. allows loading
images with incompatible minor version, potentially causing pointer-adjustment corruption.

### M10. GC: `sexp_limited_malloc` global counter unsynchronised (gc.c:96)

`allocated_bytes += size` is a data race with `SEXP_USE_LIMITED_MALLOC=1` and concurrent
contexts. doesn't affect tein (not our config, one context per thread).

### M11. GC: finaliser called with NULL self (gc.c:458)

`finalizer(ctx, NULL, 1, p)` — undocumented contract. any finaliser that dereferences `self`
will segfault.

### M12. GC: OOM returns shared global object (gc.c:731)

unchecked callers that write into the "freshly allocated" object overwrite the shared global
OOM error object's fields, corrupting future OOM reporting. tein is protected (our wrappers
check).

### M13. VM: error handler frame push without stack bounds check (vm.c:1225)

`call_error_handler` writes 4 words past `top` unconditionally. the 64-slot padding makes
this survivable normally, but adversarial code that fills the pad then triggers an exception
can overwrite past the stack.

### M14. VM: `SEXP_OP_SLOT_REF/SET` — slot index not bounds-checked (vm.c:1682)

`_UWORD1` from bytecode used directly as slot offset. type check passes but index check is
missing. corrupt bytecode → OOB heap read/write.

### M15. VM: `SEXP_OP_QUOTIENT` with `MIN_FIXNUM / -1` — UB before bignum escape (vm.c:1916) — **fixed on master**

`sexp_fx_div` is evaluated first (UB), then the result is checked and the bignum fallback
fires. the UB has already occurred. **fixed upstream in `72ec53ca`** — checks for the
specific MIN_FIXNUM/-1 case *before* calling `sexp_fx_div`. cherry-pick when rebasing.

### M16. VM: `SEXP_OP_SC_LT/SC_LE` — no type check (vm.c:2074)

string-cursor comparison opcodes compare raw pointer values without verifying operands are
actually string cursors. wrong-type values produce silent semantic corruption.

### M17. bignum: `sexp_bignum_split` unsigned underflow when `k >= alen` (bignum.c:513)

`alen - k` wraps to huge value if invariant breaks. safe today (only caller enforces
`k < alen`), but no assertion.

### M18. bignum: `sexp_write_bignum` buffer underflow for non-power-of-2 bases (bignum.c:389)

`lg_base = log2i(base)` underestimates digits for bases like 10. for some bignums, `i`
decrements past 0, writing `data[-1]` — heap corruption.

---

## low

### L1. evaluator: `sexp_warn` calls `exit(1)` in strict mode (eval.c:70)

in strict mode, any warning terminates the entire process. bypasses all rust error handling.
confirm strict mode is never enabled in tein contexts.

### L2. evaluator: `sexp_find_module_file_raw` reads `dir[-1]` on empty path (eval.c:2448)

`dirlen == 0` → `dir[dirlen-1]` is UB.

### L3. evaluator: `analyze_bind_syntax` potential NULL deref (eval.c:1069)

`sexp_env_cell` can return NULL if OOM caused `sexp_env_push` to fail.

### L4. evaluator: OOM in `sexp_expand_bcode` not propagated (eval.c:331)

exception stored in context but function returns void.

### L5. reader: `strlen` for UTF-8 length in `sexp_decode_utf8_char` (sexp.c:3098)

fails on embedded NUL bytes in character names.

### L6. reader: cycle detection only covers pairs (sexp.c:2202)

vectors containing themselves caught only by depth bound (10000).

### L7. reader: `\xNNNN;` in strings not validated for Unicode range (sexp.c:2611)

surrogates (0xD800–0xDFFF) and values > 0x10FFFF pass through.

### L8. reader: NUMBUF_LEN relies on undocumented float formatting invariant (sexp.c:2274)

three blind increments after `snprintf` safe by invariant, but not enforced.

### L9. reader: `sexp_ratio_normalize` mutates in-place (sexp.c:2907)

unsafe if called on shared/immutable ratios.

### L10. VM: `sexp_grow_stack` copies `top+2` elements (vm.c:882)

off-by-two copy of dead slots past `top`.

### L11. VM: `SEXP_OP_INT2CHAR` no Unicode range check (vm.c:2089)

negative or > 0x10FFFF fixnums silently accepted as characters.

### L12. VM: `sexp_poll_port` passes `nfds=1` instead of `fd+1` (vm.c:1059)

only watches fd 0 regardless of actual fd.

### L13. VM: variadic rest-arg construction may not sync `sexp_context_top` (vm.c:1342)

`sexp_cons` allocations before `ctx->top` is updated. if GC only scans up to `ctx->top`,
live args above it are invisible.

### L14. bignum: `sexp_make_bignum` no overflow check on size (bignum.c:19)

`len * sizeof(sexp_uint_t)` can overflow.

### L15. bignum: `sexp_read_bignum` no exception check after alloc (bignum.c:308)

writes into potentially-exception result from `sexp_make_bignum`.

### L16. bignum: `sexp_bignum_quot_rem` sign-flip lacks termination proof (bignum.c:648)

no formal bound on correction iterations.

### L17. GC: image path `snprintf` truncation → OOB write (gc_heap.c:626)

`path[512]` — long `mod_path` causes offset past buffer.

---

## config recommendations (features.h)

### enable in debug builds

```c
-DSEXP_USE_HEADER_MAGIC=1      // lightweight GC corruption detector (+4 bytes/obj)
-DSEXP_USE_SAFE_GC_MARK=1      // validate pointers before marking
```

gate on a `debug-chibi` cargo feature:
```rust
if cfg!(feature = "debug-chibi") {
    build.flag("-DSEXP_USE_HEADER_MAGIC=1");
    build.flag("-DSEXP_USE_SAFE_GC_MARK=1");
}
```

### runtime: expose heap limit

`ContextBuilder::heap_limit(usize)` → passes to `sexp_make_context`'s `max_size` parameter.
no compile-time flag needed.

### already correctly overridden

- `SEXP_USE_DL=0` — critical, eliminates dlopen attack surface
- `sexp_default_module_path = "/vfs/lib"` — VFS isolation

### solid defaults (keep as-is)

- `SEXP_USE_CONSERVATIVE_GC=0` — precise GC, correct
- `SEXP_USE_CHECK_STACK=1` + `SEXP_USE_GROW_STACK=1` — prevents stack overflow segfaults
- `SEXP_MAX_STACK_SIZE=1024000` — bounded, errors on deep recursion
- `SEXP_MAX_ANALYZE_DEPTH=8192` — prevents macro expansion stack exhaustion
- `SEXP_INITIAL_HEAP_SIZE=2MiB` — reasonable
- `SEXP_USE_GLOBAL_HEAP=0` + `SEXP_USE_GLOBAL_SYMBOLS=0` — per-context isolation

### document

`CHIBI_MODULE_PATH` env var is read by chibi's module resolver. our module policy gate blocks
non-VFS paths at the C level, so it can't break the sandbox. but a process-level env var
silently influencing a sandboxed interpreter should be documented.

---

## confirmed solid

### GC
- mark stack with `malloc` fallback — avoids stack overflow on deep structures
- sentinel free-list node — clean coalescing logic
- precise GC with type-driven slot enumeration — no false positives
- weak reference reset after mark phase — correct ephemeron logic
- two-pass DL finalisation — handles the common dependency case

### VM
- stack overflow protection with `sexp_ensure_stack` + `sexp_grow_stack`
- type checking on core opcodes (car/cdr/vector-ref/set etc.)
- integer overflow in add/sub uses wide `sexp_lsint_t` with proper bounds check
- division by zero caught on all three division opcodes
- tail call optimisation — correct frame reuse, no stack leak
- GC roots in `sexp_apply` — `self`/`tmp1`/`tmp2` properly preserved
- continuation capture/restore — correct stack snapshot and bounds check
- green thread context save/restore — all 5 VM registers synced

### reader/sexp
- dynamic buffer resizing with stack-allocated initial + malloc fallback
- fixnum-to-bignum transition in `sexp_read_number` — comprehensive check
- GC preservation in reader — `sexp_gc_var2(res, tmp)` consistently used
- `sexp_c_string` NULL safety
- exception propagation throughout reader
- `sexp_string_to_number_op` full-parse check (rejects trailing garbage)
- `#\x` character escape range validation (unlike string `\x` — see L7)

### evaluator/compiler
- `SEXP_MAX_ANALYZE_DEPTH` checked on every recursive entry
- macro hook recursion guard (`tein_macro_expand_hook_active`)
- syntactic closure hygiene — correct KFFD-style implementation
- `let-syntax`/`letrec-syntax` scope — correct ctx distinction
- immutability enforcement in `sexp_env_define`/`sexp_env_cell_define`
- cycle safety in `sexp_contains_syntax_p_bound` (Floyd's + depth cap)

### bignum
- GC rooting largely correct throughout (pervasive `sexp_gc_var`/`preserve`)
- division by zero caught at fast path and `fxrem`
- Karatsuba multiplication — correct decomposition and termination
- sign handling in mixed-sign arithmetic
- `sexp_bignum_normalize` MIN_FIXNUM corner case handled
- fixnum↔bignum transitions correct
- custom 128-bit arithmetic carefully avoids implicit overflow
