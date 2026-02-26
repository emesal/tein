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
