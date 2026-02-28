;;; (tein toml) integration tests

(import (tein toml))

;; --- toml-parse ---

;; basic types
(let ((v (toml-parse "x = 42")))
  (test-equal "parse/integer" 42 (cdr (car v))))

(let ((v (toml-parse "x = 3.14")))
  (test-equal "parse/float" 3.14 (cdr (car v))))

(let ((v (toml-parse "x = \"hello\"")))
  (test-equal "parse/string" "hello" (cdr (car v))))

(let ((v (toml-parse "x = true")))
  (test-equal "parse/true" #t (cdr (car v))))

(let ((v (toml-parse "x = false")))
  (test-equal "parse/false" #f (cdr (car v))))

;; arrays
(let ((v (toml-parse "x = [1, 2, 3]")))
  (test-equal "parse/array" '(1 2 3) (cdr (car v))))

(let ((v (toml-parse "x = []")))
  (test-equal "parse/empty-array" '() (cdr (car v))))

;; nested table
(let ((v (toml-parse "[server]\nhost = \"localhost\"\nport = 8080")))
  (test-true "parse/nested-table" (list? v))
  (test-equal "parse/nested-key" "server" (car (car v)))
  (test-true "parse/nested-val-is-alist" (list? (cdr (car v)))))

;; datetime — all 4 variants
(let ((v (toml-parse "dt = 1979-05-27T07:32:00Z")))
  (test-equal "parse/datetime-offset"
    (list 'toml-datetime "1979-05-27T07:32:00Z")
    (cdr (car v))))

(let ((v (toml-parse "dt = 1979-05-27T07:32:00")))
  (test-equal "parse/datetime-local"
    (list 'toml-datetime "1979-05-27T07:32:00")
    (cdr (car v))))

(let ((v (toml-parse "dt = 1979-05-27")))
  (test-equal "parse/datetime-date"
    (list 'toml-datetime "1979-05-27")
    (cdr (car v))))

(let ((v (toml-parse "dt = 07:32:00")))
  (test-equal "parse/datetime-time"
    (list 'toml-datetime "07:32:00")
    (cdr (car v))))

;; --- toml-stringify ---

(test-true "stringify/simple"
  (string? (toml-stringify '(("name" . "tein")))))

;; --- round-trip ---

(test-equal "round-trip/datetime"
  (list 'toml-datetime "1979-05-27T07:32:00Z")
  (cdr (car (toml-parse (toml-stringify
    '(("dt" . (toml-datetime "1979-05-27T07:32:00Z"))))))))
