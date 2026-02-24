# macro expansion hooks design

intercept macro expansion at the C level via a thread-local hook. scheme-first API with thin rust convenience layer. supports both observation (debugging/logging) and transformation (rewriting expanded forms).

## motivation

macro expansion is currently a black box — you feed chibi a form and get a result, with no visibility into what macros did along the way. for an embeddable scheme used as an extension language, being able to observe and transform macro expansions enables: macro steppers, expansion logging, DSL instrumentation, custom optimisations, and debugging tools.

## mechanism

### C layer — `tein_shim.c` + `eval.c` patch

thread-local hook slot in `tein_shim.c`:

```c
TEIN_THREAD_LOCAL sexp tein_macro_expand_hook = SEXP_FALSE;
TEIN_THREAD_LOCAL int tein_macro_expand_hook_active = 0;  // recursion guard
```

shim functions:

- `tein_macro_expand_hook_set(sexp proc)` — set hook, `sexp_preserve_object` for GC safety
- `tein_macro_expand_hook_get()` — return current hook or `SEXP_FALSE`
- `tein_macro_expand_hook_clear()` — release + reset to `SEXP_FALSE`

`eval.c` patch in `analyze_macro_once()`, after `sexp_apply()` returns the expanded form:

```c
if (!sexp_exceptionp(res) && tein_macro_expand_hook != SEXP_FALSE
    && !tein_macro_expand_hook_active) {
    tein_macro_expand_hook_active = 1;
    // build args: (name unexpanded expanded env)
    sexp hook_args = ...;  // cons up 4-arg list
    sexp hook_result = sexp_apply(ctx, tein_macro_expand_hook, hook_args);
    tein_macro_expand_hook_active = 0;
    if (!sexp_exceptionp(hook_result)) {
        res = hook_result;  // replace-and-reanalyze
    } else {
        res = hook_result;  // propagate exception
    }
}
```

the hook fires *after* the macro's transformer runs. the returned form replaces the expansion result and goes through normal reanalysis (chibi's existing `goto loop`), so macros in the returned form expand naturally.

the recursion guard (`tein_macro_expand_hook_active`) prevents infinite recursion when the hook body itself uses macros.

### scheme API via `(tein macro)` VFS module

```scheme
(import (tein macro))

;; set hook — proc receives (name unexpanded expanded env)
(set-macro-expand-hook!
  (lambda (name unexpanded expanded env)
    (display "expanding ") (display name) (newline)
    (display "  from: ") (write unexpanded) (newline)
    (display "    to: ") (write expanded) (newline)
    expanded))  ; return expanded unchanged for observation

;; clear hook
(unset-macro-expand-hook!)

;; query current hook
(macro-expand-hook)  ;; -> proc or #f
```

observation mode: return `expanded` unchanged.
transformation mode: return a different form (gets reanalyzed, including further macro expansion).

### rust API

thin convenience layer — scheme-first:

```rust
ctx.set_macro_expand_hook(proc: &Value)?;   // set a scheme procedure as hook
ctx.unset_macro_expand_hook();               // clear
ctx.macro_expand_hook() -> Option<Value>;    // query
```

rust embedders who want native hooks define a foreign function via `define_fn` and pass it as the proc. single mechanism.

## hook arguments

the hook procedure receives four arguments:

| arg | description | type |
|-----|-------------|------|
| `name` | macro identifier | symbol |
| `unexpanded` | original form before expansion | s-expression |
| `expanded` | result of macro transformer | s-expression |
| `env` | expansion-site environment | environment |

## GC safety

- hook proc stored in thread-local, not on chibi's heap-tracked root list
- `sexp_preserve_object` on set, `sexp_release_object` on clear/replace
- same pattern as reader dispatch handlers

## edge cases

- **recursion**: hook body may use macros — guarded by `tein_macro_expand_hook_active` flag, hook skipped when already active
- **errors**: hook exceptions propagate as eval errors (chibi's normal exception handling)
- **sandbox**: hook available via `(tein macro)` module import, gated by module policy. hook doesn't bypass sandbox — expanded forms still go through normal analysis
- **fuel**: hook execution consumes fuel via `sexp_apply`, no special treatment
- **cleanup**: `Context::drop()` calls `tein_macro_expand_hook_clear()`, same as reader dispatch

## module structure

- `tein_shim.c` — thread-local hook slot, set/get/clear shim functions
- `eval.c` — patch `analyze_macro_once()` to call hook after expansion
- `src/ffi.rs` — extern declarations + safe wrappers for shim functions
- `src/context.rs` — rust API methods, registration in `build()`, cleanup in `drop()`
- `vendor/chibi-scheme/lib/tein/macro.sld` — `(tein macro)` library definition
- `vendor/chibi-scheme/lib/tein/macro.scm` — module documentation

## test strategy

1. **basic hook fires** — set hook, expand a macro, verify hook was called with correct args
2. **observation mode** — hook returns `expanded` unchanged, macro works normally
3. **transformation mode** — hook returns modified form, modified form is what runs
4. **replace-and-reanalyze** — hook returns form containing another macro call, gets expanded
5. **unset hook** — set then unset, expansions proceed without hook
6. **recursion guard** — hook body uses macros internally, no infinite recursion
7. **hook error propagation** — hook raises exception, surfaces as eval error
8. **introspection** — `(macro-expand-hook)` returns current hook or `#f`
9. **cleanup on drop** — context dropped, hook cleared
10. **sandbox compatibility** — hook works in sandboxed contexts
