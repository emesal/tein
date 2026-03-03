# shadow module stubs — implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** make all OS-touching chibi modules importable in sandboxed contexts with clear error-on-call stubs, plus add chibi/channel as a normal embedded module.

**Architecture:** data-driven build-time generation. `SHADOW_STUBS` array in `vfs_registry.rs` defines exports per module. `build.rs` generates scheme `.sld` strings into `tein_shadow_stubs.rs`. `sandbox.rs` registers them alongside hand-written shadows.

**Tech Stack:** rust (build.rs codegen), scheme (generated `.sld` strings)

**Design doc:** `docs/plans/2026-03-03-shadow-module-stubs-design.md`

---

### task 1: add `ShadowStub` struct + `SHADOW_STUBS` data to `vfs_registry.rs`

**files:**
- modify: `tein/src/vfs_registry.rs`

**step 1: add the struct and array after `VFS_REGISTRY`**

add after the closing `];` of `VFS_REGISTRY` (currently line 2629):

```rust
/// a shadow module whose exports are stubbed with sandbox-denial errors.
///
/// `build.rs` reads this array and generates scheme `.sld` source strings
/// at build time. `register_vfs_shadows()` registers them into the dynamic
/// VFS for sandboxed contexts.
///
/// function exports become `(define (name . args) (error "[sandbox:path] name not available"))`.
/// constant exports become `(define name 0)`.
/// macro exports become `(define-syntax name (syntax-rules () ((_ . args) (error ...))))`.
#[derive(Clone, Debug)]
#[allow(dead_code)]
struct ShadowStub {
    /// module path, e.g. "chibi/filesystem"
    path: &'static str,
    /// function exports — stubbed with variadic error-raising definitions
    fn_exports: &'static [&'static str],
    /// constant exports — stubbed as `(define name 0)`
    const_exports: &'static [&'static str],
    /// macro exports — stubbed with `define-syntax` error-raising rules
    macro_exports: &'static [&'static str],
}

const SHADOW_STUBS: &[ShadowStub] = &[
    // --- C-backed OS modules ---
    ShadowStub {
        path: "chibi/filesystem",
        fn_exports: &[
            "duplicate-file-descriptor", "duplicate-file-descriptor-to",
            "close-file-descriptor", "renumber-file-descriptor",
            "open-input-file-descriptor", "open-output-file-descriptor",
            "delete-file", "link-file", "symbolic-link-file", "rename-file",
            "directory-files", "directory-fold", "directory-fold-tree",
            "delete-file-hierarchy", "delete-directory",
            "create-directory", "create-directory*",
            "current-directory", "change-directory", "with-directory",
            "open", "open-pipe", "make-fifo", "open-output-file/append",
            "read-link",
            "file-status", "file-link-status",
            "file-device", "file-inode", "file-mode", "file-num-links",
            "file-owner", "file-group", "file-represented-device",
            "file-size", "file-block-size", "file-num-blocks",
            "file-access-time", "file-change-time",
            "file-modification-time", "file-modification-time/safe",
            "file-regular?", "file-directory?", "file-character?",
            "file-block?", "file-fifo?", "file-link?", "file-socket?",
            "file-exists?",
            "get-file-descriptor-flags", "set-file-descriptor-flags!",
            "get-file-descriptor-status", "set-file-descriptor-status!",
            "file-lock", "file-truncate",
            "file-is-readable?", "file-is-writable?", "file-is-executable?",
            "chmod", "chown", "is-a-tty?",
        ],
        const_exports: &[
            "open/read", "open/write", "open/read-write",
            "open/create", "open/exclusive", "open/truncate",
            "open/append", "open/non-block",
            "lock/shared", "lock/exclusive", "lock/non-blocking", "lock/unlock",
        ],
        macro_exports: &[],
    },
    ShadowStub {
        path: "chibi/process",
        fn_exports: &[
            "exit", "emergency-exit", "sleep", "alarm",
            "%fork", "fork", "kill", "execute",
            "waitpid", "system", "system?",
            "process-command-line", "process-running?",
            "set-signal-action!",
            "make-signal-set", "signal-set?", "signal-set-contains?",
            "signal-set-fill!", "signal-set-add!", "signal-set-delete!",
            "current-signal-mask", "current-process-id", "parent-process-id",
            "signal-mask-block!", "signal-mask-unblock!", "signal-mask-set!",
            "call-with-process-io",
            "process->bytevector", "process->string", "process->sexp",
            "process->string-list",
            "process->output+error", "process->output+error+status",
        ],
        const_exports: &[
            "signal/hang-up", "signal/interrupt", "signal/quit",
            "signal/illegal", "signal/abort", "signal/fpe",
            "signal/kill", "signal/segv", "signal/pipe",
            "signal/alarm", "signal/term",
            "signal/user1", "signal/user2",
            "signal/child", "signal/continue", "signal/stop",
            "signal/tty-stop", "signal/tty-input", "signal/tty-output",
            "wait/no-hang",
        ],
        macro_exports: &[],
    },
    ShadowStub {
        path: "chibi/system",
        fn_exports: &[
            "get-host-name",
            "user?", "user-name", "user-password", "user-id",
            "user-group-id", "user-gecos", "user-home", "user-shell",
            "user-information",
            "group?", "group-name", "group-password", "group-id",
            "group-information",
            "current-user-id", "current-group-id",
            "current-effective-user-id", "current-effective-group-id",
            "set-current-user-id!", "set-current-effective-user-id!",
            "set-current-group-id!", "set-current-effective-group-id!",
            "current-session-id", "create-session", "set-root-directory!",
        ],
        const_exports: &[],
        macro_exports: &[],
    },
    ShadowStub {
        path: "chibi/net",
        fn_exports: &[
            "sockaddr?", "address-info?",
            "get-address-info", "make-address-info",
            "socket", "connect", "bind", "accept", "listen",
            "open-socket-pair", "get-peer-name",
            "sockaddr-name", "sockaddr-port", "make-sockaddr",
            "with-net-io", "open-net-io", "make-listener-socket",
            "send", "receive!", "receive",
            "send/non-blocking", "receive!/non-blocking", "receive/non-blocking",
            "address-info-family", "address-info-socket-type",
            "address-info-protocol", "address-info-flags",
            "address-info-address", "address-info-address-length",
            "address-info-canonname", "address-info-next",
            "get-socket-option", "set-socket-option!",
        ],
        const_exports: &[
            "address-family/unix", "address-family/inet",
            "address-family/inet6", "address-family/unspecified",
            "socket-type/stream", "socket-type/datagram", "socket-type/raw",
            "ip-proto/ip", "ip-proto/icmp", "ip-proto/tcp", "ip-proto/udp",
            "ai/passive", "ai/canonname", "ai/numeric-host",
            "level/socket",
            "socket-opt/debug", "socket-opt/broadcast",
            "socket-opt/reuseaddr", "socket-opt/keepalive",
            "socket-opt/oobinline", "socket-opt/sndbuf", "socket-opt/rcvbuf",
            "socket-opt/dontroute", "socket-opt/rcvlowat", "socket-opt/sndlowat",
        ],
        macro_exports: &[],
    },
    // --- pure scheme wrappers ---
    ShadowStub {
        path: "chibi/shell",
        fn_exports: &[
            "shell-command", "shell-pipe", "call-with-shell-io",
            "shell-if", "shell-and", "shell-or", "shell-do",
            "in<", "out>", "err>", "out>>", "err>>",
        ],
        const_exports: &[],
        macro_exports: &[
            "shell", "shell&",
            "shell->string", "shell->string-list",
            "shell->sexp", "shell->sexp-list",
            "shell->output&error",
            "><", ">>", "<<",
        ],
    },
    ShadowStub {
        path: "chibi/temp-file",
        fn_exports: &["call-with-temp-file", "call-with-temp-dir"],
        const_exports: &[],
        macro_exports: &[],
    },
    ShadowStub {
        path: "chibi/net/http",
        fn_exports: &[
            "http-get", "http-get/headers", "http-get-to-file",
            "http-head", "http-post", "http-put", "http-delete",
            "call-with-input-url", "call-with-input-url/headers",
            "with-input-from-url",
            "http-parse-request", "http-parse-form",
        ],
        const_exports: &[],
        macro_exports: &[],
    },
    ShadowStub {
        path: "chibi/net/server",
        fn_exports: &["run-net-server", "make-listener-thunk"],
        const_exports: &[],
        macro_exports: &[],
    },
    ShadowStub {
        path: "chibi/net/http-server",
        fn_exports: &[
            "run-http-server",
            "http-chain-servlets", "http-default-servlet",
            "http-wrap-default", "http-file-servlet",
            "http-procedure-servlet", "http-ext-servlet",
            "http-regexp-servlet", "http-path-regexp-servlet",
            "http-uri-regexp-servlet", "http-host-regexp-servlet",
            "http-redirect-servlet", "http-rewrite-servlet",
            "http-cgi-bin-dir-servlet", "http-scheme-script-dir-servlet",
            "http-send-file",
        ],
        const_exports: &[],
        macro_exports: &[],
    },
    ShadowStub {
        path: "chibi/net/server-util",
        fn_exports: &[
            "line-handler", "command-handler", "parse-command",
            "get-host", "file-mime-type",
        ],
        const_exports: &[],
        macro_exports: &[],
    },
    ShadowStub {
        path: "chibi/net/servlet",
        fn_exports: &[
            "upload?", "upload-name", "upload-filename", "upload-headers",
            "upload->string", "upload-input-port", "upload-save",
            "upload->bytevector", "upload->sexp", "upload-binary-input-port",
            "request?", "request-method", "request-host", "request-uploads",
            "request-uri", "request-version", "request-headers",
            "request-body", "request-params", "request-in", "request-out",
            "request-sock", "request-addr",
            "request-param", "request-param-list",
            "request-upload", "request-upload-list",
            "request-uri-string", "request-with-uri", "request-path",
            "request-method-set!", "request-host-set!", "request-uri-set!",
            "request-version-set!", "request-headers-set!",
            "request-body-set!", "request-params-set!",
            "request-in-set!", "request-out-set!",
            "request-sock-set!", "request-addr-set!",
            "copy-request", "make-request", "make-cgi-request",
            "servlet-write", "servlet-write-status", "servlet-respond",
            "servlet-parse-body!", "make-status-servlet",
            "servlet-handler", "servlet-run", "servlet-bad-request",
        ],
        const_exports: &[],
        macro_exports: &[],
    },
];
```

