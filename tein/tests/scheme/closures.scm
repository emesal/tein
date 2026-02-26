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
