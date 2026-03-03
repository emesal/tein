# module inventory completion — implementation plan

> **for claude:** REQUIRED SUB-SKILL: use superpowers:executing-plans to implement this plan task-by-task.

**goal:** close out the module inventory — every chibi-scheme module either in VFS, intentionally excluded, or tracked in a github issue. closes #92.

**architecture:** 3 pure VFS additions, 8 new shadow stubs (generated), 1 hand-written shadow (scheme/load → tein/load). all changes in `tein/src/vfs_registry.rs` (registry + SHADOW_STUBS), `tein/src/context.rs` (tests), and `docs/module-inventory.md` (status updates).

**tech stack:** rust, scheme (`.sld` module definitions), build.rs shadow stub generator

**design doc:** `docs/plans/2026-03-03-module-inventory-completion-design.md`

---

## batch structure

the work is split into 4 batches. after each batch: lint, update this plan, commit, halt for context clearing.

- **batch 1** (tasks 1-3): pure VFS additions — chibi/mime, chibi/binary-record, chibi/memoize
- **batch 2** (tasks 4-6): shadow stubs part 1 — chibi/stty, chibi/term/edit-line, chibi/log, chibi/app, chibi/config
- **batch 3** (tasks 7-9): shadow stubs part 2 — chibi/tar, srfi/193, chibi/apropos
- **batch 4** (tasks 10-13): hand-written scheme/load shadow, docs finalisation, close #92

---

## task 1: add chibi/mime to VFS

**status:** done

**files:**
- modify: `tein/src/vfs_registry.rs` (VFS_REGISTRY, insert before `chibi/uri` at ~line 3009)
- modify: `tein/src/context.rs` (add integration test after ~line 7960)

**step 1: add VFS entry**

insert before the closing `];` of VFS_REGISTRY (before `chibi/uri`), alphabetically among `chibi/*`:

```rust
    VfsEntry {
        path: "chibi/mime",
        deps: &[
            "scheme/base", "scheme/char", "scheme/write",
            "chibi/base64", "chibi/quoted-printable", "chibi/string",
        ],
        files: &["lib/chibi/mime.sld", "lib/chibi/mime.scm"],
        clib: None,
        default_safe: true,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
```

**step 2: write the integration test**

add to `context.rs` in the VFS module tests section (after `test_srfi_227_definition_loads`):

```rust
    #[test]
    fn test_chibi_mime_loads() {
        // chibi/mime is pure scheme: MIME parsing with base64/quoted-printable.
        // all deps in VFS and safe.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .unwrap();
        let result = ctx
            .evaluate(
                "(import (chibi mime)) \
                 (mime-parse-content-type \"text/html; charset=utf-8\")",
            )
            .expect("chibi/mime should load and parse content types");
        // result is an alist like (("text/html") ("charset" . "utf-8"))
        // just verify it's not an error — the exact structure depends on chibi's impl
        assert!(
            !matches!(result, Value::String(_)),
            "expected parsed content-type, got error string: {result}"
        );
    }
```

**step 3: run tests**

```bash
cargo test -p tein test_chibi_mime_loads -- --nocapture
```

expected: PASS

**step 4: commit**

```
feat: add chibi/mime to VFS registry

pure MIME parsing library — base64/quoted-printable encoding,
content-type parsing, MIME message folding. all deps already in VFS.
```

---

## task 2: add chibi/binary-record to VFS

**status:** done

**files:**
- modify: `tein/src/vfs_registry.rs` (VFS_REGISTRY, insert alphabetically among `chibi/*`)
- modify: `tein/src/context.rs` (add integration test)

**step 1: add VFS entry**

insert alphabetically (after `chibi/base64`, before `chibi/bytevector`):

```rust
    VfsEntry {
        path: "chibi/binary-record",
        deps: &["scheme/base", "srfi/1", "srfi/151", "srfi/130"],
        files: &[
            "lib/chibi/binary-record.sld",
            "lib/chibi/binary-types.scm",
            "lib/chibi/binary-record.scm",
        ],
        clib: None,
        default_safe: true,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
```

**step 2: write the integration test**

