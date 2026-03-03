;;; scheme/division — ceiling, euclidean, round, balanced division (r7rs appendix)
;;; note: floor-quotient/floor-remainder are in scheme/base, not scheme/division.
;;; scheme/division adds: ceiling, euclidean, round, balanced variants.

(import (scheme division))

;; ceiling-quotient: rounds toward +inf
(test-equal "div/ceil-q-pos"  3  (ceiling-quotient  5 2))
(test-equal "div/ceil-q-neg" -2  (ceiling-quotient -5 2))
(test-equal "div/ceil-r-pos" -1  (ceiling-remainder  5 2))
(test-equal "div/ceil-r-neg" -1  (ceiling-remainder -5 2))

;; ceiling/ returns two values
(test-equal "div/ceil/-q" 3
  (call-with-values (lambda () (ceiling/ 5 2)) (lambda (q r) q)))
(test-equal "div/ceil/-r" -1
  (call-with-values (lambda () (ceiling/ 5 2)) (lambda (q r) r)))

;; euclidean-quotient: remainder always non-negative
(test-equal "div/eucl-q-pos"  2  (euclidean-quotient  5 2))
(test-equal "div/eucl-q-neg" -3  (euclidean-quotient -5 2))
(test-equal "div/eucl-r-pos"  1  (euclidean-remainder  5 2))
(test-equal "div/eucl-r-neg"  1  (euclidean-remainder -5 2))  ; always >= 0

;; round-quotient: tie-break to nearest even
(test-equal "div/round-q-2"  2  (round-quotient  5 2))  ; rounds to even (2)
(test-equal "div/round-q-neg" -2 (round-quotient -5 2))

;; balanced/: remainder in (-divisor/2, divisor/2]
(test-equal "div/balanced-r"  1  (balanced-remainder  5 4))
(test-equal "div/balanced-r2" -1 (balanced-remainder  3 4))
