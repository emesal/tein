;;; (tein foreign) — foreign object protocol
;;;
;;; foreign objects are tagged lists: (__tein-foreign "type-name" handle-id)
;;; created by rust, manipulated via dispatch to rust-registered methods.
;;;
;;; foreign-call, foreign-methods, foreign-types, foreign-type-methods are
;;; registered from rust as native functions (they need ForeignStore access).
;;; the .sld exports them — rust injects them into the context env during
;;; register_foreign_protocol().

;; predicates and accessors for the tagged list representation
;; (__tein-foreign "type-name" handle-id)

(define (foreign? x)
  (and (pair? x)
       (eq? (car x) '__tein-foreign)
       (pair? (cdr x))
       (string? (cadr x))
       (pair? (cddr x))
       (integer? (caddr x))))

(define (foreign-type x)
  (if (foreign? x)
      (cadr x)
      (error "foreign-type: expected foreign object, got" x)))

(define (foreign-handle-id x)
  (if (foreign? x)
      (caddr x)
      (error "foreign-handle-id: expected foreign object, got" x)))
