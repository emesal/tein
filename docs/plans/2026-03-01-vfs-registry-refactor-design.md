# VFS module registry refactor — design

issue: #95

## problems

### 1. phantom modules

the VFS module registry (`VFS_MODULES_SAFE` / `VFS_MODULES_ALL` in `sandbox.rs`) lists ~80
modules, but only ~35 have their `.sld`/`.scm` files actually embedded in the VFS
(`build.rs` `VFS_FILES`). the rest are phantom entries — in the allowlist but not
importable, because chibi's module resolver only searches `/vfs/lib`.

`(import (srfi 1))` silently fails in a sandbox despite being in `VFS_MODULES_SAFE`.

### 2. dual-list architecture

`VFS_MODULES_SAFE` and `VFS_MODULES_ALL` are separate lists with overlapping entries.
the relationship between "files embedded in VFS" and "modules in allowlist" is implicit
and has drifted.

### 3. preset layer is unnecessary

the `preset()` / `allow()` API controls which individual *bindings* (like `+`, `map`,
`string-append`) are available in the restricted env, separate from which *modules* can
be imported. this is a second axis of control with no compelling use case:

- the "arithmetic only" sandbox is a demo curiosity, not a real product need
- real security boundaries are filesystem access, network, process, step limits, heap —
  all controlled at the module or context level
- removing `string-append` from a sandbox achieves nothing security-relevant
- the machinery is complex: `env_copy_named`, `Preset` structs, `ALL_PRESETS`,
  `ALWAYS_STUB`, per-binding cherry-picking from the source env

modules already provide granular control: either you have `(scheme base)` or you don't.

### 4. ALWAYS_STUB is a workaround, not a security boundary

`ALWAYS_STUB` hardcodes ~20 primitive names (`eval`, `interaction-environment`,
`scheme-report-environment`, `primitive-environment`, etc.) that get replaced with
sandbox-violation stubs in restricted envs. this was necessary because the old design
imported modules like `scheme/eval` and `scheme/repl` into the safe set and then tried
to block their dangerous exports after the fact.

the correct fix is simpler: **don't put dangerous modules in the safe set.** the vetting
process itself is the security boundary — if a module exports env-escape primitives, it
doesn't belong in `Modules::Safe`. no hardcoded blocklist needed.

investigation confirmed:
- `interaction-environment` is a chibi parameter that returns the *original full standard
  env*, not the restricted sandbox env — genuinely dangerous
- `scheme-report-environment` / `primitive-environment` construct *fresh unrestricted
  envs* from scratch — genuinely dangerous
- `scheme/eval` exports `eval` + `environment` — removed from safe set
- `scheme/repl` exports `interaction-environment` — removed from safe set
- internal primitives (`%meta-env`, `env-parent`, `%import`, etc.) are not exported by
  any module — unreachable from a null env that only gets bindings via module imports

## design

### single source of truth: `VfsRegistry`

one master registry replaces `VFS_MODULES_SAFE`, `VFS_MODULES_ALL`, and `VFS_FILES`.
each entry declares everything needed for that module:

```rust
struct VfsEntry {
    /// module path, e.g. "scheme/char", "srfi/1"
    path: &'static str,
    /// module deps (resolved transitively at builder time)
    deps: &'static [&'static str],
    /// files to embed: .sld + included .scm, relative to chibi lib/
    /// manually listed as part of vetting. build.rs validates against
    /// actual (include ...) directives in the .sld — fails if mismatched.
    files: &'static [&'static str],
    /// C static library entry, if any (replaces CLIB_ENTRIES)
    clib: Option<ClibEntry>,
    /// whether this module is in the default safe set
    default_safe: bool,
    /// source type: embedded files or dynamically registered at runtime
    source: VfsSource,
    /// cargo feature required, if any (e.g. "json", "toml")
    feature: Option<&'static str>,
}

enum VfsSource {
    /// files embedded at build time (normal case)
    Embedded,
    /// registered at runtime via #[tein_module] — no files to embed.
    /// exports are hardcoded in the registry since no .sld exists.
    Dynamic,
}

struct ClibEntry {
    /// C source file relative to chibi dir, e.g. "lib/chibi/ast.c"
    source: &'static str,
    /// init function suffix for the static lib table
    init_suffix: &'static str,
    /// VFS key for static lib lookup, e.g. "/vfs/lib/chibi/ast"
    vfs_key: &'static str,
}
```

**"on the list" = "files embedded" = "importable".**

the registry lives in a shared file (`src/vfs_registry.rs`) that's `include!`d by both
`sandbox.rs` and `build.rs`. pure const data, no dependencies.

### file listing: manual with validation

each `VfsEntry` manually lists its files as part of the security vetting process. adding
a module to the registry requires reviewing which files it includes.

