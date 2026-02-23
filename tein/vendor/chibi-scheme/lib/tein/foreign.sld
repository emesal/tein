(define-library (tein foreign)
  (import (scheme base))
  (export foreign? foreign-type foreign-handle-id
          foreign-call foreign-methods
          foreign-types foreign-type-methods)
  (include "foreign.scm"))
