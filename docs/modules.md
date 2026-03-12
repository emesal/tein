# tein modules

built-in Scheme libraries backed by Rust crates. import them like any R7RS library.

all `tein/*` modules are included in the `Modules::Safe` preset — no explicit
`allow_module()` needed. enable or disable at the cargo level with feature flags
(see [reference.md](reference.md)).

---

## (tein json)

**feature:** `json` (default) | **deps added:** `serde`, `serde_json`

```scheme
(import (tein json))

(json-parse "{\"x\": 1, \"y\": [2, 3]}")
;; => (("x" . 1) ("y" 2 3))

(json-stringify '(("name" . "tein") ("version" . 1)))
;; => "{\"name\":\"tein\",\"version\":1}"
```

### representation

| JSON | Scheme |
|------|--------|
| object `{"k": v}` | alist `((k . v) ...)` |
| empty `{}` | `'()` (same as empty array — known ambiguity) |
| array `[...]` | list `(...)` |
| empty `[]` | `'()` |
| string | string |
| integer | integer |
| float | flonum |
| `true` / `false` | `#t` / `#f` |
| `null` | symbol `null` |

### exports
`json-parse`, `json-stringify`

---

## (tein toml)

**feature:** `toml` (default) | **deps added:** `toml`

```scheme
(import (tein toml))

(define doc (toml-parse "[server]\nhost = \"localhost\"\nport = 8080\n"))
;; => (("server" ("host" . "localhost") ("port" . 8080)))

(toml-stringify '(("server" ("host" . "localhost") ("port" . 8080))))
;; => "[server]\nhost = \"localhost\"\nport = 8080\n"
```

### representation

| TOML | Scheme |
|------|--------|
| table | alist `((key . val) ...)` |
| array | list |
| string | string |
| integer | integer |
| float | flonum |
| boolean | boolean |
| datetime | tagged list `(toml-datetime "iso-string")` |

All four TOML datetime variants (offset datetime, local datetime, local date, local time)
use the same `toml-datetime` tag — the string content distinguishes them.

### exports
`toml-parse`, `toml-stringify`

---

## (tein uuid)

**feature:** `uuid` (default) | **deps added:** `uuid`

```scheme
(import (tein uuid))

(make-uuid)  ; => "f47ac10b-58cc-4372-a567-0e02b2c3d479"
(uuid? "f47ac10b-58cc-4372-a567-0e02b2c3d479")  ; => #t
uuid-nil     ; => "00000000-0000-0000-0000-000000000000"
```

### exports
`make-uuid`, `uuid?`, `uuid-nil`

---

## (tein time)

**feature:** `time` (default) | **deps added:** none (pure `std::time`)

```scheme
(import (tein time))

(current-second)         ; => 1740902400.0  (POSIX seconds, inexact)
(current-jiffy)          ; => 12345678      (nanoseconds, exact integer)
(jiffies-per-second)     ; => 1000000000
```

**jiffy epoch note:** `current-jiffy` counts nanoseconds from a process-relative epoch set
on the first call anywhere in the process. this epoch is shared across all `Context` instances —
per r7rs, it is "constant within a single run of the program".

### exports
`current-second`, `current-jiffy`, `jiffies-per-second`

---

## (tein process)

**feature:** none (always available) | **sandbox:** included in `Modules::Safe`

`(tein process)` provides process-context access, plus `exit` — an escape hatch that stops
evaluation and returns a value to the rust caller.

```scheme
(import (tein process))

;; process information
(get-environment-variable "HOME")   ; => "/home/user" (or #f if not set)
(get-environment-variables)        ; => (("HOME" . "/home/user") ...)
(command-line)                     ; => ("/path/to/binary" ...)

;; early return to rust caller
(exit)        ; => Ok(Value::Integer(0)) in rust
(exit #t)     ; => Ok(Value::Integer(0))
(exit #f)     ; => Ok(Value::Integer(1))
(exit "done") ; => Ok(Value::String("done"))
```

In sandboxed contexts, `get-environment-variable`, `get-environment-variables`, and
`command-line` consult fake process state instead of leaking host data. defaults:
`TEIN_SANDBOX=true` in the env map, `["tein", "--sandbox"]` as the command-line.
configure via `ContextBuilder::environment_variables()` and `ContextBuilder::command_line()`.