```rust
    #[test]
    fn test_chibi_binary_record_loads() {
        // chibi/binary-record provides binary record type macros.
        // pure scheme, deps: scheme/base, srfi/1, srfi/151, srfi/130.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .unwrap();
        // binary-record defines binary types — test that the macro is available
        // by importing and checking that one of the built-in types exists.
        ctx.evaluate(
            "(import (chibi binary-record)) \
             (define-binary-type (test-u8) u8 (lambda (x) x) (lambda (x) x))",
        )
        .expect("chibi/binary-record should load and define-binary-type should work");
    }
```

**note:** `define-binary-type` is the simplest exported macro. if this doesn't work cleanly (macro expansion might need more context), fall back to just testing the import:

```rust
        ctx.evaluate("(import (chibi binary-record))")
            .expect("chibi/binary-record should load");
```

**step 3: run tests**

```bash
cargo test -p tein test_chibi_binary_record_loads -- --nocapture
```

expected: PASS

**step 4: commit**

```
feat: add chibi/binary-record to VFS registry

binary record type definition macros for structured binary data.
pure scheme, no OS deps.
```

---

## task 3: add chibi/memoize to VFS + batch 1 wrap-up

**status:** done

**files:**
- modify: `tein/src/vfs_registry.rs` (VFS_REGISTRY, insert alphabetically among `chibi/*`)
- modify: `tein/src/context.rs` (add integration test)
- modify: `docs/module-inventory.md` (update 3 status markers)
- modify: this plan (mark batch 1 complete)

**step 1: add VFS entry**

insert alphabetically (after `chibi/math/prime`, before `chibi/mime`):

```rust
    VfsEntry {
        path: "chibi/memoize",
        deps: &[
            "scheme/base", "scheme/char", "scheme/file",
            "chibi/optional", "chibi/pathname", "chibi/string",
            "chibi/ast", "chibi/system", "chibi/filesystem",
            "srfi/9", "srfi/38", "srfi/69", "srfi/98",
        ],
        files: &["lib/chibi/memoize.sld", "lib/chibi/memoize.scm"],
        clib: None,
        default_safe: true,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
```

**step 2: write the integration test**

```rust
    #[test]
    fn test_chibi_memoize_loads() {
        // chibi/memoize provides in-memory LRU caching.
        // chibi cond-expand branch pulls chibi/system + chibi/filesystem
        // (both already shadowed). in-memory parts work; file-backed
        // memoize-to-file errors via shadowed deps. #105 upgrades later.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .unwrap();
        let result = ctx
            .evaluate(
                "(import (chibi memoize)) \
                 (define memo-add (memoize +)) \
                 (memo-add 2 3)",
            )
            .expect("chibi/memoize should load and in-memory memoize should work");
        assert_eq!(result, Value::Integer(5));
    }
```

**step 3: run tests**

```bash
cargo test -p tein test_chibi_memoize_loads -- --nocapture
```

expected: PASS

**step 4: update docs/module-inventory.md**

