# tein for LLM agents

tein is designed with LLM coding agents as a first-class audience. this doc explains
how — what properties tein has that make it a good execution substrate for agent-generated
code, and what the roadmap holds for agent tooling specifically.

## why scheme for agent tools

Scheme is a natural fit for agent-executed code:

- **homoiconic** — code is data. agents can construct, inspect, and transform programs as
  lists. the macro system means agents can extend the language itself.
- **sandboxable** — R7RS has a clean separation between what's in scope and what can be
  imported. tein's sandbox maps directly onto this: the null env is exactly the set of
  capabilities the host grants.
- **minimal and predictable** — a small, well-specified language with no hidden globals,
  no ambient capabilities, no implicit IO. agents can reason about what code will do.
- **composable** — `(import ...)` is the capability system. an agent knows exactly what
  it has access to by looking at its imports.

## the sandbox as a trust boundary

tein's sandbox is designed to be the trust boundary between agent-generated code and the
host environment. an agent gets exactly the capabilities the host grants — nothing more.

```rust
let ctx = Context::builder()
    .standard_env()
    .sandboxed(Modules::Safe)   // known-safe module set
    .step_limit(100_000)         // no infinite loops
    .file_read(&["/data/"])      // explicit filesystem grant
    .build()?;
```

the sandbox has four independent layers:

1. **module restriction** — which R7RS libraries can be imported
2. **VFS gate** — enforces restriction at the C level (not bypassable from scheme)
3. **file IO policy** — path-prefix-based filesystem access with traversal protection
4. **resource limits** — step limits and wall-clock timeouts

an agent running inside a `Modules::Safe` context cannot import `scheme/eval`, cannot
load arbitrary files, cannot run forever. these are hard guarantees enforced at the C
level and in the rust sandbox layer.

see [sandboxing.md](sandboxing.md) for the full model.

## LLM-navigable error messages

tein's error messages are designed to be useful to an LLM reading them:

**wrong module:**
```scheme
(map (lambda (x) x) '(1 2))
;; sandbox: 'map' requires (import (scheme base))
```

**wrong foreign method:**
```scheme
(counter-frobnicate c)
;; no method 'frobnicate' on type 'counter'. available: get, increment
```

**sandbox violation:**
```scheme
(open-input-file "/etc/passwd")
;; sandbox: file read denied: /etc/passwd (not under allowed prefix)
```

these errors tell the agent exactly what to do next — import the right module, use the
right method name, adjust the file policy. no cryptic C-level error codes.

## self-describing environments: (tein docs)

`#[tein_module]` generates documentation alists from rust doc comments. scheme code can
query these at runtime:

```scheme
(import (tein docs))
(describe mymod-docs)
;; (tein mymod)
;;   mymod-greet — greet someone by name
;;   answer — the answer to everything (42)
;;   counter? — predicate for counter type
;;   counter-get — get the counter value
;;   counter-increment — increment and return new value
```

an agent can dump this into its context before writing code that uses the module —
zero latency, no external tooling required.

## introspectable foreign types

every registered foreign type exposes its own metadata:

```scheme
(foreign-types)               ; all type names in this context
(foreign-methods "counter")   ; method names for a specific type
(foreign-type obj)            ; type name of a foreign value
```

agents can discover what types and methods exist without needing documentation.

## predictable scope

tein's sandboxed contexts use a null env — a clean environment with only the explicitly
granted bindings. there are no hidden globals, no ambient `load` or `eval` unless
granted, no way to reach outside the sandbox via side channels.

what an agent sees is what it gets. `(define x 1)` binds `x` in the null env and
nowhere else. imports work exactly as documented.

## environment introspection: (tein introspect)

`(tein introspect)` lets scheme code discover its own environment at runtime — no
external LSP or static analyser required. available in all contexts including sandboxed.

```scheme
(import (tein introspect))

(available-modules)            ; what can I import?
(imported-modules)             ; what's already imported?
(module-exports '(tein json))  ; what does this module provide?
(procedure-arity map)          ; how many args? => (2 . #f)
(env-bindings "json-")         ; what json-* bindings are in scope?
(binding-info 'json-parse)     ; everything about this binding
(describe-environment/text)    ; full text dump for prompt injection
```

`describe-environment/text` returns a multi-line string listing all available modules,
their export counts, and docstrings for tein modules — ready to inject into an LLM context.

## what's coming for agent tooling

**fake environment variables** — planned (issue #99). host-injectable env var overrides
so agents can run with a controlled `getenv` view, separate from the host process environment.

**`Modules::Safe` vet** — ongoing (issue #92). systematic review of all `chibi/*` and `srfi/*`
VFS modules to confirm sandbox safety. as more modules are vetted, `Modules::Safe` grows.
