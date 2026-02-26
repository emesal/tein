;;; error handling tests — raise, guard, error objects, condition types

(import (scheme base))

;; test-error verifies that a thunk raises an exception
(test-error "error/basic" (lambda () (error "boom" 1 2)))

;; guard: catch and inspect error object
(test-equal "guard/message" "oops"
  (guard (exn ((error-object? exn) (error-object-message exn)))
    (error "oops")))

;; error-object-irritants
(test-equal "guard/irritants" '(1 2)
  (guard (exn ((error-object? exn) (error-object-irritants exn)))
    (error "msg" 1 2)))

;; guard/else
(test-equal "guard/else" 'caught
  (guard (exn (else 'caught))
    (error "anything")))

;; guard reraises if no clause matches — verify via outer guard
(test-equal "guard/reraise" 'outer
  (guard (outer-exn (else 'outer))
    (guard (inner-exn ((string? inner-exn) 'wrong-type))
      (error "not a plain string"))))

;; error-object? predicate
(test-true "error-object?/yes"
  (guard (e (#t (error-object? e)))
    (error "test")))

;; raise with non-error object
(test-equal "raise/non-error" 42
  (guard (e (#t e))
    (raise 42)))

;; raise-continuable (handler return value flows back to raise site)
;; handler returns 99, then (+ 1 99) = 100
(test-equal "raise-continuable" 100
  (with-exception-handler
    (lambda (e) 99)
    (lambda () (+ 1 (raise-continuable 'ignored)))))
