;;; (tein time) scheme-level tests

(import (tein time))

;; current-second returns an inexact number
(test-true "time/current-second-inexact" (inexact? (current-second)))

;; current-second is positive
(test-true "time/current-second-positive" (> (current-second) 0))

;; current-second is a reasonable recent timestamp (after 2025-01-01)
(test-true "time/current-second-recent" (> (current-second) 1735689600))

;; current-jiffy returns an exact integer
(test-true "time/current-jiffy-exact" (exact? (current-jiffy)))
(test-true "time/current-jiffy-integer" (integer? (current-jiffy)))

;; current-jiffy is non-negative
(test-true "time/current-jiffy-non-negative" (>= (current-jiffy) 0))

;; current-jiffy is monotonic (let* ensures sequential evaluation)
(test-true "time/current-jiffy-monotonic"
  (let* ((a (current-jiffy))
         (b (current-jiffy)))
    (>= b a)))

;; jiffies-per-second is 10^9
(test-equal "time/jiffies-per-second" 1000000000 jiffies-per-second)

;; elapsed time via jiffies is consistent with jiffies-per-second (let* ensures order)
(test-true "time/elapsed-seconds"
  (let* ((start (current-jiffy))
         (end (current-jiffy)))
    (>= (/ (- end start) jiffies-per-second) 0)))
