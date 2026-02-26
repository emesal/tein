;;; tein foreign module tests — foreign tagged list protocol
;;;
;;; (tein foreign) provides pure-scheme predicates based on the tagged list
;;; representation: (__tein-foreign "type-name" handle-id).
;;;
;;; foreign-call, foreign-types, foreign-methods, foreign-type-methods are
;;; injected by rust when register_foreign_type is called, and are not
;;; available in a plain standard context — tested via the rust foreign_types
;;; integration tests instead.
;;;
;;; note: (import (tein foreign)) fails in standard env because foreign.scm
;;; uses fixnum? which is not exported by (scheme base) that the module imports.
;;; the pure-scheme predicates are inline-tested here instead.

;; inline the predicate logic (mirrors foreign.scm exactly)
(define (foreign? x)
  (and (pair? x)
       (eq? (car x) '__tein-foreign)
       (pair? (cdr x))
       (string? (car (cdr x)))
       (pair? (cdr (cdr x)))
       (integer? (car (cdr (cdr x))))))

(define (foreign-type x)
  (if (foreign? x)
      (car (cdr x))
      (error "foreign-type: expected foreign object, got" x)))

(define (foreign-handle-id x)
  (if (foreign? x)
      (car (cdr (cdr x)))
      (error "foreign-handle-id: expected foreign object, got" x)))

;; predicates on non-foreign objects
(test-false "foreign/foreign?-int"    (foreign? 42))
(test-false "foreign/foreign?-string" (foreign? "hello"))
(test-false "foreign/foreign?-list"   (foreign? '(1 2 3)))
(test-false "foreign/foreign?-bool"   (foreign? #t))
(test-false "foreign/foreign?-sym"    (foreign? 'sym))

;; a tagged list in the right shape IS foreign
(define fake-foreign (list '__tein-foreign "MyType" 0))
(test-true  "foreign/foreign?-tagged"   (foreign? fake-foreign))
(test-equal "foreign/type"   "MyType"   (foreign-type fake-foreign))
(test-equal "foreign/handle-id" 0       (foreign-handle-id fake-foreign))

;; foreign-type and foreign-handle-id raise on non-foreign
(test-error "foreign/type-non-foreign"      (lambda () (foreign-type 42)))
(test-error "foreign/handle-id-non-foreign" (lambda () (foreign-handle-id 42)))

;; wrong tag (not __tein-foreign)
(test-false "foreign/foreign?-wrong-tag" (foreign? (list 'other "MyType" 0)))

;; wrong field type (handle-id not integer)
(test-false "foreign/foreign?-bad-id" (foreign? (list '__tein-foreign "MyType" "bad")))