**step 2: add `VfsEntry` entries for each stub module**

add to `VFS_REGISTRY` in their appropriate sections. all have the same shape:

```rust
VfsEntry {
    path: "chibi/filesystem",
    deps: &["scheme/base"],
    files: &[],
    clib: None,
    default_safe: true,
    source: VfsSource::Shadow,
    feature: None,
    shadow_sld: None, // generated from SHADOW_STUBS by build.rs
},
```

placement:
- `chibi/filesystem`, `chibi/process`, `chibi/system` — after `chibi/highlight` in the chibi section
- `chibi/shell`, `chibi/temp-file` — after chibi/system
- `chibi/net`, `chibi/net/http`, `chibi/net/server`, `chibi/net/http-server`, `chibi/net/server-util`, `chibi/net/servlet` — grouped together after chibi/temp-file

**step 3: commit**

```
git add tein/src/vfs_registry.rs
git commit -m "feat: add SHADOW_STUBS data + VfsEntry shells for OS-touching modules"
```

---

### task 2: add `generate_shadow_stubs()` to `build.rs`

**files:**
- modify: `tein/build.rs`

**step 1: write the generation function**

add after `generate_clibs_table()`:

```rust
/// generate `tein_shadow_stubs.rs` — scheme `.sld` strings for shadow stub modules.
///
/// reads `SHADOW_STUBS` (from `vfs_registry.rs` via `include!`) and produces a
/// `GENERATED_SHADOW_SLDS` const array mapping module path → inline `.sld` source.
/// each function export becomes an error-raising variadic stub; each constant
/// export becomes `(define name 0)`; each macro export becomes a `define-syntax`
/// error-raising rule.
fn generate_shadow_stubs(out_dir: &str) {
    let out_path = Path::new(out_dir).join("tein_shadow_stubs.rs");
    let mut out = String::with_capacity(64 * 1024);

    out.push_str("// generated by build.rs — do not edit\n\n");
    out.push_str("const GENERATED_SHADOW_SLDS: &[(&str, &str)] = &[\n");

    for stub in SHADOW_STUBS.iter() {
        let sld = generate_one_stub_sld(stub);
        // escape the sld for embedding in a rust string literal
        let escaped = sld.replace('\\', "\\\\").replace('"', "\\\"");
        out.push_str(&format!("    (\"{}\", \"{}\"),\n", stub.path, escaped));
    }

    out.push_str("];\n");
    fs::write(&out_path, &out).expect("failed to write tein_shadow_stubs.rs");
}

/// generate one scheme `(define-library ...)` string for a shadow stub.
fn generate_one_stub_sld(stub: &ShadowStub) -> String {
    // convert path "chibi/filesystem" → "(chibi filesystem)"
    // and "chibi/net/http" → "(chibi net http)"
    let lib_name = stub.path.replace('/', " ");

    let mut sld = String::with_capacity(4096);
    sld.push_str(&format!("(define-library ({lib_name})\n"));
    sld.push_str("  (import (scheme base))\n");
    sld.push_str("  (export");

    for name in stub.fn_exports.iter() {
        sld.push_str(&format!("\n    {name}"));
    }
    for name in stub.const_exports.iter() {
        sld.push_str(&format!("\n    {name}"));
    }
    for name in stub.macro_exports.iter() {
        sld.push_str(&format!("\n    {name}"));
    }
    sld.push_str(")\n");

    sld.push_str("  (begin\n");

    // constants first
    for name in stub.const_exports.iter() {
        sld.push_str(&format!("    (define {name} 0)\n"));
    }

    // function stubs
    for name in stub.fn_exports.iter() {
        sld.push_str(&format!(
            "    (define ({name} . args) (error \"[sandbox:{}] {} not available\"))\n",
            stub.path, name
        ));
    }

    // macro stubs
    for name in stub.macro_exports.iter() {
        sld.push_str(&format!(
            "    (define-syntax {name} (syntax-rules () ((_ . args) (error \"[sandbox:{}] {} not available\"))))\n",
            stub.path, name
        ));
    }

    sld.push_str("  ))\n");
    sld
}
```

