# c code review — tein codebase

date: 2026-02-25
branch: bugfix/mvp-code-review-2602
scope: all tein-owned c code (tein_shim.c, eval.c/sexp.c/vm.c patches) + rust↔c FFI boundary

three parallel review agents examined every tein-owned c file plus the rust↔c boundary.
deduplicated, verified synthesis below.

### resolution status

| id | severity | status | commit |
|----|----------|--------|--------|
| C1 | critical | **resolved** | `5cc4e69` |
| C2 | critical | **resolved** | `5cc4e69` |
| C3 | critical | **resolved** | `5cc4e69` |
| H1 | high | **resolved** | pending commit |
| M1 | medium | **resolved** | pending commit |
| M2 | medium | **resolved** | pending commit |
| M3 | medium | **resolved** | pending commit |
| M4 | medium | **resolved** | pending commit |
| L1 | low | **resolved** | pending commit |
| L2 | low | **resolved** | pending commit |
| L3 | low | **resolved** | pending commit |
| L4 | low | **resolved** | pending commit |
| L5 | low | **resolved** | pending commit |
| L6 | low | **resolved** | pending commit |

---

## critical — missing GC roots (conservative scan is OFF)

chibi is built with `SEXP_USE_CONSERVATIVE_GC=0`, so the GC **cannot see rust locals**.
every sexp held only in a rust variable must be explicitly rooted with
`sexp_preserve_object` / `GcRoot`. three spots violate this:

### C1. `evaluate_port` — unrooted port across allocation loop ✓

- **location**: `src/context.rs:1852`
- **status**: **resolved** in `5cc4e69` — added `GcRoot` for `raw_port` before the loop.
- **issue**: `raw_port` is a bare sexp used across multiple `sexp_read` + `sexp_evaluate`
  iterations. both allocate. no `GcRoot`. compare with `evaluate()` at line 1313 which
  correctly roots its port. real use-after-free waiting to happen under GC pressure.

### C2. `from_raw_depth` — unrooted pair in foreign object detection ✓

- **location**: `src/value.rs:262`
- **status**: **resolved** in `5cc4e69` — added `GcRoot` for `raw` before the
  `sexp_symbol_to_string` call.
- **issue**: `sexp_symbol_to_string(ctx, car)` allocates (calls `sexp_c_string` internally).
  after that allocation, `sexp_cdr(raw)` at line 267 reads from `raw` which is unrooted.
  small window but real.

### C3. reader dispatch table — stored procs not GC-preserved ✓

- **location**: `tein_shim.c:384`, `sexp.c:3516-3518`
- **status**: **resolved** in `5cc4e69` — set/unset/clear now take `ctx` and
  preserve/release handler procs. `dispatch_chars` uses `sexp_gc_preserve1` for
  its accumulator. FFI signatures and all rust call sites updated.
- **issue**: `tein_reader_dispatch[c] = proc` stores a raw sexp without
  `sexp_preserve_object`. contrast with `tein_macro_expand_hook_set` (line 426) which
  correctly preserves/releases. if GC collects the handler between registration and
  invocation, `sexp_apply1` in the sexp.c patch operates on freed memory.
- **companion issue**: `tein_reader_dispatch_chars` (`tein_shim.c:401-408`) builds a cons
  list in a loop with the accumulator in a plain C local — also unrooted across `sexp_cons`
  allocation points.

---

## high — macro hook exception safety

### H1. hook args not checked for exception before `sexp_apply` ✓

- **location**: `eval.c:805-809`
- **status**: **resolved** — added `sexp_exceptionp(hook_args)` guard before `sexp_apply`;
  on OOM the exception is propagated as `res` and the hook is skipped cleanly.
- **issue**: the four `sexp_cons` calls building `hook_args` can each return an exception
  sexp on OOM. the code fed the result directly to `sexp_apply` without checking. on OOM,
  this constructed a stack frame from a malformed args list.
- **fix applied** (`eval.c:805-814`):
  ```c
  /* guard: any sexp_cons can return an exception on OOM; applying a
     malformed args list would corrupt the call frame, so skip the
     hook and propagate the OOM exception instead */
  if (!sexp_exceptionp(hook_args))
    res = sexp_apply(ctx, tein_macro_expand_hook, hook_args);
  else
    res = hook_args;
  ```

---

## medium — defence in depth gaps

### M1. module policy: no path traversal protection ✓

- **location**: `tein_shim.c:207`
- **status**: **resolved** — added `strstr(path, "..") != NULL` rejection.
- **issue**: `strncmp(path, "/vfs/lib/", 9)` passed any path starting with that prefix,
  including `/vfs/lib/../../etc/passwd`. in practice `/vfs/` doesn't exist on disk, so
  exploitation required an attacker to create that directory (or a symlink). trivial hardening.
