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
