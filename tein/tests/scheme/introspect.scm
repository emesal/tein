;;; integration tests for (tein introspect)

(import (scheme base) (scheme write) (tein test) (tein introspect))

;; available-modules returns a non-empty list
(test-true "available-modules non-empty"
  (pair? (available-modules)))

;; available-modules entries are lists
(test-true "available-modules entries are lists"
  (list? (car (available-modules))))

;; module-exports returns symbols
(test-true "module-exports scheme/write"
  (memq 'display (module-exports '(scheme write))))

;; module-exports for introspect itself
(test-true "module-exports tein/introspect includes available-modules"
  (memq 'available-modules (module-exports '(tein introspect))))

;; procedure-arity on a lambda
(test-equal "arity of (lambda (a b) a)"
  '(2 . 2)
  (procedure-arity (lambda (a b) a)))

;; procedure-arity on a variadic lambda
(test-equal "arity of (lambda (a . rest) a)"
  '(1 . #f)
  (procedure-arity (lambda (a . rest) a)))

;; procedure-arity on non-procedure
(test-false "arity of 42"
  (procedure-arity 42))

;; env-bindings returns alist with our defined variable
(define my-test-var 99)
(test-true "env-bindings finds my-test-var"
  (let ((entry (assq 'my-test-var (env-bindings "my-test"))))
    (and entry (eq? (cdr entry) 'variable))))

;; binding-info on procedure
(let ((info (binding-info 'map)))
  (test-true "binding-info map has name"
    (and info (assq 'name info)))
  (test-equal "binding-info map kind"
    'procedure
    (cdr (assq 'kind info))))

;; binding-info on undefined symbol
(test-false "binding-info undefined"
  (binding-info 'this-symbol-definitely-does-not-exist-xyz))

;; describe-environment returns structured data with modules key
(let ((env-data (describe-environment)))
  (test-true "describe-environment has modules"
    (or (assq 'modules env-data)
        ;; from_raw collapses (modules . list) → list, check first element
        (and (pair? env-data)
             (equal? (caar env-data) 'modules)))))

;; describe-environment/text returns a string
(test-true "describe-environment/text is a string"
  (string? (describe-environment/text)))

;; introspect-docs is an alist with __module__ key
(test-true "introspect-docs has __module__"
  (assq '__module__ introspect-docs))

;; imported-modules includes at least tein/introspect
(test-true "imported-modules includes tein/introspect"
  (member '(tein introspect) (imported-modules)))
