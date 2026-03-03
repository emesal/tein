;;; scheme/char — unicode-aware character operations

(import (scheme char))

;; predicates
(test-true  "char/alphabetic-latin"   (char-alphabetic? #\a))
(test-false "char/alphabetic-digit"   (char-alphabetic? #\0))
(test-true  "char/numeric"            (char-numeric? #\5))
(test-false "char/numeric-alpha"      (char-numeric? #\a))
(test-true  "char/whitespace-space"   (char-whitespace? #\space))
(test-true  "char/whitespace-newline" (char-whitespace? #\newline))
(test-true  "char/upper"              (char-upper-case? #\A))
(test-false "char/upper-lower"        (char-upper-case? #\a))
(test-true  "char/lower"              (char-lower-case? #\a))

;; case conversion
(test-equal "char/upcase"   #\A (char-upcase #\a))
(test-equal "char/downcase" #\a (char-downcase #\A))

;; unicode: greek uppercase Σ <-> lowercase σ
(test-equal "char/upcase-greek"   #\Σ (char-upcase #\σ))
(test-equal "char/downcase-greek" #\σ (char-downcase #\Σ))

;; string-upcase / string-downcase (r7rs via scheme/char)
(test-equal "char/string-upcase"   "HELLO" (string-upcase "hello"))
(test-equal "char/string-downcase" "hello" (string-downcase "HELLO"))
(test-equal "char/string-upcase-greek" "ΑΒΓΔ" (string-upcase "αβγδ"))

;; ci comparisons
(test-true  "char/ci=?"    (char-ci=? #\A #\a))
(test-false "char/ci=?-ne" (char-ci=? #\A #\b))
(test-true  "char/ci<?"    (char-ci<? #\a #\B))
