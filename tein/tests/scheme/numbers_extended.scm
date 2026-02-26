;;; extended number tests — gcd/lcm, rounding, exact/inexact edge cases

(import (scheme base))
(import (scheme inexact))

;; gcd / lcm
(test-equal "num/gcd" 4 (gcd 12 8))
(test-equal "num/gcd-zero" 5 (gcd 5 0))
(test-equal "num/gcd-nullary" 0 (gcd))
(test-equal "num/lcm" 12 (lcm 4 6))
(test-equal "num/lcm-nullary" 1 (lcm))

;; floor, ceiling, round, truncate
(test-equal "num/floor"    -2.0 (floor -1.5))
(test-equal "num/ceiling"  -1.0 (ceiling -1.5))
(test-equal "num/round/even" 2.0 (round 2.5))   ; round to even
(test-equal "num/round/even2" 4.0 (round 3.5))  ; round to even
(test-equal "num/truncate" -1.0 (truncate -1.9))

;; exact rounding
(test-equal "num/floor-exact"    -2 (floor -2))
(test-equal "num/ceiling-exact"  -1 (ceiling -1))
(test-equal "num/truncate-exact" -1 (truncate -1))

;; expt
(test-equal "num/expt" 8 (expt 2 3))
(test-equal "num/expt-zero" 1 (expt 5 0))
(test-equal "num/sqrt-exact" 3 (sqrt 9))

;; inexact math (scheme inexact)
(test-true "num/finite?" (finite? 1.0))
(test-false "num/finite?-inf" (finite? +inf.0))
(test-true "num/infinite?" (infinite? +inf.0))
(test-true "num/nan?" (nan? +nan.0))

;; floor/ and truncate/ — use call-with-values to avoid define-values batch-compile quirk
(call-with-values (lambda () (floor/ 13 4))
  (lambda (q r)
    (test-equal "num/floor/-q" 3 q)
    (test-equal "num/floor/-r" 1 r)))

(call-with-values (lambda () (truncate/ -13 4))
  (lambda (q r)
    (test-equal "num/truncate/-q" -3 q)
    (test-equal "num/truncate/-r" -1 r)))

;; number->string and string->number radix
(test-equal "num/->string-hex" "ff" (number->string 255 16))
(test-equal "num/->string-bin" "1010" (number->string 10 2))
(test-equal "num/string->-hex" 255 (string->number "ff" 16))
(test-equal "num/string->-bin" 10 (string->number "1010" 2))
(test-false "num/string->-invalid" (string->number "xyz"))