**step 2: call `generate_shadow_stubs()` from `main()`**

find where `generate_vfs_data` and `generate_clibs_table` are called (around line 423) and add:

```rust
generate_shadow_stubs(&out_dir);
```

**step 3: commit**

```
git add tein/build.rs
git commit -m "feat: build.rs generates shadow stub .sld strings from SHADOW_STUBS"
```

---

### task 3: update `register_vfs_shadows()` in `sandbox.rs`

**files:**
- modify: `tein/src/sandbox.rs`

**step 1: include generated stubs and update registration**

add the `include!` after the existing `tein_exports.rs` include (line 258):

```rust
// generated by build.rs — shadow stub .sld strings for OS-touching modules
include!(concat!(env!("OUT_DIR"), "/tein_shadow_stubs.rs"));
```

**step 2: update `register_vfs_shadows()` to handle both hand-written and generated**

replace the function (lines 238-255):

```rust
/// Inject VFS shadow modules for sandboxed contexts.
///
/// Iterates `VFS_REGISTRY` for `VfsSource::Shadow` entries and registers
/// their `.sld` content into the dynamic VFS under canonical `/vfs/lib/`
/// paths. Hand-written shadows use `shadow_sld`; generated stubs (from
/// `SHADOW_STUBS` via build.rs) are looked up in `GENERATED_SHADOW_SLDS`.
///
/// Must be called before the VFS gate is armed (before `VFS_GATE` is set
/// to `GATE_CHECK`).
pub(crate) fn register_vfs_shadows() {
    use std::ffi::CString;

    let register_one = |path: &str, sld: &str| {
        let vfs_path = format!("/vfs/lib/{}.sld", path);
        let c_path = CString::new(vfs_path).expect("valid VFS path");
        unsafe {
            crate::ffi::tein_vfs_register(
                c_path.as_ptr(),
                sld.as_ptr() as *const std::ffi::c_char,
                sld.len() as std::ffi::c_uint,
            );
        }
    };

    for entry in VFS_REGISTRY.iter() {
        if entry.source != VfsSource::Shadow {
            continue;
        }
        if let Some(sld) = entry.shadow_sld {
            // hand-written shadow (scheme/file, scheme/process-context, etc.)
            register_one(entry.path, sld);
        }
        // generated stubs have shadow_sld: None — handled below
    }

    // generated stubs from SHADOW_STUBS (via build.rs)
    for &(path, sld) in GENERATED_SHADOW_SLDS.iter() {
        register_one(path, sld);
    }
}
```

