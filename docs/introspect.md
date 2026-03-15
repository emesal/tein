# (tein introspect) — environment introspection

Runtime discovery of the current tein scheme environment. Use this when you need to know
what is available before writing code — no external tooling or documentation required.

```scheme
(import (tein introspect))
```

Available in all contexts including sandboxed. In sandboxed contexts, results are filtered
to the active module allowlist so you only see what you can actually use.

---

## API

### `(available-modules)`

Returns a list of module path lists that can be imported in this context.

```scheme
(available-modules)
;; => ((scheme base) (scheme write) (tein json) (tein introspect) ...)
```

### `(imported-modules)`

Returns a list of modules already imported in this context (i.e. whose environments have
been loaded, not just registered).

```scheme
(import (scheme write))
(imported-modules)
;; => ((tein introspect) (scheme write) (scheme base) ...)
```

### `(module-exports mod-path)`

Returns a list of exported binding symbols for the given module. Raises an error if the
module is not available in the current context.

```scheme
(module-exports '(scheme base))
;; => (define let lambda if cond ... map for-each ...)

(module-exports '(tein json))
;; => (json-parse json-stringify)
```

### `(procedure-arity proc)`

Returns `(min . max)` where `max` is `#f` if variadic. Returns `#f` for non-procedures.

```scheme
(procedure-arity cons)     ;; => (2 . 2)
(procedure-arity map)      ;; => (2 . #f)
(procedure-arity 42)       ;; => #f
```

Note: native trampolines (tein internal functions) report `(0 . #f)` — they are variadic
at the C level regardless of their scheme-visible signature.

### `(env-bindings)` / `(env-bindings prefix-string)`

Returns an alist of `(name . kind)` pairs for all bindings in the current environment,
walking the full env chain. `kind` is one of `procedure`, `syntax`, or `variable`.

Optional string prefix filters by symbol name prefix.

```scheme
(env-bindings "json-")
;; => ((json-stringify . procedure) (json-parse . procedure))

(env-bindings "define")
;; => ((define-record-type . syntax) (define-values . syntax) (define . syntax))

(assq 'map (env-bindings "map"))
;; => (map . procedure)
```

### `(binding-info sym)`

Returns an alist with details about a binding, or `#f` if undefined. Keys:

| key | value |
|-----|-------|
| `name` | symbol |
| `kind` | `procedure`, `syntax`, or `variable` |
| `arity` | `(min . max)` — only present for procedures |
| `module` | module path list — only if the symbol appears in a known module's exports |
| `doc` | docstring — only for tein modules with generated docs |

```scheme
(binding-info 'json-parse)
;; => ((name . json-parse) (kind . procedure) (arity 1 . 1)
;;     (module tein json) (doc . "parse a JSON string to scheme data"))

(binding-info 'map)
;; => ((name . map) (kind . procedure) (arity 2 . #f) (module scheme base))

(binding-info 'undefined-xyz)
;; => #f
```

`binding-info` uses a module index built once at `(import (tein introspect))` time —
first module in `(available-modules)` that exports the symbol wins.

### `(describe-environment)`

Returns a structured alist describing the full environment:

```scheme
(describe-environment)
;; => ((modules
;;       ((name scheme base) (exports define let ...) )
;;       ((name tein json)   (exports json-parse json-stringify)
;;                           (docs (json-parse . "parse a JSON string...") ...))
;;       ...))
```

Each module entry has `name` and `exports` keys; tein modules with docs also have a
`docs` key (alist of `(symbol . docstring)`, `__module__` entry excluded).

### `(describe-environment/text)`

Returns a multi-line string overview of the environment — suitable for injecting into an
LLM context window.

```scheme
(display (describe-environment/text))
;; (tein introspect) — environment overview
;;
;; 12 modules available:
;;
;; (scheme base) — 147 exports
;;   define, let, lambda, if, cond, map, for-each, ...
;;
;; (tein json) — 2 exports
;;   json-parse — parse a JSON string to scheme data
;;   json-stringify — serialise scheme data to a JSON string
;;
;; ...
```

For tein modules with docs, each export is listed with its docstring. For other modules,
exports are shown as a comma-separated summary.

### `introspect-docs`

Documentation alist for `(tein introspect)` itself — same format as other tein doc alists.

```scheme
(assq 'binding-info introspect-docs)
;; => (binding-info . "detailed info about a binding: kind, arity, module, docs")
```

---

## startup cost

`*binding-module-index*` and `*doc-alist-cache*` are built once at import time
(O(modules × exports)). All subsequent calls are cheap lookups. The cost is paid on the
first `(import (tein introspect))` in a given context.

## sandbox behaviour

- `(available-modules)` and `(imported-modules)` return only allowlisted modules.
- `(module-exports mod)` raises an error for modules outside the allowlist.
- `(env-bindings)` and `(binding-info)` reflect only what is actually in scope.
- `(describe-environment/text)` naturally reflects the above — safe to call in any context.