**r7rs deviation:** both `exit` and `emergency-exit` have emergency-exit semantics in tein —
neither runs `dynamic-wind` "after" thunks. r7rs `exit` should run them. see issue #101.

### exports
`get-environment-variable`, `get-environment-variables`, `command-line`, `exit`, `emergency-exit`

---

## (tein file)

**feature:** none | **sandbox:** included in `Modules::Safe`

R7RS file operations with sandbox-aware IO policy enforcement. use this in sandboxed
contexts instead of `(scheme file)`.

```scheme
(import (tein file))

(file-exists? "/tmp/data.txt")       ; => #t or #f
(delete-file "/tmp/old.txt")         ; respects FsPolicy

(call-with-input-file "/data/in.txt"
  (lambda (port) (read port)))

(with-output-to-file "/tmp/out.txt"
  (lambda () (display "hello")))
```

Policy enforcement (`.file_read()`, `.file_write()`) applies to all file operations
in sandboxed contexts. unsandboxed contexts allow all paths.

### exports
`file-exists?`, `delete-file`, `open-input-file`, `open-binary-input-file`,
`open-output-file`, `open-binary-output-file`,
`call-with-input-file`, `call-with-output-file`,
`with-input-from-file`, `with-output-to-file`

---

## (tein docs)

**feature:** none | **sandbox:** included in `Modules::Safe`

runtime access to module documentation alists generated by `#[tein_module]`.
designed for LLM context dumps — a module can describe itself to an agent.

```scheme
(import (tein docs))

;; given a doc alist from a #[tein_module]:
(describe mymod-docs)
;; (tein mymod)
;;   mymod-greet — greet someone
;;   answer — the answer to everything
;;   counter? — predicate for counter type

(module-doc mymod-docs 'mymod-greet)
;; => "greet someone"

(module-docs mymod-docs)
;; => ((mymod-greet . "greet someone") (answer . "the answer to everything") ...)
```

`#[tein_module]` scrapes `///` doc comments at compile time into the alist. the `__module__`
key holds the module name string; `module-docs` filters it out.

See [tein-for-agents.md](tein-for-agents.md) for how `(tein docs)` fits into the agent
tooling story.

### exports
`describe`, `module-doc`, `module-docs`

---

## (tein load)

**feature:** none | **sandbox:** included in `Modules::Safe`

VFS-restricted `load` — loads a scheme file via the VFS, respecting the active
module allowlist.

```scheme
(import (tein load))
(load "/vfs/lib/my-module.scm")
```

The built-in chibi `load` is not exported globally (overriding it breaks module imports).
`(tein load)` exports it under the name `load`.

### exports
`load`

---

## (tein introspect)

**feature:** none | **sandbox:** included in `Modules::Safe`

Environment introspection API for LLM agents and tooling. Lets scheme code discover
available modules, inspect exports, query procedure arity, and dump structured
environment overviews — all from within a running context.

```scheme
(import (tein introspect))

(available-modules)           ; => ((scheme base) (scheme write) (tein json) ...)
(imported-modules)            ; => ((scheme base) (tein introspect) ...)
(module-exports '(tein json)) ; => (json-parse json-stringify ...)
(env-bindings)                ; => ((map . procedure) (filter . procedure) ...)
(env-bindings "json-")        ; => ((json-parse . procedure) ...)
(procedure-arity map)         ; => (2 . #f)  ; min=2, variadic
(procedure-arity cons)        ; => (2 . 2)   ; exactly 2
(procedure-arity 42)          ; => #f        ; not a procedure
(binding-info 'json-parse)
; => ((name . json-parse) (kind . procedure) (arity . (1 . 1))
;     (module tein json) (doc . "parse a json string to scheme"))
(describe-environment/text)   ; => "(tein introspect) — environment overview\n..."
```

`*binding-module-index*` and `*doc-alist-cache*` are built once at import time
(O(modules × exports)). `describe-environment/text` produces a prompt-injectable
overview of all available modules, their exports, and any tein module docstrings.

### exports
`available-modules`, `imported-modules`, `module-exports`, `env-bindings`,
`procedure-arity`, `binding-info`, `describe-environment`, `describe-environment/text`,
`introspect-docs`
