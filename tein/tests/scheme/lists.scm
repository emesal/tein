;;; list tests — cons cells, list operations, higher-order fns

;; construction
(test-equal "cons" '(1 . 2) (cons 1 2))
(test-equal "cons/list" '(1 2 3) (cons 1 '(2 3)))
(test-equal "list" '(1 2 3) (list 1 2 3))
(test-equal "list/empty" '() (list))

;; accessors
(test-equal "car" 1 (car '(1 2 3)))
(test-equal "cdr" '(2 3) (cdr '(1 2 3)))
(test-equal "caar" 1 (car (car '((1 2) 3))))
(test-equal "cdar" '(2) (cdr (car '((1 2) 3))))

;; predicates
(test-true "null?/yes" (null? '()))
(test-false "null?/no" (null? '(1)))
(test-true "pair?/yes" (pair? '(1 2)))
(test-false "pair?/null" (pair? '()))
(test-true "list?/yes" (list? '(1 2 3)))
(test-true "list?/empty" (list? '()))
(test-false "list?/pair" (list? '(1 . 2)))

;; length and reverse
(test-equal "length" 3 (length '(1 2 3)))
(test-equal "length/empty" 0 (length '()))
(test-equal "reverse" '(3 2 1) (reverse '(1 2 3)))
(test-equal "reverse/empty" '() (reverse '()))

;; append
(test-equal "append" '(1 2 3 4) (append '(1 2) '(3 4)))
(test-equal "append/empty-left" '(3 4) (append '() '(3 4)))
(test-equal "append/empty-right" '(1 2) (append '(1 2) '()))

;; map and for-each
(test-equal "map" '(2 4 6) (map (lambda (x) (* x 2)) '(1 2 3)))
(test-equal "map/empty" '() (map (lambda (x) x) '()))

(let ((sum 0))
  (for-each (lambda (x) (set! sum (+ sum x))) '(1 2 3))
  (test-equal "for-each" 6 sum))

;; assoc
(test-equal "assoc/found" '(b 2) (assoc 'b '((a 1) (b 2) (c 3))))
(test-false "assoc/missing" (assoc 'd '((a 1) (b 2) (c 3))))

;; member
(test-equal "member/found" '(2 3) (member 2 '(1 2 3)))
(test-false "member/missing" (member 4 '(1 2 3)))

;; nested lists
(test-equal "nested/car" '(1 2) (car '((1 2) (3 4))))
(test-equal "nested/cadr" '(3 4) (car (cdr '((1 2) (3 4)))))
