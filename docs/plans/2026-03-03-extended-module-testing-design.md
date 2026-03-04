# extended module testing — design

**date**: 2026-03-03
**branch**: feature/extended-module-testing-2603
**issue**: #103 follow-up — now that srfi/144, scheme/bytevector, chibi/time are fixed, validate full VFS coverage

## goal

comprehensive scheme-level test coverage for all VFS-registered modules. reuse chibi-scheme's bundled `(srfi N test)` / `(chibi X-test)` suites wherever they exist; hand-write targeted `.scm` files for gaps.

## approach: real chibi/test + custom applier

add the real `(chibi test)` library to the VFS. before each `run-tests` call, replace `current-test-applier` with one that raises immediately on failure rather than incrementing a counter. this gives cargo test clean abort-on-first-fail semantics with the test name and expected/actual in the error message.

### why not a shim?

a hand-written shim mapping `(chibi test)` exports to `(tein test)` semantics would need to cover a wide API surface (`test`, `test-equal`, `test-assert`, `test-not`, `test-values`, `test-group`, `test-begin`, `test-end`, `test-error`, `test-propagate-info`, `test-skip`, `current-test-epsilon`, `current-test-comparator`, ...). bugs in the shim → false passes, which is the worst outcome for a test framework. the real `(chibi test)` is battle-tested; we just redirect its failure path.

### test-exit

`(test-exit)` calls `(exit (zero? (test-failure-count)))`. with the custom applier, failures raise before `test-exit` is reached. in the success path, `(exit #t)` → tein exit hatch → `Ok(Value::Integer(0))` — harmless.

## components

### 1. VFS additions

**`(chibi test)`** — real library, `default_safe: false`, deps already in VFS:
- `(scheme base)`, `(scheme case-lambda)`, `(scheme write)`, `(scheme complex)`
- `(scheme process-context)`, `(scheme time)`, `(chibi diff)`, `(chibi term ansi)`, `(chibi optional)`
- files: `lib/chibi/test.sld`, `lib/chibi/test.scm`

**`(srfi N test)`** — 38 modules, all `default_safe: false`, each depends on `(chibi test)` + its srfi:

| module | srfi |
|--------|------|
| srfi/1/test | srfi/1 list library |
| srfi/2/test | srfi/2 and-let* |
| srfi/14/test | srfi/14 char-sets |
| srfi/16/test | srfi/16 case-lambda |
| srfi/18/test | srfi/18 threads |
| srfi/26/test | srfi/26 cut/cute |
| srfi/27/test | srfi/27 random |
| srfi/33/test | srfi/33 bitwise |
| srfi/35/test | srfi/35 conditions |
| srfi/38/test | srfi/38 write/read |
| srfi/41/test | srfi/41 streams |
| srfi/69/test | srfi/69 hash tables |
| srfi/95/test | srfi/95 sorting |
| srfi/99/test | srfi/99 records |
| srfi/101/test | srfi/101 random-access lists |
| srfi/113/test | srfi/113 sets |
| srfi/116/test | srfi/116 immutable lists |
| srfi/117/test | srfi/117 list-queues |
| srfi/121/test | srfi/121 generators |
| srfi/125/test | srfi/125 hash tables |
| srfi/127/test | srfi/127 lseq |
| srfi/128/test | srfi/128 comparators |
| srfi/129/test | srfi/129 titlecase |
| srfi/130/test | srfi/130 string cursors |
| srfi/132/test | srfi/132 sorting |
| srfi/133/test | srfi/133 vectors |
| srfi/134/test | srfi/134 ideque |
| srfi/135/test | srfi/135 texts |
| srfi/139/test | srfi/139 syntax parameters |
| srfi/143/test | srfi/143 fixnums |
| srfi/144/test | srfi/144 flonums |
| srfi/146/test | srfi/146 mappings |
| srfi/151/test | srfi/151 bitwise |
| srfi/158/test | srfi/158 generators |
| srfi/160/test | srfi/160 uniform vectors |
| srfi/166/test | srfi/166 formatting |
| srfi/211/test | srfi/211 syntax transformers |
| srfi/219/test | srfi/219 define-record-type |
| srfi/229/test | srfi/229 tagged procedures |
| srfi/231/test | srfi/231 arrays |

**`(scheme bytevector-test)`** — chibi ships this at `lib/scheme/bytevector-test.sld`, covers the full SRFI-4/R6RS endian-aware bytevector API.

**`(chibi X-test)`** — applicable chibi test modules (all `default_safe: false`):

