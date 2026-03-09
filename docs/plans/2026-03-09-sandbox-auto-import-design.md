# sandbox auto-import of scheme/base and scheme/write

## problem

sandboxed contexts start in a null env containing only r7rs core syntax forms
(`quote`, `lambda`, `if`, `set!`, `begin`, `define`, `define-syntax`,
`let-syntax`, `letrec-syntax`, `syntax-rules`) plus `import`. derived syntax
(`let`, `cond`, `and`, `or`, ...), standard procedures (`+`, `display`, `map`,
...), and output (`write`, `newline`, ...) are unavailable until an explicit
`(import ...)`.

this is technically correct r7rs, but creates a severe UX footgun:

- tein-bin's REPL wraps input in `(let ...)` — immediately crashes with
  `undefined variable` in sandbox mode.
- tein-bin's script runner can't run even trivial sandboxed scripts like
  `(display 42)`.
- every sandbox test in the library does `(import (scheme base))` as its first
  line — it's effectively always required.
- a sandboxed context without `scheme/base` isn't meaningfully "scheme".

unsandboxed contexts don't have this problem because `load_standard_env` dumps
~200 bindings into the context env directly.

## design

the sandbox `build()` path auto-imports `(scheme base)` and `(scheme write)`
into the null env after constructing it. these two modules establish a usable
baseline for all sandboxed contexts:

- **`scheme/base`**: core syntax, arithmetic, list ops, control flow, ports,
  `flush-output`, `current-output-port`, etc. (~160 bindings)
- **`scheme/write`**: `display`, `write`, `newline`, `write-shared`,
  `write-simple`. a context that can compute but not print isn't useful.

### what changes

1. **`context.rs` sandbox build path**: after setting the null env as active and
   arming the VFS gate, evaluate `(import (scheme base) (scheme write))`.
   this must happen after VFS gate + allowlist are set (so the import goes
   through the sandbox's own module resolution). error from the import is
   propagated as `InitError`.

2. **documentation**: update sandboxing docs + AGENTS.md to reflect that
   sandboxed contexts start with `scheme/base` + `scheme/write` pre-imported.

3. **tein-bin**: no changes needed — the REPL wrapper's `(let ...)` and
   `flush-output` are covered by `scheme/base`, and `display` by
   `scheme/write`.

### edge cases

- **`Modules::None`**: auto-import is **skipped**. `None` is the "build your own
  allowlist from scratch" entry point — users combine it with `allow_module()`
  to get exactly the modules they need (transitive deps pull in `scheme/base`
  when required). the null env with only syntax + `import` is preserved.
- **`Modules::Only` without `scheme/base`**: the auto-import will fail because
  `scheme/base` isn't in the allowlist. this is a clear, early error — if you
  want a custom allowlist, include `scheme/base` (you need it for any real
  work) or use `Modules::None` + `allow_module()` for dep resolution.
- **`set_current_output_port` in tein-bin**: currently warns "can't set
  non-parameter: current-output-port" because the null env lacks the parameter
  binding. with auto-import of `scheme/base`, the parameter exists before
  tein-bin sets the port, fixing the warning.

### what doesn't change

- embedders still explicitly import anything beyond base + write
  (`scheme/read`, `scheme/time`, `srfi/*`, `tein/*`, etc.)
- the VFS gate, allowlist, UX stubs, and FS policy are unaffected.
- unsandboxed contexts are unaffected.

## scope

- modify `context.rs` sandbox build path (~5 lines)
- add/update tests
- update `docs/sandboxing.md`, `docs/reference.md`, `AGENTS.md`
