;;; quasiquote tests — unquote, splicing, nesting

;; basic quasiquote
(test-equal "qq/basic" '(1 2 3) `(1 2 3))
(test-equal "qq/unquote" '(1 2 3) (let ((x 2)) `(1 ,x 3)))
(test-equal "qq/splice"  '(1 2 3 4) (let ((xs '(2 3))) `(1 ,@xs 4)))

;; splice at start/end
(test-equal "qq/splice-start" '(1 2 3) (let ((xs '(1 2))) `(,@xs 3)))
(test-equal "qq/splice-end"   '(1 2 3) (let ((xs '(2 3))) `(1 ,@xs)))
(test-equal "qq/splice-only"  '(1 2 3) (let ((xs '(1 2 3))) `(,@xs)))

;; dotted pair
(test-equal "qq/dotted" '(1 . 2) `(1 . ,(+ 1 1)))

;; vector quasiquote
(test-equal "qq/vector" '#(1 2 3) `#(1 2 3))
(test-equal "qq/vector-unquote" '#(1 2 3) (let ((x 2)) `#(1 ,x 3)))