**step 3: commit**

```
git add tein/src/sandbox.rs
git commit -m "feat: register_vfs_shadows handles both hand-written and generated stubs"
```

---

### task 4: add `chibi/channel` as normal embedded module

**files:**
- modify: `tein/src/vfs_registry.rs`

**step 1: add VfsEntry**

add after `chibi/highlight` (or nearby in the chibi section):

```rust
VfsEntry {
    path: "chibi/channel",
    deps: &["srfi/9", "srfi/18"],
    files: &["lib/chibi/channel.sld", "lib/chibi/channel.scm"],
    clib: None,
    default_safe: true,
    source: VfsSource::Embedded,
    feature: None,
    shadow_sld: None,
},
```

**step 2: commit**

```
git add tein/src/vfs_registry.rs
git commit -m "feat: add chibi/channel as embedded module (pure scheme, not OS-touching)"
```

---

### task 5: build and verify

**step 1: build**

```
cargo build
```

expected: success. build.rs generates `tein_shadow_stubs.rs` and compiles cleanly.

**step 2: run tests**

```
just test
```

expected: 724+ tests pass (existing tests unaffected, no new tests yet).

**step 3: verify generated output**

```
cat target/debug/build/tein-*/out/tein_shadow_stubs.rs | head -30
```

expected: `GENERATED_SHADOW_SLDS` array with 11 entries, each containing valid scheme `.sld` source.

