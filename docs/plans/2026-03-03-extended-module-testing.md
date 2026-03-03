# extended module testing implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** add comprehensive scheme-level test coverage for all VFS-registered modules by wiring chibi-scheme's bundled `(srfi N test)` / `(chibi X-test)` suites into cargo test, plus hand-written `.scm` files for gaps.

**Architecture:** add `(chibi test)` + all applicable `(srfi N test)` and `(chibi X-test)` modules to `vfs_registry.rs`; create `tests/vfs_module_tests.rs` with one `#[test]` per module that installs a raising applier then calls `(run-tests)`; hand-write targeted `.scm` files for modules without chibi coverage.

**Tech Stack:** rust (cargo test), chibi-scheme scheme, existing `tein::Context` API, `tein/src/vfs_registry.rs` pattern.

**Design doc:** `docs/plans/2026-03-03-extended-module-testing-design.md`

---

## execution status (as of 2026-03-03 context break)

**completed:**
- ✅ task 1: `(chibi test)` added to VFS registry (commit `220090a`)
- ✅ task 2: `scheme/bytevector-test` + all `srfi/N/test` entries added (commit `a0ae862`)
- ✅ task 3: all `chibi/X-test` entries added (commit `b7bba15`)
- 🔴 task 4: `tests/vfs_module_tests.rs` created but blocked — `(chibi test)` fails to load; WIP commit `28f4898`

**tasks 5–9:** pending

**branch:** `feature/extended-module-testing-2603`

---

## blocking issue: `(chibi test)` load fails — root cause & fix required

### root cause

`(chibi test)` → `(scheme time)` → `(include-shared "time")` (clib) raises `EvalError("")` in any context tein can construct. additionally, `(chibi test)` → `(scheme process-context)` shadow previously failed because shadow SLDs used `(import (tein X))` inside library body — fixed in WIP commit but `scheme/time` is the remaining blocker.

**chain:**
1. `(chibi test)` → `(scheme process-context)` [shadow, fixed] → `(scheme time)` [clib, broken]
2. `scheme/time.sld` does `(include-shared "time")` → calls `sexp_init_lib_scheme_time` → raises empty exception in tein context

### what was tried
- `Context::new_standard()` — no shadows registered, `scheme/process-context` not found
- `Context::builder().standard_env().sandboxed(Modules::All)` — shadows registered, but `(tein process)` library export problem in null env; then `scheme/time` clib fails
- `Context::builder().standard_env().with_vfs_shadows()` (new builder option added in WIP) — shadows registered, `scheme/process-context` shadow fixed, but `scheme/time` still raises empty EvalError
- shadow SLD fix: replaced `(import (tein process))` / `(import (tein file))` in shadow bodies with `(import (scheme base))` + `(define x x)` captures. fixes the process-context shadow but doesn't help scheme/time

### what was added in WIP
- `ContextBuilder::with_vfs_shadows()` field + method in `context.rs` — registers VFS shadows without sandboxing
- `vfs_registry.rs` shadow SLD fixes:
  - `scheme/process-context`: replaced `(import (tein process))` with `(import (scheme base))` + explicit `(define exit exit)` etc.
  - `scheme/file`: same pattern — `(import (scheme base))` + `(define file-exists? file-exists?)` etc.
- `tests/vfs_module_tests.rs` created with RAISING_APPLIER + all 63 test functions

### next step for task 4

investigate why `(scheme time)` clib raises `EvalError("")` in a `new_standard()` + `with_vfs_shadows()` context. specifically:

1. check `scheme/time.c` `sexp_init_lib_scheme_time` — does it call `(get-environment-variable ...)` or read files at init time?
2. check if `scheme/time` needs the actual leap second list file from the filesystem
3. consider: `scheme/time` may simply not be needed for `(chibi test)` applier functionality — the relevant exports are `current-test-applier`, `current-test-comparator`, etc. which don't depend on time. so: can we skip `(scheme time)` entirely by pre-loading it from `(tein time)` which IS available?

**proposed fix**: before `(import (chibi test))`, pre-import `(tein time)` which provides `current-second`, `current-jiffy`, `jiffies-per-second` (the same things `scheme/time` provides). then register a stub `scheme/time` VFS shadow that just re-exports from tein/time. add to `vfs_registry.rs`:

```rust
// scheme/time shadow for chibi/test context — re-exports tein/time trampolines
// avoids loading scheme/time.c which has filesystem deps at init time
VfsEntry {
    path: "scheme/time",
    // ... but wait: scheme/time is already VfsSource::Embedded (not Shadow)
    // cannot have two entries with same path
```

actually this won't work — `scheme/time` already has an Embedded entry with a clib. adding a Shadow entry would conflict.

**alternative**: look at the actual `sexp_init_lib_scheme_time` to understand what fails, then fix it OR pre-import `(scheme time)` differently. the `scheme/time` clib `sexp_init_lib_scheme_time` is compiled from `lib/scheme/time.c`. check that file for what it does at init — if it only exports C functions and doesn't call into scheme at init, the failure is elsewhere.

**most likely diagnosis**: `scheme/time.sld` does `(import (scheme process-context))` then uses `get-environment-variable`. after our shadow fix, `scheme/process-context` works. but `scheme/time` also does `(import (chibi))` — in `new_standard()` non-sandboxed context, `(chibi)` resolves to the eval env and provides various chibi-specific procs. the clib `(include-shared "time")` provides `current-clock-second` and `jiffies-per-second` from C. if the clib init itself fails (empty EvalError), maybe it's a GC issue during clib loading. try running with `cargo test --features debug-chibi` to get GC instrumentation output.

**simplest working alternative**: use `(tein time)` to provide time procs, and avoid loading `(scheme time)` in the harness. need to check if `(chibi test)` actually USES any time function or just needs the import to succeed at module load time. if scheme/time is only used by chibi/test's `test-exit` (which calls `(exit)`) and not by the applier we install, we could pre-define `current-jiffy` and `jiffies-per-second` from tein/time and skip the real scheme/time.

---

## reference: VfsEntry pattern

every entry in `VFS_REGISTRY` looks like this (copy from any existing entry):

```rust
VfsEntry {
    path: "chibi/test",
    deps: &["scheme/base", "scheme/case-lambda", "scheme/write",
            "scheme/complex", "scheme/process-context", "scheme/time",
            "chibi/diff", "chibi/term/ansi", "chibi/optional"],
    files: &["lib/chibi/test.sld", "lib/chibi/test.scm"],
    clib: None,
    default_safe: false,
    source: VfsSource::Embedded,
    feature: None,
    shadow_sld: None,
},
```

