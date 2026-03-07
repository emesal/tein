;;; (tein file) integration tests
;;; NOTE: run in unsandboxed standard context — no FsPolicy restriction

(import (tein file))

;; file-exists? on known file
(test-true "file-exists?/cargo" (file-exists? "Cargo.toml"))
(test-false "file-exists?/nonexistent" (file-exists? "/nonexistent/path/xyz.txt"))
