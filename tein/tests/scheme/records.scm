;;; record type tests — define-record-type (r7rs)

(import (scheme base))

(define-record-type <point>
  (make-point x y)
  point?
  (x point-x)
  (y point-y))

(define p (make-point 3 4))
(test-true  "record/predicate" (point? p))
(test-false "record/predicate-other" (point? 42))
(test-equal "record/accessor-x" 3 (point-x p))
(test-equal "record/accessor-y" 4 (point-y p))

;; mutable fields
(define-record-type <counter>
  (make-counter val)
  counter?
  (val counter-val set-counter-val!))

(define ct (make-counter 0))
(set-counter-val! ct 5)
(test-equal "record/mutator" 5 (counter-val ct))

;; record with multiple mutable fields
(define-record-type <person>
  (make-person name age)
  person?
  (name person-name set-person-name!)
  (age  person-age  set-person-age!))

(define alice (make-person "Alice" 30))
(test-equal "record/person-name" "Alice" (person-name alice))
(set-person-age! alice 31)
(test-equal "record/person-age-mutated" 31 (person-age alice))
