;;; (tein process) integration tests

(import (tein process))

;; command-line returns a list
(test-true "command-line/list" (list? (command-line)))

;; get-environment-variables returns an alist
(test-true "get-env-vars/pair" (pair? (get-environment-variables)))

;; get-environment-variable for missing var returns #f
(test-false "get-env-var/missing" (get-environment-variable "TEIN_NONEXISTENT_VAR_XYZ"))
