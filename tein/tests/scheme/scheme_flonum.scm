;;; scheme/flonum — flonum constants and transcendentals (srfi/144)
;;; (scheme flonum) re-exports srfi/144 with r7rs comparison names fl= fl< fl> fl<= fl>=

(import (scheme flonum))

;; --- constants ---

;; fl-e: Euler's number ~2.71828
(test-true "fl/e-approx"
  (fl< (flabs (fl- fl-e 2.718281828459045)) 1e-10))

;; fl-pi: π ~3.14159
(test-true "fl/pi-approx"
  (fl< (flabs (fl- fl-pi 3.141592653589793)) 1e-10))

;; fl-greatest: finite maximum flonum (must be positive and finite)
(test-true  "fl/greatest-pos"    (fl> fl-greatest 0.0))
(test-true  "fl/greatest-finite" (flfinite? fl-greatest))
(test-false "fl/greatest*2-inf"  (flfinite? (fl* fl-greatest 2.0)))

;; fl-least: smallest positive flonum (subnormal threshold)
(test-true "fl/least-pos"  (fl> fl-least 0.0))
(test-true "fl/least-tiny" (fl< fl-least 1e-300))

;; fl-epsilon: machine epsilon (1.0 + epsilon != 1.0, 1.0 + epsilon/2 == 1.0)
(test-false "fl/epsilon+1-ne-1" (fl= (fl+ 1.0 fl-epsilon) 1.0))
(test-true  "fl/epsilon/2+1=1"  (fl= (fl+ 1.0 (fl/ fl-epsilon 2.0)) 1.0))

;; nan: r7rs literal +nan.0 — not equal to itself
(test-false "fl/nan-ne-self" (fl= +nan.0 +nan.0))
(test-true  "fl/nan?"        (flnan? +nan.0))

;; infinities: r7rs literals +inf.0 / -inf.0
(test-true  "fl/+inf-infinite" (flinfinite? +inf.0))
(test-true  "fl/-inf-negative" (fl< -inf.0 0.0))
(test-false "fl/+inf-nan"      (flnan? +inf.0))

;; --- transcendentals ---

;; flsin / flcos
(test-true "fl/sin-pi"   (fl< (flabs (flsin fl-pi)) 1e-10))
(test-true "fl/cos-pi"   (fl< (flabs (fl+ (flcos fl-pi) 1.0)) 1e-10))
(test-true "fl/sin-pi/2" (fl< (flabs (fl- (flsin (fl/ fl-pi 2.0)) 1.0)) 1e-10))

;; flexp / fllog round-trip
(test-true "fl/exp-log"
  (fl< (flabs (fl- (flexp (fllog 2.0)) 2.0)) 1e-10))

;; flsqrt
(test-true "fl/sqrt-4"
  (fl< (flabs (fl- (flsqrt 4.0) 2.0)) 1e-10))
(test-true "fl/sqrt-2"
  (fl< (flabs (fl- (flsqrt 2.0) 1.4142135623730951)) 1e-10))

;; flfloor / flceiling / fltruncate / flround
(test-equal "fl/floor"      1.0  (flfloor 1.7))
(test-equal "fl/floor-neg" -2.0  (flfloor -1.7))
(test-equal "fl/ceil"       2.0  (flceiling 1.2))
(test-equal "fl/trunc"      1.0  (fltruncate 1.9))
(test-equal "fl/round-even" 2.0  (flround 2.5))  ; banker's rounding

;; --- predicates ---
(test-true  "fl/finite-1"   (flfinite? 1.0))
(test-false "fl/finite-inf" (flfinite? +inf.0))
(test-false "fl/finite-nan" (flfinite? +nan.0))
(test-true  "fl/inf?-+inf"  (flinfinite? +inf.0))
(test-false "fl/inf?-1"     (flinfinite? 1.0))

;; --- arithmetic ---
(test-equal "fl/add" 5.0 (fl+ 2.0 3.0))
(test-equal "fl/mul" 6.0 (fl* 2.0 3.0))
(test-equal "fl/div" 2.5 (fl/ 5.0 2.0))
(test-true  "fl/max" (fl= 3.0 (flmax 1.0 2.0 3.0)))
(test-true  "fl/min" (fl= 1.0 (flmin 1.0 2.0 3.0)))