- **fix applied** (`tein_shim.c:205-209`):
  ```c
  if (strncmp(path, "/vfs/lib/", 9) != 0) return 0;
  if (strstr(path, "..") != NULL) return 0;  /* no path traversal */
  return 1;
  ```

### M2. port trampoline: no buffer size validation ✓

- **location**: `src/context.rs:312`
- **status**: **resolved** — added `buf_len = sexp_string_size(buf_sexp)` check; both
  read and write trampolines now reject `end > buf_len`.
- **issue**: the read/write trampolines validated `start >= 0` and `end >= start` but never
  checked `end <= buffer_size`. a sandboxed scheme program calling `tein-port-read` directly
  with crafted args could trigger an OOB write via `copy_nonoverlapping`.

### M3. `tein_sexp_make_bytes` / `tein_sexp_make_vector` — fixnum overflow ✓

- **location**: `tein_shim.c:41, 96`
- **status**: **resolved** — added explanatory comments documenting the constraint and why
  it's safe in practice: rust `Vec` sizes are bounded by `isize::MAX`, which is below
  `SEXP_MAX_FIXNUM` on all supported platforms.
- **issue**: `sexp_make_fixnum(len)` casts `sexp_uint_t` (unsigned) to signed. extremely
  large `len` values wrap to negative fixnums. chibi would likely reject negative sizes,
  but the semantics were wrong.

### M4. `env_copy_named` — `sym`/`val` not GC-rooted ✓

- **location**: `tein_shim.c:274`
- **status**: **resolved** — added `sexp_gc_var2(sym, val)` / `sexp_gc_preserve2` at top,
  restructured early returns to a `goto done` so `sexp_gc_release2` is always called.
- **issue**: `sexp_intern` and `sexp_env_define` are allocation points. `sym` is likely safe
  (interned symbols are reachable from the global table), but `val` extracted from rename
  cells was held only in a C local.

---

## low / notes

### L1. `_tein_dispatch` in sexp.c patch — unrooted local ✓

- **location**: `sexp.c:3516`
- **status**: **resolved** — replaced `sexp _tein_dispatch` local with `tmp`, the
  already-rooted gc var from the enclosing `sexp_read_raw` scope.
- **issue**: was safe with no allocation between read and apply, but fragile to future edits.

### L2. `tein_make_error` dead `len` parameter ✓

- **location**: `tein_shim.c:190`
- **status**: **resolved** — added `@param` doc explaining `len` is reserved, retained for
  potential future use, and that `sexp_user_exception` reads `msg` as a C string.
- **issue**: parameter existed but was explicitly `(void)len`'d with no explanation.

### L3. `tein_vfs_lookup` no NULL guard on `out_length` ✓

- **location**: `tein_shim.c:247`
- **status**: **resolved** — added `if (out_length)` guard before the dereference.

### L4. `SEXP_PROC_VARIADIC` signedness divergence ✓

- **location**: `ffi.rs:431`
- **status**: **resolved** — added doc comment explaining C defines as `sexp_uint_t`,
  rust uses `c_int`, and why value 1 is safe for both.

### L5. sexp_proc2 cast comment wording ✓

- **location**: `tein_shim.c:70-73`
- **status**: **resolved** — clarified that `tein_sexp_define_foreign` accepts `sexp_proc1`
  directly (no cast needed); the fn-pointer shim is only in `tein_sexp_define_foreign_proc`.

### L6. `tein_reader_dispatch_unset` allows unsetting reserved chars ✓

- **location**: `tein_shim.c:388`
- **status**: **resolved** — added `tein_reader_char_reserved(c)` guard to match `set`,
  returning `-1` for reserved chars (harmless no-op before, now consistent).
- **issue**: asymmetry with `set` which guarded against reserved chars.

---

## confirmed solid

- VFS patches (A, B, C) — clean integration, correct GC rooting, well-commented
- fuel budget system — elegant thread-local design, correct in both green-thread and
  standalone paths
- macro hook recursion guard — simple, effective, `tein_macro_expand_hook_active` flag works
- `GcRoot` RAII pattern in rust — consistently used in most paths
- `env_copy_named` walk limit (65536) — good defence in depth
- `is_proper_list` cycle detection — proper tortoise-and-hare
- macro hook GC handling — correct `sexp_preserve_object` / `sexp_release_object` pairing
- loop-count cap in `analyze()` — well-targeted fix for hook-induced infinite re-analysis
- reserved-character list for reader dispatch — comprehensive, correct
