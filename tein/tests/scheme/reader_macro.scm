;;; reader dispatch and macro expansion hook tests
;;;
;;; verifies that (tein reader) and (tein macro) work via import,
;;; including in sandboxed contexts (issue 31 fix).
;;;
;;; note: reader dispatch tests must be structured carefully because
;;; reader syntax like #j is parsed at read time, not eval time.
;;; we cannot use #j after unsetting the handler within the same
;;; evaluate() call, since the reader would encounter it first.

;; --- reader dispatch ---

(import (tein reader))

;; basic registration and invocation
(set-reader! #\j (lambda (port) 42))
(test-equal "reader/basic" 42 #j)

;; overwrite handler
(set-reader! #\j (lambda (port) 99))
(test-equal "reader/overwrite" 99 #j)

;; reserved char rejection
(test-error "reader/reserved-t" (lambda () (set-reader! #\t (lambda (port) 0))))
(test-error "reader/reserved-f" (lambda () (set-reader! #\f (lambda (port) 0))))

;; introspection
(set-reader! #\k (lambda (port) 2))
(let ((chars (reader-dispatch-chars)))
  (test-true "reader/chars-is-list" (list? chars))
  (test-true "reader/chars-has-j" (member #\j chars))
  (test-true "reader/chars-has-k" (member #\k chars)))

;; unset and verify via introspection (can't use #j after unsetting —
;; it would be a read-time error within the same evaluate() call)
(unset-reader! #\j)
(unset-reader! #\k)
(test-equal "reader/unset-empty" '() (reader-dispatch-chars))

;; --- macro expansion hook ---

(import (tein macro))

;; baseline: no hook
(test-false "macro-hook/initial" (macro-expand-hook))

;; set hook, verify it fires
(define-syntax double (syntax-rules () ((double x) (+ x x))))
(define hook-fired #f)
(set-macro-expand-hook!
  (lambda (name unexpanded expanded env)
    (set! hook-fired #t)
    expanded))
(double 5)
(test-true "macro-hook/fired" hook-fired)

;; introspection
(test-true "macro-hook/get" (procedure? (macro-expand-hook)))

;; unset hook
(unset-macro-expand-hook!)
(test-false "macro-hook/unset" (macro-expand-hook))
