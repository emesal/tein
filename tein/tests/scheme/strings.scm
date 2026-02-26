;;; string tests — creation, access, manipulation

;; basics
(test-equal "string-length" 5 (string-length "hello"))
(test-equal "string-length/empty" 0 (string-length ""))
(test-equal "string-ref" #\e (string-ref "hello" 1))

;; substring
(test-equal "substring" "ell" (substring "hello" 1 4))
(test-equal "substring/full" "hello" (substring "hello" 0 5))
(test-equal "substring/empty" "" (substring "hello" 2 2))

;; append
(test-equal "string-append" "hello world" (string-append "hello" " " "world"))
(test-equal "string-append/empty" "hello" (string-append "hello" ""))
(test-equal "string-append/nullary" "" (string-append))

;; comparisons
(test-true "string=?" (string=? "abc" "abc"))
(test-false "string=?/neq" (string=? "abc" "abd"))
(test-true "string<?" (string<? "abc" "abd"))
(test-false "string<?/eq" (string<? "abc" "abc"))
(test-true "string>?" (string>? "abd" "abc"))

;; predicates
(test-true "string?" (string? "hello"))
(test-false "string?/num" (string? 42))

;; conversions
(test-equal "number->string" "42" (number->string 42))
(test-equal "string->number" 42 (string->number "42"))
(test-false "string->number/bad" (string->number "nope"))
(test-equal "symbol->string" "hello" (symbol->string 'hello))
(test-equal "string->symbol" 'hello (string->symbol "hello"))
(test-equal "string->list" '(#\h #\i) (string->list "hi"))
(test-equal "list->string" "hi" (list->string '(#\h #\i)))

;; string-copy
(let ((s (string-copy "hello")))
  (test-equal "string-copy" "hello" s)
  (test-true "string-copy/eq" (string=? s "hello")))
