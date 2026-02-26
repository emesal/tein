# Scheme Test Coverage Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add comprehensive r7rs scheme test coverage to tein, porting and trimming r7rs-tests.scm to exercise all major language features not yet covered by existing tests.

**Architecture:** Each gap area becomes a dedicated `.scm` file in `tein/tests/scheme/`, following the existing pattern (no imports needed besides `(tein test)`, which `run_scheme_test` pre-loads). Each new `.scm` file gets a corresponding `#[test]` fn in `tein/tests/scheme_tests.rs`. Source material is adapted from the chibi-scheme r7rs test suite — replace `(chibi test)` with `(tein test)`, `(test expected expr)` with `(test-equal "name" expected expr)`.

**Tech Stack:** Rust (cargo test), chibi-scheme r7rs, `(tein test)` assertion framework (`test-equal`, `test-true`, `test-false`, `test-error`)

---

## Progress

- [x] Task 1: `control_flow.scm` — DONE (commit e00649d)
- [x] Task 2: `binding_forms.scm` — DONE (commit e00649d)
- [x] Task 3: `tail_calls.scm` — DONE (commit e00649d)
- [x] Task 4: `closures.scm` — DONE (commit 73ab72b)
- [x] Task 5: `continuations.scm` — DONE (commit 73ab72b)
- [x] Task 6: `error_handling.scm` — DONE (commit 73ab72b)
- [ ] Task 7: `records.scm`
- [ ] Task 8: `bytevectors.scm`
- [ ] Task 9: `io.scm`
- [ ] Task 10: `macros.scm`
- [ ] Task 11: `quasiquote.scm`
- [ ] Task 12: `case_lambda.scm`
- [ ] Task 13: `lazy.scm`
- [ ] Task 14: `numbers_extended.scm`
- [ ] Task 15: `scheme_eval.scm`
- [ ] Task 16: `tein_foreign.scm`
- [ ] Task 17: Run full test suite + document findings

---

## Existing coverage (do not duplicate)

- `arithmetic.scm` — `+`, `-`, `*`, `/`, `quotient`, `remainder`, `modulo`, comparisons, `exact?`/`inexact?`, `exact->inexact`, `min`/`max`, `abs`
- `lists.scm` — `cons`, `car`, `cdr`, `list`, `length`, `reverse`, `append`, `map`, `for-each`, `assoc`, `member`
- `strings.scm` — string construction, access, comparison, mutation, number/string conversion
- `types.scm` — type predicates, `boolean?`, `char?`, `vector?`, `bytevector?`, `procedure?`, `symbol?`, `port?`
- `reader_macro.scm` — `(tein reader)` dispatch, `(tein macro)` expansion hook

## Key conventions

- test name strings use `"category/subcategory"` style e.g. `"cond/else"`, `"tco/named-let"`
- every `.scm` file starts with `;;; <topic> tests — <brief description>`
- `run_scheme_test` in `scheme_tests.rs` pre-imports `(tein test)` before evaluating the file
- `test-error` takes a label and a zero-arg thunk: `(test-error "name" (lambda () ...))`

## Confirmed import requirements (discovered during implementation)

`Context::new_standard()` calls `sexp_load_standard_env` which loads `init-7.scm` into the
env, but does **not** automatically re-export all `(scheme base)` macros to the toplevel.
A plain `(import (scheme base))` at the top of the file fixes this (produces harmless
"importing already defined binding" warnings for `equal?`, `let-syntax`, `letrec-syntax`).

**Available without any import (defined in init-7.scm):**
`cond`, `case`, `and`, `or`, `do`, `let`, `let*`, `letrec`, `letrec*`, `named let`,
`dynamic-wind`, `call/cc`, `call-with-current-continuation`, `values`, `call-with-values`,
`with-exception-handler`, `raise`, `raise-continuable`, `define-record-type`,
`define-syntax`, `syntax-rules`, `let-syntax`, `letrec-syntax`, `quasiquote`

**Require `(import (scheme base))`:**
`when`, `unless`, `define-values`, `guard`, `error-object?`, `error-object-message`,
`error-object-irritants`, `floor/`, `truncate/`

**Require other imports as originally planned:**
- `(import (scheme inexact))` — `finite?`, `infinite?`, `nan?`
- `(import (scheme lazy))` — `delay`, `force`, `promise?`, `make-promise`
- `(import (scheme case-lambda))` — `case-lambda`
- `(import (scheme eval))` — `eval`, `interaction-environment`, `scheme-report-environment`

## Discovered chibi quirks

**`condition/report-string` does not exist** — use `error-object-message` instead.

**`raise-continuable` expected value**: handler return flows back to the raise site.
`(+ 1 (raise-continuable x))` with a handler returning 99 yields **100** (not 99).
The original plan had the wrong expected value.

**call/cc re-entry with top-level defines**: calling a saved continuation from a separate
`ctx.evaluate()` call does not re-enter (C stack boundary). Within a single evaluate call,
re-entry also fails when state is in top-level `define`s — chibi's batch-compiled toplevel
re-executes from the continuation point but the define bindings reset. **Fix:** keep mutable
state in `let` scope, not top-level defines. Example:
```scheme
;; works:
(let ((k #f) (n 0))
  (call/cc (lambda (c) (set! k c)))
  (set! n (+ n 1))
  (if (< n 3) (k 'ignored) n))  ; => 3

;; does NOT work (returns 1, not 3):
(define saved-k #f)
(define counter 0)
(call/cc (lambda (k) (set! saved-k k)))
(set! counter (+ counter 1))
(if (< counter 3) (saved-k #f) counter)
```

