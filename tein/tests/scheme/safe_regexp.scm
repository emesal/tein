;;; (tein safe-regexp) integration tests

(import (tein safe-regexp))

;; --- compilation + predicate ---

(test-true "safe-regexp/regexp-is-regexp" (regexp? (regexp "\\d+")))
(test-false "safe-regexp/string-not-regexp" (regexp? "hello"))
(test-false "safe-regexp/integer-not-regexp" (regexp? 42))
(test-true "safe-regexp/invalid-pattern-returns-string"
  (string? (regexp "[")))

;; --- search ---

(test-true "safe-regexp/search-finds-match"
  (vector? (regexp-search "\\d+" "abc42def")))
(test-false "safe-regexp/search-no-match"
  (regexp-search "xyz" "abc"))
(test-equal "safe-regexp/search-whole-match-text" "42"
  (vector-ref (vector-ref (regexp-search "\\d+" "abc42def") 0) 0))
(test-equal "safe-regexp/search-capture-group" "7"
  (vector-ref (vector-ref (regexp-search "(\\d+)-(\\d+)" "x-42-7") 2) 0))

;; --- matches ---

(test-true "safe-regexp/matches?-full" (regexp-matches? "\\d+" "42"))
(test-false "safe-regexp/matches?-partial-rejects" (regexp-matches? "\\d+" "abc42"))
(test-true "safe-regexp/matches-returns-vector"
  (vector? (regexp-matches "\\d+" "42")))
(test-false "safe-regexp/matches-rejects-partial"
  (regexp-matches "\\d+" "abc42"))

;; --- replace ---

(test-equal "safe-regexp/replace-first" "aXb2c3"
  (regexp-replace "\\d+" "a1b2c3" "X"))
(test-equal "safe-regexp/replace-all" "aXbXcX"
  (regexp-replace-all "\\d+" "a1b2c3" "X"))
(test-equal "safe-regexp/replace-no-match" "hello"
  (regexp-replace "xyz" "hello" "X"))

;; --- split ---

(test-equal "safe-regexp/split-basic" '("a" "b" "c")
  (regexp-split "," "a,b,c"))
(test-equal "safe-regexp/split-no-delimiter" '("hello")
  (regexp-split "," "hello"))

;; --- extract ---

(test-equal "safe-regexp/extract-count" 3
  (length (regexp-extract "\\d+" "a1b22c333")))

;; --- match accessors ---

(let ((m (regexp-search "(a)(b)?(c)" "ac")))
  (test-equal "safe-regexp/match-count" 4 (regexp-match-count m))
  (test-equal "safe-regexp/submatch-0" "ac" (regexp-match-submatch m 0))
  (test-false "safe-regexp/submatch-unmatched-group"
    (regexp-match-submatch m 2))
  (test-equal "safe-regexp/match->list" '("ac" "a" #f "c")
    (regexp-match->list m)))

;; --- fold ---

(test-equal "safe-regexp/fold-collect" '("333" "22" "1")
  (regexp-fold "\\d+"
    (lambda (i m s acc)
      (cons (regexp-match-submatch m 0) acc))
    '()
    "a1b22c333"))

(test-equal "safe-regexp/fold-count" 3
  (regexp-fold "\\d+"
    (lambda (i m s acc) (+ acc 1))
    0
    "a1b22c333"))

(test-equal "safe-regexp/fold-no-match" 0
  (regexp-fold "\\d+"
    (lambda (i m s acc) (+ acc 1))
    0
    "no numbers"))

;; --- string-or-regexp dispatch ---

(test-equal "safe-regexp/search-with-string" "42"
  (vector-ref (vector-ref (regexp-search "\\d+" "abc42") 0) 0))
(test-equal "safe-regexp/search-with-compiled" "42"
  (let ((rx (regexp "\\d+")))
    (vector-ref (vector-ref (regexp-search rx "abc42") 0) 0)))
