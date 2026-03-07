(import (tein test) (tein file) (scheme base))

;;; (tein file) higher-order wrapper tests
;;; verifies that call-with-* and with-*-from/to-file are exported + callable

(test-true "call-with-input-file is procedure"
           (procedure? call-with-input-file))
(test-true "call-with-output-file is procedure"
           (procedure? call-with-output-file))
(test-true "with-input-from-file is procedure"
           (procedure? with-input-from-file))
(test-true "with-output-to-file is procedure"
           (procedure? with-output-to-file))