---

## Task 1: control_flow.scm — cond, case, when, unless, and, or, do

**Files:**
- Create: `tein/tests/scheme/control_flow.scm`
- Modify: `tein/tests/scheme_tests.rs`

**Step 1: Write the test file**

```scheme
;;; control flow tests — cond, case, when, unless, and, or, do

;; cond
(test-equal "cond/first" 1 (cond (#t 1) (else 2)))
(test-equal "cond/second" 2 (cond (#f 1) (#t 2) (else 3)))
(test-equal "cond/else" 3 (cond (#f 1) (else 3)))
(test-equal "cond/arrow" 5 (cond ((+ 2 3) => (lambda (x) x)) (else 0)))

;; case
(test-equal "case/match" 'two (case 2 ((1) 'one) ((2) 'two) (else 'other)))
(test-equal "case/else" 'other (case 5 ((1) 'one) ((2) 'two) (else 'other)))
(test-equal "case/first-of-list" 'ab (case 'a ((a b) 'ab) (else 'other)))

;; when / unless
(test-equal "when/true" 2 (when #t 1 2))
(test-equal "when/false" (if #f #f) (when #f 1 2))
(test-equal "unless/false" 2 (unless #f 1 2))
(test-equal "unless/true" (if #f #f) (unless #t 1 2))

;; and / or
(test-true  "and/empty" (and))
(test-equal "and/all-true" 3 (and 1 2 3))
(test-false "and/short-circuit" (and 1 #f 3))
(test-false "or/empty" (or))
(test-equal "or/first" 1 (or 1 2 3))
(test-equal "or/skip-false" 2 (or #f 2 3))
(test-false "or/all-false" (or #f #f))

;; do loop
(test-equal "do/sum" 10
  (do ((i 0 (+ i 1))
       (sum 0 (+ sum i)))
      ((= i 5) sum)))

(test-equal "do/list-build" '(4 3 2 1 0)
  (do ((i 0 (+ i 1))
       (acc '() (cons i acc)))
      ((= i 5) acc)))

;; do with vector fill
(test-equal "do/vector-fill" '#(0 1 2 3 4)
  (let ((v (make-vector 5)))
    (do ((i 0 (+ i 1)))
        ((= i 5) v)
      (vector-set! v i i))))
```

**Step 2: Add test fn to scheme_tests.rs**

In `tein/tests/scheme_tests.rs`, after the last `#[test]` fn, add:

```rust
#[test]
fn test_scheme_control_flow() {
    run_scheme_test(include_str!("scheme/control_flow.scm"));
}
```

**Step 3: Run the test**

```bash
cd tein && cargo test test_scheme_control_flow -- --nocapture
```

Expected: PASS. If any assertion fails the output will show the test name.

**Step 4: Commit**

```bash
git add tein/tests/scheme/control_flow.scm tein/tests/scheme_tests.rs
git commit -m "test: scheme control flow coverage (cond, case, when/unless, and/or, do)"
```

---

## Task 2: binding_forms.scm — let*, letrec, letrec*, define-values

**Files:**
- Create: `tein/tests/scheme/binding_forms.scm`
- Modify: `tein/tests/scheme_tests.rs`

**Step 1: Write the test file**

```scheme
;;; binding form tests — let*, letrec, letrec*, named let, define-values

;; let* (sequential bindings)
(test-equal "let*/seq" 3 (let* ((x 1) (y (+ x 1))) (+ x y)))
(test-equal "let*/shadow" 2 (let* ((x 1) (x (+ x 1))) x))

;; letrec (mutually recursive)
(test-true "letrec/even?" (letrec ((even? (lambda (n) (if (= n 0) #t (odd? (- n 1)))))
                                   (odd?  (lambda (n) (if (= n 0) #f (even? (- n 1))))))
                            (even? 10)))
(test-false "letrec/odd?" (letrec ((even? (lambda (n) (if (= n 0) #t (odd? (- n 1)))))
                                   (odd?  (lambda (n) (if (= n 0) #f (even? (- n 1))))))
                            (odd? 10)))

;; letrec*
(test-equal "letrec*/seq" 3 (letrec* ((x 1) (y (+ x 2))) y))

;; named let
(test-equal "named-let/sum" 55
  (let loop ((i 1) (acc 0))
    (if (> i 10) acc (loop (+ i 1) (+ acc i)))))

(test-equal "named-let/fib" 55
  (let fib ((n 10))
    (if (< n 2) n (+ (fib (- n 1)) (fib (- n 2))))))

;; define-values
(define-values (a b c) (values 1 2 3))
(test-equal "define-values/a" 1 a)
(test-equal "define-values/b" 2 b)
(test-equal "define-values/c" 3 c)
```

**Step 2: Add test fn**

```rust
#[test]
fn test_scheme_binding_forms() {
    run_scheme_test(include_str!("scheme/binding_forms.scm"));
}
```

**Step 3: Run**

```bash
cd tein && cargo test test_scheme_binding_forms -- --nocapture
```

**Step 4: Commit**

```bash
git add tein/tests/scheme/binding_forms.scm tein/tests/scheme_tests.rs
git commit -m "test: scheme binding forms coverage (let*, letrec, letrec*, named let, define-values)"
```

---

## Task 3: tail_calls.scm — TCO correctness

**Files:**
- Create: `tein/tests/scheme/tail_calls.scm`
- Modify: `tein/tests/scheme_tests.rs`

**Step 1: Write the test file**

