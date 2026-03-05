# chibi regexp VFS smoke tests — implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** add smoke tests for `(chibi regexp)` / SRFI-115 / `(scheme regex)` and fix docs, then close #85.

**Architecture:** VFS wiring is already complete (`vfs_registry.rs` has entries for all three aliases with `default_safe: false`). we add a scheme-level smoke test exercising the SRFI-115 API, rust-side sandbox gating tests, and update reference.md.

**Tech Stack:** scheme (test file), rust (test harness), markdown (docs)

**Design doc:** `docs/plans/2026-03-05-chibi-regexp-vfs-design.md`

---

### task 1: scheme smoke test file

**Files:**
- Create: `tein/tests/scheme/chibi_regexp.scm`

**Step 1: write the scheme smoke test**

```scheme
;;; (chibi regexp) / SRFI-115 smoke tests

(import (chibi regexp))

;; --- compilation + predicate ---

(test-true "regexp/compiled-is-regexp" (regexp? (regexp '(+ digit))))
(test-true "regexp/string-sre-compiles" (regexp? (regexp "abc")))
(test-false "regexp/string-not-regexp" (regexp? "hello"))
(test-false "regexp/integer-not-regexp" (regexp? 42))
(test-true "regexp/valid-sre" (valid-sre? '(+ digit)))
(test-false "regexp/invalid-sre" (valid-sre? '(??? bad)))

;; --- regexp-matches / regexp-matches? ---

(test-true "regexp/matches?-full"
  (regexp-matches? '(+ digit) "42"))
(test-false "regexp/matches?-partial-rejects"
  (regexp-matches? '(+ digit) "abc42"))
(test-true "regexp/matches-returns-match"
  (regexp-match? (regexp-matches '(+ digit) "42")))
(test-false "regexp/matches-rejects-partial"
  (regexp-matches '(+ digit) "abc42"))

;; --- regexp-search ---

(test-true "regexp/search-finds-match"
  (regexp-match? (regexp-search '(+ digit) "abc42def")))
(test-false "regexp/search-no-match"
  (regexp-search '(+ digit) "abcdef"))

;; --- submatch extraction ---

(let ((m (regexp-search '(: (-> num (+ digit)) "-" (-> tag (+ alpha))) "item-42-abc")))
  (test-true "regexp/search-named-match" (regexp-match? m))
  (test-equal "regexp/submatch-named-num" "42"
    (regexp-match-submatch m 'num))
  (test-equal "regexp/submatch-named-tag" "abc"
    (regexp-match-submatch m 'tag)))

(let ((m (regexp-matches '(: ($ (+ digit)) "-" ($ (+ alpha))) "42-abc")))
  (test-equal "regexp/match->list" '("42-abc" "42" "abc")
    (regexp-match->list m))
  (test-equal "regexp/match-count" 3
    (regexp-match-count m))
  (test-equal "regexp/submatch-by-index" "42"
    (regexp-match-submatch m 1)))

;; --- replace ---

(test-equal "regexp/replace-first" "aXb2c3"
  (regexp-replace '(+ digit) "a1b2c3" "X"))
(test-equal "regexp/replace-all" "aXbXcX"
  (regexp-replace-all '(+ digit) "a1b2c3" "X"))
(test-equal "regexp/replace-no-match" "hello"
  (regexp-replace '(+ digit) "hello" "X"))

;; --- split + extract ---

(test-equal "regexp/split-basic" '("a" "b" "c")
  (regexp-split '(+ (~ alpha)) "a,b,c"))
(test-equal "regexp/extract-digits" '("1" "22" "333")
  (regexp-extract '(+ digit) "a1b22c333"))

;; --- fold ---

(test-equal "regexp/fold-collect" '("333" "22" "1")
  (regexp-fold '(+ digit)
    (lambda (i m s acc)
      (cons (regexp-match-submatch m 0) acc))
    '()
    "a1b22c333"))

;; --- SRE syntax features ---

(test-true "regexp/sre-or"
  (regexp-matches? '(or "cat" "dog") "dog"))
(test-false "regexp/sre-or-no-match"
  (regexp-matches? '(or "cat" "dog") "fish"))
(test-true "regexp/sre-nocase"
  (regexp-matches? '(w/nocase "hello") "HELLO"))
(test-true "regexp/sre-seq"
  (regexp-matches? '(: alpha (+ digit)) "a42"))
(test-true "regexp/sre-repetition"
  (regexp-matches? '(: (= 3 digit)) "123"))
(test-false "regexp/sre-repetition-too-short"
  (regexp-matches? '(: (= 3 digit)) "12"))

;; --- regexp->sre round-trip ---

(test-true "regexp/regexp->sre-returns-sre"
  (pair? (regexp->sre (regexp '(+ digit)))))
```

**Step 2: commit**

```
git add tein/tests/scheme/chibi_regexp.scm
git commit -m "test: add (chibi regexp) SRFI-115 smoke tests (#85)"
```

---

### task 2: test runner entry

**Files:**
- Modify: `tein/tests/scheme_tests.rs` (append before EOF)

**Step 1: add the test fn**

append after the last test (`test_scheme_tein_crypto`):

```rust
#[test]
fn test_chibi_regexp() {
    run_scheme_test(include_str!("scheme/chibi_regexp.scm"));
}
```

no feature gate — `(chibi regexp)` is pure scheme, always available.

**Step 2: run it, verify pass**

```bash
cargo test -p tein --test scheme_tests test_chibi_regexp -- --nocapture
```

expected: PASS. if any assertion fails, fix the scheme test (SRE syntax etc).

**Step 3: commit**

