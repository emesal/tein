;;; (tein macro) — macro expansion hook
;;;
;;; the hook receives (name unexpanded expanded env) after each macro expansion
;;; and returns the form to use. return expanded unchanged for observation.
;;;
;;; the underlying native dispatch fn is tein-macro-expand-hook-dispatch,
;;; registered in the context env. these wrappers provide the public API.

(define (set-macro-expand-hook! proc)
  (tein-macro-expand-hook-dispatch 'set proc))

(define (unset-macro-expand-hook!)
  (tein-macro-expand-hook-dispatch 'unset))

(define (macro-expand-hook)
  (tein-macro-expand-hook-dispatch 'get))