`default_safe: false` for all test modules. `clib: None` for all (pure scheme). `source: VfsSource::Embedded`. `feature: None`. `shadow_sld: None`.

## reference: test harness pattern

the rust test runner in `tests/vfs_module_tests.rs`:

```rust
use tein::Context;

/// applier that raises immediately on failure instead of incrementing a counter.
const RAISING_APPLIER: &str = r#"
(import (chibi test))
(current-test-applier
  (lambda (expect expr info)
    (let* ((expected (guard (exn (#t (cons 'exception exn))) (expect)))
           (result   (guard (exn (#t (cons 'exception exn))) (expr)))
           (pass?    (if (assq-ref info 'assertion)
                         result
                         ((current-test-comparator) expected result))))
      (unless pass?
        (error (string-append "FAIL: " (or (assq-ref info 'name) "?"))
               'expected expected 'got result)))))
"#;

fn run_chibi_test(import: &str) {
    let ctx = Context::new_standard().expect("context");
    ctx.evaluate(RAISING_APPLIER).expect("applier setup");
    ctx.evaluate(&format!("(import {})", import)).expect("import");
    ctx.evaluate("(run-tests)").expect("run-tests");
}

#[test]
fn test_srfi_1_list() {
    run_chibi_test("(srfi 1 test)");
}
```

## reference: modules to exclude

- `chibi/regexp-test` — imports `(chibi regexp pcre)` which is not in VFS
- `chibi/crypto/*-test`, `chibi/mime-test`, `chibi/memoize-test` — filesystem/network deps
- `chibi/filesystem-test`, `chibi/process-test`, `chibi/system-test` — OS-level
- `srfi/179/test`, `srfi/231/test` — large array tests with fuel concerns; add later

## reference: modules that import `(chibi)` directly

these test modules do `(import (chibi))` — chibi's core. in tein's standard context this resolves to the base eval environment and provides `er-macro-transformer`, `pair-source`, `print-exception`. these work fine in `Context::new_standard()`.

affected: `srfi/1/test`, `srfi/2/test`, `srfi/16/test`, `srfi/18/test`, `srfi/26/test`, `srfi/38/test`, `srfi/69/test`, `srfi/95/test`, `srfi/99/test`, `srfi/211/test`, `srfi/219/test`, `chibi/assert-test`, `chibi/generic-test`, `chibi/io-test`, `chibi/loop-test`, `chibi/weak-test`

for VfsEntry deps, list the relevant deps explicitly — no need to add `(chibi)` as a dep since it resolves to the eval env.

---

## task 1: add `(chibi test)` to VFS

**files:**
- modify: `tein/src/vfs_registry.rs` (append before closing `];` of `VFS_REGISTRY`)

**step 1:** find the closing `];` of `VFS_REGISTRY`. it's after the `chibi/uri` entry (around line 3183). insert a new `VfsEntry` block for `(chibi test)` just before `];`:

```rust
    VfsEntry {
        path: "chibi/test",
        deps: &[
            "scheme/base",
            "scheme/case-lambda",
            "scheme/write",
            "scheme/complex",
            "scheme/process-context",
            "scheme/time",
            "chibi/diff",
            "chibi/term/ansi",
            "chibi/optional",
            "srfi/130",   // string-contains fallback
        ],
        files: &["lib/chibi/test.sld", "lib/chibi/test.scm"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
```

**step 2:** build to confirm the new entry compiles and chibi/test is embedded:

```bash
cd ~/projects/tein && cargo build 2>&1 | tail -5
```

expected: `Compiling tein ...` then `Finished`. no errors.

**step 3:** smoke-test that `(chibi test)` loads in a standard context:

```bash
cargo test --lib -- --nocapture 2>&1 | grep -E "FAILED|error" | head -10
```

expected: no new failures.

**step 4:** commit:

```bash
git add tein/src/vfs_registry.rs
git commit -m "feat: add (chibi test) to VFS registry"
```

---

## task 2: add `(scheme bytevector-test)` and all `(srfi N test)` entries

**files:**
- modify: `tein/src/vfs_registry.rs`

**step 1:** append the following entries after the `chibi/test` entry (still before `];`). these are all pure-scheme, `default_safe: false`:

