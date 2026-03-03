;;; scheme/fixnum — fixed-width integer operations (srfi/143)

(import (scheme fixnum))

;; arithmetic
(test-equal "fx/add"  5   (fx+ 2 3))
(test-equal "fx/sub"  1   (fx- 3 2))
(test-equal "fx/mul"  6   (fx* 2 3))
(test-equal "fx/neg" -3   (fx- 3))
(test-equal "fx/abs"  3   (fxabs -3))

;; comparisons
(test-true  "fx/=?"    (fx=? 3 3))
(test-false "fx/=?-ne" (fx=? 3 4))
(test-true  "fx/<?"    (fx<? 2 3))
(test-true  "fx/<=?"   (fx<=? 3 3))
(test-true  "fx/>?"    (fx>? 4 3))

;; bitwise
(test-equal "fx/and"    2  (fxand  6 3))   ; 110 & 011 = 010
(test-equal "fx/ior"    7  (fxior  6 3))   ; 110 | 011 = 111
(test-equal "fx/xor"    5  (fxxor  6 3))   ; 110 ^ 011 = 101
(test-equal "fx/not"   -7  (fxnot  6))
(test-equal "fx/shift-l" 12 (fxarithmetic-shift 3  2))
(test-equal "fx/shift-r"  1 (fxarithmetic-shift 4 -2))

;; constants
(test-true "fx/width-positive" (> fx-width 0))
(test-true "fx/greatest-pos"   (> fx-greatest 0))
(test-true "fx/least-neg"      (< fx-least 0))
(test-equal "fx/greatest-least-sym"
  (+ fx-greatest fx-least) -1)
