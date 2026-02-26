;;; tein module integration — tests generated module from scheme side

(import (tein testmod))

;; free functions
(test-equal "module/greet" "hello, world!" (testmod-greet "world"))
(test-equal "module/add" 7 (testmod-add 3 4))

;; type predicate
(test-false "module/counter?-int" (counter? 42))