---

### task 6: add integration tests

**files:**
- modify: `tein/src/context.rs`

**step 1: write test for shadow stub import + error**

add near the other clib/module tests (around line 7840):

```rust
#[test]
fn test_shadow_stub_chibi_filesystem_raises_error() {
    let ctx = ContextBuilder::new()
        .standard_env()
        .sandboxed(Modules::Safe)
        .allow_module("chibi/filesystem")
        .build()
        .unwrap();
    let result = ctx.evaluate(
        "(import (chibi filesystem)) (create-directory \"/tmp/test\")"
    );
    // stub raises an error containing the sandbox marker
    match result {
        Err(e) => assert!(
            e.to_string().contains("[sandbox:chibi/filesystem]"),
            "expected sandbox error, got: {e}"
        ),
        Ok(v) => panic!("expected error, got: {v:?}"),
    }
}

#[test]
fn test_shadow_stub_constants_are_zero() {
    let ctx = ContextBuilder::new()
        .standard_env()
        .sandboxed(Modules::Safe)
        .allow_module("chibi/filesystem")
        .build()
        .unwrap();
    let result = ctx.evaluate(
        "(import (chibi filesystem)) open/read"
    ).unwrap();
    assert_eq!(result, Value::Integer(0));
}

#[test]
fn test_shadow_stub_chibi_shell_macro_raises_error() {
    let ctx = ContextBuilder::new()
        .standard_env()
        .sandboxed(Modules::Safe)
        .allow_module("chibi/shell")
        .build()
        .unwrap();
    let result = ctx.evaluate(
        "(import (chibi shell)) (shell \"echo hello\")"
    );
    match result {
        Err(e) => assert!(
            e.to_string().contains("[sandbox:chibi/shell]"),
            "expected sandbox error, got: {e}"
        ),
        Ok(v) => panic!("expected error, got: {v:?}"),
    }
}

#[test]
fn test_chibi_channel_loads_in_sandbox() {
    let ctx = ContextBuilder::new()
        .standard_env()
        .sandboxed(Modules::Safe)
        .allow_module("chibi/channel")
        .build()
        .unwrap();
    let result = ctx.evaluate(
        "(import (chibi channel)) (channel? (make-channel))"
    ).unwrap();
    assert_eq!(result, Value::Boolean(true));
}
```

**step 2: run tests**

```
just test
```

expected: 728+ tests pass (724 existing + 4 new).

**step 3: commit**

```
git add tein/src/context.rs
git commit -m "test: shadow stub integration tests — import, error, constants, macros, channel"
```

---

### task 7: lint + final verification

**step 1: lint**

```
just lint
```

expected: clean.

**step 2: update handoff doc**

modify `docs/handoff-module-inventory.md`:
- move shadow modules from "what remains" to "what has been done" (session 3)
- note stub vs gated status for each
- update test count
- add remaining work: progressive gating (tier 3 priority list)

**step 3: commit**

```
git add docs/handoff-module-inventory.md
git commit -m "docs: update handoff — shadow stubs complete, document gating roadmap"
```

---

### task 8: update module-inventory.md statuses

**files:**
- modify: `docs/module-inventory.md`

update the status markers for all 11 stub modules + chibi/channel from their current markers to `✅` (or whatever marker indicates "in VFS"). note in each entry that it's a sandbox stub (phase 1).

**step 1: update entries and commit**

```
git add docs/module-inventory.md
git commit -m "docs: update module inventory — shadow stubs + chibi/channel marked done"
```

---

## notes for the implementer

- `vfs_registry.rs` is `include!`'d by both `build.rs` and `sandbox.rs`. both contexts need access to `ShadowStub` and `SHADOW_STUBS`. mark with `#[allow(dead_code)]` as needed (same pattern as existing `ClibEntry`).
- the `generate_one_stub_sld()` function must produce valid scheme. test by eyeballing the generated output at `target/debug/build/tein-*/out/tein_shadow_stubs.rs`.
- `chibi/shell` macros like `><`, `>>`, `<<` are auxiliary syntax — the `define-syntax` stub with `syntax-rules` handles these fine since any use expands to an error.
- existing hand-written shadows (`scheme/file`, `scheme/repl`, `scheme/process-context`, `srfi/98`) are unchanged — they keep their `shadow_sld: Some(...)` and are registered via the existing path in `register_vfs_shadows()`.
- `chibi/net` exports type names `sockaddr` and `addrinfo` (C struct descriptors from include-shared). these are omitted from the stub — they're not directly callable and code won't reference them as values.
