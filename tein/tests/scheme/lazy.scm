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
;; note: stream-cons must be a macro to delay the tail without eager evaluation
(define-syntax stream-cons
  (syntax-rules ()
    ((stream-cons h t) (cons h (delay t)))))
(define (stream-car s) (car s))
(define (stream-cdr s) (force (cdr s)))

(define (integers-from n)
  (stream-cons n (integers-from (+ n 1))))

(define (stream-take n s)
  (if (= n 0) '()
      (cons (stream-car s) (stream-take (- n 1) (stream-cdr s)))))

(define nats (integers-from 0))
(test-equal "lazy/stream-take" '(0 1 2 3 4) (stream-take 5 nats))
