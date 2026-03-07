;;; (chibi regexp) SRFI-115 SRE smoke tests

(import (chibi regexp))

;; --- compilation + predicates ---

(test-true "regexp/compile-sre" (regexp? (regexp '(+ digit))))
(test-true "regexp/compile-string-literal" (regexp? (regexp "hello")))
(test-false "regexp/string-not-regexp" (regexp? "hello"))
(test-false "regexp/integer-not-regexp" (regexp? 42))
(test-true "regexp/valid-sre-digit" (valid-sre? '(+ digit)))
(test-true "regexp/valid-sre-or" (valid-sre? '(or "a" "b")))

;; --- regexp-matches (full match) ---

(test-true "regexp/matches?-digits" (regexp-matches? '(+ digit) "42"))
(test-false "regexp/matches?-partial-rejects" (regexp-matches? '(+ digit) "abc42"))
(test-true "regexp/matches-returns-match" (regexp-match? (regexp-matches '(+ digit) "42")))
(test-false "regexp/matches-rejects-partial" (regexp-matches '(+ digit) "abc42"))

(test-equal "regexp/matches-submatch-0" "42"
  (regexp-match-submatch (regexp-matches '(+ digit) "42") 0))

;; --- regexp-search ---

(test-true "regexp/search-returns-match"
  (regexp-match? (regexp-search '(+ digit) "abc42def")))
(test-false "regexp/search-no-match"
  (regexp-search '(+ digit) "no digits here!"))
(test-equal "regexp/search-submatch-0" "42"
  (regexp-match-submatch (regexp-search '(+ digit) "abc42def") 0))

;; --- submatches: indexed ($) ---

(let ((m (regexp-search '(: ($ (+ alpha)) "=" ($ (+ digit))) "foo=123")))
  (test-true "regexp/indexed-submatch-is-match" (regexp-match? m))
  (test-equal "regexp/indexed-submatch-whole" "foo=123"
    (regexp-match-submatch m 0))
  (test-equal "regexp/indexed-submatch-1" "foo"
    (regexp-match-submatch m 1))
  (test-equal "regexp/indexed-submatch-2" "123"
    (regexp-match-submatch m 2))
  (test-equal "regexp/match-count" 2 (regexp-match-count m)))

;; --- submatches: named (->) ---

(let ((m (regexp-search '(: (-> key (+ alpha)) "=" (-> val (+ digit))) "foo=123")))
  (test-equal "regexp/named-submatch-key" "foo"
    (regexp-match-submatch m 'key))
  (test-equal "regexp/named-submatch-val" "123"
    (regexp-match-submatch m 'val)))

;; --- match->list ---

(let ((m (regexp-matches '(: ($ (+ alpha)) "-" ($ (+ digit))) "abc-99")))
  (test-equal "regexp/match->list" '("abc-99" "abc" "99")
    (regexp-match->list m)))

;; --- replace ---

(test-equal "regexp/replace-first" "X1b2c3"
  (regexp-replace '(+ alpha) "a1b2c3" "X"))
(test-equal "regexp/replace-all" "XbXcX"
  (regexp-replace-all '(+ digit) "1b2c3" "X"))
(test-equal "regexp/replace-no-match" "hello"
  (regexp-replace '(+ digit) "hello" "X"))

;; --- split ---

(test-equal "regexp/split-comma" '("a" "b" "c")
  (regexp-split '(+ ",") "a,b,c"))
(test-equal "regexp/split-no-delimiter" '("hello")
  (regexp-split '(+ ",") "hello"))
(test-equal "regexp/split-whitespace" '("one" "two" "three")
  (regexp-split '(+ space) "one two  three"))

;; --- extract ---

(test-equal "regexp/extract-digits" '("1" "22" "333")
  (regexp-extract '(+ digit) "a1b22c333"))
(test-equal "regexp/extract-no-match" '()
  (regexp-extract '(+ digit) "no digits"))

;; --- fold ---

(test-equal "regexp/fold-collect" '("333" "22" "1")
  (regexp-fold '(+ digit)
    (lambda (i m s acc)
      (cons (regexp-match-submatch m 0) acc))
    '()
    "a1b22c333"))

(test-equal "regexp/fold-count" 3
  (regexp-fold '(+ digit)
    (lambda (i m s acc) (+ acc 1))
    0
    "a1b22c333"))

(test-equal "regexp/fold-no-match" 0
  (regexp-fold '(+ digit)
    (lambda (i m s acc) (+ acc 1))
    0
    "no digits"))

;; --- SRE syntax: or, w/nocase, :, = (repetition) ---

(test-true "regexp/sre-or"
  (regexp-matches? '(or "cat" "dog") "dog"))
(test-false "regexp/sre-or-no-match"
  (regexp-matches? '(or "cat" "dog") "fish"))

(test-true "regexp/sre-nocase"
  (regexp-matches? '(w/nocase "hello") "HeLLo"))
(test-false "regexp/sre-nocase-wrong"
  (regexp-matches? '(w/nocase "hello") "world"))

(test-true "regexp/sre-seq"
  (regexp-matches? '(: "ab" "cd") "abcd"))
(test-false "regexp/sre-seq-partial"
  (regexp-matches? '(: "ab" "cd") "abce"))

(test-true "regexp/sre-exact-repetition"
  (regexp-matches? '(= 3 digit) "123"))
(test-false "regexp/sre-exact-repetition-wrong-count"
  (regexp-matches? '(= 3 digit) "12"))

;; --- round-trip: regexp->sre ---

(let ((sre '(: "a" (+ digit) "b")))
  (test-true "regexp/round-trip-sre"
    (regexp? (regexp (regexp->sre (regexp sre))))))
