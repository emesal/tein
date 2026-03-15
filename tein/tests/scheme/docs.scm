;;; (tein docs) integration tests

(import (tein testmod docs))
(import (tein docs))

;; module-doc — existing symbol
(test-equal "docs/module-doc-fn"
  ""
  (module-doc testmod-docs 'testmod-greet))

;; module-doc — missing symbol
(test-equal "docs/module-doc-missing"
  #f
  (module-doc testmod-docs 'nonexistent))

;; module-doc — __module__ metadata
(test-equal "docs/module-doc-module"
  "tein testmod"
  (module-doc testmod-docs '__module__))

;; module-docs — strips __module__
(test-false "docs/module-docs-no-metadata"
  (assq '__module__ (module-docs testmod-docs)))

;; module-docs — all entries present
(test-true "docs/module-docs-has-greet"
  (and (assq 'testmod-greet (module-docs testmod-docs)) #t))
(test-true "docs/module-docs-has-add"
  (and (assq 'testmod-add (module-docs testmod-docs)) #t))
(test-true "docs/module-docs-has-counter?"
  (and (assq 'counter? (module-docs testmod-docs)) #t))

;; describe — returns a string
(test-true "docs/describe-string"
  (string? (describe testmod-docs)))

;; describe — contains module name
(test-true "docs/describe-module-name"
  (let ((d (describe testmod-docs)))
    (and (string? d)
         (let loop ((i 0))
           (cond
             ((> i (- (string-length d) 14)) #f)
             ((string=? (substring d i (+ i 14)) "(tein testmod)") #t)
             (else (loop (+ i 1))))))))

;; describe — symbol input returns helpful error, not crash
(test-true "docs/describe-symbol-returns-helpful-string"
  (let ((d (describe 'some-symbol)))
    (and (string? d) (not (= (string-length d) 0)))))

;; describe — number input returns helpful error, not crash
(test-true "docs/describe-number-returns-helpful-string"
  (let ((d (describe 42)))
    (and (string? d) (not (= (string-length d) 0)))))

;; describe — string input returns helpful error (not a list)
(test-true "docs/describe-string-returns-helpful-string"
  (let ((d (describe "hello")))
    (and (string? d) (not (= (string-length d) 0)))))

;; describe — flat list (not pairs) returns helpful error
(test-true "docs/describe-flat-list-returns-helpful-string"
  (let ((d (describe '(a b c))))
    (and (string? d) (not (= (string-length d) 0)))))

;; describe — empty list is a valid alist (vacuously)
(test-true "docs/describe-empty-list"
  (string? (describe '())))
