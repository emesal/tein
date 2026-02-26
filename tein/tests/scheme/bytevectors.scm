;;; bytevector tests — creation, access, mutation, copy, utf8

(import (scheme base))

;; construction
(test-equal "bv/make" '#u8(0 0 0) (make-bytevector 3))
(test-equal "bv/make-fill" '#u8(7 7 7) (make-bytevector 3 7))
(test-equal "bv/literal" 3 (bytevector-length #u8(1 2 3)))

;; access and mutation
(define bv (bytevector 10 20 30))
(test-equal "bv/u8-ref" 20 (bytevector-u8-ref bv 1))
(bytevector-u8-set! bv 1 99)
(test-equal "bv/u8-set" 99 (bytevector-u8-ref bv 1))

;; length
(test-equal "bv/length" 3 (bytevector-length bv))
(test-equal "bv/empty" 0 (bytevector-length (bytevector)))

;; copy
(define bv2 (bytevector-copy bv))
(bytevector-u8-set! bv2 0 42)
(test-equal "bv/copy-independent" 10 (bytevector-u8-ref bv 0))
(test-equal "bv/copy-mutated" 42 (bytevector-u8-ref bv2 0))

;; copy with start/end (bv[1] was mutated to 99 above)
(test-equal "bv/copy-slice" '#u8(99 30) (bytevector-copy bv 1 3))

;; append
(test-equal "bv/append" '#u8(1 2 3 4) (bytevector-append #u8(1 2) #u8(3 4)))

;; utf8 round-trip
(test-equal "bv/utf8->string" "hello" (utf8->string (string->utf8 "hello")))
(test-equal "bv/string->utf8" '#u8(104 105) (string->utf8 "hi"))
