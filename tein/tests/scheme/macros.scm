;;; macro tests — define-syntax, let-syntax, letrec-syntax, syntax-rules

;; basic define-syntax
(define-syntax my-if
  (syntax-rules ()
    ((my-if c t f) (cond (c t) (else f)))))
(test-equal "macro/my-if-true"  1 (my-if #t 1 2))
(test-equal "macro/my-if-false" 2 (my-if #f 1 2))

;; variadic pattern with ellipsis
(define-syntax my-or
  (syntax-rules ()
    ((my-or) #f)
    ((my-or e) e)
    ((my-or e1 e2 ...)
     (let ((t e1))
       (if t t (my-or e2 ...))))))
(test-equal "macro/my-or-empty" #f (my-or))
(test-equal "macro/my-or-first" 1  (my-or 1 2))
(test-equal "macro/my-or-second" 2 (my-or #f 2))

;; ellipsis in output
(define-syntax my-list
  (syntax-rules ()
    ((my-list x ...) (list x ...))))
(test-equal "macro/ellipsis" '(1 2 3) (my-list 1 2 3))

;; let-syntax (local macro, no letrec semantics)
(let-syntax ((dbl (syntax-rules ()
                    ((dbl x) (* 2 x)))))
  (test-equal "let-syntax/double" 10 (dbl 5)))

;; letrec-syntax (local macros can reference each other)
(letrec-syntax
    ((my-and (syntax-rules ()
               ((my-and) #t)
               ((my-and e) e)
               ((my-and e1 e2 ...) (if e1 (my-and e2 ...) #f)))))
  (test-equal "letrec-syntax/and-t" 3 (my-and 1 2 3))
  (test-false "letrec-syntax/and-f" (my-and 1 #f 3)))

;; hygiene: macro-introduced bindings don't capture user variables
(define-syntax swap!
  (syntax-rules ()
    ((swap! a b)
     (let ((tmp a))
       (set! a b)
       (set! b tmp)))))
(define x 1)
(define y 2)
(swap! x y)
(test-equal "macro/hygiene-x" 2 x)
(test-equal "macro/hygiene-y" 1 y)