```rust
    // ── test suites ───────────────────────────────────────────────────────────
    VfsEntry {
        path: "scheme/bytevector-test",
        deps: &["scheme/base", "scheme/bytevector", "scheme/list", "chibi/test"],
        files: &["lib/scheme/bytevector-test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/1/test",
        deps: &["scheme/base", "srfi/1", "chibi/test"],
        files: &["lib/srfi/1/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/2/test",
        deps: &["scheme/base", "srfi/2", "chibi/test"],
        files: &["lib/srfi/2/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/14/test",
        deps: &["scheme/base", "srfi/14", "chibi/test"],
        files: &["lib/srfi/14/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/16/test",
        deps: &["scheme/base", "srfi/16", "chibi/test"],
        files: &["lib/srfi/16/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/18/test",
        deps: &["scheme/base", "srfi/18", "srfi/39", "chibi/test"],
        files: &["lib/srfi/18/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/26/test",
        deps: &["scheme/base", "srfi/26", "chibi/test"],
        files: &["lib/srfi/26/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/27/test",
        deps: &["scheme/base", "srfi/27", "chibi/test"],
        files: &["lib/srfi/27/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/33/test",
        deps: &["scheme/base", "srfi/33", "chibi/test"],
        files: &["lib/srfi/33/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/35/test",
        deps: &["scheme/base", "srfi/35", "chibi/test"],
        files: &["lib/srfi/35/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/38/test",
        deps: &["scheme/base", "srfi/38", "chibi/test"],
        files: &["lib/srfi/38/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/41/test",
        deps: &["scheme/base", "srfi/41", "chibi/test"],
        files: &["lib/srfi/41/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/69/test",
        deps: &["scheme/base", "srfi/69", "chibi/test"],
        files: &["lib/srfi/69/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/95/test",
        deps: &["scheme/base", "srfi/95", "chibi/test"],
        files: &["lib/srfi/95/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/99/test",
        deps: &["scheme/base", "srfi/99", "chibi/test"],
        files: &["lib/srfi/99/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/101/test",
        deps: &["scheme/base", "srfi/101", "chibi/test"],
        files: &["lib/srfi/101/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/113/test",
        deps: &["scheme/base", "srfi/113", "chibi/test"],
        files: &["lib/srfi/113/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/116/test",
        deps: &["scheme/base", "srfi/116", "chibi/test"],
        files: &["lib/srfi/116/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/117/test",
        deps: &["scheme/base", "srfi/117", "chibi/test"],
        files: &["lib/srfi/117/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/121/test",
        deps: &["scheme/base", "srfi/121", "chibi/test"],
        files: &["lib/srfi/121/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/125/test",
        deps: &["scheme/base", "srfi/125", "chibi/test"],
        files: &["lib/srfi/125/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/127/test",
        deps: &["scheme/base", "srfi/127", "chibi/test"],
        files: &["lib/srfi/127/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/128/test",
        deps: &["scheme/base", "srfi/128", "chibi/test"],
        files: &["lib/srfi/128/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/129/test",
        deps: &["scheme/base", "srfi/129", "chibi/test"],
        files: &["lib/srfi/129/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/130/test",
        deps: &["scheme/base", "srfi/130", "chibi/test"],
        files: &["lib/srfi/130/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/132/test",
        deps: &["scheme/base", "srfi/132", "chibi/test"],
        files: &["lib/srfi/132/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/133/test",
        deps: &["scheme/base", "srfi/133", "chibi/test"],
        files: &["lib/srfi/133/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/134/test",
        deps: &["scheme/base", "srfi/134", "chibi/test"],
        files: &["lib/srfi/134/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/135/test",
        deps: &["scheme/base", "srfi/135", "chibi/test"],
        files: &["lib/srfi/135/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/139/test",
        deps: &["scheme/base", "srfi/139", "chibi/test"],
        files: &["lib/srfi/139/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/143/test",
        deps: &["scheme/base", "srfi/143", "chibi/test"],
        files: &["lib/srfi/143/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/144/test",
        deps: &["scheme/base", "srfi/144", "chibi/test"],
        files: &["lib/srfi/144/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/146/test",
        deps: &["scheme/base", "srfi/146", "chibi/test"],
        files: &["lib/srfi/146/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/151/test",
        deps: &["scheme/base", "srfi/151", "chibi/test"],
        files: &["lib/srfi/151/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/158/test",
        deps: &["scheme/base", "srfi/158", "chibi/test"],
        files: &["lib/srfi/158/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/160/test",
        deps: &[
            "scheme/base",
            "srfi/160/base", "srfi/160/u32", "srfi/160/u64", "srfi/160/s64",
            "chibi/test",
        ],
        files: &["lib/srfi/160/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/166/test",
        deps: &["scheme/base", "srfi/166", "chibi/test"],
        files: &["lib/srfi/166/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/211/test",
        deps: &[
            "scheme/base",
            "srfi/211/variable-transformer",
            "srfi/211/identifier-syntax",
            "chibi/test",
        ],
        files: &["lib/srfi/211/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/219/test",
        deps: &["scheme/base", "srfi/219", "chibi/test"],
        files: &["lib/srfi/219/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "srfi/229/test",
        deps: &["scheme/base", "srfi/229", "chibi/test"],
        files: &["lib/srfi/229/test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
```

**step 2:** build and verify no compile errors:

```bash
cd ~/projects/tein && cargo build 2>&1 | tail -5
```

**step 3:** commit:

```bash
git add tein/src/vfs_registry.rs
git commit -m "feat: add scheme/bytevector-test + srfi/N/test suites to VFS"
```

---

## task 3: add `(chibi X-test)` entries

**files:**
- modify: `tein/src/vfs_registry.rs`

**step 1:** append the following entries after the srfi test entries (before `];`):

```rust
    VfsEntry {
        path: "chibi/assert-test",
        deps: &["scheme/base", "chibi/assert", "chibi/test"],
        files: &["lib/chibi/assert-test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "chibi/base64-test",
        deps: &["scheme/base", "chibi/base64", "chibi/string", "chibi/test"],
        files: &["lib/chibi/base64-test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "chibi/binary-record-test",
        deps: &["scheme/base", "chibi/binary-record", "chibi/test"],
        files: &["lib/chibi/binary-record-test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "chibi/bytevector-test",
        deps: &["scheme/base", "chibi/bytevector", "chibi/test"],
        files: &["lib/chibi/bytevector-test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "chibi/csv-test",
        deps: &["scheme/base", "srfi/227", "chibi/csv", "chibi/test"],
        files: &["lib/chibi/csv-test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "chibi/diff-test",
        deps: &["scheme/base", "chibi/diff", "chibi/test"],
        files: &["lib/chibi/diff-test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "chibi/edit-distance-test",
        deps: &["scheme/base", "chibi/edit-distance", "chibi/test"],
        files: &["lib/chibi/edit-distance-test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "chibi/generic-test",
        deps: &["scheme/base", "chibi/generic", "chibi/test"],
        files: &["lib/chibi/generic-test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "chibi/io-test",
        deps: &["scheme/base", "chibi/io", "chibi/test"],
        files: &["lib/chibi/io-test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "chibi/iset-test",
        deps: &["scheme/base", "scheme/write", "srfi/1", "chibi/iset", "chibi/iset/optimize", "chibi/test"],
        files: &["lib/chibi/iset-test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "chibi/loop-test",
        deps: &["scheme/base", "chibi/loop", "chibi/test"],
        files: &["lib/chibi/loop-test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "chibi/match-test",
        deps: &["scheme/base", "chibi/match", "chibi/test"],
        files: &["lib/chibi/match-test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "chibi/math/prime-test",
        deps: &["scheme/base", "chibi/math/prime", "chibi/test"],
        files: &["lib/chibi/math/prime-test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "chibi/optional-test",
        deps: &["scheme/base", "chibi/optional", "chibi/test"],
        files: &["lib/chibi/optional-test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "chibi/parse-test",
        deps: &[
            "scheme/base", "scheme/char",
            "chibi/parse", "chibi/parse/common",
            "chibi/char-set", "chibi/char-set/ascii",
            "chibi/test",
        ],
        files: &["lib/chibi/parse-test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "chibi/pathname-test",
        deps: &["scheme/base", "chibi/pathname", "chibi/test"],
        files: &["lib/chibi/pathname-test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "chibi/quoted-printable-test",
        deps: &["scheme/base", "chibi/quoted-printable", "chibi/string", "chibi/test"],
        files: &["lib/chibi/quoted-printable-test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "chibi/string-test",
        deps: &["scheme/base", "scheme/char", "chibi/string", "chibi/test"],
        files: &["lib/chibi/string-test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "chibi/sxml-test",
        deps: &["scheme/base", "chibi/sxml", "chibi/test"],
        files: &["lib/chibi/sxml-test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "chibi/syntax-case-test",
        deps: &["scheme/base", "chibi/syntax-case", "chibi/test"],
        files: &["lib/chibi/syntax-case-test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "chibi/text-test",
        deps: &["scheme/base", "scheme/write", "chibi/text", "chibi/test"],
        files: &["lib/chibi/text-test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "chibi/uri-test",
        deps: &["scheme/base", "chibi/uri", "chibi/test"],
        files: &["lib/chibi/uri-test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
    VfsEntry {
        path: "chibi/weak-test",
        deps: &["scheme/base", "chibi/weak", "chibi/ast", "chibi/test"],
        files: &["lib/chibi/weak-test.sld"],
        clib: None,
        default_safe: false,
        source: VfsSource::Embedded,
        feature: None,
        shadow_sld: None,
    },
```

