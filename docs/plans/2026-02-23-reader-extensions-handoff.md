# reader extensions handoff

## context

implementing custom ports + hash dispatch reader extensions for tein. working in worktree `.worktrees/reader-extensions` on branch `feature/reader-extensions`.

**reference docs:**
- design: `docs/plans/2026-02-23-reader-extensions-design.md`
- implementation plan: `~/.claude-mani/plans/woolly-wobbling-stearns.md`

## status: tasks 1-9 complete (of 15), uncommitted

all changes are staged-ready but **not committed** — the plan calls for per-task commits. you can either commit them individually with the messages below or squash into feature commits per your preference.

### what's done

**feature 1: custom ports — fully working + tested + documented**

| task | description | suggested commit |
|------|-------------|-----------------|
| 1 | PortStore scaffolding (`src/port.rs`, wired into Context) | `feat: add PortStore scaffolding for custom ports` |
| 2 | C shim + FFI for `sexp_make_custom_input_port`/`output_port` | `feat: add C shim + FFI for custom port creation` |
| 3 | read trampoline + `open_input_port()` + test | `feat: add read trampoline + open_input_port for custom ports` |
| 4 | `Context::read()` — read one s-expression from a port | `feat: add Context::read() for reading s-expressions from ports` |
| 5 | `Context::evaluate_port()` — read+eval loop from a port | `feat: add Context::evaluate_port() for read+eval from ports` |
| 6 | write trampoline + `open_output_port()` + test | `feat: add output port support (write trampoline + open_output_port)` |
| 7 | edge case tests: multi-sexp read, `input-port?`, scheme `read`, EOF | `test: add edge case tests for custom ports` |
| 8 | docs: AGENTS.md (port.rs + custom port flow), DEVELOPMENT.md (custom port protocol) | `docs: add custom port architecture to AGENTS.md and DEVELOPMENT.md` |

**feature 2: hash dispatch reader extensions — C layer done**

| task | description | suggested commit |
|------|-------------|-----------------|
| 9 | C dispatch table: `tein_reader_dispatch[128]`, set/unset/get/chars/clear, reserved char check | `feat: add reader dispatch table to tein_shim.c` |

**modified files (cumulative):**
- `tein/src/port.rs` (NEW) — PortStore with reader/writer map
- `tein/src/lib.rs` — added `mod port;`
- `tein/src/context.rs` — PORT_STORE_PTR thread-local, PortStoreGuard, read/write trampolines, `register_port_protocol`, `open_input_port`, `open_output_port`, `read`, `evaluate_port`, 9 tests, 3 doctests
- `tein/src/ffi.rs` — extern decls + safe wrappers for `tein_make_custom_input_port`/`output_port`
- `tein/vendor/chibi-scheme/tein_shim.c` — custom port wrappers + reader dispatch table (set/unset/get/chars/clear/reserved)
- `AGENTS.md` — added port.rs to architecture, custom port data flow
- `DEVELOPMENT.md` — added custom port protocol section

**test counts:** 174 lib (was 165) + 13 doctests (was 9). all passing, clippy clean, fmt clean.

### critical findings during implementation

**1. chibi custom port read callback protocol**

the read proc receives `(buf start end)` where `buf[0..start)` already has valid data from prior partial fills. the return value must be `start + new_bytes_read`, NOT just `new_bytes_read`. chibi does `memcpy(C_buffer, string_data(buf), result)` — copies from position 0.

**2. chibi `flush-output-port` availability**

`flush-output-port` is in `(scheme extras)`, not directly available in the standard env. chibi's primitive name is `flush-output`. the output port test uses `flush-output` to trigger the write callback.

**3. plan deviations**

- plan references `ffi::exception_message(ctx, result)` — this function doesn't exist. used the existing pattern of returning exceptions via `Value::from_raw()` instead.
- plan says "no dedicated rust registration API" for reader dispatch (design doc line 85), but task 14 adds `register_reader()`. follow the plan — it's a good improvement.
- plan says register reader protocol "always in build() for standard_env contexts" (task 12), but port protocol uses lazy registration with a flag. both patterns are fine, follow the plan for each.

## what's left: tasks 10-15

### feature 2: hash dispatch reader extensions (remaining)

**task 10: patch sexp.c # dispatch** — patch `sexp.c` reader to check dispatch table before the hardcoded `#` switch. small surgical patch. verify line numbers in vendored copy (plan says ~3511-3512).

**task 11: FFI bindings** — add extern decls + safe wrappers for the 6 dispatch table functions.

**task 12: native dispatch fns + (tein reader) VFS module** — extern "C" wrappers (`reader_set_wrapper`, `reader_unset_wrapper`, `reader_chars_wrapper`), `register_reader_protocol`, create `lib/tein/reader.sld` + `reader.scm`, register in `build.rs` VFS_FILES. the big integration task.

**task 13: reader dispatch tests** — reserved char rejection, handler that reads further from port, unset, introspection via `reader-dispatch-chars`, multi-char sub-dispatch.

**task 14: rust-side `register_reader()` convenience API** — `Context::register_reader(char, &Value)` that calls `ffi::tein_reader_dispatch_set` directly. error on reserved chars.

**task 15: final docs + cleanup** — update AGENTS.md with reader dispatch flow, update TODO.md, final `cargo test && cargo clippy && cargo fmt --check`.

### important notes for tasks 10-15

- the sexp.c patch location (line 3511-3512) should be verified — line numbers may differ in the vendored copy
- reader dispatch state is thread-local (C-level), matching chibi's !Send context model
- `register_reader_protocol` should always run for standard_env contexts per the plan (unlike port protocol's lazy init)
- VFS module registration in build.rs follows the pattern at lines 68-70 (tein/foreign entries)
- reader dispatch must be cleared on Context drop (add to Drop impl, similar to module policy cleanup)

## how to continue

```bash
cd /home/fey/projects/tein/tein-dev/.worktrees/reader-extensions
cargo test                  # verify green baseline (174 lib + 13 doc)
```

execute the implementation plan at `~/.claude-mani/plans/woolly-wobbling-stearns.md` starting from task 10. use the executing-plans skill. commit per the plan's suggested messages (or squash per preference). the plan has exact code for most tasks — follow it but watch for the gotchas documented above.