```
git add tein/tests/scheme_tests.rs
git commit -m "test: wire chibi_regexp smoke test into scheme_tests runner (#85)"
```

---

### task 3: sandbox gating tests

**Files:**
- Modify: `tein/src/context.rs` — add tests in the sandbox test section

**Step 1: write the gating tests**

find the sandbox/VFS test section (near `test_vfs_gate_modules_all`, around line 5750). add:

```rust
#[test]
fn test_chibi_regexp_blocked_in_modules_safe() {
    use crate::sandbox::Modules;
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::Safe)
        .build()
        .expect("build");
    let err = ctx.evaluate("(import (chibi regexp))").unwrap_err();
    assert!(
        matches!(err, Error::SandboxViolation(_) | Error::EvalError(_)),
        "(chibi regexp) should be blocked in Modules::Safe, got: {err:?}"
    );
}

#[test]
fn test_chibi_regexp_allowed_in_modules_all() {
    use crate::sandbox::Modules;
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::All)
        .build()
        .expect("build");
    let r = ctx.evaluate("(import (chibi regexp)) (regexp? (regexp '(+ digit)))");
    assert!(
        r.is_ok(),
        "(chibi regexp) should work under Modules::All: {:?}",
        r.err()
    );
}

#[test]
fn test_chibi_regexp_allowed_via_allow_module() {
    use crate::sandbox::Modules;
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::Safe)
        .allow_module("chibi/regexp")
        .build()
        .expect("build");
    let r = ctx.evaluate("(import (chibi regexp)) (regexp? (regexp '(+ digit)))");
    assert!(
        r.is_ok(),
        "(chibi regexp) should work via allow_module: {:?}",
        r.err()
    );
}

#[test]
fn test_srfi_115_alias_blocked_in_safe() {
    use crate::sandbox::Modules;
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::Safe)
        .build()
        .expect("build");
    let err = ctx.evaluate("(import (srfi 115))").unwrap_err();
    assert!(
        matches!(err, Error::SandboxViolation(_) | Error::EvalError(_)),
        "(srfi 115) should be blocked in Modules::Safe, got: {err:?}"
    );
}

#[test]
fn test_scheme_regex_alias_blocked_in_safe() {
    use crate::sandbox::Modules;
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::Safe)
        .build()
        .expect("build");
    let err = ctx.evaluate("(import (scheme regex))").unwrap_err();
    assert!(
        matches!(err, Error::SandboxViolation(_) | Error::EvalError(_)),
        "(scheme regex) should be blocked in Modules::Safe, got: {err:?}"
    );
}
```

**Step 2: run the tests**

```bash
cargo test -p tein --lib chibi_regexp_blocked chibi_regexp_allowed srfi_115_alias scheme_regex_alias -- --nocapture
```

expected: all PASS.

**Step 3: commit**

```
git add tein/src/context.rs
git commit -m "test: sandbox gating tests for (chibi regexp) and aliases (#85)"
```

---

### task 4: docs update

**Files:**
- Modify: `docs/reference.md`

**Step 1: fix the srfi section intro**

in `docs/reference.md` around line 113, change:

```
All `srfi/*` modules in the registry are `default_safe: true` except `srfi/18` (threads,
POSIX-only) and `srfi/146/hash` (depends on unsafe internals).
```

to:

```
All `srfi/*` modules in the registry are `default_safe: true` except `srfi/18` (threads,
POSIX-only), `srfi/115` (regexp — backtracking engine, ReDoS risk with untrusted patterns),
and `srfi/146/hash` (depends on unsafe internals).
```

**Step 2: annotate srfi/115 in the table**

change line 128:

```
| `srfi/115` | regular expressions |
```

to:

```
| `srfi/115` | regular expressions (alias for `(chibi regexp)`; `default_safe: false` — ReDoS risk) |
```

**Step 3: add chibi/regexp + scheme/regex to the appropriate sections**

the reference doesn't have a chibi/* section beyond "Full list: see `tein/src/vfs_registry.rs`". add `(chibi regexp)` and `(scheme regex)` mention near the srfi/115 entry or in a short note after the srfi table:

after the "Full list" line (~line 135), add:

```markdown
### regexp modules

Three aliases provide the same regexp engine (SRFI-115 / chibi IrRegex):

| module | notes |
|--------|-------|
| `(chibi regexp)` | canonical implementation — SRE syntax, submatches, fold, split, extract |
| `(srfi 115)` | SRFI-115 alias |
| `(scheme regex)` | R7RS-large alias |

All three are `default_safe: false` — the engine can exhibit superlinear time on
pathological patterns. use `.allow_module("chibi/regexp")` or `Modules::All` to
enable. for untrusted patterns, prefer `(tein safe-regexp)` which guarantees
linear time.
```

**Step 4: run docs lint if available, then commit**

```
git add docs/reference.md
git commit -m "docs: document (chibi regexp) sandbox gating and ReDoS caveat (#85)"
```

---

### task 5: lint + close

**Step 1: run lint**

```bash
just lint
```

fix anything that comes up.

**Step 2: run the full test suite**

```bash
just test
```

expected: all pass with the new tests included.

**Step 3: commit any lint fixes, then create the branch + PR**

branch should already exist from `just feature`. if not:

```bash
just feature chibi-regexp-vfs-2603
```

push + PR against `dev`, title: `feat: (chibi regexp) SRFI-115 smoke tests + sandbox gating (#85)`, body references #85 with "closes #85".

---

### notes for AGENTS.md

- `(chibi regexp)` header says "non-backtracking Thompson NFA" but the issue says backtracking. the engine uses NFA by default but falls back to backtracking for backrefs/lookaround — worth noting if we ever update the security note.