These tests use large iteration counts. If TCO is broken they will either stack-overflow (signal) or hit the fuel limit. A standard context has a 10M step limit — the loops below stay well under that while being deep enough to catch non-TCO.

```scheme
;;; tail call tests — tco correctness via large iteration counts

;; basic tail recursion (100k iterations — would stack-overflow without TCO)
(define (count-down n)
  (if (= n 0) 'done (count-down (- n 1))))
(test-equal "tco/count-down" 'done (count-down 100000))

;; accumulator pattern
(define (sum-to n acc)
  (if (= n 0) acc (sum-to (- n 1) (+ acc n))))
(test-equal "tco/sum" 5050 (sum-to 100 0))

;; named let loop
(test-equal "tco/named-let" 100000
  (let loop ((i 0))
    (if (= i 100000) i (loop (+ i 1)))))

;; mutual tail recursion
(define (my-even? n) (if (= n 0) #t (my-odd? (- n 1))))
(define (my-odd?  n) (if (= n 0) #f (my-even? (- n 1))))
(test-true  "tco/mutual-even" (my-even? 10000))
(test-false "tco/mutual-odd"  (my-odd?  10000))

;; tail position in cond
(define (count-cond n)
  (cond ((= n 0) 'done) (else (count-cond (- n 1)))))
(test-equal "tco/cond" 'done (count-cond 100000))

;; tail position in and/or
(define (count-and n)
  (and (> n -1) (if (= n 0) 'done (count-and (- n 1)))))
(test-equal "tco/and" 'done (count-and 100000))

;; tail position in when
(define (count-when n)
  (when (>= n 0) (if (= n 0) 'done (count-when (- n 1)))))
(test-equal "tco/when" 'done (count-when 100000))

;; tail position in let
(define (count-let n)
  (let ((m (- n 1)))
    (if (= n 0) 'done (count-let m))))
(test-equal "tco/let" 'done (count-let 100000))

;; tail position in begin
(define (count-begin n)
  (begin
    (if (= n 0) 'done (count-begin (- n 1)))))
(test-equal "tco/begin" 'done (count-begin 100000))
```

**Step 2: Add test fn**

```rust
#[test]
fn test_scheme_tail_calls() {
    run_scheme_test(include_str!("scheme/tail_calls.scm"));
}
```

**Step 3: Run**

```bash
cd tein && cargo test test_scheme_tail_calls -- --nocapture
```

**Step 4: Commit**

```bash
git add tein/tests/scheme/tail_calls.scm tein/tests/scheme_tests.rs
git commit -m "test: tco correctness via 100k-iteration tail call patterns"
```

---

## Task 4: closures.scm — lexical scope, mutation, higher-order

**Files:**
- Create: `tein/tests/scheme/closures.scm`
- Modify: `tein/tests/scheme_tests.rs`

**Step 1: Write the test file**

```scheme
;;; closure tests — lexical scope, mutation through closure, higher-order fns

;; basic closure captures variable
(define (make-adder n) (lambda (x) (+ x n)))
(define add5 (make-adder 5))
(test-equal "closure/capture" 8 (add5 3))
(test-equal "closure/independent" 10 ((make-adder 7) 3))

;; mutation through closure (counter)
(define (make-counter)
  (let ((n 0))
    (lambda () (set! n (+ n 1)) n)))
(define c (make-counter))
(test-equal "closure/counter-1" 1 (c))
(test-equal "closure/counter-2" 2 (c))
(test-equal "closure/counter-3" 3 (c))

;; two closures share same mutable state
(define (make-pair-counter)
  (let ((n 0))
    (cons (lambda () (set! n (+ n 1)) n)
          (lambda () n))))
(define pc (make-pair-counter))
((car pc))
((car pc))
(test-equal "closure/shared-state" 2 ((cdr pc)))

;; higher-order: compose
(define (compose f g) (lambda (x) (f (g x))))
(define inc (lambda (x) (+ x 1)))
(define dbl (lambda (x) (* x 2)))
(test-equal "closure/compose" 7 ((compose inc dbl) 3))

;; variadic lambda
(define (sum . args)
  (apply + args))
(test-equal "closure/variadic" 6 (sum 1 2 3))
(test-equal "closure/variadic-empty" 0 (sum))

;; rest args with required
(define (head-and-tail x . rest) (cons x rest))
(test-equal "closure/rest-args" '(1 2 3) (head-and-tail 1 2 3))
(test-equal "closure/rest-args-empty" '(1) (head-and-tail 1))

;; apply
(test-equal "apply/list" 6 (apply + '(1 2 3)))
(test-equal "apply/mixed" 10 (apply + 1 2 '(3 4)))
(test-equal "apply/lambda" '(1 2 3) (apply list '(1 2 3)))
```

**Step 2: Add test fn**

```rust
#[test]
fn test_scheme_closures() {
    run_scheme_test(include_str!("scheme/closures.scm"));
}
```

**Step 3: Run**

```bash
cd tein && cargo test test_scheme_closures -- --nocapture
```

**Step 4: Commit**

```bash
git add tein/tests/scheme/closures.scm tein/tests/scheme_tests.rs
git commit -m "test: closure and higher-order function coverage"
```

---

## Task 5: continuations.scm — call/cc, dynamic-wind, multiple values

**Files:**
- Create: `tein/tests/scheme/continuations.scm`
- Modify: `tein/tests/scheme_tests.rs`

**Step 1: Write the test file**

