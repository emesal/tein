;;; (tein macro) — macro expansion hook
;;;
;;; set-macro-expand-hook!, unset-macro-expand-hook!, macro-expand-hook are
;;; registered from rust as native functions in the context env. this module
;;; re-exports them for idiomatic r7rs (import (tein macro)) usage.
;;;
;;; note: these bindings are already available in the global env for
;;; standard_env contexts — the import is optional but recommended.
