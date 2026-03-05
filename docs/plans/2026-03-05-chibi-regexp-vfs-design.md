# chibi regexp VFS integration — design

closes #85

## summary

add smoke tests for `(chibi regexp)` / SRFI-115 / `(scheme regex)` in the VFS,
and verify sandbox gating (`default_safe: false`). the VFS wiring itself is
already complete.

## scope

1. **scheme smoke tests** (`chibi_regexp.scm`) — exercise the SRFI-115 SRE API:
   - SRE compilation (`regexp`), predicate (`regexp?`)
   - `regexp-matches`, `regexp-matches?`
   - `regexp-search`
   - `regexp-replace`, `regexp-replace-all` (if available)
   - `regexp-split`, `regexp-extract`
   - submatch accessors (`regexp-match-submatch`, `regexp-match->list`)
   - SRE syntax: `(: ...)`, `(or ...)`, `(* ...)`, `(w/nocase ...)`, named submatches

2. **alias tests** — confirm `(srfi 115)` and `(scheme regex)` resolve correctly

3. **sandbox gating tests** (rust-side):
   - `Modules::Safe` rejects `(import (chibi regexp))`
   - `Modules::All` allows it
   - `.allow_module("chibi/regexp")` allows it
   - same for `(srfi 115)` and `(scheme regex)` aliases

4. **docs** — reference.md already lists these modules; add a note about
   sandbox availability and the ReDoS caveat to the modules section

## non-goals

- exhaustive SRFI-115 conformance testing (chibi's own test suite covers that)
- changing `default_safe` — backtracking engine ReDoS risk is correctly gated

## test runner

standard `run_scheme_test` pattern in `scheme_tests.rs` — no feature gate needed
since chibi regexp is pure scheme with no optional cargo features.
