;;; (tein reader) — custom reader dispatch extensions
;;;
;;; set-reader!, unset-reader!, reader-dispatch-chars are registered from
;;; rust as native functions in the context env. this module re-exports them
;;; for idiomatic r7rs (import (tein reader)) usage.
;;;
;;; note: these bindings are already available in the global env for
;;; standard_env contexts — the import is optional but recommended.