```scheme
;;; continuation tests — call/cc, dynamic-wind, values/call-with-values

;; basic escape continuation
(test-equal "callcc/escape" 42
  (call-with-current-continuation
    (lambda (k) (k 42) 99)))

;; escape from nested context
(test-equal "callcc/nested-escape" 1
  (+ 1 (call/cc (lambda (k) (+ 2 (k 0))))))

;; store and invoke continuation (upward)
(define saved-k #f)
(define counter 0)
(call/cc (lambda (k) (set! saved-k k)))
(set! counter (+ counter 1))
(when (< counter 3) (saved-k #f))
(test-equal "callcc/reentry" 3 counter)

;; dynamic-wind: enter/exit thunks called
(define log '())
(dynamic-wind
  (lambda () (set! log (cons 'in log)))
  (lambda () (set! log (cons 'body log)))
  (lambda () (set! log (cons 'out log))))
(test-equal "dynwind/normal" '(out body in) log)

;; dynamic-wind with escape continuation
(define log2 '())
(call/cc
  (lambda (k)
    (dynamic-wind
      (lambda () (set! log2 (cons 'in log2)))
      (lambda () (set! log2 (cons 'body log2)) (k 'escaped))
      (lambda () (set! log2 (cons 'out log2))))))
(test-equal "dynwind/escape" '(out body in) log2)

;; multiple values
(define-values (q r) (floor/ 17 5))
(test-equal "values/floor-q" 3 q)
(test-equal "values/floor-r" 2 r)

(test-equal "call-with-values" 5
  (call-with-values (lambda () (values 2 3)) +))

(test-equal "values/passthrough" '(1 2 3)
  (call-with-values (lambda () (values 1 2 3)) list))
```

**Step 2: Add test fn**

```rust
#[test]
fn test_scheme_continuations() {
    run_scheme_test(include_str!("scheme/continuations.scm"));
}
```

**Step 3: Run**

```bash
cd tein && cargo test test_scheme_continuations -- --nocapture
```

**Step 4: Commit**

```bash
git add tein/tests/scheme/continuations.scm tein/tests/scheme_tests.rs
git commit -m "test: call/cc, dynamic-wind, and multiple values coverage"
```

---

## Task 6: error_handling.scm — raise, guard, error, conditions

**Files:**
- Create: `tein/tests/scheme/error_handling.scm`
- Modify: `tein/tests/scheme_tests.rs`

**Step 1: Write the test file**

```scheme
;;; error handling tests — raise, guard, error objects, condition types

;; test-error verifies that a thunk raises an exception
(test-error "error/basic" (lambda () (error "boom" 1 2)))

;; guard: catch and inspect error object
(test-equal "guard/message" "oops"
  (guard (exn ((error? exn) (condition/report-string exn)))
    (error "oops")))

;; guard/else
(test-equal "guard/else" 'caught
  (guard (exn (else 'caught))
    (error "anything")))

;; guard reraises if no clause matches — verify via outer guard
(test-equal "guard/reraise" 'outer
  (guard (outer-exn (else 'outer))
    (guard (inner-exn ((string? inner-exn) 'wrong-type))
      (error "not a plain string"))))

;; error? predicate
(test-true "error?/yes"
  (guard (e (#t (error? e)))
    (error "test")))

;; raise with non-error object
(test-equal "raise/non-error" 42
  (guard (e (#t e))
    (raise 42)))

;; raise-continuable (value returned to raise site)
(test-equal "raise-continuable" 99
  (with-exception-handler
    (lambda (e) 99)
    (lambda () (+ 1 (raise-continuable 'ignored)))))
```

**Step 2: Add test fn**

```rust
#[test]
fn test_scheme_error_handling() {
    run_scheme_test(include_str!("scheme/error_handling.scm"));
}
```

**Step 3: Run**

```bash
cd tein && cargo test test_scheme_error_handling -- --nocapture
```

Note: `condition/report-string` is chibi-specific. If it doesn't work, use `(error-message exn)` — check what chibi exposes in standard env.

**Step 4: Commit**

```bash
git add tein/tests/scheme/error_handling.scm tein/tests/scheme_tests.rs
git commit -m "test: error handling coverage (raise, guard, error objects)"
```

---

## Task 7: records.scm — define-record-type

**Files:**
- Create: `tein/tests/scheme/records.scm`
- Modify: `tein/tests/scheme_tests.rs`

**Step 1: Write the test file**

```scheme
;;; record type tests — define-record-type (r7rs)

(define-record-type <point>
  (make-point x y)
  point?
  (x point-x)
  (y point-y))

(define p (make-point 3 4))
(test-true  "record/predicate" (point? p))
(test-false "record/predicate-other" (point? 42))
(test-equal "record/accessor-x" 3 (point-x p))
(test-equal "record/accessor-y" 4 (point-y p))

;; mutable fields
(define-record-type <counter>
  (make-counter val)
  counter?
  (val counter-val set-counter-val!))

(define ct (make-counter 0))
(set-counter-val! ct 5)
(test-equal "record/mutator" 5 (counter-val ct))

;; record with multiple mutable fields
(define-record-type <person>
  (make-person name age)
  person?
  (name person-name set-person-name!)
  (age  person-age  set-person-age!))

(define alice (make-person "Alice" 30))
(test-equal "record/person-name" "Alice" (person-name alice))
(set-person-age! alice 31)
(test-equal "record/person-age-mutated" 31 (person-age alice))
```

**Step 2: Add test fn**

```rust
#[test]
fn test_scheme_records() {
    run_scheme_test(include_str!("scheme/records.scm"));
}
```

**Step 3: Run**

```bash
cd tein && cargo test test_scheme_records -- --nocapture
```

