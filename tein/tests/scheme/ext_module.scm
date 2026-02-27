;; scheme-level tests for the cdylib extension system.
;;
;; the extension is pre-loaded by the rust test harness before this file
;; is evaluated. see tein/tests/scheme_tests.rs :: test_scheme_ext_module.
(import (tein testext))
(import (tein test))

;; free functions — integer
(test-equal "ext/add" 42 (testext-add 20 22))
(test-equal "ext/add-negative" 0 (testext-add -5 5))

;; free functions — float
(test-equal "ext/multiply" 10.0 (testext-multiply 2.5 4.0))

;; free functions — string
(test-equal "ext/greet" "hello, world!" (testext-greet "world"))
(test-equal "ext/greet-empty" "hello, !" (testext-greet ""))

;; free functions — bool
(test-true "ext/positive-true" (testext-positive? 5))
(test-false "ext/positive-false" (testext-positive? -3))
(test-false "ext/positive-zero" (testext-positive? 0))

;; free functions — result ok
(test-equal "ext/safe-div" 5 (testext-safe-div 10 2))

;; constants — no module prefix (GREETING → "greeting", ANSWER → "answer")
(test-equal "ext/greeting" "hello from testext" greeting)
(test-equal "ext/answer" 42 answer)
