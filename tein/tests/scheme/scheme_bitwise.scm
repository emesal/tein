;;; scheme/bitwise — arbitrary-precision bitwise operations (srfi/151)

(import (scheme bitwise))

;; basic ops
(test-equal "bw/and"    4  (bitwise-and  12  5))   ; 1100 & 0101 = 0100
(test-equal "bw/ior"   13  (bitwise-ior  12  5))   ; 1100 | 0101 = 1101
(test-equal "bw/xor"    9  (bitwise-xor  12  5))   ; 1100 ^ 0101 = 1001
(test-equal "bw/not"   -6  (bitwise-not   5))
(test-equal "bw/eqv"  -10  (bitwise-eqv  12  5))   ; ~xor

;; shifts
(test-equal "bw/shift-l"   12 (arithmetic-shift  3  2))
(test-equal "bw/shift-r"    1 (arithmetic-shift  4 -2))
(test-equal "bw/shift-neg" -4 (arithmetic-shift -1  2))

;; bit-count / bit-set?
(test-equal "bw/bit-count"   3  (bit-count  7))   ; 0b111
(test-equal "bw/bit-count-0" 0  (bit-count  0))
(test-true  "bw/bit-set?-t"     (bit-set? 2 7))   ; bit 2 of 0b111
(test-false "bw/bit-set?-f"     (bit-set? 3 7))   ; bit 3 of 0b0111

;; integer-length
(test-equal "bw/int-length-0" 0 (integer-length 0))
(test-equal "bw/int-length-1" 1 (integer-length 1))
(test-equal "bw/int-length-7" 3 (integer-length 7))  ; 0b111 needs 3 bits

;; large integers (arbitrary precision)
(test-equal "bw/large-and"
  (expt 2 64)
  (bitwise-and (+ (expt 2 64) (expt 2 32))
               (+ (expt 2 64) 1)))