**step 2:** build:

```bash
cd ~/projects/tein && cargo build 2>&1 | tail -5
```

**step 3:** commit:

```bash
git add tein/src/vfs_registry.rs
git commit -m "feat: add chibi/X-test suites to VFS registry"
```

---

## task 4: fix `(chibi test)` load + verify test harness [PARTIALLY DONE — BLOCKED]

**status:** `tests/vfs_module_tests.rs` exists with all 63 test functions (WIP commit `28f4898`). `run_chibi_test` uses `Context::builder().standard_env().with_vfs_shadows().build()`. blocked on `(scheme time)` load failure.

**files:**
- modify: `tein/tests/vfs_module_tests.rs` (update `run_chibi_test` once fixed)
- possibly modify: `tein/src/vfs_registry.rs` (scheme/time shadow or tein/time dep fix)
- possibly modify: `tein/src/context.rs` (if with_vfs_shadows needs adjustment)

**step 1:** diagnose `(scheme time)` failure. run:

```bash
cd ~/projects/tein
# check what sexp_init_lib_scheme_time does at init vs. what chibi test actually uses from scheme/time
grep -n "current-jiffy\|jiffies-per-second\|current-second\|get-environment-variable" \
    target/chibi-scheme/lib/scheme/time.c | head -30
# check if (tein time) feature provides the same exports
cargo test --lib -- --nocapture test_scheme_time 2>&1 | head -20
```

**step 2:** if `scheme/time` at init time calls `get-environment-variable` from C (not scheme):
check if this is the actual crash — pre-evaluating `(import (tein time))` before `(import (chibi test))` may make `current-jiffy` / `jiffies-per-second` available so `scheme/time` doesn't need to fully init. try adding to `run_chibi_test`:

```rust
ctx.evaluate("(import (tein time))").expect("tein/time pre-load");
```

**step 3:** if that doesn't work, check `scheme/time.c`'s `sexp_init_lib_scheme_time` — specifically whether it reads the leap second list file. if yes, run tests with filesystem access or provide a stub file.

**step 4:** alternative — if `(chibi test)` doesn't actually CALL any scheme/time proc during our test (only at `test-exit` time), we can pre-define stubs:

```rust
// before (import (chibi test)), define time stubs from tein/time
ctx.evaluate("(import (tein time))").ok();
// register scheme/time as a thin wrapper around tein/time
ctx.evaluate("(define current-second (tein-current-second))").ok(); // or similar
```

**step 5:** once `(import (chibi test))` succeeds, run the full test file:

```bash
cd ~/projects/tein && cargo test -p tein --test vfs_module_tests 2>&1 | tail -30
```

fix any individual test failures (likely dep issues in specific srfi entries).

**step 6:** commit (squashing WIP):

```bash
git add tein/tests/vfs_module_tests.rs tein/src/vfs_registry.rs tein/src/context.rs
git commit -m "feat: vfs_module_tests harness + (chibi test) load fix + srfi test suite integration"
```

---

## task 4 ORIGINAL (preserved for reference): create `tests/vfs_module_tests.rs` — srfi tests

**files:**
- create: `tein/tests/vfs_module_tests.rs`

**step 1:** create the file with the harness and all srfi test functions:

