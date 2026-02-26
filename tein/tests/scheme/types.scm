;;; type predicate tests — type checking and basic conversions

;; type predicates
(test-true "integer?" (integer? 42))
(test-false "integer?/float" (integer? 3.14))
(test-true "number?" (number? 42))
(test-true "number?/float" (number? 3.14))
(test-true "string?" (string? "hello"))
(test-false "string?/num" (string? 42))
(test-true "boolean?" (boolean? #t))
(test-true "boolean?/false" (boolean? #f))
(test-false "boolean?/num" (boolean? 0))
(test-true "char?" (char? #\a))
(test-false "char?/str" (char? "a"))
(test-true "pair?" (pair? '(1 2)))
(test-false "pair?/null" (pair? '()))
(test-true "null?" (null? '()))
(test-false "null?/pair" (null? '(1)))
(test-true "vector?" (vector? #(1 2 3)))
(test-false "vector?/list" (vector? '(1 2 3)))
(test-true "procedure?" (procedure? car))
(test-true "procedure?/lambda" (procedure? (lambda (x) x)))
(test-false "procedure?/num" (procedure? 42))
(test-true "symbol?" (symbol? 'foo))
(test-false "symbol?/str" (symbol? "foo"))

;; boolean operations
(test-true "not/false" (not #f))
(test-false "not/true" (not #t))
(test-false "not/num" (not 42))

;; char conversions
(test-equal "char->integer" 65 (char->integer #\A))
(test-equal "integer->char" #\A (integer->char 65))
(test-true "char-alphabetic?" (char-alphabetic? #\a))
(test-false "char-alphabetic?/num" (char-alphabetic? #\0))
(test-true "char-numeric?" (char-numeric? #\0))
(test-false "char-numeric?/alpha" (char-numeric? #\a))

;; vector operations
(test-equal "vector-length" 3 (vector-length #(1 2 3)))
(test-equal "vector-ref" 2 (vector-ref #(1 2 3) 1))
(test-equal "make-vector" #(0 0 0) (make-vector 3 0))
(test-equal "vector->list" '(1 2 3) (vector->list #(1 2 3)))
(test-equal "list->vector" #(1 2 3) (list->vector '(1 2 3)))

;; equivalence
(test-true "eq?/symbols" (eq? 'a 'a))
(test-true "eq?/booleans" (eq? #t #t))
(test-true "eqv?/numbers" (eqv? 42 42))
(test-true "equal?/lists" (equal? '(1 2 3) '(1 2 3)))
(test-false "equal?/different" (equal? '(1 2) '(1 3)))

;; values/call-with-values
(test-equal "values/single" 42
  (call-with-values (lambda () (values 42)) (lambda (x) x)))
