;;; control flow tests — cond, case, when, unless, and, or, do

(import (scheme base))

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
