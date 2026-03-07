;;; (tein uuid) scheme-level tests

(import (tein uuid))

;; make-uuid returns a string
(test-true "uuid/make-uuid-string?" (string? (make-uuid)))

;; make-uuid returns a valid uuid
(test-true "uuid/make-uuid-valid" (uuid? (make-uuid)))

;; two calls return different values
(test-false "uuid/make-uuid-unique"
  (equal? (make-uuid) (make-uuid)))

;; uuid? on a known valid uuid
(test-true "uuid/predicate-valid"
  (uuid? "f47ac10b-58cc-4372-a567-0e02b2c3d479"))

;; uuid? returns #f for non-uuids
(test-false "uuid/predicate-int"    (uuid? 42))
(test-false "uuid/predicate-bool"   (uuid? #t))
(test-false "uuid/predicate-list"   (uuid? '()))
(test-false "uuid/predicate-empty"  (uuid? ""))
(test-false "uuid/predicate-junk"   (uuid? "not-a-uuid"))

;; uuid-nil is the nil uuid string
(test-equal "uuid/nil-value"
  "00000000-0000-0000-0000-000000000000"
  uuid-nil)

;; uuid-nil passes uuid?
(test-true "uuid/nil-valid" (uuid? uuid-nil))
