# VFS Shadow: (scheme file) + (scheme repl) + (scheme show) Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Expand `(tein file)` to the full `(scheme file)` surface, introduce a data-driven VFS shadow system so sandboxed contexts resolve shadowed modules through policy-checked or neutered replacements, and enable `(scheme show)` / `(srfi 166)` in `Modules::Safe`.

**Architecture:** `VfsSource::Shadow { sld }` — a new registry variant declaring inline `.sld` content injected into the dynamic VFS at sandbox build time. shadows are declared in `VFS_REGISTRY` alongside all other modules (single source of truth). `register_vfs_shadows()` loops over `Shadow` entries and calls `tein_vfs_register()`. two shadows ship in this PR:

- `scheme/file` → re-exports all 10 names from `(tein file)` (policy-checked trampolines)
- `scheme/repl` → provides neutered `interaction-environment` via `(current-environment)` from `(chibi)`

The old IO wrapper system (`check_and_delegate`, `wrapper_open_*`, `wrapper_fn_for`, `has_io` block) is removed — policy enforcement unified in `(tein file)` trampolines.

**Design doc:** `docs/plans/2026-03-01-vfs-shadow-scheme-file-design.md` (partially outdated — this plan supersedes it)

**Base branch:** `dev`
**Branch to create:** `just feature vfs-shadow-scheme-file-2603`

**Closes:** #91, #92 (partially — this PR vets and enables the srfi/166 tree)

---

## Architecture notes (read before implementing)

### build() flow — two paths

The sandboxed context build path in `build()` is inside `if let Some(ref modules) = self.sandbox_modules.take()`. This block has access to `source_env` (captured just before sandbox restriction). After the sandbox block, `build()` creates the `Context` struct and then calls `register_file_module` etc. on it.

**Critical:** The 4 `open-*` originals must be captured **inside the sandbox block** (where `source_env` is live), not in `register_file_module` which runs after the env has been restricted.

### IO wrapper system — current vs target

**Current (to remove):**
- `check_and_delegate` — shared impl for 4 wrappers, checks `FS_POLICY` directly (denies when `None`)
- `wrapper_open_input_file` + 3 others — registered directly into `null_env` in `has_io` block
- `wrapper_fn_for` — dispatch table
- `has_io` block — captures originals from `source_env`, registers wrappers into `null_env`, sets `FS_POLICY`

**Target (new):**
- `open_file_trampoline(ctx, args, op)` — shared impl using `check_fs_access` (checks `IS_SANDBOXED` first — unsandboxed = allow all, sandboxed = check policy)
- `open_input_file_trampoline` + 3 others — registered via `register_file_module`
- Original capture: `capture_file_originals()` called in `build()` sandbox block (from `source_env`) AND unsandboxed path (from context env)
- `FS_POLICY` set outside the sandbox block (works for both paths)

**Key semantic change:** The old wrappers hardcode `FS_POLICY.is_none() → deny`. The new trampolines use `check_fs_access` which checks `IS_SANDBOXED` first — unsandboxed contexts pass through unconditionally. This is correct: the trampolines are always registered (via `register_file_module`), and the `IS_SANDBOXED` guard makes them transparent when not sandboxed.

### VFS shadow system — data-driven via registry

Shadows are declared as `VfsSource::Shadow { sld }` entries in `VFS_REGISTRY`. The `.sld` content is a `&'static str` inline in the registry entry. `register_vfs_shadows()` iterates the registry, finds all `Shadow` entries, and calls `tein_vfs_register()` for each.

**Timing:** Must be called before `VFS_GATE` is armed (before `GATE_CHECK`). Call site is in the sandbox block, after `IS_SANDBOXED` is set.

**Unsandboxed:** No shadows registered — modules resolve to chibi's native versions.

### ORIGINAL_PROCS capture — both paths

