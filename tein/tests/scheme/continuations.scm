;;; continuation tests — call/cc, dynamic-wind, values/call-with-values

(import (scheme base))

;; basic escape continuation
(test-equal "callcc/escape" 42
  (call-with-current-continuation
    (lambda (k) (k 42) 99)))

;; escape from nested context
(test-equal "callcc/nested-escape" 1
  (+ 1 (call/cc (lambda (k) (+ 2 (k 0))))))

;; re-entrant continuation via let-local state
;; (top-level define + call/cc re-entry doesn't work in chibi's
;;  batch-compiled toplevel — use let to keep state in closure scope)
(test-equal "callcc/reentry" 3
  (let ((k #f) (n 0))
    (call/cc (lambda (c) (set! k c)))
    (set! n (+ n 1))
    (if (< n 3) (k 'ignored) n)))

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