**Step 4: Commit**

```bash
git add tein/tests/scheme/records.scm tein/tests/scheme_tests.rs
git commit -m "test: define-record-type coverage"
```

---

## Task 8: bytevectors.scm — creation, access, copy, ports

**Files:**
- Create: `tein/tests/scheme/bytevectors.scm`
- Modify: `tein/tests/scheme_tests.rs`

**Step 1: Write the test file**

```scheme
;;; bytevector tests — creation, access, mutation, copy, utf8

;; construction
(test-equal "bv/make" '#u8(0 0 0) (make-bytevector 3))
(test-equal "bv/make-fill" '#u8(7 7 7) (make-bytevector 3 7))
(test-equal "bv/literal" 3 (bytevector-length #u8(1 2 3)))

;; access and mutation
(define bv (bytevector 10 20 30))
(test-equal "bv/u8-ref" 20 (bytevector-u8-ref bv 1))
(bytevector-u8-set! bv 1 99)
(test-equal "bv/u8-set" 99 (bytevector-u8-ref bv 1))

;; length
(test-equal "bv/length" 3 (bytevector-length bv))
(test-equal "bv/empty" 0 (bytevector-length (bytevector)))

;; copy
(define bv2 (bytevector-copy bv))
(bytevector-u8-set! bv2 0 42)
(test-equal "bv/copy-independent" 10 (bytevector-u8-ref bv 0))
(test-equal "bv/copy-mutated" 42 (bytevector-u8-ref bv2 0))

;; copy with start/end
(test-equal "bv/copy-slice" '#u8(20 30) (bytevector-copy bv 1 3))

;; append
(test-equal "bv/append" '#u8(1 2 3 4) (bytevector-append #u8(1 2) #u8(3 4)))

;; utf8 round-trip
(test-equal "bv/utf8->string" "hello" (utf8->string (string->utf8 "hello")))
(test-equal "bv/string->utf8" '#u8(104 105) (string->utf8 "hi"))
```

**Step 2: Add test fn**

```rust
#[test]
fn test_scheme_bytevectors() {
    run_scheme_test(include_str!("scheme/bytevectors.scm"));
}
```

**Step 3: Run**

```bash
cd tein && cargo test test_scheme_bytevectors -- --nocapture
```

**Step 4: Commit**

```bash
git add tein/tests/scheme/bytevectors.scm tein/tests/scheme_tests.rs
git commit -m "test: bytevector coverage"
```

---

## Task 9: io.scm — string ports, write/display/read

**Files:**
- Create: `tein/tests/scheme/io.scm`
- Modify: `tein/tests/scheme_tests.rs`

**Step 1: Write the test file**

```scheme
;;; i/o tests — string ports, write, display, read

;; open-input-string / read
(let ((p (open-input-string "(1 2 3)")))
  (test-equal "io/read-list" '(1 2 3) (read p)))

(let ((p (open-input-string "hello")))
  (test-equal "io/read-symbol" 'hello (read p)))

(let ((p (open-input-string "")))
  (test-true "io/eof" (eof-object? (read p))))

;; read-char / peek-char
(let ((p (open-input-string "abc")))
  (test-equal "io/peek-char" #\a (peek-char p))
  (test-equal "io/read-char-1" #\a (read-char p))
  (test-equal "io/read-char-2" #\b (read-char p)))

;; open-output-string / get-output-string
(let ((p (open-output-string)))
  (write-char #\h p)
  (write-char #\i p)
  (test-equal "io/write-char" "hi" (get-output-string p)))

;; write vs display
(let ((p (open-output-string)))
  (write "hello" p)
  (test-equal "io/write-string" "\"hello\"" (get-output-string p)))

(let ((p (open-output-string)))
  (display "hello" p)
  (test-equal "io/display-string" "hello" (get-output-string p)))

(let ((p (open-output-string)))
  (write '(1 "two" #t) p)
  (test-equal "io/write-list" "(1 \"two\" #t)" (get-output-string p)))

;; newline
(let ((p (open-output-string)))
  (newline p)
  (test-equal "io/newline" "\n" (get-output-string p)))

;; read multiple datums
(let ((p (open-input-string "1 2 3")))
  (let ((a (read p)) (b (read p)) (c (read p)))
    (test-equal "io/read-multi" '(1 2 3) (list a b c))))
```

**Step 2: Add test fn**

```rust
#[test]
fn test_scheme_io() {
    run_scheme_test(include_str!("scheme/io.scm"));
}
```

**Step 3: Run**

```bash
cd tein && cargo test test_scheme_io -- --nocapture
```

**Step 4: Commit**

```bash
git add tein/tests/scheme/io.scm tein/tests/scheme_tests.rs
git commit -m "test: string port and i/o coverage (read, write, display)"
```

---

## Task 10: macros.scm — define-syntax, let-syntax, syntax-rules patterns

**Files:**
- Create: `tein/tests/scheme/macros.scm`
- Modify: `tein/tests/scheme_tests.rs`

**Step 1: Write the test file**