Unsandboxed contexts also need originals captured for the `open-*-file` trampolines (which delegate to chibi's original procs unconditionally when `IS_SANDBOXED` is false).

Helper `capture_file_originals(ctx, env)` captures from the given env into `ORIGINAL_PROCS`. Called:
1. In the sandbox block before env restriction (captures from `source_env`)
2. In the unsandboxed path (captures from the default context env)

### No `tein/file` → `scheme/file` reverse dep

`scheme/file` shadow declares `deps: &["tein/file"]` — transitive resolution pulls `tein/file` when `scheme/file` is allowed. No reverse dep needed. Both are `default_safe: true` so both appear in `Modules::Safe` independently.

---

## Task 1: Create feature branch + GH issue for scheme/eval

**Step 1: Create branch**
```bash
cd /home/fey/projects/tein
just feature vfs-shadow-scheme-file-2603
```

**Step 2: Create GH issue for deferred scheme/eval + full sandboxed REPL**

```bash
gh issue create \
  --title "feat: sandboxed (scheme eval) + (scheme repl) — full r7rs eval in sandbox" \
  --body "$(cat <<'EOF'
## context

the VFS shadow PR (#91 follow-up) introduces a neutered `(scheme repl)` shadow that provides `interaction-environment` via `(current-environment)` — enough for `srfi/166` but not a full REPL.

this issue tracks expanding the sandbox to support full `(scheme eval)` and `(scheme repl)`:

### scheme/eval

exports: `eval`, `environment`

- `eval` — evaluate an expression in a given environment. in sandbox, the default env (via `(current-environment)`) is already the restricted sandbox env, so basic `eval` is safe. can likely be implemented as pure scheme delegating to chibi's native `eval` from `(chibi)`.
- `environment` — `(environment '(scheme base) '(scheme write))` creates a fresh env from library names. in sandbox, this should only create envs from allowlisted modules. this is the hard part — needs runtime access to the VFS allowlist.

### scheme/repl

exports: `interaction-environment`

currently returns `(current-environment)`. for a full REPL, `interaction-environment` should return a mutable env that accumulates definitions — this may require a dedicated trampoline or env management.

### design insight

`(current-environment)` from `(chibi)` is always available (primitive core, never gated) and returns the caller's env. in sandbox, this is the restricted env — making basic `eval` surprisingly simple. the complexity is in `environment` (needs allowlist enforcement) and a proper mutable interaction env.
EOF
)"
```

---

## Task 2: Add `VfsSource::Shadow` variant to the registry

**Files:**
- Modify: `tein/src/vfs_registry.rs`

**Step 1: Update `VfsSource` enum**

Add `Shadow` variant:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
enum VfsSource {
    /// .sld/.scm files embedded at build time
    Embedded,
    /// registered at runtime via #[tein_module] — no files to embed
    Dynamic,
    /// shadow module — .sld injected into dynamic VFS at sandbox build time only.
    /// unsandboxed contexts use chibi's native version.
    Shadow,
}
```

Note: the `.sld` content lives in a new field on `VfsEntry` (see step 2), not on the enum variant, because `VfsSource` derives `Copy` and `&'static str` is `Copy` but putting it on the enum makes the non-Shadow variants carry dead weight. Instead:

**Step 2: Add `shadow_sld` field to `VfsEntry`**

```rust
struct VfsEntry {
    path: &'static str,
    deps: &'static [&'static str],
    files: &'static [&'static str],
    clib: Option<ClibEntry>,
    default_safe: bool,
    source: VfsSource,
    feature: Option<&'static str>,
    /// shadow .sld content — only used when source is `Shadow`.
    /// injected into dynamic VFS by `register_vfs_shadows()` in sandboxed contexts.
    shadow_sld: Option<&'static str>,
}
```

**Step 3: Add `shadow_sld: None` to all existing entries**

Every existing `VfsEntry` gets `shadow_sld: None`. (use `replace_all` or add to each entry.)

**Step 4: Add `scheme/file` and `scheme/repl` shadow entries to the registry**

Place after the existing `scheme/write` entry (in the r7rs section), before `scheme/time`:

```rust
// scheme/file: VFS shadow — sandboxed contexts resolve to (tein file) trampolines.
// unsandboxed contexts use chibi's native scheme/file directly.
VfsEntry {
    path: "scheme/file",
    deps: &["tein/file"],
    files: &[],
    clib: None,
    default_safe: true,
    source: VfsSource::Shadow,
    feature: None,
    shadow_sld: Some("\
(define-library (scheme file)
  (import (tein file))
  (export open-input-file open-output-file
          open-binary-input-file open-binary-output-file
          call-with-input-file call-with-output-file
          with-input-from-file with-output-to-file
          file-exists? delete-file))
"),
},
// scheme/repl: VFS shadow — sandboxed contexts get neutered interaction-environment.
// returns (current-environment) = the sandbox's restricted env.
// full sandboxed eval/repl tracked in GH issue.
VfsEntry {
    path: "scheme/repl",
    deps: &[],
    files: &[],
    clib: None,
    default_safe: true,
    source: VfsSource::Shadow,
    feature: None,
    shadow_sld: Some("\
(define-library (scheme repl)
  (import (chibi))
  (export interaction-environment)
  (begin
    (define (interaction-environment) (current-environment))))
"),
},
```

**Step 5: Add `scheme/file` to `srfi/166/columnar` deps**

Find the `srfi/166/columnar` entry. Add `"scheme/file"` to its deps (so transitive resolution pulls the shadow when columnar is allowed):

```rust
deps: &[
    "scheme/base",
    "scheme/char",
    "scheme/file",   // shadow — resolves via (tein file) in sandbox
    "srfi/1",
    "srfi/117",
    "srfi/130",
    "srfi/166/base",
    "chibi/optional",
],
```

**Step 6: Add `scheme/repl` to `srfi/166/base` deps (already there — verify)**

Check that `srfi/166/base` already has `"scheme/repl"` in its deps. It should (line ~1117). If not, add it.

**Step 7: Compile check**
```bash
cargo build 2>&1 | tail -20
```

**Step 8: Run existing tests**
```bash
just test 2>&1 | tail -30
```

All existing tests should pass — we only added data, no behaviour change yet.

**Step 9: Commit**
```bash
git add tein/src/vfs_registry.rs
git commit -m "feat: add VfsSource::Shadow variant + scheme/file and scheme/repl shadow entries"
```

---

## Task 3: Add `register_vfs_shadows()` + call it in build()

**Files:**
- Modify: `tein/src/context.rs`

**Step 1: Write failing test**

Add to the sandboxed tests section in context.rs:

```rust
#[test]
fn test_scheme_repl_shadow_importable_in_sandbox() {
    use crate::sandbox::Modules;
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::Safe)
        .build()
        .expect("builder");
    // (scheme repl) in sandbox should resolve to our shadow
    let r = ctx.evaluate(
        "(import (scheme repl)) (procedure? interaction-environment)"
    );
    assert_eq!(r.expect("scheme repl shadow works"), Value::Bool(true));
}
```

**Step 2: Run to confirm failure**
```bash
cargo test test_scheme_repl_shadow_importable_in_sandbox 2>&1 | tail -20
```
Expected: FAIL — `(scheme repl)` blocked by VFS gate.

**Step 3: Add `register_vfs_shadows()` to context.rs**

Add as a free fn near `register_file_module`:

```rust
/// Inject VFS shadow modules for sandboxed contexts.
///
/// Iterates `VFS_REGISTRY` for `VfsSource::Shadow` entries and registers
/// their `.sld` content into the dynamic VFS under canonical `/vfs/lib/`
/// paths. Chibi's module resolver then finds our replacements instead of
/// native implementations.
///
/// Must be called before the VFS gate is armed (before `VFS_GATE` is set
/// to `GATE_CHECK`).
fn register_vfs_shadows() {
    for entry in VFS_REGISTRY.iter() {
        if entry.source != VfsSource::Shadow {
            continue;
        }
        let sld = entry
            .shadow_sld
            .expect("Shadow entry must have shadow_sld");
        let vfs_path = format!("/vfs/lib/{}.sld", entry.path);
        let c_path = CString::new(vfs_path).expect("valid VFS path");
        unsafe {
            ffi::tein_vfs_register(
                c_path.as_ptr(),
                sld.as_ptr() as *const std::ffi::c_char,
                sld.len() as std::ffi::c_uint,
            );
        }
    }
}
```

**Step 4: Call `register_vfs_shadows()` in `build()` sandbox path**

Inside the `if let Some(ref modules) = self.sandbox_modules.take()` block, find where `IS_SANDBOXED` is set to `true`:

```rust
IS_SANDBOXED.with(|c| c.set(true));
register_vfs_shadows(); // inject shadow modules before gate is armed
```

Add immediately after `IS_SANDBOXED.with(...)`, before `VFS_GATE.with(...)`.

**Step 5: Run test**
```bash
cargo test test_scheme_repl_shadow_importable_in_sandbox 2>&1 | tail -20
```
Expected: PASS.

**Step 6: Compile check + full test suite**
```bash
cargo build 2>&1 | tail -20
just test 2>&1 | tail -30
```

**Step 7: Commit**
```bash
git add tein/src/context.rs
git commit -m "feat: register_vfs_shadows() — data-driven shadow injection from VFS_REGISTRY"
```

---

## Task 4: Add 4 open-* trampolines + capture_file_originals

**Files:**
- Modify: `tein/src/context.rs`

**Step 1: Write failing tests**

Add inside the `#[cfg(test)]` IO policy test section (find `IO_TEST_LOCK`):

```rust
#[test]
fn test_open_input_file_trampoline_allowed() {
    let _lock = IO_TEST_LOCK.lock().unwrap();
    let dir = io_test_dir("open_input_allowed");
    let file = dir.join("data.txt");
    std::fs::write(&file, "hello").expect("write");
    let canon_dir = dir.canonicalize().unwrap();
    let path = file.to_str().unwrap().to_string();
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(crate::sandbox::Modules::Safe)
        .file_read(&[canon_dir.to_str().unwrap()])
        .build()
        .expect("builder");
    let code = format!(
        "(import (tein file)) (let ((p (open-input-file \"{path}\"))) (close-input-port p) #t)"
    );
    let r = ctx.evaluate(&code).expect("open-input-file allowed");
    assert_eq!(r, Value::Bool(true));
}

#[test]
fn test_open_input_file_trampoline_denied() {
    let _lock = IO_TEST_LOCK.lock().unwrap();
    let dir = io_test_dir("open_input_denied");
    let file = dir.join("secret.txt");
    std::fs::write(&file, "no").expect("write");
    let path = file.to_str().unwrap().to_string();
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(crate::sandbox::Modules::Safe)
        .file_read(&["/tmp/__nonexistent_prefix__/"])
        .build()
        .expect("builder");
    let code = format!("(import (tein file)) (open-input-file \"{path}\")");
    assert!(ctx.evaluate(&code).is_err(), "should be denied");
}

#[test]
fn test_open_output_file_trampoline_allowed() {
    let _lock = IO_TEST_LOCK.lock().unwrap();
    let dir = io_test_dir("open_output_allowed");
    let file = dir.join("out.txt");
    let canon_dir = dir.canonicalize().unwrap();
    let path = file.to_str().unwrap().to_string();
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(crate::sandbox::Modules::Safe)
        .file_write(&[canon_dir.to_str().unwrap()])
        .build()
        .expect("builder");
    let code = format!(
        "(import (tein file)) (let ((p (open-output-file \"{path}\"))) (close-output-port p) #t)"
    );
    let r = ctx.evaluate(&code).expect("open-output-file allowed");
    assert_eq!(r, Value::Bool(true));
}

#[test]
fn test_open_output_file_trampoline_denied() {
    let _lock = IO_TEST_LOCK.lock().unwrap();
    let dir = io_test_dir("open_output_denied");
    let path = dir.join("nope.txt").to_str().unwrap().to_string();
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(crate::sandbox::Modules::Safe)
        .file_write(&["/tmp/__nonexistent_prefix__/"])
        .build()
        .expect("builder");
    let code = format!("(import (tein file)) (open-output-file \"{path}\")");
    assert!(ctx.evaluate(&code).is_err(), "should be denied");
}

#[test]
fn test_open_input_file_unsandboxed_passthrough() {
    // unsandboxed: open-input-file trampoline delegates to chibi original unconditionally
    let tmp = "/tmp/tein_open_unsandboxed_test.txt";
    std::fs::write(tmp, "test").expect("write");
    let ctx = Context::builder()
        .standard_env()
        .build()
        .expect("builder");
    let r = ctx.evaluate(&format!(
        "(import (tein file)) (let ((p (open-input-file \"{tmp}\"))) (close-input-port p) #t)"
    ));
    assert_eq!(r.expect("unsandboxed passthrough"), Value::Bool(true));
}
```

**Step 2: Run to confirm failure**
```bash
cargo test test_open_input_file_trampoline_allowed test_open_input_file_trampoline_denied test_open_output_file_trampoline_allowed test_open_output_file_trampoline_denied test_open_input_file_unsandboxed_passthrough 2>&1 | tail -20
```
Expected: FAIL — `(tein file)` doesn't export `open-input-file` yet.

**Step 3: Add `capture_file_originals` + 4 trampolines + shared impl**

Find the `// --- (tein file) trampolines ---` comment block (around line 1187). Add below `delete_file_trampoline`:

```rust
// --- open-*-file trampolines ---

/// Capture chibi's native `open-*-file` primitives from `env` into `ORIGINAL_PROCS`.
///
/// Must be called before env restriction (sandbox) or on the full env (unsandboxed).
/// Safe to call multiple times — later calls overwrite earlier ones.
///
/// # Safety
/// `ctx` and `env` must be valid chibi context and env pointers.
unsafe fn capture_file_originals(ctx: ffi::sexp, env: ffi::sexp) {
    unsafe {
        let undefined = ffi::get_void();
        for op in IoOp::ALL {
            let name = op.name();
            let c_name = CString::new(name).unwrap();
            let sym =
                ffi::sexp_intern(ctx, c_name.as_ptr(), name.len() as ffi::sexp_sint_t);
            let val = ffi::sexp_env_ref(ctx, env, sym, undefined);
            if val != undefined {
                ORIGINAL_PROCS.with(|procs| procs[op as usize].set(val));
            }
        }
    }
}

/// Shared implementation for all 4 `open-*-file` trampolines.
///
/// Checks `IS_SANDBOXED` + `FsPolicy` via `check_fs_access`, then delegates
/// to the captured original chibi primitive. Unsandboxed contexts delegate
/// unconditionally.
///
/// # Safety
/// `ctx` and `args` must be valid sexp values.
unsafe fn open_file_trampoline(ctx: ffi::sexp, args: ffi::sexp, op: IoOp) -> ffi::sexp {
    unsafe {
        let path = match extract_string_arg(ctx, args, op.name()) {
            Ok(s) => s,
            Err(e) => return e,
        };

        let access = if op.is_read() {
            FsAccess::Read
        } else {
            FsAccess::Write
        };
        if !check_fs_access(path, access) {
            let dir = if op.is_read() { "read" } else { "write" };
            let msg = format!("[sandbox:file] {} ({dir} not permitted)", path);
            let c_msg = CString::new(msg.as_str()).unwrap_or_default();
            return ffi::make_error(ctx, c_msg.as_ptr(), msg.len() as ffi::sexp_sint_t);
        }

        let original = ORIGINAL_PROCS.with(|procs| procs[op as usize].get());
        ffi::sexp_apply_proc(ctx, original, args)
    }
}

/// `open-input-file` trampoline: policy-checked textual input port opener.
unsafe extern "C" fn open_input_file_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe { open_file_trampoline(ctx, args, IoOp::InputFile) }
}

/// `open-binary-input-file` trampoline: policy-checked binary input port opener.
unsafe extern "C" fn open_binary_input_file_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe { open_file_trampoline(ctx, args, IoOp::BinaryInputFile) }
}

/// `open-output-file` trampoline: policy-checked textual output port opener.
unsafe extern "C" fn open_output_file_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe { open_file_trampoline(ctx, args, IoOp::OutputFile) }
}

/// `open-binary-output-file` trampoline: policy-checked binary output port opener.
unsafe extern "C" fn open_binary_output_file_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe { open_file_trampoline(ctx, args, IoOp::BinaryOutputFile) }
}
```

**Step 4: Update `register_file_module`**

Find `fn register_file_module`. Update docstring and add 4 new registrations:

```rust
/// Register all 6 `(tein file)` trampolines.
///
/// Called during `build()` after context creation. Originals are captured
/// separately via `capture_file_originals()` before env restriction.
fn register_file_module(&self) -> Result<()> {
    self.define_fn_variadic("file-exists?", file_exists_trampoline)?;
    self.define_fn_variadic("delete-file", delete_file_trampoline)?;
    self.define_fn_variadic("open-input-file", open_input_file_trampoline)?;
    self.define_fn_variadic("open-binary-input-file", open_binary_input_file_trampoline)?;
    self.define_fn_variadic("open-output-file", open_output_file_trampoline)?;
    self.define_fn_variadic("open-binary-output-file", open_binary_output_file_trampoline)?;
    Ok(())
}
```

**Step 5: Capture originals in both build paths**

In `build()`, inside the sandbox block, right after `let source_env = ffi::sexp_context_env(ctx)`, add:

```rust
// capture open-*-file originals before env restriction
capture_file_originals(ctx, source_env);
```

For the unsandboxed path: find `if self.standard_env { context.register_file_module()?; ... }`. Add capture before registration using the raw ctx (which is still accessible):

```rust
if self.standard_env {
    // capture open-*-file originals from full standard env (unsandboxed)
    unsafe { capture_file_originals(ctx, ffi::sexp_context_env(ctx)); }
    context.register_file_module()?;
    context.register_load_module()?;
    context.register_process_module()?;
}
```

**Step 6: Compile check**
```bash
cargo build 2>&1 | tail -20
```
Should compile clean. Tests will still fail until `file.sld` exports are updated (task 6).

**Step 7: Commit**
```bash
git add tein/src/context.rs
git commit -m "feat: add open-*-file trampolines + capture_file_originals for (tein file)"
```

---

## Task 5: Remove the old IO wrapper system from context.rs

**Files:**
- Modify: `tein/src/context.rs`

**Step 1: Delete dead code**

Remove these items (search by name):
- `unsafe fn check_and_delegate(...)` (~40 lines)
- `unsafe extern "C" fn wrapper_open_input_file(...)` (~10 lines)
- `unsafe extern "C" fn wrapper_open_binary_input_file(...)` (~10 lines)
- `unsafe extern "C" fn wrapper_open_output_file(...)` (~10 lines)
- `unsafe extern "C" fn wrapper_open_binary_output_file(...)` (~10 lines)
- `fn wrapper_fn_for(...)` (~10 lines)

**Step 2: Remove the `has_io` block, relocate FsPolicy setup**

Find the block starting `// IO wrappers: capture original procs from source env, register wrappers`. This block:
1. Takes `file_read_prefixes` / `file_write_prefixes` from `self`
2. Captures originals into `ORIGINAL_PROCS` (now done by `capture_file_originals` earlier)
3. Registers wrapper fns into `null_env` (now done by `register_file_module`)
4. Sets `FS_POLICY`

**Remove** the entire `has_io` block (`let file_read_prefixes = ...; let file_write_prefixes = ...; let has_io = ...; if has_io { ... }`).

**Then add** FsPolicy setup **after** the sandbox block closes (right before `let context = Context { ... }`), unconditional on `has_io`:

```rust
// set FsPolicy if file_read() or file_write() was configured.
// placed outside the sandbox block so it works for both sandboxed and
// unsandboxed contexts with file policy configured.
{
    let file_read_prefixes = self.file_read_prefixes.take();
    let file_write_prefixes = self.file_write_prefixes.take();
    if file_read_prefixes.is_some() || file_write_prefixes.is_some() {
        FS_POLICY.with(|cell| {
            *cell.borrow_mut() = Some(FsPolicy {
                read_prefixes: file_read_prefixes.unwrap_or_default(),
                write_prefixes: file_write_prefixes.unwrap_or_default(),
            });
        });
    }
}
```

**Step 3: Keep** `IoOp` enum, `IoOp::ALL`, `IoOp::name()`, `IoOp::is_read()`, `ORIGINAL_PROCS` — still used by new trampolines.

**Step 4: Compile check**
```bash
cargo build 2>&1 | tail -20
```

**Step 5: Run full test suite**
```bash
just test 2>&1 | tail -30
```
The IO policy tests (`test_file_read_allowed_path`, `test_file_write_allowed_path`, etc.) must still pass.

**Step 6: Lint**
```bash
just lint
```

**Step 7: Commit**
```bash
git add tein/src/context.rs
git commit -m "refactor: remove old IO wrapper system, policy enforcement via (tein file) trampolines"
```

---

## Task 6: Expand (tein file) scheme files in the chibi fork

**IMPORTANT:** Changes go in `target/chibi-scheme/` then **must be pushed** to `emesal/chibi-scheme` branch `emesal-tein` before `cargo build` (which hard-resets from remote).

**Files:**
- Modify: `target/chibi-scheme/lib/tein/file.sld`
- Modify: `target/chibi-scheme/lib/tein/file.scm`

**Step 1: Write failing scheme test**

Create `tein/tests/scheme/tein_file_open.scm`:
```scheme
(import (tein test) (tein file) (scheme base))

(test-group "tein file higher-order wrappers"
  (test "call-with-input-file is procedure"
        #t (procedure? call-with-input-file))
  (test "call-with-output-file is procedure"
        #t (procedure? call-with-output-file))
  (test "with-input-from-file is procedure"
        #t (procedure? with-input-from-file))
  (test "with-output-to-file is procedure"
        #t (procedure? with-output-to-file)))
```

**Step 2: Run to confirm failure**
```bash
cargo test tein_file_open 2>&1 | tail -20
```
Expected: FAIL — `call-with-input-file` not exported.

**Step 3: Update `file.sld`**

Replace `target/chibi-scheme/lib/tein/file.sld`:
```scheme
(define-library (tein file)
  (import (scheme base))
  (export file-exists? delete-file
          open-input-file open-binary-input-file
          open-output-file open-binary-output-file
          call-with-input-file call-with-output-file
          with-input-from-file with-output-to-file)
  (include "file.scm"))
```

**Step 4: Update `file.scm`**

Replace `target/chibi-scheme/lib/tein/file.scm`:
```scheme
;;; (tein file) — safe file IO with FsPolicy enforcement
;;;
;;; file-exists?, delete-file, open-input-file, open-binary-input-file,
;;; open-output-file, open-binary-output-file are rust trampolines registered
;;; by register_file_module() in context.rs. policy:
;;;   - unsandboxed: allow all (delegate to chibi original)
;;;   - sandboxed + policy: check prefix, then delegate
;;;   - sandboxed + no policy: deny (sandbox violation)
;;;
;;; the 4 higher-order wrappers below delegate to the above primitives —
;;; policy enforcement happens at open-* (single point of check).

(define (call-with-input-file filename proc)
  (let ((port (open-input-file filename)))
    (dynamic-wind
      (lambda () #f)
      (lambda () (proc port))
      (lambda () (close-input-port port)))))

(define (call-with-output-file filename proc)
  (let ((port (open-output-file filename)))
    (dynamic-wind
      (lambda () #f)
      (lambda () (proc port))
      (lambda () (close-output-port port)))))

(define (with-input-from-file filename thunk)
  (let ((port (open-input-file filename)))
    (dynamic-wind
      (lambda () #f)
      (lambda ()
        (parameterize ((current-input-port port))
          (thunk)))
      (lambda () (close-input-port port)))))

(define (with-output-to-file filename thunk)
  (let ((port (open-output-file filename)))
    (dynamic-wind
      (lambda () #f)
      (lambda ()
        (parameterize ((current-output-port port))
          (thunk)))
      (lambda () (close-output-port port)))))
```

**Step 5: Push chibi fork changes**
```bash
cd /home/fey/projects/tein/target/chibi-scheme
git add lib/tein/file.sld lib/tein/file.scm
git commit -m "feat: expand (tein file) to full (scheme file) surface — 10 exports"
git push
cd /home/fey/projects/tein
```

**Step 6: Rebuild + run tests**
```bash
just clean && cargo build 2>&1 | tail -20
cargo test tein_file_open 2>&1 | tail -20
```
Expected: PASS.

**Step 7: Run all trampoline tests**
```bash
cargo test test_open_input_file_trampoline_allowed test_open_input_file_trampoline_denied test_open_output_file_trampoline_allowed test_open_output_file_trampoline_denied test_open_input_file_unsandboxed_passthrough 2>&1 | tail -20
```
Expected: PASS.

**Step 8: Full test suite**
```bash
just test 2>&1 | tail -30
```

**Step 9: Commit tein side**
```bash
git add tein/tests/scheme/tein_file_open.scm
git commit -m "test: scheme test for (tein file) higher-order wrappers"
```

---

## Task 7: VFS shadow integration tests for (scheme file) + (scheme repl)

**Files:**
- Modify: `tein/src/context.rs`

**Step 1: Write tests**

```rust
#[test]
fn test_scheme_file_shadow_importable_in_sandbox() {
    let _lock = IO_TEST_LOCK.lock().unwrap();
    use crate::sandbox::Modules;
    let tmp = "/tmp/tein_shadow_file_test.txt";
    std::fs::write(tmp, "shadowed").expect("write");
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::Safe)
        .file_read(&["/tmp/"])
        .build()
        .expect("builder");
    // (scheme file) in sandbox should resolve to our shadow
    let r = ctx.evaluate(&format!(
        "(import (scheme file)) (let ((p (open-input-file \"{tmp}\"))) (close-input-port p) #t)"
    ));
    assert_eq!(r.expect("scheme file shadow works"), Value::Bool(true));
}

#[test]
fn test_scheme_file_shadow_denies_without_policy() {
    use crate::sandbox::Modules;
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::Safe)
        // no file_read configured
        .build()
        .expect("builder");
    let r = ctx.evaluate("(import (scheme file)) (open-input-file \"/etc/passwd\")");
    assert!(r.is_err(), "scheme/file open-input-file denied without policy");
}

#[test]
fn test_scheme_file_not_shadowed_unsandboxed() {
    // unsandboxed: (scheme file) resolves to chibi's native, still works
    let tmp = "/tmp/tein_unsandboxed_scheme_file.txt";
    std::fs::write(tmp, "native").expect("write");
    let ctx = Context::builder()
        .standard_env()
        .build()
        .expect("builder");
    let r = ctx.evaluate(&format!(
        "(import (scheme file)) (let ((p (open-input-file \"{tmp}\"))) (close-input-port p) #t)"
    ));
    assert_eq!(r.expect("unsandboxed scheme file works"), Value::Bool(true));
}

#[test]
fn test_scheme_repl_shadow_returns_environment() {
    use crate::sandbox::Modules;
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::Safe)
        .build()
        .expect("builder");
    // interaction-environment should return an env (not #f, not error)
    // verify it's callable and the result is usable (environments are opaque,
    // but we can check that it doesn't error)
    let r = ctx.evaluate(
        "(import (scheme repl)) (let ((e (interaction-environment))) #t)"
    );
    assert_eq!(r.expect("scheme repl shadow works"), Value::Bool(true));
}
```

**Step 2: Run**
```bash
cargo test test_scheme_file_shadow_importable test_scheme_file_shadow_denies test_scheme_file_not_shadowed test_scheme_repl_shadow_returns 2>&1 | tail -20
```
Expected: PASS.

**Step 3: Full test suite**
```bash
just test 2>&1 | tail -30
```

**Step 4: Commit**
```bash
git add tein/src/context.rs
git commit -m "test: VFS shadow integration tests for (scheme file) + (scheme repl)"
```

---

## Task 8: Enable (scheme show) / (srfi 166) in Modules::Safe

**Files:**
- Modify: `tein/src/vfs_registry.rs`

**Step 1: Write failing tests**

Add to context.rs tests:

```rust
#[test]
fn test_scheme_show_importable_in_sandbox() {
    use crate::sandbox::Modules;
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::Safe)
        .step_limit(10_000_000)
        .build()
        .expect("builder");
    let r = ctx.evaluate("(import (scheme show)) (show #f \"hello\")");
    assert!(r.is_ok(), "scheme show importable in sandbox: {:?}", r);
}

#[test]
fn test_srfi_166_base_importable_in_sandbox() {
    use crate::sandbox::Modules;
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::Safe)
        .step_limit(10_000_000)
        .build()
        .expect("builder");
    let r = ctx.evaluate("(import (srfi 166 base)) (show #f (displayed \"test\"))");
    assert!(r.is_ok(), "srfi/166/base importable in sandbox: {:?}", r);
}
```

**Step 2: Run to confirm failure**
```bash
cargo test test_scheme_show_importable_in_sandbox test_srfi_166_base_importable_in_sandbox 2>&1 | tail -20
```
Expected: FAIL — blocked by gate.

**Step 3: Update registry — flip safe flags**

In `tein/src/vfs_registry.rs`:

1. `scheme/show` — flip `default_safe: false` → `default_safe: true`. Update comment:
```rust
// scheme/show: deps satisfied via shadows — scheme/file via (tein file),
// scheme/repl via (current-environment)
```

2. `srfi/166` — flip `default_safe: false` → `default_safe: true`

3. `srfi/166/base` — flip `default_safe: false` → `default_safe: true`. Update comment:
```rust
// scheme/repl dep satisfied via VFS shadow (neutered interaction-environment)
```

4. `srfi/166/pretty` — flip `default_safe: false` → `default_safe: true`

5. `srfi/166/columnar` — flip `default_safe: false` → `default_safe: true`. Update comment:
```rust
// scheme/file dep satisfied via VFS shadow → (tein file)
```

6. `srfi/166/unicode` — flip `default_safe: false` → `default_safe: true`

7. `srfi/166/color` — flip `default_safe: false` → `default_safe: true`

**Step 4: Run tests**
```bash
cargo test test_scheme_show_importable_in_sandbox test_srfi_166_base_importable_in_sandbox 2>&1 | tail -20
```
Expected: PASS.

**Step 5: Update `registry_safe_allowlist_contains_expected_modules` test in `sandbox.rs`**

Add assertions:
```rust
assert!(
    safe.iter().any(|m| m == "scheme/file"),
    "scheme/file missing from safe (shadow)"
);
assert!(
    safe.iter().any(|m| m == "scheme/repl"),
    "scheme/repl missing from safe (shadow)"
);
assert!(
    safe.iter().any(|m| m == "scheme/show"),
    "scheme/show missing from safe"
);
assert!(
    safe.iter().any(|m| m == "srfi/166"),
    "srfi/166 missing from safe"
);
```

**Step 6: Full test suite + lint**
```bash
just test 2>&1 | tail -30
just lint
```

**Step 7: Commit**
```bash
git add tein/src/vfs_registry.rs tein/src/sandbox.rs
git commit -m "feat: enable (scheme show) / (srfi 166) in Modules::Safe via VFS shadows"
```

---

## Task 9: Integration test — srfi/166/columnar from-file with FsPolicy

**Files:**
- Modify: `tein/src/context.rs`

**Step 1: Write tests**

```rust
#[test]
fn test_srfi_166_columnar_from_file_with_policy() {
    let _lock = IO_TEST_LOCK.lock().unwrap();
    use crate::sandbox::Modules;
    let tmp = "/tmp/tein_from_file_test.txt";
    std::fs::write(tmp, "line1\nline2\n").expect("write");
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::Safe)
        .file_read(&["/tmp/"])
        .step_limit(10_000_000)
        .build()
        .expect("builder");
    let r = ctx.evaluate(&format!(
        "(import (srfi 166 columnar)) (show #f (from-file \"{tmp}\"))"
    ));
    assert!(r.is_ok(), "from-file with read policy: {:?}", r);
}

#[test]
fn test_srfi_166_columnar_from_file_denied_without_policy() {
    use crate::sandbox::Modules;
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::Safe)
        .step_limit(10_000_000)
        .build()
        .expect("builder");
    // from-file calls open-input-file which hits the policy check
    let r = ctx.evaluate("(import (srfi 166 columnar)) (show #f (from-file \"/tmp/x\"))");
    assert!(r.is_err(), "from-file without policy should fail");
}
```

**Step 2: Run**
```bash
cargo test test_srfi_166_columnar_from_file_with_policy test_srfi_166_columnar_from_file_denied_without_policy 2>&1 | tail -20
```
Expected: PASS.

**Step 3: Full test suite**
```bash
just test 2>&1 | tail -30
```

**Step 4: Commit**
```bash
git add tein/src/context.rs
git commit -m "test: srfi/166/columnar from-file integration with FsPolicy"
```

---

## Task 10: Docs — sandbox.rs comment + AGENTS.md + design doc

**Files:**
- Modify: `tein/src/sandbox.rs`
- Modify: `AGENTS.md`
- Modify: `docs/plans/2026-03-01-vfs-shadow-scheme-file-design.md`

**Step 1: Update sandbox.rs comment block**

Find `// modules NOT in the VFS registry:`. Replace with:

```rust
// VFS shadow modules:
//
// the following modules have `VfsSource::Shadow` entries in the registry.
// in sandboxed contexts, `register_vfs_shadows()` injects replacement `.sld`
// files that re-export from safe tein counterparts or provide neutered stubs.
// unsandboxed contexts use chibi's native versions (no shadow registered).
//
// - `scheme/file` — re-exports (tein file), providing FsPolicy enforcement
// - `scheme/repl` — neutered interaction-environment via (current-environment)
//
// modules NOT shadowed and intentionally blocked:
//
// - `scheme/process-context` — exit/emergency-exit kills the host process.
//   use (tein process) instead.
// - `scheme/load` — loads arbitrary files. use (tein load) instead.
// - `scheme/eval` — eval + environment. tracked for future shadow (GH issue).
// - `scheme/r5rs` — re-exports scheme/file, scheme/load, scheme/process-context.
```

**Step 2: Update AGENTS.md sandboxing flow**

Find: `set IS_SANDBOXED thread-local →`

Insert after `set IS_SANDBOXED thread-local`:
```
→ register_vfs_shadows() (injects scheme/file.sld → (tein file), scheme/repl.sld → neutered) →
```

**Step 3: Update design doc status**

Change the status line at the top of `docs/plans/2026-03-01-vfs-shadow-scheme-file-design.md` from `**status: BLOCKED**` to `**status: IMPLEMENTED**` and add a note:

```
**status: IMPLEMENTED** — superseded by implementation plan `2026-03-01-vfs-shadow-scheme-file.md`.
Architecture evolved: `VfsSource::Shadow` variant in registry (data-driven) instead of separate
`VFS_SHADOWS` array. Added `scheme/repl` shadow for `interaction-environment`.
```

**Step 4: Commit**
```bash
git add tein/src/sandbox.rs AGENTS.md docs/plans/2026-03-01-vfs-shadow-scheme-file-design.md
git commit -m "docs: update sandbox comments, AGENTS.md, and design doc for VFS shadows"
```

---

## Task 11: Final verification + lint + batch notes

**Step 1: Full test suite**
```bash
just test 2>&1 | tail -40
```
Expected: all pass.

**Step 2: Lint**
```bash
just lint
```

**Step 3: Update this plan**

Mark all tasks as complete. Add notes for AGENTS.md collection (final step per workflow):

- `VfsSource::Shadow` — new registry variant for sandbox-only module replacements
- `register_vfs_shadows()` — iterates registry, injects Shadow entries before VFS gate
- `scheme/file` shadow + `scheme/repl` shadow — first two uses of the pattern
- `capture_file_originals()` — must be called in both sandbox and unsandbox build paths
- old IO wrapper system fully removed — `check_and_delegate`, `wrapper_fn_for` etc. gone
- `(current-environment)` from `(chibi)` is always available and returns the sandbox env in sandboxed contexts — useful for neutering env-exposing procedures

**Step 4: Commit plan update**
```bash
git add docs/plans/2026-03-01-vfs-shadow-scheme-file.md
git commit -m "docs: mark implementation plan complete with batch notes"
```

**Step 5: Halt for context clear**

---

## Task 12: PR

After context clear and final verification:

```bash
gh pr create \
  --base dev \
  --title "feat: VFS shadow system + (scheme file/repl/show) in sandbox" \
  --body "$(cat <<'EOF'
## summary

- introduces `VfsSource::Shadow` — data-driven VFS shadow system in the module registry. shadow `.sld` content is declared inline in `VFS_REGISTRY` entries; `register_vfs_shadows()` injects them into the dynamic VFS at sandbox build time only
- `scheme/file` shadow: re-exports all 10 names from `(tein file)` with FsPolicy enforcement
- `scheme/repl` shadow: neutered `interaction-environment` via `(current-environment)` — returns the sandbox's restricted env
- expands `(tein file)` from 2 to 10 exports: 4 new `open-*-file` rust trampolines + 4 higher-order scheme wrappers
- removes old IO wrapper system (`check_and_delegate`, `wrapper_open_*`, `wrapper_fn_for`, `has_io` block) — policy enforcement unified in trampolines
- enables `(scheme show)` / `(srfi 166)` + all sub-modules in `Modules::Safe`
- `srfi/166/columnar` `from-file` works in sandbox with `file_read` policy

closes #91

## test plan

- [ ] trampoline policy tests (allowed/denied for input+output)
- [ ] unsandboxed passthrough for open-input-file
- [ ] scheme/file shadow resolution + policy checks in sandbox
- [ ] scheme/repl shadow returns usable environment
- [ ] scheme/show importable in sandbox
- [ ] srfi/166/columnar from-file with/without policy
- [ ] higher-order wrappers (call-with-*, with-*-from/to-file)
- [ ] all existing IO policy tests pass
- [ ] registry safe allowlist contains new modules
- [ ] `just test` green
EOF
)"
```
