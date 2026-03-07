;;; (tein modules) dynamic module registration integration tests

(import (tein modules))
(import (scheme base))

;; register a module
(register-module
  "(define-library (test greeter)
     (import (scheme base))
     (export greet)
     (begin (define (greet x) (string-append \"hello \" x))))")

;; verify registered
(test-true "module-registered?/after-register"
  (module-registered? '(test greeter)))

;; import and use the dynamically registered module
(import (test greeter))
(test-equal "greet/correct-value"
  "hello world"
  (greet "world"))

;; module-registered? for non-existent module
(test-false "module-registered?/nonexistent"
  (module-registered? '(nonexistent module)))

;; module-registered? for built-in module
(test-true "module-registered?/builtin"
  (module-registered? '(scheme base)))

;; register a second module that imports the first (transitive deps)
(register-module
  "(define-library (test greeter-loud)
     (import (scheme base) (test greeter))
     (export greet-loud)
     (begin (define (greet-loud x)
              (string-append (greet x) \"!\"))))")

(import (test greeter-loud))
(test-equal "greet-loud/transitive-import"
  "hello world!"
  (greet-loud "world"))