build.rs validates: for each `.sld` in the registry, parse `(include ...)` /
`(include-ci ...)` / `(include-shared ...)` directives (including inside `cond-expand`
branches) and verify all referenced files are in the entry's `files` list. if a `.sld`
references a file not in the list, the build fails.

for `cond-expand`, embed files from ALL branches (both threads and no-threads variants
etc.). the cost is a few extra KiB; the benefit is one binary works for all configs.

### auto-extracted export lists for UX stubs

build.rs uses `tein-sexp` (workspace crate, build dependency) to parse each registry
entry's `.sld` and extract `(export ...)` binding names. only vetted modules in the
registry are parsed — arbitrary `.sld` files that happen to exist in the chibi tree
are ignored. handles `(rename old new)` → takes `new`. generates a rust data file
(`tein_exports.rs` in `OUT_DIR`) mapping module path → `&[&str]` of exported names.

for dynamic modules (`tein/uuid`, `tein/time`) whose `.sld` files don't exist in the
chibi tree, exports are hardcoded in the registry — they're small, stable, and defined
in our own code.

build.rs validates: for each embedded `.sld`, the extracted exports must match the
registry. if a module adds or removes an export upstream, the build fails until the
registry is updated. single source of truth, build-time validated.

at sandbox build time, the export data drives **UX stubs**: for each binding exported
by a registry module that *isn't* importable in this sandbox configuration, a stub is
registered that returns a clear error message:

> sandbox violation: `map` is provided by `(scheme base)`, which is not imported in
> this context

this replaces the old `ALWAYS_STUB` + `ALL_PRESETS` stub machinery with an
automatically maintained, per-binding, informative system. stubs are purely a UX
feature for LLMs and developers — not a security mechanism. security comes from the
vetting process and the VFS gate.

### drop the preset layer

