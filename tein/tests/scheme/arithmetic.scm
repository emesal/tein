;;; arithmetic tests — basic numeric operations

;; addition
(test-equal "add/nullary" 0 (+))
(test-equal "add/unary" 5 (+ 5))
(test-equal "add/binary" 7 (+ 3 4))
(test-equal "add/ternary" 15 (+ 3 5 7))
(test-equal "add/negative" -1 (+ 3 -4))

;; subtraction
(test-equal "sub/unary" -5 (- 5))
(test-equal "sub/binary" 3 (- 7 4))
(test-equal "sub/ternary" 1 (- 10 5 4))

;; multiplication
(test-equal "mul/nullary" 1 (*))
(test-equal "mul/unary" 5 (* 5))
(test-equal "mul/binary" 12 (* 3 4))
(test-equal "mul/ternary" 60 (* 3 4 5))

;; division and quotient
(test-equal "div/exact" 3 (/ 9 3))
(test-equal "quotient" 3 (quotient 10 3))
(test-equal "remainder" 1 (remainder 10 3))
(test-equal "modulo" 1 (modulo 10 3))
(test-equal "modulo/negative" 2 (modulo -1 3))

;; comparisons
(test-true "lt" (< 1 2))
(test-false "lt/eq" (< 2 2))
(test-true "gt" (> 3 2))
(test-false "gt/eq" (> 2 2))
(test-true "le" (<= 2 2))
(test-true "le/lt" (<= 1 2))
(test-true "ge" (>= 2 2))
(test-true "ge/gt" (>= 3 2))
(test-true "num-eq" (= 5 5))
(test-false "num-neq" (= 5 6))

;; exact/inexact
(test-true "exact?" (exact? 42))
(test-true "inexact?" (inexact? 3.14))
(test-equal "exact->inexact" 3.0 (exact->inexact 3))
(test-equal "inexact->exact" 3 (inexact->exact 3.0))

;; predicates
(test-true "zero?/yes" (zero? 0))
(test-false "zero?/no" (zero? 1))
(test-true "positive?" (positive? 1))
(test-false "positive?/zero" (positive? 0))
(test-true "negative?" (negative? -1))
(test-false "negative?/zero" (negative? 0))
(test-true "even?" (even? 4))
(test-false "even?/odd" (even? 3))
(test-true "odd?" (odd? 3))
(test-false "odd?/even" (odd? 4))

;; min/max
(test-equal "min" 1 (min 3 1 2))
(test-equal "max" 3 (max 1 3 2))

;; abs
(test-equal "abs/pos" 5 (abs 5))
(test-equal "abs/neg" 5 (abs -5))
(test-equal "abs/zero" 0 (abs 0))
