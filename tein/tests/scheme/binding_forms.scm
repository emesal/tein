;;; binding form tests — let*, letrec, letrec*, named let, define-values

(import (scheme base))

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