remove:
- `Preset` struct and all 16 preset definitions
- `ALL_PRESETS`, `ALWAYS_STUB`
- `allow()` method (the per-binding variant)
- `preset()` method
- `allowed_primitives` field on `ContextBuilder`
- the `env_copy_named` / cherry-picking / sandbox-stub machinery in `build()`
- `has_io_wrappers` field on `Context` (IO unification from #91 design)

the sandbox model becomes: `sandboxed()` creates a restricted env with syntax forms +
`import`. all other bindings come through module imports. the VFS gate controls which
modules are importable.

the preset layer provided per-binding granularity (e.g. "allow `+` but not
`string-append`") which has no compelling use case. real security boundaries are
filesystem, network, process, step limits, heap — all controlled at the module or
context level. modules already provide sufficient granularity.

### security model: vetting IS the boundary

the old design had two security layers for binding access: (1) preset allowlists
controlling which bindings exist, and (2) `ALWAYS_STUB` blocking env-escape primitives.
both are replaced by a single principle: **if a module is in the safe set, all its
exports are safe.** if any export is dangerous, the module doesn't belong in the safe
set.

modules removed from `Modules::Safe` (available via `Modules::All` or explicit
`allow_module()`):
- `scheme/eval` — exports `eval`, `environment`; `environment` can construct envs
  from arbitrary import specs
- `scheme/repl` — exports `interaction-environment`, which returns the original full
  standard env (chibi parameter, not context-aware)
- `scheme/time` — transitively depends on unvetted modules (unchanged from current)
- `scheme/show` — transitively depends on unvetted modules (unchanged from current)
- `tein/process` — `command-line` leaks host argv (unchanged from current)

internal chibi primitives (`%meta-env`, `env-parent`, `env-exports`, `%import`, `%load`,
`find-module-file`, `add-module-directory`, etc.) are not exported by any module. they
exist in the standard env but the sandbox starts from a null env — these are simply
unreachable via module imports. no blocklist needed.

### new builder API

```rust
/// module set presets for sandboxed contexts.
enum Modules {
    /// conservative safe set — default for sandboxed contexts.
    /// scheme/base, scheme/write, scheme/read, scheme/char, tein/*, etc.
    Safe,
    /// all vetted modules in the registry.
    All,
    /// no modules — syntax + import only.
    None,
    /// custom explicit module list (deps resolved automatically).
    Only(Vec<String>),
}

impl Modules {
    /// construct a custom module list. deps resolved automatically.
    pub fn only(modules: &[&str]) -> Self {
        Modules::Only(modules.iter().map(|s| s.to_string()).collect())
    }
}

impl Default for Modules {
    fn default() -> Self { Modules::Safe }
}
```

```rust
// unsandboxed — full chibi env, no restrictions
Context::builder().standard_env().build()

// sandboxed with default safe modules
Context::builder().standard_env().sandboxed(Modules::Safe).build()

// sandboxed + extra modules beyond default set
Context::builder().standard_env().sandboxed(Modules::Safe)
    .allow_module("tein/process")
    .build()

// sandboxed with all vetted modules
Context::builder().standard_env().sandboxed(Modules::All).build()

// sandboxed with specific modules only
Context::builder().standard_env()
    .sandboxed(Modules::only(&["scheme/base", "scheme/write"]))
    .build()

// sandboxed with nothing — syntax + import only
Context::builder().standard_env().sandboxed(Modules::None).build()

// step limit + sandbox
Context::builder().standard_env().sandboxed(Modules::Safe)
    .step_limit(50_000)
    .build()

// sandbox + file IO policy
Context::builder().standard_env().sandboxed(Modules::Safe)
    .file_read(&["/data/"])
    .build()
```

- `sandboxed(preset)` = restricted env + VFS gate with given module set
- `allow_module(path)` = additive, extends the set + transitive deps
- `file_read()` / `file_write()` = configure FsPolicy (unchanged)

`Modules::Safe` is the recommended default. future presets (e.g. `Modules::Minimal`,
`Modules::Compute`) can be added as enum variants without new API methods.

### sandbox env construction (simplified)

current flow: create null env → cherry-pick bindings via `env_copy_named` → install
stubs for uncovered bindings → install IO wrappers.

new flow:
1. load standard env (needed so `import` macro and module system exist)
2. create null env with syntax forms
3. copy `import` from standard env into null env via `env_copy_named`
   (`import` is a macro defined in `meta-7.scm`, not a core form — confirmed that
   `sexp_make_null_env` only includes 10 core syntax forms)
4. register UX stubs for unexported bindings (auto-generated from export lists)
5. set VFS gate with allowlist from resolved module set
6. set `IS_SANDBOXED = true`
7. set `FsPolicy` if configured
8. set null env as context env

the module system handles everything from there — `(import (scheme base))` loads the
module's bindings into the env, shadowing any stubs for those names.

### dynamic / feature-gated modules

- **uuid, time**: generated by `#[tein_module]`, registered at runtime via
  `tein_vfs_register()`. registry entry has `source: VfsSource::Dynamic` and
  `feature: Some("uuid")` / `Some("time")`. build.rs skips them for embedding.
  exports hardcoded in registry (small, stable, our own code).
- **json, toml**: feature-gated. registry entry has `feature: Some("json")` /
  `Some("toml")`. build.rs conditionally includes their files. runtime registry
  lookup is also conditional.

### what about modules NOT in the registry?

not embeddable, not importable in sandboxes. in unsandboxed contexts (VFS gate off),
chibi can find them on the filesystem via `CHIBI_MODULE_PATH` as usual.

### bootstrap modules

`init-7.scm` and `meta-7.scm` are bootstrap files, not modules. they're loaded before
the module system exists. they stay as separate entries in build.rs, outside the registry.

similarly, `scheme/base.sld` and its `.scm` includes are special — they're loaded during
`load_standard_env` before the sandbox is set up. they must always be embedded regardless
of module selection. the registry still lists them (for allowlist purposes) but build.rs
embeds them unconditionally.

### VFS shadow modules (future, #91)

the shadow system (dynamic VFS registration of replacement `.sld` files for
`scheme/file`, `scheme/load`, etc.) sits on top of this foundation. once the registry is
solid, shadows are just additional dynamic VFS entries registered during sandboxed
context build.

## backward compatibility

this is a **breaking API change**. `preset()`, `allow()`, `safe()`, `pure_computation()`
are all removed. migration guide:

```rust
// before                                  // after
.safe()                                    .sandboxed(Modules::Safe)
.safe().allow_module("tein/process")       .sandboxed(Modules::Safe).allow_module("tein/process")
.preset(&ARITHMETIC)                       .sandboxed(Modules::only(&["scheme/base"]))
.pure_computation()                        .sandboxed(Modules::only(&["scheme/base"]))
.preset(&ARITHMETIC).allow(&["import"])    .sandboxed(Modules::only(&["scheme/base"]))
```

backward compat is not a priority per AGENTS.md. version bump accordingly.

## tests

- for every default-safe module: `(import (...))` succeeds in `Modules::Safe` context
- for every non-default module: `(import (...))` fails in `Modules::Safe` but succeeds
  when explicitly allowed via `allow_module()`
- `Modules::None`: only syntax available, `(+ 1 2)` → undefined variable
- `Modules::All`: every registry module importable
- `allow_module("X")` automatically makes transitive deps importable
- feature-gated modules only available when feature enabled
- dynamic modules (uuid, time) available when feature enabled
- build.rs validation: `.sld` with undeclared `(include ...)` fails the build
- build.rs validation: `.sld` with undeclared `(export ...)` binding fails the build
- unsandboxed context: all modules importable regardless of registry
- `file_read()` / `file_write()` still works with new sandbox model
- UX stubs: calling an unexported binding returns informative error naming the module
- `scheme/eval` and `scheme/repl` not importable in `Modules::Safe`
- env-escape attempt via `Modules::All` + `(scheme eval)`: `eval` + `environment`
  work but are scoped to the sandbox env (no unrestricted env accessible)
