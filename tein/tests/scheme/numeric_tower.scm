;;; numeric tower tests — bignum, rational, complex

(import (scheme base))
(import (scheme complex))
(import (scheme inexact))

;; ── bignums ──────────────────────────────────────────────────────────────────

;; basic bignum
(test-equal "bignum/expt-2-100" 1267650600228229401496703205376 (expt 2 100))

;; bignums are exact integers
(test-true "bignum/integer?" (integer? (expt 2 100)))
(test-true "bignum/exact?" (exact? (expt 2 100)))

;; arithmetic preserves exactness
(test-equal "bignum/add" 1267650600228229401496703205377 (+ (expt 2 100) 1))
(test-equal "bignum/mul" (expt 2 101) (* (expt 2 100) 2))

;; negative bignum
(test-true "bignum/negative?" (negative? (- (expt 2 100))))

;; ── rationals ────────────────────────────────────────────────────────────────

;; basic rational
(test-equal "rational/basic" 1/3 (/ 1 3))

;; rationals are exact, rational, real
(test-true "rational/exact?" (exact? (/ 1 3)))
(test-true "rational/rational?" (rational? (/ 1 3)))
(test-true "rational/real?" (real? (/ 1 3)))

;; rational simplification
(test-equal "rational/reduce" 1/2 (/ 2 4))
(test-equal "rational/reduce-large" 1/3 (/ 1000000 3000000))

;; rational arithmetic
(test-equal "rational/add" 5/6 (+ 1/2 1/3))
(test-equal "rational/sub" 1/6 (- 1/2 1/3))
(test-equal "rational/mul" 1/6 (* 1/2 1/3))
(test-equal "rational/div" 3/2 (/ 1/2 1/3))

;; extractors
(test-equal "rational/numerator" 1 (numerator 1/3))
(test-equal "rational/denominator" 3 (denominator 1/3))

;; ── complex ──────────────────────────────────────────────────────────────────

;; avoid complex literals as source constants — chibi can't use them in
;; compiled positions; use make-rectangular and compare components instead.

;; basic construction and predicates
(test-true "complex/complex?" (complex? (make-rectangular 1 2)))
(test-true "complex/number?" (number? (make-rectangular 1 2)))

;; extractors
(test-equal "complex/real-part" 1 (real-part (make-rectangular 1 2)))
(test-equal "complex/imag-part" 2 (imag-part (make-rectangular 1 2)))
(test-equal "complex/imag-part-neg" -2 (imag-part (make-rectangular 1 -2)))

;; make-polar round-trip (approx equality for floats)
(test-true "complex/polar-real" (< (abs (- (real-part (make-polar 1.0 0.0)) 1.0)) 1e-10))
(test-true "complex/polar-imag" (< (abs (imag-part (make-polar 1.0 0.0))) 1e-10))

;; complex arithmetic — check via component accessors
(let ((sum (+ (make-rectangular 1 2) (make-rectangular 2 3))))
  (test-equal "complex/add-real" 3 (real-part sum))
  (test-equal "complex/add-imag" 5 (imag-part sum)))

(let ((prod (* (make-rectangular 1 2) (make-rectangular 2 3))))
  (test-equal "complex/mul-real" -4 (real-part prod))
  (test-equal "complex/mul-imag" 7 (imag-part prod)))
