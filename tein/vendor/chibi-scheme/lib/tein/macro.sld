(define-library (tein macro)
  (export set-macro-expand-hook! unset-macro-expand-hook! macro-expand-hook)
  (include "macro.scm"))