```scheme
;;; macro tests — define-syntax, let-syntax, letrec-syntax, syntax-rules

;; basic define-syntax
(define-syntax my-if
  (syntax-rules ()
    ((my-if c t f) (cond (c t) (else f)))))
(test-equal "macro/my-if-true"  1 (my-if #t 1 2))
(test-equal "macro/my-if-false" 2 (my-if #f 1 2))

;; variadic pattern with ellipsis
(define-syntax my-or
  (syntax-rules ()
    ((my-or) #f)
    ((my-or e) e)
    ((my-or e1 e2 ...)
     (let ((t e1))
       (if t t (my-or e2 ...))))))
(test-equal "macro/my-or-empty" #f (my-or))
(test-equal "macro/my-or-first" 1  (my-or 1 2))
(test-equal "macro/my-or-second" 2 (my-or #f 2))

;; ellipsis in output
(define-syntax my-list
  (syntax-rules ()
    ((my-list x ...) (list x ...))))
(test-equal "macro/ellipsis" '(1 2 3) (my-list 1 2 3))

;; let-syntax (local macro, no letrec semantics)
(let-syntax ((dbl (syntax-rules ()
                    ((dbl x) (* 2 x)))))
  (test-equal "let-syntax/double" 10 (dbl 5)))

;; letrec-syntax (local macros can reference each other)
(letrec-syntax
    ((my-and (syntax-rules ()
               ((my-and) #t)
               ((my-and e) e)
               ((my-and e1 e2 ...) (if e1 (my-and e2 ...) #f)))))
  (test-equal "letrec-syntax/and-t" 3 (my-and 1 2 3))
  (test-false "letrec-syntax/and-f" (my-and 1 #f 3)))

;; hygiene: macro-introduced bindings don't capture user variables
(define-syntax swap!
  (syntax-rules ()
    ((swap! a b)
     (let ((tmp a))
       (set! a b)
       (set! b tmp)))))
(define x 1)
(define y 2)
(swap! x y)
(test-equal "macro/hygiene-x" 2 x)
(test-equal "macro/hygiene-y" 1 y)
```

**Step 2: Add test fn**

```rust
#[test]
fn test_scheme_macros() {
    run_scheme_test(include_str!("scheme/macros.scm"));
}
```

**Step 3: Run**

```bash
cd tein && cargo test test_scheme_macros -- --nocapture
```

**Step 4: Commit**

```bash
git add tein/tests/scheme/macros.scm tein/tests/scheme_tests.rs
git commit -m "test: syntax-rules macro coverage (define-syntax, let-syntax, hygiene)"
```

---

## Task 11: quasiquote.scm — nested quasiquote, splicing

**Files:**
- Create: `tein/tests/scheme/quasiquote.scm`
- Modify: `tein/tests/scheme_tests.rs`

**Step 1: Write the test file**

```scheme
;;; quasiquote tests — unquote, splicing, nesting

;; basic quasiquote
(test-equal "qq/basic" '(1 2 3) `(1 2 3))
(test-equal "qq/unquote" '(1 2 3) (let ((x 2)) `(1 ,x 3)))
(test-equal "qq/splice"  '(1 2 3 4) (let ((xs '(2 3))) `(1 ,@xs 4)))

;; splice at start/end
(test-equal "qq/splice-start" '(1 2 3) (let ((xs '(1 2))) `(,@xs 3)))
(test-equal "qq/splice-end"   '(1 2 3) (let ((xs '(2 3))) `(1 ,@xs)))
(test-equal "qq/splice-only"  '(1 2 3) (let ((xs '(1 2 3))) `(,@xs)))

