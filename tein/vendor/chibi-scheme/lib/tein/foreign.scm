;;; (tein foreign) — foreign object protocol
;;;
;;; foreign objects are tagged lists: (__tein-foreign "type-name" handle-id)
;;; created by rust, manipulated via dispatch to rust-registered methods.
;;;
;;; foreign-call, foreign-methods, foreign-types, foreign-type-methods are
;;; registered from rust as native functions (they need ForeignStore access).
;;; the .sld exports them — rust injects them into the context env during
;;; register_foreign_protocol().
;;;
;;; uses only car/cdr (scheme base) rather than cadr/caddr (require scheme/cxr)
;;; so the module loads in minimal environments.

;; predicates and accessors for the tagged list representation
;; (__tein-foreign "type-name" handle-id)

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
