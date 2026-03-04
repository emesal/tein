# sandbox: fake environment variables + command-line

github issue: #99
date: 2026-03-04
status: approved

## motivation

sandboxed contexts neuter `get-environment-variable` (always `#f`),
`get-environment-variables` (always `'()`), and `command-line` (always
`'("tein")`). this is safe but inflexible — some scheme code legitimately
needs env-like configuration (e.g. `srfi/128` reads `CHIBI_HASH_SALT`).

## design

### approach: separate thread-locals (approach A)

two new thread-locals following the existing `FS_POLICY` / `VFS_ALLOWLIST`
pattern, each with a `prev_*` field on `Context` for restore-on-drop.

### thread-locals

```rust
thread_local! {
    static SANDBOX_ENV: RefCell<Option<HashMap<String, String>>> = RefCell::new(None);
    static SANDBOX_COMMAND_LINE: RefCell<Option<Vec<String>>> = RefCell::new(None);
}
```

`None` = not configured. `Some(map)` / `Some(vec)` = use this data.

### Context fields

```rust
prev_sandbox_env: Option<HashMap<String, String>>,
prev_sandbox_command_line: Option<Vec<String>>,
```

### ContextBuilder fields + methods

```rust
sandbox_env: Option<HashMap<String, String>>,
sandbox_command_line: Option<Vec<String>>,
```

- `.environment_variables(&[(&str, &str)])` — merges with default seed
- `.command_line(&[&str])` — overrides default entirely

### defaults (sandboxed contexts)

- env: `{"TEIN_SANDBOX": "true"}`
- command-line: `["tein", "--sandbox"]`

explicit builder calls merge (env) or replace (command-line) these defaults.
unsandboxed contexts ignore both settings — trampolines use real
`std::env` / real argv.

### build() logic

when sandboxed:
1. seed defaults
2. if builder `.environment_variables()` set, merge user entries on top (user wins)
3. if builder `.command_line()` set, replace defaults entirely
4. save prev thread-local values, set new values

### trampoline changes

- `get_env_var_trampoline`: IS_SANDBOXED → check SANDBOX_ENV → lookup or `#f`
- `get_env_vars_trampoline`: IS_SANDBOXED → check SANDBOX_ENV → alist or `'()`
- `command_line_trampoline`: IS_SANDBOXED → check SANDBOX_COMMAND_LINE → list

### shadow modules

`scheme/process-context` and `srfi/98` re-export `(tein process)` — no
changes needed; they inherit the new behaviour.

### testing

- sandboxed defaults: `(get-environment-variable "TEIN_SANDBOX")` → `"true"`,
  `(command-line)` → `("tein" "--sandbox")`
- custom env merges with defaults, custom command-line replaces
- unsandboxed ignores fake env/command-line
- existing tests updated for new defaults