```rust
//! scheme-level integration tests using chibi-scheme's bundled srfi and
//! library test suites. each test imports `(chibi test)` with a custom
//! applier that raises immediately on failure, giving cargo test clean
//! abort-on-first-fail semantics with the failing assertion name.

use tein::Context;

/// installs a raising applier into the current context's `(chibi test)`.
/// must be evaluated before importing any `(srfi N test)` or `(chibi X-test)` module.
const RAISING_APPLIER: &str = r#"
(import (chibi test))
(current-test-applier
  (lambda (expect expr info)
    (let* ((expected (guard (exn (#t (cons 'exception exn))) (expect)))
           (result   (guard (exn (#t (cons 'exception exn))) (expr)))
           (pass?    (if (assq-ref info 'assertion)
                         result
                         ((current-test-comparator) expected result))))
      (unless pass?
        (error (string-append "FAIL: " (or (assq-ref info 'name) "?"))
               'expected expected 'got result)))))
"#;

fn run_chibi_test(import: &str) {
    let ctx = Context::new_standard().expect("context");
    ctx.evaluate(RAISING_APPLIER).expect("applier setup");
    ctx.evaluate(&format!("(import {})", import))
        .expect("import test module");
    ctx.evaluate("(run-tests)").expect("run-tests");
}

// ── srfi test suites ─────────────────────────────────────────────────────────

#[test]
fn test_scheme_bytevector() {
    run_chibi_test("(scheme bytevector-test)");
}

#[test]
fn test_srfi_1_list() {
    run_chibi_test("(srfi 1 test)");
}

#[test]
fn test_srfi_2_and_let_star() {
    run_chibi_test("(srfi 2 test)");
}

#[test]
fn test_srfi_14_char_sets() {
    run_chibi_test("(srfi 14 test)");
}

#[test]
fn test_srfi_16_case_lambda() {
    run_chibi_test("(srfi 16 test)");
}

#[test]
fn test_srfi_18_threads() {
    run_chibi_test("(srfi 18 test)");
}

#[test]
fn test_srfi_26_cut() {
    run_chibi_test("(srfi 26 test)");
}

#[test]
fn test_srfi_27_random() {
    run_chibi_test("(srfi 27 test)");
}

#[test]
fn test_srfi_33_bitwise() {
    run_chibi_test("(srfi 33 test)");
}

#[test]
fn test_srfi_35_conditions() {
    run_chibi_test("(srfi 35 test)");
}

#[test]
fn test_srfi_38_write_read() {
    run_chibi_test("(srfi 38 test)");
}

#[test]
fn test_srfi_41_streams() {
    run_chibi_test("(srfi 41 test)");
}

#[test]
fn test_srfi_69_hash_tables() {
    run_chibi_test("(srfi 69 test)");
}

#[test]
fn test_srfi_95_sorting() {
    run_chibi_test("(srfi 95 test)");
}

#[test]
fn test_srfi_99_records() {
    run_chibi_test("(srfi 99 test)");
}

#[test]
fn test_srfi_101_random_access_lists() {
    run_chibi_test("(srfi 101 test)");
}

#[test]
fn test_srfi_113_sets() {
    run_chibi_test("(srfi 113 test)");
}

#[test]
fn test_srfi_116_immutable_lists() {
    run_chibi_test("(srfi 116 test)");
}

#[test]
fn test_srfi_117_list_queues() {
    run_chibi_test("(srfi 117 test)");
}

#[test]
fn test_srfi_121_generators() {
    run_chibi_test("(srfi 121 test)");
}

#[test]
fn test_srfi_125_hash_tables() {
    run_chibi_test("(srfi 125 test)");
}

#[test]
fn test_srfi_127_lseq() {
    run_chibi_test("(srfi 127 test)");
}

#[test]
fn test_srfi_128_comparators() {
    run_chibi_test("(srfi 128 test)");
}

#[test]
fn test_srfi_129_titlecase() {
    run_chibi_test("(srfi 129 test)");
}

#[test]
fn test_srfi_130_string_cursors() {
    run_chibi_test("(srfi 130 test)");
}

#[test]
fn test_srfi_132_sorting() {
    run_chibi_test("(srfi 132 test)");
}

#[test]
fn test_srfi_133_vectors() {
    run_chibi_test("(srfi 133 test)");
}

#[test]
fn test_srfi_134_ideque() {
    run_chibi_test("(srfi 134 test)");
}

#[test]
fn test_srfi_135_texts() {
    run_chibi_test("(srfi 135 test)");
}

#[test]
fn test_srfi_139_syntax_parameters() {
    run_chibi_test("(srfi 139 test)");
}

#[test]
fn test_srfi_143_fixnums() {
    run_chibi_test("(srfi 143 test)");
}

#[test]
fn test_srfi_144_flonums() {
    run_chibi_test("(srfi 144 test)");
}

#[test]
fn test_srfi_146_mappings() {
    run_chibi_test("(srfi 146 test)");
}

#[test]
fn test_srfi_151_bitwise() {
    run_chibi_test("(srfi 151 test)");
}

#[test]
fn test_srfi_158_generators() {
    run_chibi_test("(srfi 158 test)");
}

#[test]
fn test_srfi_160_uniform_vectors() {
    run_chibi_test("(srfi 160 test)");
}

#[test]
fn test_srfi_166_formatting() {
    run_chibi_test("(srfi 166 test)");
}

#[test]
fn test_srfi_211_syntax_transformers() {
    run_chibi_test("(srfi 211 test)");
}

#[test]
fn test_srfi_219_define_record_type() {
    run_chibi_test("(srfi 219 test)");
}

#[test]
fn test_srfi_229_tagged_procedures() {
    run_chibi_test("(srfi 229 test)");
}
```

**step 2:** run just the srfi tests to catch any module load failures early:

```bash
cd ~/projects/tein && cargo test -p tein --test vfs_module_tests 2>&1 | tail -30
```

expected: most pass; note any that fail with `EvalError` — these indicate missing deps or import issues to fix (dep list in task 2 may need adjusting). fix any dep issues in `vfs_registry.rs` and re-run before committing.

**step 3:** commit:

```bash
git add tein/tests/vfs_module_tests.rs tein/src/vfs_registry.rs
git commit -m "test: vfs_module_tests harness + srfi test suite integration"
```

---

## task 5: add chibi test functions to `vfs_module_tests.rs`

**files:**
- modify: `tein/tests/vfs_module_tests.rs`

**step 1:** append the chibi test functions after the srfi block:

```rust
// ── chibi library test suites ─────────────────────────────────────────────────

#[test]
fn test_chibi_assert() {
    run_chibi_test("(chibi assert-test)");
}

#[test]
fn test_chibi_base64() {
    run_chibi_test("(chibi base64-test)");
}

#[test]
fn test_chibi_binary_record() {
    run_chibi_test("(chibi binary-record-test)");
}

#[test]
fn test_chibi_bytevector() {
    run_chibi_test("(chibi bytevector-test)");
}

#[test]
fn test_chibi_csv() {
    run_chibi_test("(chibi csv-test)");
}

#[test]
fn test_chibi_diff() {
    run_chibi_test("(chibi diff-test)");
}

#[test]
fn test_chibi_edit_distance() {
    run_chibi_test("(chibi edit-distance-test)");
}

#[test]
fn test_chibi_generic() {
    run_chibi_test("(chibi generic-test)");
}

#[test]
fn test_chibi_io() {
    run_chibi_test("(chibi io-test)");
}

#[test]
fn test_chibi_iset() {
    run_chibi_test("(chibi iset-test)");
}

#[test]
fn test_chibi_loop() {
    run_chibi_test("(chibi loop-test)");
}

#[test]
fn test_chibi_match() {
    run_chibi_test("(chibi match-test)");
}

#[test]
fn test_chibi_math_prime() {
    run_chibi_test("(chibi math/prime-test)");
}

#[test]
fn test_chibi_optional() {
    run_chibi_test("(chibi optional-test)");
}

#[test]
fn test_chibi_parse() {
    run_chibi_test("(chibi parse-test)");
}

#[test]
fn test_chibi_pathname() {
    run_chibi_test("(chibi pathname-test)");
}

#[test]
fn test_chibi_quoted_printable() {
    run_chibi_test("(chibi quoted-printable-test)");
}

#[test]
fn test_chibi_string() {
    run_chibi_test("(chibi string-test)");
}

#[test]
fn test_chibi_sxml() {
    run_chibi_test("(chibi sxml-test)");
}

#[test]
fn test_chibi_syntax_case() {
    run_chibi_test("(chibi syntax-case-test)");
}

#[test]
fn test_chibi_text() {
    run_chibi_test("(chibi text-test)");
}

#[test]
fn test_chibi_uri() {
    run_chibi_test("(chibi uri-test)");
}

#[test]
fn test_chibi_weak() {
    run_chibi_test("(chibi weak-test)");
}
```

