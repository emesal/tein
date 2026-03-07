;;; (tein json) integration tests

(import (tein json))

;; --- json-parse ---

(test-equal "parse/integer" 42 (json-parse "42"))
(test-equal "parse/float" 3.14 (json-parse "3.14"))
(test-equal "parse/string" "hello" (json-parse "\"hello\""))
(test-equal "parse/true" #t (json-parse "true"))
(test-equal "parse/false" #f (json-parse "false"))
(test-equal "parse/null" 'null (json-parse "null"))
(test-equal "parse/empty-array" '() (json-parse "[]"))
(test-equal "parse/array" '(1 2 3) (json-parse "[1, 2, 3]"))

;; object → alist
(let ((obj (json-parse "{\"a\": 1}")))
  (test-true "parse/object-is-list" (list? obj))
  (test-equal "parse/object-key" "a" (car (car obj)))
  (test-equal "parse/object-val" 1 (cdr (car obj))))

;; nested null
(test-equal "parse/null-in-array" '(1 null 3)
  (json-parse "[1, null, 3]"))

;; unicode
(test-equal "parse/unicode" "こんにちは"
  (json-parse "\"こんにちは\""))

;; --- json-stringify ---

(test-equal "stringify/integer" "42" (json-stringify 42))
(test-equal "stringify/string" "\"hello\"" (json-stringify "hello"))
(test-equal "stringify/true" "true" (json-stringify #t))
(test-equal "stringify/false" "false" (json-stringify #f))
(test-equal "stringify/null" "null" (json-stringify 'null))
(test-equal "stringify/array" "[1,2,3]" (json-stringify '(1 2 3)))

;; alist → object
(test-equal "stringify/object"
  "{\"name\":\"tein\"}"
  (json-stringify '(("name" . "tein"))))

;; --- round-trip ---

(test-equal "round-trip/object"
  "{\"a\":1,\"b\":\"two\"}"
  (json-stringify (json-parse "{\"a\":1,\"b\":\"two\"}")))

(test-equal "round-trip/array"
  "[1,2,3]"
  (json-stringify (json-parse "[1,2,3]")))

(test-equal "round-trip/null"
  "null"
  (json-stringify (json-parse "null")))

(test-equal "round-trip/nested"
  "{\"x\":{\"y\":1}}"
  (json-stringify (json-parse "{\"x\":{\"y\":1}}")))

(test-equal "round-trip/mixed"
  "[1,\"two\",true,null]"
  (json-stringify (json-parse "[1,\"two\",true,null]")))