change these lines in the chibi/* table:

| old | new |
|-----|-----|
| `chibi/binary-record` ❌ `binary i/o record types — needs review` | `chibi/binary-record` ✅ `binary record type macros — pure scheme` |
| `chibi/memoize` ❌ `memoization — cond-expand uses chibi/system + chibi/filesystem ⚠️` | `chibi/memoize` ✅ `in-memory LRU cache works; file-backed errors via shadowed deps (#105)` |
| `chibi/mime` ❌ `MIME parsing — needs file i/o ⚠️` | `chibi/mime` ✅ `pure MIME parsing — base64, content-type, message folding` |

**step 5: run lint**

```bash
just lint
```

**step 6: update this plan** — mark tasks 1-3 as `done`, add any notes/caveats discovered

**step 7: commit**

```
feat: add chibi/memoize to VFS + batch 1 docs update

in-memory LRU cache works in sandbox; file-backed memoize-to-file
errors via already-shadowed chibi/filesystem + chibi/system deps.
future #105 (writable VFS compartment) upgrades automatically.

update module-inventory.md: chibi/mime, chibi/binary-record,
chibi/memoize all ✅
```

**step 8: halt** — context clear point. next session starts at batch 2.

---

## task 4: add shadow stubs — chibi/stty + chibi/term/edit-line

**status:** done

**files:**
- modify: `tein/src/vfs_registry.rs` (VFS_REGISTRY: 2 VfsEntry, SHADOW_STUBS: 2 ShadowStub)
- modify: `tein/src/context.rs` (2 integration tests)

**step 1: add VFS entries**

insert in the `OS-touching modules` shadow section (~line 1957), after `chibi/temp-file`:

```rust
    VfsEntry {
        path: "chibi/stty",
        deps: &["scheme/base"],
        files: &[],
        clib: None,
        default_safe: true,
        source: VfsSource::Shadow,
        feature: None,
        shadow_sld: None, // generated from SHADOW_STUBS by build.rs
    },
    VfsEntry {
        path: "chibi/term/edit-line",
        deps: &["scheme/base"],
        files: &[],
        clib: None,
        default_safe: true,
        source: VfsSource::Shadow,
        feature: None,
        shadow_sld: None, // generated from SHADOW_STUBS by build.rs
    },
```

**step 2: add SHADOW_STUBS entries**

append to `SHADOW_STUBS` array before the closing `];` (after `chibi/net/servlet`):

```rust
    // --- terminal control ---
    ShadowStub {
        path: "chibi/stty",
        fn_exports: &[
            "stty", "with-stty", "with-raw-io",
            "get-terminal-width", "get-terminal-dimensions",
            // record types stubbed as fns
            "winsize", "winsize?", "make-winsize", "winsize-row", "winsize-col",
            "termios", "term-attrs?",
        ],
        const_exports: &["TCSANOW", "TCSADRAIN", "TCSAFLUSH"],
        macro_exports: &[],
    },
    ShadowStub {
        path: "chibi/term/edit-line",
        fn_exports: &[
            "make-line-editor", "edit-line", "edit-line-repl",
            "make-history", "history-insert!", "history-reset!",
            "history-commit!", "history->list", "list->history",
            "buffer->string", "make-buffer", "buffer-make-completer",
            "buffer-clear", "buffer-refresh", "buffer-draw",
            "buffer-row", "buffer-col",
            "make-keymap", "make-standard-keymap",
        ],
        const_exports: &[],
        macro_exports: &[],
    },
```

**step 3: write integration tests**

```rust
    #[test]
    fn test_shadow_stub_chibi_stty_raises_error() {
        // chibi/stty is C-backed terminal control — shadow stub blocks all access.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_module("chibi/stty")
            .build()
            .unwrap();
        let result = ctx.evaluate("(import (chibi stty)) (get-terminal-width)");
        let err_str = format!("{}", result.unwrap());
        assert!(
            err_str.contains("[sandbox:chibi/stty]"),
            "expected sandbox error, got: {err_str}"
        );
    }

    #[test]
    fn test_shadow_stub_chibi_term_edit_line_raises_error() {
        // chibi/term/edit-line depends on stty — shadow stub blocks all access.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_module("chibi/term/edit-line")
            .build()
            .unwrap();
        let result = ctx.evaluate(
            "(import (chibi term edit-line)) (make-line-editor)",
        );
        let err_str = format!("{}", result.unwrap());
        assert!(
            err_str.contains("[sandbox:chibi/term/edit-line]"),
            "expected sandbox error, got: {err_str}"
        );
    }
```

**note on test pattern:** shadow stub fns return a scheme string containing the error message (see AGENTS.md: "Result::Err returns a scheme string"). so `result.unwrap()` gives `Value::String(msg)`. verify this matches the existing shadow stub tests — if they use a different pattern, match it.

**step 4: run tests**

```bash
cargo test -p tein test_shadow_stub_chibi_stty -- --nocapture
cargo test -p tein test_shadow_stub_chibi_term_edit_line -- --nocapture
```

expected: PASS

**step 5: commit**

```
feat: add chibi/stty + chibi/term/edit-line shadow stubs

terminal control (ioctl, raw mode, line editing) blocked in sandbox.
stty is C-backed; edit-line is pure scheme depending on stty.
both importable; all exports raise [sandbox:path] errors.
```

---

## task 5: add shadow stubs — chibi/log + chibi/app + chibi/config

**status:** done

**files:**
- modify: `tein/src/vfs_registry.rs` (VFS_REGISTRY: 3 VfsEntry, SHADOW_STUBS: 3 ShadowStub)
- modify: `tein/src/context.rs` (3 integration tests)

**step 1: add VFS entries**

insert in the shadow section, alphabetically:

```rust
    VfsEntry {
        path: "chibi/app",
        deps: &["scheme/base"],
        files: &[],
        clib: None,
        default_safe: true,
        source: VfsSource::Shadow,
        feature: None,
        shadow_sld: None, // generated from SHADOW_STUBS by build.rs
    },
    VfsEntry {
        path: "chibi/config",
        deps: &["scheme/base"],
        files: &[],
        clib: None,
        default_safe: true,
        source: VfsSource::Shadow,
        feature: None,
        shadow_sld: None, // generated from SHADOW_STUBS by build.rs
    },
    VfsEntry {
        path: "chibi/log",
        deps: &["scheme/base"],
        files: &[],
        clib: None,
        default_safe: true,
        source: VfsSource::Shadow,
        feature: None,
        shadow_sld: None, // generated from SHADOW_STUBS by build.rs
    },
```

**step 2: add SHADOW_STUBS entries**

```rust
    // --- application framework ---
    ShadowStub {
        path: "chibi/app",
        fn_exports: &[
            "parse-option", "parse-options", "parse-app",
            "run-application", "app-help", "app-help-command",
        ],
        const_exports: &[],
        macro_exports: &[],
    },
    ShadowStub {
        path: "chibi/config",
        fn_exports: &[
            "make-conf", "conf?", "conf-load", "conf-load-in-path",
            "conf-load-cascaded", "conf-verify", "conf-extend",
            "conf-append", "conf-set", "conf-unfold-key",
            "conf-get", "conf-get-list", "conf-get-cdr", "conf-get-multi",
            "conf-specialize", "read-from-file",
            "conf-source", "conf-head", "conf-parent",
            "assoc-get", "assoc-get-list",
        ],
        const_exports: &[],
        macro_exports: &[],
    },
    // --- logging ---
    ShadowStub {
        path: "chibi/log",
        fn_exports: &[
            "Logger", "logger?",
            "logger-levels", "logger-levels-set!",
            "logger-level-abbrevs", "logger-level-abbrevs-set!",
            "logger-current-level", "logger-current-level-set!",
            "logger-prefix", "logger-prefix-set!",
            "logger-counts", "logger-counts-set!",
            "logger-file", "logger-file-set!",
            "logger-port", "logger-port-set!",
            "logger-locked?", "logger-locked?-set!",
            "logger-zipped?", "logger-zipped?-set!",
            "log-open", "log-close", "log-show", "log-show-every-n",
            "log-compile-prefix",
            "log-level-index", "log-level-name", "log-level-abbrev",
            "log-emergency", "log-alert", "log-critical",
            "log-error", "log-warn", "log-notice",
            "log-info", "log-debug", "log-trace",
            "default-logger",
        ],
        const_exports: &[],
        macro_exports: &[
            "define-logger",
            "with-logged-errors",
            "with-logged-and-reraised-errors",
        ],
    },
```

**step 3: write integration tests**

```rust
    #[test]
    fn test_shadow_stub_chibi_app_raises_error() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_module("chibi/app")
            .build()
            .unwrap();
        let result = ctx.evaluate("(import (chibi app)) (app-help '())");
        let err_str = format!("{}", result.unwrap());
        assert!(
            err_str.contains("[sandbox:chibi/app]"),
            "expected sandbox error, got: {err_str}"
        );
    }

    #[test]
    fn test_shadow_stub_chibi_config_raises_error() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_module("chibi/config")
            .build()
            .unwrap();
        let result = ctx.evaluate("(import (chibi config)) (make-conf '())");
        let err_str = format!("{}", result.unwrap());
        assert!(
            err_str.contains("[sandbox:chibi/config]"),
            "expected sandbox error, got: {err_str}"
        );
    }

    #[test]
    fn test_shadow_stub_chibi_log_raises_error() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_module("chibi/log")
            .build()
            .unwrap();
        let result = ctx.evaluate("(import (chibi log)) (log-open \"test\")");
        let err_str = format!("{}", result.unwrap());
        assert!(
            err_str.contains("[sandbox:chibi/log]"),
            "expected sandbox error, got: {err_str}"
        );
    }
```

**step 4: run tests**

```bash
cargo test -p tein test_shadow_stub_chibi_app -- --nocapture
cargo test -p tein test_shadow_stub_chibi_config -- --nocapture
cargo test -p tein test_shadow_stub_chibi_log -- --nocapture
```

expected: PASS

**step 5: commit**

```
feat: add chibi/app, chibi/config, chibi/log shadow stubs

app: CLI framework depending on config + process-context
config: config file reader depending on scheme/file + chibi/filesystem
log: logging framework with file locking, PIDs, UIDs

all importable in sandbox; all exports raise [sandbox:path] errors.
#105 could enable scoped file access for config and log in future.
```

---

## task 6: batch 2 wrap-up

**status:** done

**files:**
- modify: `docs/module-inventory.md` (update 5 status markers)
- modify: this plan (mark batch 2 complete)

**step 1: update docs/module-inventory.md**

change these lines in the chibi/* table:

| old | new |
|-----|-----|
| `chibi/app` ❌ `CLI app framework — reads env/args, needs shadow` | `chibi/app` 🌑 `shadow stub — CLI framework; depends on config + process-context` |
| `chibi/config` ❌ `reads config files — file i/o` | `chibi/config` 🌑 `shadow stub — config file reader; filesystem access (#105)` |
| `chibi/log` ❌ `logging — writes to stderr, file` | `chibi/log` 🌑 `shadow stub — logging with file locking + OS identity (#105)` |
| `chibi/stty` ❌ `terminal control — OS ⚠️` | `chibi/stty` 🌑 `shadow stub — terminal ioctl, C-backed` |
| `chibi/term/edit-line` ❌ `line editing — terminal i/o ⚠️` | `chibi/term/edit-line` 🌑 `shadow stub — line editor, depends on stty` |

**step 2: run lint**

```bash
just lint
```

**step 3: update this plan** — mark tasks 4-6 as `done`, add notes

**step 4: commit**

```
docs: update module inventory — batch 2 shadow stubs

chibi/stty, chibi/term/edit-line, chibi/log, chibi/app, chibi/config
all now shadow stubs (🌑) in VFS.
```

**step 5: halt** — context clear point. next session starts at batch 3.

---

## task 7: add shadow stubs — chibi/tar + srfi/193 + chibi/apropos

**status:** done

**files:**
- modify: `tein/src/vfs_registry.rs` (VFS_REGISTRY: 3 VfsEntry, SHADOW_STUBS: 3 ShadowStub)
- modify: `tein/src/context.rs` (3 integration tests)

**step 1: add VFS entries**

for `chibi/apropos` and `chibi/tar` — insert in shadow section alphabetically among `chibi/*`.
for `srfi/193` — insert in shadow section (or create a new srfi shadow subsection).

```rust
    VfsEntry {
        path: "chibi/apropos",
        deps: &["scheme/base"],
        files: &[],
        clib: None,
        default_safe: true,
        source: VfsSource::Shadow,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "chibi/tar",
        deps: &["scheme/base"],
        files: &[],
        clib: None,
        default_safe: true,
        source: VfsSource::Shadow,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/193",
        deps: &["scheme/base"],
        files: &[],
        clib: None,
        default_safe: true,
        source: VfsSource::Shadow,
        feature: None,
        shadow_sld: None,
    },
```

**step 2: add SHADOW_STUBS entries**

```rust
    // --- environment introspection ---
    ShadowStub {
        path: "chibi/apropos",
        fn_exports: &["apropos", "apropos-list"],
        const_exports: &[],
        macro_exports: &[],
    },
    // --- tar archives ---
    ShadowStub {
        path: "chibi/tar",
        fn_exports: &[
            "tar", "make-tar", "tar?", "read-tar", "write-tar",
            "tar-safe?", "tar-files", "tar-fold",
            "tar-extract", "tar-extract-file", "tar-create",
            "tar-path", "tar-path-prefix", "tar-mode",
            "tar-uid", "tar-gid", "tar-owner", "tar-group",
            "tar-size", "tar-time", "tar-type", "tar-link-name",
            "tar-path-set!", "tar-mode-set!", "tar-uid-set!",
            "tar-gid-set!", "tar-owner-set!", "tar-group-set!",
            "tar-size-set!", "tar-time-set!", "tar-type-set!",
            "tar-link-name-set!",
            "tar-device-major", "tar-device-major-set!",
            "tar-device-minor", "tar-device-minor-set!",
            "tar-ustar", "tar-ustar-set!",
        ],
        const_exports: &[],
        macro_exports: &[],
    },
    // --- SRFI-193 command-line ---
    ShadowStub {
        path: "srfi/193",
        fn_exports: &[
            "command-line", "command-name", "command-args",
            "script-file", "script-directory",
        ],
        const_exports: &[],
        macro_exports: &[],
    },
```

**step 3: write integration tests**

```rust
    #[test]
    fn test_shadow_stub_chibi_tar_raises_error() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_module("chibi/tar")
            .build()
            .unwrap();
        let result = ctx.evaluate("(import (chibi tar)) (tar-files \"test.tar\")");
        let err_str = format!("{}", result.unwrap());
        assert!(
            err_str.contains("[sandbox:chibi/tar]"),
            "expected sandbox error, got: {err_str}"
        );
    }

    #[test]
    fn test_shadow_stub_srfi_193_raises_error() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_module("srfi/193")
            .build()
            .unwrap();
        let result = ctx.evaluate("(import (srfi 193)) (script-file)");
        let err_str = format!("{}", result.unwrap());
        assert!(
            err_str.contains("[sandbox:srfi/193]"),
            "expected sandbox error, got: {err_str}"
        );
    }

    #[test]
    fn test_shadow_stub_chibi_apropos_raises_error() {
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .allow_module("chibi/apropos")
            .build()
            .unwrap();
        let result = ctx.evaluate("(import (chibi apropos)) (apropos \"test\")");
        let err_str = format!("{}", result.unwrap());
        assert!(
            err_str.contains("[sandbox:chibi/apropos]"),
            "expected sandbox error, got: {err_str}"
        );
    }
```

**step 4: run tests**

```bash
cargo test -p tein test_shadow_stub_chibi_tar -- --nocapture
cargo test -p tein test_shadow_stub_srfi_193 -- --nocapture
cargo test -p tein test_shadow_stub_chibi_apropos -- --nocapture
```

expected: PASS

**step 5: commit**

```
feat: add chibi/tar, srfi/193, chibi/apropos shadow stubs

tar: archive handling hard-wired to chibi/filesystem (#105)
srfi/193: command-line args + script path leak in sandbox
apropos: env introspection exposes internal module structure
```

---

## task 8: batch 3 wrap-up

**status:** done

**files:**
- modify: `docs/module-inventory.md` (update 3 status markers)
- modify: this plan (mark batch 3 complete)

**step 1: update docs/module-inventory.md**

| old | new |
|-----|-----|
| `chibi/apropos` ❌ `reflects on env/module contents` | `chibi/apropos` 🌑 `shadow stub — env introspection, info leak` |
| `chibi/tar` ❌ `tar format — file i/o ⚠️` | `chibi/tar` 🌑 `shadow stub — tar archives, hard-wired to filesystem (#105)` |
| `srfi/193` ❌ `command channel — not in VFS` | `srfi/193` 🌑 `shadow stub — leaks argv + script path` |

**step 2: run lint**

```bash
just lint
```

**step 3: update this plan** — mark tasks 7-8 as `done`

**step 4: commit**

```
docs: update module inventory — batch 3 shadow stubs

chibi/tar, srfi/193, chibi/apropos now shadow stubs (🌑) in VFS.
```

**step 5: halt** — context clear point. next session starts at batch 4.

---

## task 9: add scheme/load hand-written shadow

**status:** done

**files:**
- modify: `tein/src/vfs_registry.rs` (VFS_REGISTRY: 1 VfsEntry with `shadow_sld: Some(...)`)
- modify: `tein/src/context.rs` (1 integration test)

**step 1: add VFS entry**

insert alphabetically in `scheme/*` section (after `scheme/lazy` at ~line 434, before `scheme/list`):

```rust
    // scheme/load: VFS shadow — re-exports VFS-restricted load from (tein load).
    // chibi's native (scheme load) exposes unrestricted file loading.
    // this shadow provides safe load semantics via tein-load-vfs-internal.
    // see also: tein/load.sld exports load as (rename tein-load-vfs-internal load).
    VfsEntry {
        path: "scheme/load",
        deps: &["tein/load"],
        files: &[],
        clib: None,
        default_safe: true,
        source: VfsSource::Shadow,
        feature: None,
        shadow_sld: Some("\
(define-library (scheme load)
  (import (tein load))
  (export load))
"),
    },
```

**step 2: write integration test**

```rust
    #[test]
    fn test_scheme_load_shadow_uses_tein_load() {
        // scheme/load shadow re-exports from (tein load) — VFS-restricted load.
        // loading a VFS module path should work; loading a non-VFS path should fail.
        use crate::sandbox::Modules;
        let ctx = Context::builder()
            .standard_env()
            .sandboxed(Modules::Safe)
            .build()
            .unwrap();
        // load a known VFS file — srfi/8.scm defines receive
        ctx.evaluate(
            "(import (scheme load)) \
             (load \"/vfs/lib/srfi/8.scm\")",
        )
        .expect("(scheme load) should provide VFS-restricted load");
    }
```

**step 3: run tests**

```bash
cargo test -p tein test_scheme_load_shadow -- --nocapture
```

expected: PASS

**step 4: commit**

```
feat: add (scheme load) shadow — re-exports VFS-restricted load

(import (scheme load)) now provides VFS-restricted load via (tein load)
instead of chibi's unrestricted native load. forward-compatible with
#105 (writable VFS compartment).
```

---

## task 10: update summary table + priority queue in module-inventory.md

**status:** pending

**files:**
- modify: `docs/module-inventory.md` (summary table, priority queue section)

**step 1: update the summary table**

recalculate counts. after all changes:
- scheme/*: 48 safe + 7 unsafe + **4** shadow (was 3, +scheme/load) + **1** not in VFS (scheme/r5rs)
- srfi/*: 101 safe + 3 unsafe + **2** shadow (was 1, +srfi/193) + **0** not in VFS
- chibi/*: **68** safe (was 65, +mime, binary-record, memoize) + 0 unsafe + **8** shadow (new) + **26** not in VFS (was 34, -3 pure -8 shadow = 23 excluded)
- tein/*: 12 safe + 0 unsafe + 0 shadow + 0 not in VFS

```markdown
| category | ✅ safe | 🔒 unsafe | 🌑 shadow | ❌ excluded |
|----------|---------|----------|----------|-------------|
| scheme/* | 48 | 7 | 4 | 1 |
| srfi/* | 101 | 3 | 2 | 0 |
| chibi/* | 68 | 0 | 8 | 23 |
| tein/* | 12 | 0 | 0 | 0 |
| **total** | **229** | **10** | **14** | **24** |
```

note: rename "❌ not in VFS" column to "❌ excluded" since all remaining are intentionally excluded (documented in appendix B) or tracked in issues.

**step 2: update priority queue section**

replace the entire priority queue section with:

```markdown
### status

**✅ all modules resolved.** every chibi-scheme module is either in the VFS, intentionally excluded (appendix B), or tracked in a github issue.

**shadow stubs (phase 1 — error-on-call): 14 modules**
- OS filesystem: `chibi/filesystem`, `chibi/temp-file`
- OS process/system: `chibi/process`, `chibi/system`
- OS terminal: `chibi/stty`, `chibi/term/edit-line`
- OS network: `chibi/net`, `chibi/net/http`, `chibi/net/server`, `chibi/net/http-server`, `chibi/net/server-util`, `chibi/net/servlet`
- application: `chibi/shell`, `chibi/app`, `chibi/config`, `chibi/log`, `chibi/tar`
- info leak: `srfi/193`, `chibi/apropos`

**hand-written shadows (functional): 5 modules**
- `scheme/file` → re-exports from `(tein file)` with FsPolicy
- `scheme/process-context` → re-exports from `(tein process)` (neutered env/argv)
- `scheme/repl` → neutered `interaction-environment`
- `scheme/load` → re-exports from `(tein load)` (VFS-restricted)
- `srfi/98` → neutered env var stubs

**tracked in issues:**
- `scheme/r5rs` — #106 (blocked on #97, sandboxed eval)
- phase 2 progressive gating — #105 (writable VFS compartment)
```

**step 3: commit**

```
docs: finalise module inventory summary — all modules resolved
```

---

## task 11: update docs/module-inventory.md — scheme/load entry in table

**status:** pending

**files:**
- modify: `docs/module-inventory.md` (scheme/* table row for scheme/load)

**step 1: update scheme/load row**

| old | new |
|-----|-----|
| `scheme/load` ❌ `blocked; use tein/load instead` | `scheme/load` 🌑 `shadow → re-exports from (tein load) (VFS-restricted)` |

**step 2: commit** (fold into task 10 commit if same session)

---

## task 12: update handoff doc + close #92

**status:** pending

**files:**
- modify: `docs/handoff-module-inventory.md` (add session 6 entry, update "what remains")
- close: github issue #92

**step 1: add session 6 entry to handoff doc**

add after "session 5" section:

```markdown
### session 6 — module inventory completion

24. added `chibi/mime` as pure VFS entry (commit pending)
    - pure MIME parsing: base64, content-type, message folding
25. added `chibi/binary-record` as pure VFS entry (commit pending)
    - binary record type macros, pure scheme
26. added `chibi/memoize` as pure VFS entry (commit pending)
    - in-memory LRU cache works; file-backed errors via shadowed deps
    - future #105 (writable VFS compartment) upgrades automatically
27. added 8 shadow stubs (commits pending):
    - `chibi/stty`, `chibi/term/edit-line` — terminal control
    - `chibi/log`, `chibi/app`, `chibi/config` — application framework + logging
    - `chibi/tar` — tar archives
    - `srfi/193` — command-line info leak
    - `chibi/apropos` — env introspection info leak
28. added `scheme/load` as hand-written shadow (commit pending)
    - re-exports VFS-restricted load from `(tein load)`
29. created #105 (writable VFS compartment) for future progressive gating
30. created #106 (scheme/r5rs shadow, blocked on #97)
31. updated module-inventory.md: appendices A/B, all status markers, summary table
32. closed #92 (vet VFS modules for sandbox safety)
```

**step 2: update "what remains" section**

replace with:

```markdown
## what remains

### ✅ module inventory complete

all chibi-scheme modules are resolved — in VFS, intentionally excluded
(with documented rationale), or tracked in github issues.

**open issues for future work:**
- #97 — sandboxed (scheme eval) + (scheme load) + (scheme repl)
- #105 — writable VFS compartment (progressive gating for filesystem stubs)
- #106 — (scheme r5rs) shadow (blocked on #97)
```

**step 3: update "current state"**

update test count and branch name to reflect final state.

**step 4: close #92**

```bash
gh issue close 92 --comment "closed by module inventory completion on feature/module-inventory-completion-2603. all VFS modules vetted — 229 safe, 10 unsafe, 14 shadow, 24 intentionally excluded with documented rationale."
```

**step 5: run full test suite**

```bash
just test
```

expected: all tests pass

**step 6: run lint**

```bash
just lint
```

**step 7: commit**

```
docs: complete module inventory — close #92

session 6: 3 pure VFS additions, 8 shadow stubs, 1 hand-written shadow.
every chibi-scheme module now resolved: in VFS, excluded with rationale,
or tracked in issue.

closes #92
```

---

## task 13: create PR

**status:** pending

**step 1: create branch** (if not already on feature branch)

```bash
just feature module-inventory-completion-2603
```

**step 2: push + create PR**

target: `dev` (base branch per AGENTS.md)

PR title: `feat: complete module inventory — 3 VFS additions, 9 shadows, closes #92`

PR body should summarise:
- 3 pure VFS additions (mime, binary-record, memoize)
- 8 generated shadow stubs (stty, edit-line, log, app, config, tar, srfi/193, apropos)
- 1 hand-written shadow (scheme/load → tein/load)
- scheme/r5rs deferred (#106)
- all modules documented with rationale (appendices A/B)
- new issues: #105 (writable VFS compartment), #106 (scheme/r5rs)
- closes #92

---

## progress tracker

update this section after each batch:

| batch | tasks | status | notes |
|-------|-------|--------|-------|
| 1 | 1-3 | done | pure VFS: mime, binary-record, memoize |
| 2 | 4-6 | done | shadows: stty, edit-line, log, app, config |
| 3 | 7-8 | done | shadows: tar, srfi/193, apropos |
| 4 | 9-13 | in progress | scheme/load shadow done; docs, close #92, PR remaining |