**step 2:** run the chibi tests:

```bash
cd ~/projects/tein && cargo test -p tein --test vfs_module_tests chibi 2>&1 | tail -30
```

fix any dep issues in `vfs_registry.rs` as needed.

**step 3:** run the full new test file:

```bash
cd ~/projects/tein && cargo test -p tein --test vfs_module_tests 2>&1 | tail -20
```

**step 4:** commit:

```bash
git add tein/tests/vfs_module_tests.rs tein/src/vfs_registry.rs
git commit -m "test: chibi library test suite integration"
```

---

## task 6: hand-written scheme tests — `scheme/char`, `scheme/division`, `scheme/fixnum`, `scheme/bitwise`

**files:**
- create: `tein/tests/scheme/scheme_char.scm`
- create: `tein/tests/scheme/scheme_division.scm`
- create: `tein/tests/scheme/scheme_fixnum.scm`
- create: `tein/tests/scheme/scheme_bitwise.scm`
- modify: `tein/tests/scheme_tests.rs`

**step 1:** create `tein/tests/scheme/scheme_char.scm`:

```scheme
;;; scheme/char — unicode-aware character operations

(import (scheme char))

;; predicates
(test-true  "char/alphabetic-latin"   (char-alphabetic? #\a))
(test-false "char/alphabetic-digit"   (char-alphabetic? #\0))
(test-true  "char/numeric"            (char-numeric? #\5))
(test-false "char/numeric-alpha"      (char-numeric? #\a))
(test-true  "char/whitespace-space"   (char-whitespace? #\space))
(test-true  "char/whitespace-newline" (char-whitespace? #\newline))
(test-true  "char/upper"              (char-upper-case? #\A))
(test-false "char/upper-lower"        (char-upper-case? #\a))
(test-true  "char/lower"              (char-lower-case? #\a))

;; case conversion
(test-equal "char/upcase"   #\A (char-upcase #\a))
(test-equal "char/downcase" #\a (char-downcase #\A))

;; unicode: greek uppercase Σ <-> lowercase σ
(test-equal "char/upcase-greek"   #\Σ (char-upcase #\σ))
(test-equal "char/downcase-greek" #\σ (char-downcase #\Σ))

;; string-upcase / string-downcase (r7rs via scheme/char)
(test-equal "char/string-upcase"   "HELLO" (string-upcase "hello"))
(test-equal "char/string-downcase" "hello" (string-downcase "HELLO"))
(test-equal "char/string-upcase-greek" "ΑΒΓΔ" (string-upcase "αβγδ"))

;; ci comparisons
(test-true  "char/ci=?"  (char-ci=? #\A #\a))
(test-false "char/ci=?-ne" (char-ci=? #\A #\b))
(test-true  "char/ci<?"  (char-ci<? #\a #\B))
```

**step 2:** create `tein/tests/scheme/scheme_division.scm`:

```scheme
;;; scheme/division — floor, truncate, ceiling, round division

(import (scheme division))

;; floor-quotient and floor-remainder
(test-equal "div/floor-q-pos"  2  (floor-quotient  5 2))
(test-equal "div/floor-q-neg" -3  (floor-quotient -5 2))
(test-equal "div/floor-r-pos"  1  (floor-remainder  5 2))
(test-equal "div/floor-r-neg"  1  (floor-remainder -5 2))  ; sign follows divisor

;; floor/ returns two values
(test-equal "div/floor/-q" 2
  (call-with-values (lambda () (floor/ 5 2)) (lambda (q r) q)))
(test-equal "div/floor/-r" 1
  (call-with-values (lambda () (floor/ 5 2)) (lambda (q r) r)))

;; truncate-quotient and truncate-remainder
(test-equal "div/trunc-q-pos"  2  (truncate-quotient  5 2))
(test-equal "div/trunc-q-neg" -2  (truncate-quotient -5 2))
(test-equal "div/trunc-r-pos"  1  (truncate-remainder  5 2))
(test-equal "div/trunc-r-neg" -1  (truncate-remainder -5 2))  ; sign follows dividend

;; exact-integer-sqrt
(test-equal "div/isqrt-9"
  '(3 0)
  (call-with-values (lambda () (exact-integer-sqrt 9)) list))
(test-equal "div/isqrt-10"
  '(3 1)
  (call-with-values (lambda () (exact-integer-sqrt 10)) list))
(test-equal "div/isqrt-0"
  '(0 0)
  (call-with-values (lambda () (exact-integer-sqrt 0)) list))
```

**step 3:** create `tein/tests/scheme/scheme_fixnum.scm`:

```scheme
;;; scheme/fixnum — fixed-width integer operations (srfi/143)

(import (scheme fixnum))

;; arithmetic
(test-equal "fx/add"  5   (fx+ 2 3))
(test-equal "fx/sub"  1   (fx- 3 2))
(test-equal "fx/mul"  6   (fx* 2 3))
(test-equal "fx/neg" -3   (fx- 3))
(test-equal "fx/abs"  3   (fxabs -3))

;; comparisons
(test-true  "fx/=?"  (fx=? 3 3))
(test-false "fx/=?-ne" (fx=? 3 4))
(test-true  "fx/<?"  (fx<? 2 3))
(test-true  "fx/<=?" (fx<=? 3 3))
(test-true  "fx/>?"  (fx>? 4 3))

;; bitwise
(test-equal "fx/and"    2  (fxand  6 3))   ; 110 & 011 = 010
(test-equal "fx/ior"    7  (fxior  6 3))   ; 110 | 011 = 111
(test-equal "fx/xor"    5  (fxxor  6 3))   ; 110 ^ 011 = 101
(test-equal "fx/not"   -7  (fxnot  6))
(test-equal "fx/shift-l" 12 (fxarithmetic-shift 3  2))
(test-equal "fx/shift-r"  1 (fxarithmetic-shift 4 -2))

;; constants
(test-true "fx/width-positive" (> fx-width 0))
(test-true "fx/greatest-pos"   (> fx-greatest 0))
(test-true "fx/least-neg"      (< fx-least 0))
(test-equal "fx/greatest-least-sym"
  (+ fx-greatest fx-least) -1)
```

