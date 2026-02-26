;;; i/o tests — string ports, write, display, read

;; open-input-string / read
(let ((p (open-input-string "(1 2 3)")))
  (test-equal "io/read-list" '(1 2 3) (read p)))

(let ((p (open-input-string "hello")))
  (test-equal "io/read-symbol" 'hello (read p)))

(let ((p (open-input-string "")))
  (test-true "io/eof" (eof-object? (read p))))

;; read-char / peek-char
(let ((p (open-input-string "abc")))
  (test-equal "io/peek-char" #\a (peek-char p))
  (test-equal "io/read-char-1" #\a (read-char p))
  (test-equal "io/read-char-2" #\b (read-char p)))

;; open-output-string / get-output-string
(let ((p (open-output-string)))
  (write-char #\h p)
  (write-char #\i p)
  (test-equal "io/write-char" "hi" (get-output-string p)))

;; write vs display
(let ((p (open-output-string)))
  (write "hello" p)
  (test-equal "io/write-string" "\"hello\"" (get-output-string p)))

(let ((p (open-output-string)))
  (display "hello" p)
  (test-equal "io/display-string" "hello" (get-output-string p)))

(let ((p (open-output-string)))
  (write '(1 "two" #t) p)
  (test-equal "io/write-list" "(1 \"two\" #t)" (get-output-string p)))

;; newline
(let ((p (open-output-string)))
  (newline p)
  (test-equal "io/newline" "\n" (get-output-string p)))

;; read multiple datums — use let* for sequential read ordering
(let ((p (open-input-string "1 2 3")))
  (let* ((a (read p)) (b (read p)) (c (read p)))
    (test-equal "io/read-multi" '(1 2 3) (list a b c))))