| module | covers |
|--------|--------|
| chibi/assert-test | chibi/assert |
| chibi/base64-test | chibi/base64 |
| chibi/binary-record-test | chibi/binary-record |
| chibi/bytevector-test | chibi/bytevector |
| chibi/csv-test | chibi/csv |
| chibi/diff-test | chibi/diff |
| chibi/edit-distance-test | chibi/edit-distance |
| chibi/generic-test | chibi/generic |
| chibi/io-test | chibi/io |
| chibi/iset-test | chibi/iset |
| chibi/loop-test | chibi/loop |
| chibi/match-test | chibi/match |
| chibi/math/prime-test | chibi/math/prime |
| chibi/optional-test | chibi/optional |
| chibi/parse-test | chibi/parse |
| chibi/pathname-test | chibi/pathname |
| chibi/quoted-printable-test | chibi/quoted-printable |
| chibi/regexp-test | chibi/regexp |
| chibi/string-test | chibi/string |
| chibi/sxml-test | chibi/sxml |
| chibi/syntax-case-test | chibi/syntax-case |
| chibi/text-test | chibi/text |
| chibi/uri-test | chibi/uri |
| chibi/weak-test | chibi/weak |

note: `chibi/crypto/*-test`, `chibi/mime-test`, `chibi/memoize-test` omitted — depend on filesystem/network ops not available in standard context.

### 2. custom applier preamble

a rust `const` string evaluated once per test context before `(run-tests)`:

```scheme
(import (chibi test))
(current-test-applier
  (lambda (expect expr info)
    (let* ((expected (guard (exn (#t (cons 'exception exn))) (expect)))
           (result   (guard (exn (#t (cons 'exception exn))) (expr)))
           (pass?    (if (assq-ref info 'assertion)
                         result
                         ((current-test-comparator) expected result))))
      (unless pass?
        (error (string-append "FAIL: " (or (assq-ref info 'name) "unknown"))
               'expected expected 'got result)))))
```

### 3. rust test harness — `tests/vfs_module_tests.rs`

```rust
fn run_chibi_test(import: &str) {
    let ctx = Context::new_standard().expect("context");
    ctx.evaluate(APPLIER_PREAMBLE).expect("applier setup");
    ctx.evaluate(&format!("(import {})", import)).expect("import");
    ctx.evaluate("(run-tests)").expect("run-tests");
}

#[test] fn test_srfi_1_list()     { run_chibi_test("(srfi 1 test)"); }
#[test] fn test_srfi_14_charset() { run_chibi_test("(srfi 14 test)"); }
// ... one per module
```

one `#[test]` per module, named `test_<module_path_underscored>`.

### 4. hand-written `.scm` files

for modules without chibi test coverage or needing tein-specific validation:

- **`scheme/char.scm`** — `char-alphabetic?`, `char-ci=?`, `string-upcase` (unicode), `char-downcase` on Greek etc. smoke tests of the unicode tables that chibi/char-set brings in.
- **`scheme/division.scm`** — `floor/`, `floor-quotient`, `truncate/`, `exact-integer-sqrt`, edge cases around negatives and zero.
- **`scheme/fixnum.scm`** — `fx+`, `fx*`, `fxand`, `fxior`, `fxarithmetic-shift`, overflow/wrapping, `fx-width`.
- **`scheme/bitwise.scm`** — `bitwise-and`, `arithmetic-shift`, `bit-count`, `integer-length`, cross-check against srfi/151.
- **`scheme/flonum.scm`** — key constants (`fl-pi`, `fl-e`, `fl-greatest`, `fl-least`, `fl-epsilon`) checked against known values within epsilon; transcendentals (`flsin`, `flcos`, `flexp`, `fllog`).
- **`srfi/18_threads.scm`** — thread create/start/join, mutex lock/unlock, condition variable signal/wait with timeout. run with high `step_limit` (threads are cooperative in chibi).

### 5. registration pattern for test modules

test module VfsEntries follow the same pattern as library modules but:
- `default_safe: false` (always — test infra isn't sandbox-safe)
- `clib: None` (all pure-scheme)
- no `scheme_alias` needed

## file layout

```
tein/src/vfs_registry.rs            — VfsEntry additions for chibi/test, srfi/N/test, chibi/X-test
tein/tests/vfs_module_tests.rs      — new test file, one #[test] per module
tein/tests/scheme/scheme_char.scm
tein/tests/scheme/scheme_division.scm
tein/tests/scheme/scheme_fixnum.scm
tein/tests/scheme/scheme_bitwise.scm
tein/tests/scheme/scheme_flonum.scm
tein/tests/scheme/srfi_18_threads.scm
```

existing `scheme_tests.rs` and its `.scm` files are untouched.

## not in scope

- `chibi/crypto/*-test` — depend on platform file ops
- `chibi/mime-test`, `chibi/memoize-test` — heavy filesystem deps
- `chibi/filesystem-test`, `chibi/process-test`, `chibi/system-test` — OS-level, not appropriate for standard context
- `srfi/179`, `srfi/231` arrays — these are large and have known issues with chibi's array implementation under tein's fuel model; can be added later