**step 4:** create `tein/tests/scheme/scheme_bitwise.scm`:

```scheme
;;; scheme/bitwise — arbitrary-precision bitwise operations (srfi/151)

(import (scheme bitwise))

;; basic ops
(test-equal "bw/and"    4  (bitwise-and  12  5))   ; 1100 & 0101 = 0100
(test-equal "bw/ior"   13  (bitwise-ior  12  5))   ; 1100 | 0101 = 1101
(test-equal "bw/xor"    9  (bitwise-xor  12  5))   ; 1100 ^ 0101 = 1001
(test-equal "bw/not"   -6  (bitwise-not   5))
(test-equal "bw/eqv"  -10  (bitwise-eqv  12  5))   ; ~xor

;; shifts
(test-equal "bw/shift-l"   12 (arithmetic-shift  3  2))
(test-equal "bw/shift-r"    1 (arithmetic-shift  4 -2))
(test-equal "bw/shift-neg" -4 (arithmetic-shift -1  2))

;; bit-count / bit-set?
(test-equal "bw/bit-count"   3  (bit-count  7))   ; 0b111
(test-equal "bw/bit-count-0" 0  (bit-count  0))
(test-true  "bw/bit-set?-t"     (bit-set? 2 7))   ; bit 2 of 0b111
(test-false "bw/bit-set?-f"     (bit-set? 3 7))   ; bit 3 of 0b0111

;; integer-length
(test-equal "bw/int-length-0" 0 (integer-length 0))
(test-equal "bw/int-length-1" 1 (integer-length 1))
(test-equal "bw/int-length-7" 3 (integer-length 7))  ; 0b111 needs 3 bits

;; large integers (arbitrary precision)
(test-equal "bw/large-and"
  (expt 2 64)
  (bitwise-and (+ (expt 2 64) (expt 2 32))
               (+ (expt 2 64) 1)))
```

**step 5:** add test functions to `tein/tests/scheme_tests.rs`. append before the final closing brace:

```rust
#[test]
fn test_scheme_char() {
    run_scheme_test(include_str!("scheme/scheme_char.scm"));
}

#[test]
fn test_scheme_division() {
    run_scheme_test(include_str!("scheme/scheme_division.scm"));
}

#[test]
fn test_scheme_fixnum() {
    run_scheme_test(include_str!("scheme/scheme_fixnum.scm"));
}

#[test]
fn test_scheme_bitwise() {
    run_scheme_test(include_str!("scheme/scheme_bitwise.scm"));
}
```

**step 6:** run these four tests:

```bash
cd ~/projects/tein && cargo test -p tein scheme_char scheme_division scheme_fixnum scheme_bitwise -- --nocapture 2>&1 | tail -20
```

expected: all pass.

**step 7:** commit:

```bash
git add tein/tests/scheme/scheme_char.scm tein/tests/scheme/scheme_division.scm \
        tein/tests/scheme/scheme_fixnum.scm tein/tests/scheme/scheme_bitwise.scm \
        tein/tests/scheme_tests.rs
git commit -m "test: scheme/char, scheme/division, scheme/fixnum, scheme/bitwise coverage"
```

---

## task 7: hand-written scheme tests — `scheme/flonum` and `srfi/18` threads

**files:**
- create: `tein/tests/scheme/scheme_flonum.scm`
- create: `tein/tests/scheme/srfi_18_threads.scm`
- modify: `tein/tests/scheme_tests.rs`

**step 1:** create `tein/tests/scheme/scheme_flonum.scm`:

```scheme
;;; scheme/flonum — flonum constants and transcendentals (srfi/144)
;;; comprehensive because this module was recently fixed (issue #103)

(import (scheme flonum))

;; --- constants ---

;; fl-e: Euler's number ~2.71828
(test-true "fl/e-approx"
  (< (abs (- fl-e 2.718281828459045)) 1e-10))

;; fl-pi: π ~3.14159
(test-true "fl/pi-approx"
  (< (abs (- fl-pi 3.141592653589793)) 1e-10))

;; fl-greatest: finite maximum flonum (must be positive and finite)
(test-true  "fl/greatest-pos"    (fl> fl-greatest 0.0))
(test-true  "fl/greatest-finite" (flfinite? fl-greatest))
(test-false "fl/greatest+1-inf"  (flfinite? (fl* fl-greatest 2.0)))

;; fl-least: smallest positive flonum (subnormal threshold)
(test-true "fl/least-pos"    (fl> fl-least 0.0))
(test-true "fl/least-tiny"   (fl< fl-least 1e-300))

;; fl-epsilon: machine epsilon (1.0 + epsilon != 1.0, 1.0 + epsilon/2 == 1.0)
(test-false "fl/epsilon+1-ne-1" (fl= (fl+ 1.0 fl-epsilon) 1.0))
(test-true  "fl/epsilon/2+1=1"  (fl= (fl+ 1.0 (fl/ fl-epsilon 2.0)) 1.0))

;; fl-nan: not equal to itself
(test-false "fl/nan-ne-self" (fl= fl-nan fl-nan))
(test-true  "fl/nan?"         (flnan? fl-nan))

;; fl-positive-infinity / fl-negative-infinity
(test-true  "fl/+inf-infinite" (flinfinite? fl-positive-infinity))
(test-true  "fl/-inf-negative" (fl< fl-negative-infinity 0.0))
(test-false "fl/+inf-nan"      (flnan? fl-positive-infinity))

;; --- transcendentals ---

;; flsin / flcos
(test-true "fl/sin-pi"   (fl< (flabs (flsin fl-pi)) 1e-10))
(test-true "fl/cos-pi"   (fl< (flabs (fl+ (flcos fl-pi) 1.0)) 1e-10))
(test-true "fl/sin-pi/2" (fl< (flabs (fl- (flsin (fl/ fl-pi 2.0)) 1.0)) 1e-10))

;; flexp / fllog round-trip
(test-true "fl/exp-log"
  (fl< (flabs (fl- (flexp (fllog 2.0)) 2.0)) 1e-10))

;; flsqrt
(test-true "fl/sqrt-4"
  (fl< (flabs (fl- (flsqrt 4.0) 2.0)) 1e-10))
(test-true "fl/sqrt-2"
  (fl< (flabs (fl- (flsqrt 2.0) 1.4142135623730951)) 1e-10))

;; flfloor / flceiling / fltruncate / flround
(test-equal "fl/floor"    1.0  (flfloor 1.7))
(test-equal "fl/floor-n" -2.0  (flfloor -1.7))
(test-equal "fl/ceil"     2.0  (flceiling 1.2))
(test-equal "fl/trunc"    1.0  (fltruncate 1.9))
(test-equal "fl/round-even" 2.0 (flround 2.5))  ; banker's rounding

;; --- predicates ---
(test-true  "fl/finite-1"   (flfinite? 1.0))
(test-false "fl/finite-inf" (flfinite? fl-positive-infinity))
(test-false "fl/finite-nan" (flfinite? fl-nan))
(test-true  "fl/inf?-+inf"  (flinfinite? fl-positive-infinity))
(test-false "fl/inf?-1"     (flinfinite? 1.0))

;; --- arithmetic ---
(test-equal "fl/add" 5.0 (fl+ 2.0 3.0))
(test-equal "fl/mul" 6.0 (fl* 2.0 3.0))
(test-equal "fl/div" 2.5 (fl/ 5.0 2.0))
(test-true  "fl/max" (fl= 3.0 (flmax 1.0 2.0 3.0)))
(test-true  "fl/min" (fl= 1.0 (flmin 1.0 2.0 3.0)))
```

