;;; tail call tests — tco correctness via large iteration counts

(import (scheme base))

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