;; dotted pair
(test-equal "qq/dotted" '(1 . 2) `(1 . ,(+ 1 1)))

;; vector quasiquote
(test-equal "qq/vector" '#(1 2 3) `#(1 2 3))
(test-equal "qq/vector-unquote" '#(1 2 3) (let ((x 2)) `#(1 ,x 3)))
```

**Step 2: Add test fn**

```rust
#[test]
fn test_scheme_quasiquote() {
    run_scheme_test(include_str!("scheme/quasiquote.scm"));
}
```

**Step 3: Run**

```bash
cd tein && cargo test test_scheme_quasiquote -- --nocapture
```

**Step 4: Commit**

```bash
git add tein/tests/scheme/quasiquote.scm tein/tests/scheme_tests.rs
git commit -m "test: quasiquote, unquote-splicing, and nested quasiquote coverage"
```

---

## Task 12: case_lambda.scm — arity dispatch

**Files:**
- Create: `tein/tests/scheme/case_lambda.scm`
- Modify: `tein/tests/scheme_tests.rs`

**Step 1: Write the test file**

```scheme
;;; case-lambda tests — arity dispatch

(import (scheme case-lambda))

(define add
  (case-lambda
    (() 0)
    ((x) x)
    ((x y) (+ x y))
    ((x y . rest) (apply + x y rest))))

(test-equal "case-lambda/0-args" 0 (add))
(test-equal "case-lambda/1-arg"  5 (add 5))
(test-equal "case-lambda/2-args" 7 (add 3 4))
(test-equal "case-lambda/3-args" 10 (add 1 2 3 4))

;; wrong arity raises error
(define exact-two
  (case-lambda
    ((x y) (+ x y))))
(test-error "case-lambda/wrong-arity" (lambda () (exact-two 1)))

;; case-lambda as method dispatch
(define (make-adder . args)
  (case-lambda
    ((n) (+ n (car args)))
    (() (car args))))
(define add10 (make-adder 10))
(test-equal "case-lambda/method-1" 15 (add10 5))
(test-equal "case-lambda/method-0" 10 (add10))
```

**Step 2: Add test fn**

```rust
#[test]
fn test_scheme_case_lambda() {
    run_scheme_test(include_str!("scheme/case_lambda.scm"));
}
```

**Step 3: Run**

```bash
cd tein && cargo test test_scheme_case_lambda -- --nocapture
```

**Step 4: Commit**

```bash
git add tein/tests/scheme/case_lambda.scm tein/tests/scheme_tests.rs
git commit -m "test: case-lambda arity dispatch coverage"
```

---

## Task 13: lazy.scm — delay/force/make-promise

**Files:**
- Create: `tein/tests/scheme/lazy.scm`
- Modify: `tein/tests/scheme_tests.rs`

**Step 1: Write the test file**

```scheme
;;; lazy evaluation tests — delay, force, make-promise

(import (scheme lazy))

;; basic delay/force
(define cnt 0)
(define p (delay (begin (set! cnt (+ cnt 1)) cnt)))
(test-equal "lazy/not-forced-yet" 0 cnt)
(test-equal "lazy/force-1" 1 (force p))
(test-equal "lazy/memoised" 1 (force p))  ; not re-evaluated
(test-equal "lazy/count-still-1" 1 cnt)

;; promise?
(test-true  "lazy/promise?" (promise? p))
(test-false "lazy/promise?-no" (promise? 42))

;; make-promise wraps already-forced value
(define q (make-promise 42))
(test-equal "lazy/make-promise" 42 (force q))
(test-true  "lazy/make-promise?" (promise? q))

;; streams via delay — basic take
(define (stream-cons h t) (cons h (delay t)))
(define (stream-car s) (car s))
(define (stream-cdr s) (force (cdr s)))

(define (integers-from n)
  (stream-cons n (integers-from (+ n 1))))

(define (stream-take n s)
  (if (= n 0) '()
      (cons (stream-car s) (stream-take (- n 1) (stream-cdr s)))))

(define nats (integers-from 0))
(test-equal "lazy/stream-take" '(0 1 2 3 4) (stream-take 5 nats))
```

**Step 2: Add test fn**

```rust
#[test]
fn test_scheme_lazy() {
    run_scheme_test(include_str!("scheme/lazy.scm"));
}
```

**Step 3: Run**

```bash
cd tein && cargo test test_scheme_lazy -- --nocapture
```

**Step 4: Commit**

```bash
git add tein/tests/scheme/lazy.scm tein/tests/scheme_tests.rs
git commit -m "test: delay/force/make-promise lazy evaluation coverage"
```

---

## Task 14: numbers_extended.scm — gcd/lcm, rounding, number syntax

**Files:**
- Create: `tein/tests/scheme/numbers_extended.scm`
- Modify: `tein/tests/scheme_tests.rs`

**Step 1: Write the test file**

```scheme
;;; extended number tests — gcd/lcm, rounding, exact/inexact edge cases

(import (scheme inexact))

;; gcd / lcm
(test-equal "num/gcd" 4 (gcd 12 8))
(test-equal "num/gcd-zero" 5 (gcd 5 0))
(test-equal "num/gcd-nullary" 0 (gcd))
(test-equal "num/lcm" 12 (lcm 4 6))
(test-equal "num/lcm-nullary" 1 (lcm))

;; floor, ceiling, round, truncate
(test-equal "num/floor"    -2.0 (floor -1.5))
(test-equal "num/ceiling"  -1.0 (ceiling -1.5))
(test-equal "num/round/even" 2.0 (round 2.5))   ; round to even
(test-equal "num/round/even2" 4.0 (round 3.5))  ; round to even
(test-equal "num/truncate" -1.0 (truncate -1.9))

;; exact rounding
(test-equal "num/floor-exact"    -2 (floor -2))
(test-equal "num/ceiling-exact"  -1 (ceiling -1))
(test-equal "num/truncate-exact" -1 (truncate -1))

;; expt
(test-equal "num/expt" 8 (expt 2 3))
(test-equal "num/expt-zero" 1 (expt 5 0))
(test-equal "num/sqrt-exact" 3 (sqrt 9))

;; inexact math (scheme inexact)
(test-true "num/finite?" (finite? 1.0))
(test-false "num/finite?-inf" (finite? +inf.0))
(test-true "num/infinite?" (infinite? +inf.0))
(test-true "num/nan?" (nan? +nan.0))

;; floor/ and truncate/
(define-values (fq fr) (floor/ 13 4))
(test-equal "num/floor/-q" 3 fq)
(test-equal "num/floor/-r" 1 fr)

(define-values (tq tr) (truncate/ -13 4))
(test-equal "num/truncate/-q" -3 tq)
(test-equal "num/truncate/-r" -1 tr)

;; number->string and string->number radix
(test-equal "num/->string-hex" "ff" (number->string 255 16))
(test-equal "num/->string-bin" "1010" (number->string 10 2))
(test-equal "num/string->-hex" 255 (string->number "ff" 16))
(test-equal "num/string->-bin" 10 (string->number "1010" 2))
(test-false "num/string->-invalid" (string->number "xyz"))
```

**Step 2: Add test fn**

```rust
#[test]
fn test_scheme_numbers_extended() {
    run_scheme_test(include_str!("scheme/numbers_extended.scm"));
}
```

**Step 3: Run**

```bash
cd tein && cargo test test_scheme_numbers_extended -- --nocapture
```

**Step 4: Commit**

```bash
git add tein/tests/scheme/numbers_extended.scm tein/tests/scheme_tests.rs
git commit -m "test: extended number coverage (gcd/lcm, rounding, inexact predicates)"
```

---

## Task 15: scheme_eval.scm — scheme's eval procedure and environments

**Files:**
- Create: `tein/tests/scheme/scheme_eval.scm`
- Modify: `tein/tests/scheme_tests.rs`

**Step 1: Write the test file**

```scheme
;;; eval tests — eval with environment specifiers

(import (scheme eval))

;; basic eval in interaction-environment
(test-equal "eval/basic" 42 (eval '(+ 40 2) (interaction-environment)))

;; eval can define bindings
(eval '(define eval-test-var 99) (interaction-environment))
(test-equal "eval/define" 99 (eval 'eval-test-var (interaction-environment)))

;; eval lambda
(test-equal "eval/lambda" 7
  (eval '((lambda (x y) (+ x y)) 3 4) (interaction-environment)))

;; scheme-report-environment (r7rs)
(test-equal "eval/scheme-report" 6
  (eval '(* 2 3) (scheme-report-environment 7)))
```

**Step 2: Add test fn**

```rust
#[test]
fn test_scheme_eval() {
    run_scheme_test(include_str!("scheme/scheme_eval.scm"));
}
```

**Step 3: Run**

```bash
cd tein && cargo test test_scheme_eval -- --nocapture
```

Note: `scheme-report-environment` behaviour varies. If `eval/scheme-report` fails, remove that assertion and add a comment explaining why.

**Step 4: Commit**

```bash
git add tein/tests/scheme/scheme_eval.scm tein/tests/scheme_tests.rs
git commit -m "test: eval and environment specifier coverage"
```

---

## Task 16: tein_foreign.scm — (tein foreign) scheme predicates

**Files:**
- Create: `tein/tests/scheme/tein_foreign.scm`
- Modify: `tein/tests/scheme_tests.rs`

**Step 1: Write the test file**

The `(tein foreign)` module provides pure-scheme predicates. We test these without a real Rust foreign type — verify predicates return false for non-foreign values and that the module loads correctly.

```scheme
;;; tein foreign module tests — predicates on non-foreign values

(import (tein foreign))

;; predicates on non-foreign objects
(test-false "foreign/foreign?-int"    (foreign? 42))
(test-false "foreign/foreign?-string" (foreign? "hello"))
(test-false "foreign/foreign?-list"   (foreign? '(1 2 3)))
(test-false "foreign/foreign?-bool"   (foreign? #t))
(test-false "foreign/foreign?-sym"    (foreign? 'sym))

;; foreign-type and foreign-handle-id raise on non-foreign
(test-error "foreign/type-non-foreign"      (lambda () (foreign-type 42)))
(test-error "foreign/handle-id-non-foreign" (lambda () (foreign-handle-id 42)))

;; verify foreign-types returns a list (may be empty without registration)
(test-true "foreign/types-is-list" (list? (foreign-types)))
```

**Step 2: Add test fn**

```rust
#[test]
fn test_scheme_tein_foreign() {
    run_scheme_test(include_str!("scheme/tein_foreign.scm"));
}
```

**Step 3: Run**

```bash
cd tein && cargo test test_scheme_tein_foreign -- --nocapture
```

**Step 4: Commit**

```bash
git add tein/tests/scheme/tein_foreign.scm tein/tests/scheme_tests.rs
git commit -m "test: (tein foreign) scheme predicate coverage"
```

---

## Task 17: Run full test suite, verify, and document findings

**Step 1: Run all scheme tests**

```bash
cd tein && cargo test test_scheme -- --nocapture 2>&1 | tail -30
```

Expected: all 21 scheme test fns pass (5 existing + 16 new).

**Step 2: Run full suite to catch regressions**

```bash
cd tein && cargo test 2>&1 | tail -10
```

Expected: all tests pass. Count should be 208 lib + 12 scheme_fn + 21 scheme + N doc-tests.

**Step 3: Document findings in ARCHITECTURE.md**

Add a new section (or update existing) in `tein/ARCHITECTURE.md` (or `docs/`) summarising:
- Which r7rs identifiers require `(import (scheme base))` vs available without import
- The call/cc re-entry quirk (let vs top-level define)
- The `error-object-message` vs `condition/report-string` finding
- Any other chibi quirks discovered during tasks 7–16

The "Confirmed import requirements" and "Discovered chibi quirks" sections at the top of
this plan file are the source of truth — consolidate them into the permanent docs.

**Step 4: Final commit**

```bash
git add -A
git commit -m "docs: document chibi/tein scheme environment quirks from test coverage work"
```

---

## Notes for the implementor

- **`condition/report-string` does not exist** — use `error-object-message` instead (confirmed).
- **`error?` does not exist** — use `error-object?` instead (confirmed).
- **`(import (scheme base))` needed** for: `when`, `unless`, `define-values`, `guard`,
  `error-object?`, `error-object-message`, `error-object-irritants`, `floor/`, `truncate/`.
- **`(if #f #f)` as unspecified**: r7rs says `(when #f ...)` returns an unspecified value. `(if #f #f)` produces it idiomatically for `test-equal`. If comparison fails, remove those specific assertions.
- **Fuel limits**: default step limit is 10M. The 100k-iteration TCO tests use ~100k steps. Well within budget.
- **TCO test failure mode**: if TCO is broken for a specific tail position, the test hits `StepLimitExceeded` or segfaults — both are bugs.
- **`(import (scheme eval))`**: the file is named `scheme_eval.scm` (not `eval.scm`) to avoid shadowing the built-in `eval` identifier in any tooling.
- **call/cc re-entry**: works within a single `evaluate()` call when state is in `let` scope. Does NOT work with top-level `define` bindings (chibi batch-compile quirk). See the "Discovered chibi quirks" section above.
- **`raise-continuable` return value**: handler return flows back to the call site. `(+ 1 (raise-continuable x))` with handler returning 99 → result is 100.
- **Order**: tasks 7–16 are independent — implement in any order. Commit after each task passes.