**step 2:** create `tein/tests/scheme/srfi_18_threads.scm`:

```scheme
;;; srfi/18 — threads, mutexes, condition variables
;;; runs with default step_limit — chibi threads are cooperative

(import (srfi 18))

;; --- threads ---

;; thread? predicate
(test-true "thread/main-is-thread?" (thread? (current-thread)))

;; create and start a simple thread
(test-equal "thread/join-result" 42
  (let ((t (make-thread (lambda () 42))))
    (thread-start! t)
    (thread-join! t)))

;; thread state: unstarted thread doesn't run
(test-true "thread/unstarted-ok"
  (let ((ran #f))
    (make-thread (lambda () (set! ran #t)))
    (not ran)))

;; thread-yield is safe
(test-true "thread/yield-ok"
  (begin (thread-yield!) #t))

;; thread-sleep! (very short)
(test-true "thread/sleep-ok"
  (begin (thread-sleep! 0.001) #t))

;; join with timeout on infinite loop → returns timeout value
(test-equal "thread/join-timeout" 'timed-out
  (let ((t (make-thread (lambda () (let lp () (lp))))))
    (thread-start! t)
    (thread-join! t 0.05 'timed-out)))

;; --- mutexes ---

(test-true  "mutex/is-mutex?" (mutex? (make-mutex)))
(test-equal "mutex/lock-unlock" 'done
  (let ((m (make-mutex)))
    (mutex-lock! m)
    (mutex-unlock! m)
    'done))

;; mutex exclusive access between threads
(test-equal "mutex/exclusive" '(1 2)
  (let ((m   (make-mutex))
        (log '()))
    (define (with-lock thunk)
      (mutex-lock! m)
      (let ((r (thunk)))
        (mutex-unlock! m)
        r))
    (let ((t (make-thread
               (lambda ()
                 (with-lock (lambda () (set! log (cons 1 log))))))))
      (thread-start! t)
      (with-lock (lambda () (set! log (cons 2 log))))
      (thread-join! t)
      (list-sort < log))))

;; --- condition variables ---

(test-true "condvar/is-condvar?" (condition-variable? (make-condition-variable)))

;; signal wakes a waiting thread
(test-equal "condvar/signal-wait" 'signalled
  (let ((m  (make-mutex))
        (cv (make-condition-variable))
        (result #f))
    (mutex-lock! m)
    (let ((t (make-thread
               (lambda ()
                 (mutex-lock! m)
                 (set! result 'signalled)
                 (mutex-unlock! m)
                 (condition-variable-signal! cv)))))
      (thread-start! t)
      (mutex-unlock! m cv 0.5)   ; unlock + wait with timeout
      (thread-join! t)
      result)))
```

**step 3:** add the two test functions to `tein/tests/scheme_tests.rs`:

```rust
#[test]
fn test_scheme_flonum() {
    run_scheme_test(include_str!("scheme/scheme_flonum.scm"));
}

#[test]
fn test_srfi_18_threads() {
    run_scheme_test(include_str!("scheme/srfi_18_threads.scm"));
}
```

**step 4:** run both:

```bash
cd ~/projects/tein && cargo test -p tein scheme_flonum srfi_18 -- --nocapture 2>&1 | tail -20
```

expected: both pass. if srfi/18 hangs (infinite-loop test), the `thread-join! t 0.05 'timed-out` timeout should kill it. if it still hangs adjust to 0.5.

**step 5:** commit:

```bash
git add tein/tests/scheme/scheme_flonum.scm tein/tests/scheme/srfi_18_threads.scm \
        tein/tests/scheme_tests.rs
git commit -m "test: scheme/flonum constants + transcendentals, srfi/18 thread/mutex/condvar"
```

---

## task 8: full test run, lint, collect AGENTS.md notes

**step 1:** run the complete test suite:

```bash
cd ~/projects/tein && just test 2>&1 | tail -30
```

expected: all pre-existing tests pass plus all new `vfs_module_tests` and scheme tests. if any newly-added test fails due to a dep issue in `vfs_registry.rs`, fix the deps and re-run.

**step 2:** lint:

```bash
cd ~/projects/tein && just lint
```

fix any clippy warnings (likely unused imports if any test module entry is unreachable) or formatting issues.

**step 3:** collect any notes for AGENTS.md. things to note:
- `(chibi test)` is now in VFS as `default_safe: false` — not available in sandboxed contexts
- `RAISING_APPLIER` pattern for wiring chibi test suites is in `tests/vfs_module_tests.rs`
- modules excluded from test suite: `chibi/regexp-test` (needs pcre), crypto/mime/memoize (fs deps), `srfi/179/231` (fuel concerns)

**step 4:** commit all fixups:

```bash
git add -p   # stage only the intended changes
git commit -m "fix: resolve any dep/test issues from full suite run"
```

---

## task 9: final commit

```bash
cd ~/projects/tein && git log --oneline feature/extended-module-testing-2603 ^dev | head -10
```

verify the branch has the expected commits, then the branch is ready for PR.
