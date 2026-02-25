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
| H1 | high | open | |
| M1 | medium | open | |
| M2 | medium | open | |
| M3 | medium | open | |
| M4 | medium | open | |
| L1 | low | open | |
| L2 | low | open | |
| L3 | low | open | |
| L4 | low | open | |
| L5 | low | open | |
| L6 | low | open | |

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

### H1. hook args not checked for exception before `sexp_apply`

- **location**: `eval.c:805-809`
- **issue**: the four `sexp_cons` calls building `hook_args` can each return an exception
  sexp on OOM. the code feeds the result directly to `sexp_apply` without checking. on OOM,
  this constructs a stack frame from a malformed args list.
- **fix**: check `sexp_exceptionp(hook_args)` before calling apply:
  ```c
  hook_args = sexp_cons(ctx, name, hook_args);
  if (!sexp_exceptionp(hook_args))
    res = sexp_apply(ctx, tein_macro_expand_hook, hook_args);
  tein_macro_expand_hook_active = 0;
  ```

---

## medium — defence in depth gaps

### M1. module policy: no path traversal protection

- **location**: `tein_shim.c:207`
- **issue**: `strncmp(path, "/vfs/lib/", 9)` passes any path starting with that prefix,
  including `/vfs/lib/../../etc/passwd`. in practice `/vfs/` doesn't exist on disk, so
  exploitation requires an attacker to create that directory (or a symlink). but adding
  `strstr(path, "..") != NULL → reject` is trivial hardening.
- **fix**:
  ```c
  int tein_module_allowed(const char *path) {
      if (tein_module_policy == 0) return 1;
      if (strncmp(path, "/vfs/lib/", 9) != 0) return 0;
      if (strstr(path, "..") != NULL) return 0;
      return 1;
  }
  ```

### M2. port trampoline: no buffer size validation

- **location**: `src/context.rs:312`
- **issue**: the read/write trampolines validate `start >= 0` and `end >= start` but never
  check `end <= buffer_size`. normally chibi manages indices correctly, but a sandboxed
  scheme program that can call `tein-port-read` directly with crafted args could trigger an
  OOB write via `copy_nonoverlapping`.
- **fix**: add `let buf_len = ffi::sexp_string_size(buf_sexp) as usize;` and check
  `end <= buf_len` before proceeding.

### M3. `tein_sexp_make_bytes` / `tein_sexp_make_vector` — fixnum overflow

- **location**: `tein_shim.c:41, 96`
- **issue**: `sexp_make_fixnum(len)` casts `sexp_uint_t` (unsigned) to signed. extremely
  large `len` values wrap to negative fixnums. chibi would likely reject negative sizes,
  but the semantics are wrong.
- **fix**: document or enforce maximum safe sizes on the rust side. consider using a signed
  type in the C interface.

### M4. `env_copy_named` — `sym`/`val` not GC-rooted

- **location**: `tein_shim.c:274`
- **issue**: `sexp_intern` and `sexp_env_define` are allocation points. `sym` is likely safe
  (interned symbols are reachable from the global table), but `val` extracted from rename
  cells is held only in a C local.
- **fix**: add `sexp_gc_var2(sym, val); sexp_gc_preserve2(ctx, sym, val);` for defence in
  depth.

---

## low / notes

### L1. `_tein_dispatch` in sexp.c patch — unrooted local

- **location**: `sexp.c:3516`
- **issue**: currently safe because no allocation between read and apply, but fragile to
  future edits.
- **fix**: use the already-rooted `tmp` variable from the enclosing `sexp_read_raw` scope.

### L2. `tein_make_error` dead `len` parameter

- **location**: `tein_shim.c:190`
- **issue**: parameter exists but is explicitly `(void)len`'d.
- **fix**: consider removing from both C and rust signatures, or keep with a clear `@param`
  note explaining it's reserved.

### L3. `tein_vfs_lookup` no NULL guard on `out_length`

- **location**: `tein_shim.c:247`
- **issue**: all callers pass valid locals, but defence in depth warrants
  `if (out_length) *out_length = ...;`

### L4. `SEXP_PROC_VARIADIC` signedness divergence

- **location**: `ffi.rs:431`
- **issue**: C defines as `sexp_uint_t`, rust as `c_int`. value 1 is fine, but type
  divergence is worth a comment.

### L5. sexp_proc2 cast comment wording

- **location**: `tein_shim.c:70-73`
- **issue**: says "single intentional shim" but `tein_sexp_define_foreign` relies on the
  same ABI assumption. minor wording fix needed.

### L6. `tein_reader_dispatch_unset` allows unsetting reserved chars

- **location**: `tein_shim.c:388`
- **issue**: asymmetry with `set` which guards against reserved chars. harmless no-op but
  inconsistent.
- **fix**: add the same reserved-character guard, or document that unsetting reserved chars
  is intentionally allowed.

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
