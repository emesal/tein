;;; eval tests — eval with environment specifiers
;;;
;;; note: (scheme eval) module not available in this chibi build, but eval,
;;; interaction-environment, and scheme-report-environment are in the standard env.

;; basic eval in interaction-environment
(test-equal "eval/basic" 42 (eval '(+ 40 2) (interaction-environment)))

;; eval can define bindings
(eval '(define eval-test-var 99) (interaction-environment))
(test-equal "eval/define" 99 (eval 'eval-test-var (interaction-environment)))

;; eval lambda
(test-equal "eval/lambda" 7
  (eval '((lambda (x y) (+ x y)) 3 4) (interaction-environment)))

;; scheme-report-environment (r7rs)
(test-equal "eval/scheme-report" 6
  (eval '(* 2 3) (scheme-report-environment 7)))
