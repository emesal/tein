;;; SRFI-19 integration tests
;;; adapted from reference test suite (MIT, Will Fitzgerald / I/NET Inc)

(import (srfi 19))

;; --- time type constants ---
(test-true "srfi19/time-type-symbols"
  (and (symbol? time-utc) (symbol? time-tai) (symbol? time-monotonic)
       (symbol? time-duration) (symbol? time-process) (symbol? time-thread)))

;; --- creating time structures ---
(test-true "srfi19/current-time-utc" (time? (current-time time-utc)))
(test-true "srfi19/current-time-tai" (time? (current-time time-tai)))
(test-true "srfi19/current-time-monotonic" (time? (current-time time-monotonic)))
(test-true "srfi19/current-time-default-utc" (time? (current-time)))

;; --- time-process/time-thread raise errors ---
(test-error "srfi19/time-process-unsupported"
  (lambda () (current-time time-process)))
(test-error "srfi19/time-thread-unsupported"
  (lambda () (current-time time-thread)))

;; --- time comparisons ---
(test-true "srfi19/time-equal"
  (let ((t1 (make-time time-utc 0 1000))
        (t2 (make-time time-utc 0 1000)))
    (time=? t1 t2)))

(test-true "srfi19/time-less"
  (time<? (make-time time-utc 0 1) (make-time time-utc 0 2)))

(test-true "srfi19/time-greater"
  (time>? (make-time time-utc 0 2) (make-time time-utc 0 1)))

(test-true "srfi19/time-leq"
  (and (time<=? (make-time time-utc 0 1) (make-time time-utc 0 1))
       (time<=? (make-time time-utc 0 1) (make-time time-utc 0 2))))

(test-true "srfi19/time-geq"
  (and (time>=? (make-time time-utc 0 2) (make-time time-utc 0 2))
       (time>=? (make-time time-utc 0 2) (make-time time-utc 0 1))))

;; --- time comparisons with nanoseconds ---
(test-true "srfi19/time-cmp-nanos"
  (let ((t1 (make-time time-utc 1001 1))
        (t2 (make-time time-utc 1001 1))
        (t3 (make-time time-utc 1001 2)))
    (and (time=? t1 t2) (time<? t1 t3) (time>? t3 t1))))

;; --- time difference ---
(test-true "srfi19/time-difference"
  (let ((t1 (make-time time-utc 0 3000))
        (t2 (make-time time-utc 0 1000)))
    (time=? (make-time time-duration 0 2000)
            (time-difference t1 t2))))

;; --- add/subtract duration ---
(test-true "srfi19/add-duration"
  (let ((t (make-time time-utc 0 1000))
        (d (make-time time-duration 0 500)))
    (time=? (make-time time-utc 0 1500) (add-duration t d))))

(test-true "srfi19/subtract-duration"
  (let ((t (make-time time-utc 0 1000))
        (d (make-time time-duration 0 500)))
    (time=? (make-time time-utc 0 500) (subtract-duration t d))))

;; --- TAI-UTC edge conversions ---
(test-true "srfi19/tai-utc-edge"
  (let* ((utc (make-time time-utc 0 915148800))    ;; 1999-01-01 boundary
         (tai (time-utc->time-tai utc))
         (back (time-tai->time-utc tai)))
    (time=? utc back)))

;; --- date creation and accessors ---
(test-true "srfi19/make-date"
  (let ((d (make-date 0 30 15 10 4 3 2026 0)))
    (and (date? d)
         (= (date-year d) 2026)
         (= (date-month d) 3)
         (= (date-day d) 4)
         (= (date-hour d) 10)
         (= (date-minute d) 15)
         (= (date-second d) 30)
         (= (date-nanosecond d) 0)
         (= (date-zone-offset d) 0))))

;; --- date <-> time-utc round-trip ---
(test-true "srfi19/date-time-utc-roundtrip"
  (let* ((d1 (make-date 0 0 0 0 1 1 2000 0))
         (t (date->time-utc d1))
         (d2 (time-utc->date t 0)))
    (and (= (date-year d1) (date-year d2))
         (= (date-month d1) (date-month d2))
         (= (date-day d1) (date-day d2))
         (= (date-hour d1) (date-hour d2))
         (= (date-minute d1) (date-minute d2))
         (= (date-second d1) (date-second d2)))))

;; --- date->string formatting ---
(test-equal "srfi19/date->string-iso"
  "2006-05-04T03:02:01"
  (date->string (make-date 0 1 2 3 4 5 2006 0) "~5"))

(test-equal "srfi19/date->string-time"
  "03:02:01"
  (date->string (make-date 0 1 2 3 4 5 2006 0) "~3"))

(test-equal "srfi19/date->string-tz"
  "2006-05-04T03:02:01Z"
  (date->string (make-date 0 1 2 3 4 5 2006 0) "~4"))

(test-equal "srfi19/date->string-padded-hour"
  " 3"
  (date->string (make-date 0 1 2 3 4 5 2006 0) "~k"))

;; --- string->date parsing ---
(test-true "srfi19/string->date"
  (let ((d (string->date "2026-03-04" "~Y-~m-~d")))
    (and (= (date-year d) 2026)
         (= (date-month d) 3)
         (= (date-day d) 4))))

;; --- julian day ---
(test-true "srfi19/julian-day-roundtrip"
  (let* ((d1 (make-date 0 0 0 12 1 1 2000 0))
         (jd (date->julian-day d1))
         (d2 (julian-day->date jd 0)))
    (and (= (date-year d1) (date-year d2))
         (= (date-month d1) (date-month d2))
         (= (date-day d1) (date-day d2)))))

;; --- current-date ---
(test-true "srfi19/current-date"
  (let ((d (current-date 0)))
    (and (date? d)
         (>= (date-year d) 2026))))

;; --- leap-year? ---
(test-true "srfi19/leap-year-2000" (leap-year? (make-date 0 0 0 0 1 1 2000 0)))
(test-true "srfi19/not-leap-year-1900"
  (not (leap-year? (make-date 0 0 0 0 1 1 1900 0))))
(test-true "srfi19/leap-year-2024" (leap-year? (make-date 0 0 0 0 1 1 2024 0)))
(test-true "srfi19/not-leap-year-2023"
  (not (leap-year? (make-date 0 0 0 0 1 1 2023 0))))

;; --- date-week-day (0=Sunday) ---
(test-true "srfi19/week-day"
  (let ((d (make-date 0 0 0 0 4 3 2026 0)))  ;; 2026-03-04 is Wednesday
    (= (date-week-day d) 3)))

;; --- date-year-day ---
(test-true "srfi19/year-day"
  (let ((d (make-date 0 0 0 0 1 3 2026 0)))  ;; March 1 = day 60 in non-leap
    (= (date-year-day d) 60)))

;; --- copy-time ---
(test-true "srfi19/copy-time"
  (let* ((t (make-time time-utc 123 456))
         (t2 (copy-time t)))
    (and (time=? t t2)
         (not (eq? t t2)))))

;; --- time-resolution ---
(test-true "srfi19/time-resolution"
  (and (integer? (time-resolution time-utc))
       (> (time-resolution time-utc) 0)))
