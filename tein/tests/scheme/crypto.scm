;;; integration tests for (tein crypto)

(import (tein crypto))

;; sha256 of empty string — NIST test vector
(test-equal "crypto/sha256-empty"
  "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
  (sha256 ""))

;; sha256 of "hello"
(test-equal "crypto/sha256-hello"
  "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
  (sha256 "hello"))

;; sha512 of empty string — NIST test vector
(test-equal "crypto/sha512-empty"
  "cf83e1357eefb8bdf1542850d66d8007d620e4050b5715dc83f4a921d36ce9ce47d0d13c5d85f2b0ff8318d2877eec2f63b931bd47417a81a538327af927da3e"
  (sha512 ""))

;; blake3 of empty string — reference implementation vector
(test-equal "crypto/blake3-empty"
  "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
  (blake3 ""))

;; bytevector output lengths
(test-equal "crypto/sha256-bytes-length" 32 (bytevector-length (sha256-bytes "x")))
(test-equal "crypto/sha512-bytes-length" 64 (bytevector-length (sha512-bytes "x")))
(test-equal "crypto/blake3-bytes-length" 32 (bytevector-length (blake3-bytes "x")))

;; string and bytevector inputs produce the same hash
;; "hello" = #u8(104 101 108 108 111)
(test-equal "crypto/sha256-bv-equiv"
  (sha256 "hello")
  (sha256 #u8(104 101 108 108 111)))

;; random-bytes returns correct length
(test-equal "crypto/random-bytes-0"  0  (bytevector-length (random-bytes 0)))
(test-equal "crypto/random-bytes-32" 32 (bytevector-length (random-bytes 32)))

;; random-integer stays in bounds
(test-true "crypto/random-integer-bounds"
  (let loop ((i 0) (ok #t))
    (if (= i 100) ok
      (let ((r (random-integer 10)))
        (loop (+ i 1) (and ok (>= r 0) (< r 10)))))))

;; random-float stays in [0.0, 1.0)
(test-true "crypto/random-float-bounds"
  (let loop ((i 0) (ok #t))
    (if (= i 100) ok
      (let ((r (random-float)))
        (loop (+ i 1) (and ok (>= r 0.0) (< r 1.0)))))))
