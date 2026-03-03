# module inventory

status of all chibi-scheme modules in tein's VFS registry.

**legend:**
- ✅ in VFS, safe (`default_safe: true`)
- 🔒 in VFS, unsafe (`default_safe: false`) — available in `Modules::All` only
- 🌑 shadow — VFS entry replaces native with sandboxed impl
- ❌ not in VFS — blocked/inaccessible in sandboxed contexts
- ➕ not in VFS — needs adding (pure/safe, no sandboxing needed)
- ⚠️  not in VFS — needs shadow/trampoline before it can be added
- 🔧 in VFS but needs review (fields tagged `?` or safety unclear)

---

## r7rs standard library (`scheme/*`)

r7rs small: `scheme/base` + the 25 standard libraries.

| module | status | notes |
|--------|--------|-------|
| `scheme/base` | ✅ | core |
| `scheme/bitwise` | ✅ | |
| `scheme/box` | ✅ | |
| `scheme/bytevector` | ✅ | |
| `scheme/case-lambda` | ✅ | |
| `scheme/char` | ✅ | |
| `scheme/charset` | ✅ | non-standard extension of r7rs |
| `scheme/comparator` | ✅ | |
| `scheme/complex` | ✅ | |
| `scheme/cxr` | ✅ | |
| `scheme/division` | ✅ | |
| `scheme/ephemeron` | ✅ | |
| `scheme/eval` | 🔒 | exposes `eval` + `environment`; shadowing tracked in GH #97 |
| `scheme/file` | 🌑 | shadow → `tein/file` (FsPolicy enforcement) |
| `scheme/fixnum` | ✅ | |
| `scheme/flonum` | ✅ | |
| `scheme/generator` | ✅ | |
| `scheme/hash-table` | ✅ | |
| `scheme/ideque` | ✅ | |
| `scheme/ilist` | ✅ | |
| `scheme/inexact` | ✅ | |
| `scheme/lazy` | ✅ | |
| `scheme/list` | ✅ | |
| `scheme/list-queue` | ✅ | |
| `scheme/load` | ❌ | blocked; use `tein/load` instead |
| `scheme/lseq` | ✅ | |
| `scheme/mapping` | ✅ | |
| `scheme/mapping/hash` | 🔒 | hash-backed mappings; pulls in `srfi/146/hash` (unsafe) |
| `scheme/process-context` | 🌑 | shadow → `tein/process` (neutered env/argv) |
| `scheme/r5rs` | ❌ | re-exports scheme/file+load+process-context; blocked |
| `scheme/read` | ✅ | |
| `scheme/red` | 🔒 | r7rs red standard — pulls `scheme/eval + scheme/load`; use `.allow_module("scheme/red")` to enable |
| `scheme/regex` | ✅ | |
| `scheme/repl` | 🌑 | shadow → neutered `interaction-environment` |
| `scheme/rlist` | ✅ | |
| `scheme/set` | ✅ | |
| `scheme/show` | ✅ | |
| `scheme/small` | 🔒 | r7rs small standard — pulls `scheme/eval + scheme/load`; use `.allow_module("scheme/small")` to enable |
| `scheme/sort` | ✅ | |
| `scheme/stream` | ✅ | |
| `scheme/text` | ✅ | |
| `scheme/time` | 🔒 | depends on scheme/process-context; use `tein/time` instead |
| `scheme/time/tai` | 🔒 | needs external TAI data; unsafe by default |
| `scheme/time/tai-to-utc-offset` | 🔒 | same |
| `scheme/vector` | ✅ | |
| `scheme/vector/base` | ✅ | r7rs alias to `srfi/160/base` |
| `scheme/vector/c128` | ✅ | r7rs alias to `srfi/160/c128` |
| `scheme/vector/c64` | ✅ | r7rs alias to `srfi/160/c64` |
| `scheme/vector/f32` | ✅ | r7rs alias to `srfi/160/f32` |
| `scheme/vector/f64` | ✅ | r7rs alias to `srfi/160/f64` |
| `scheme/vector/s8` | ✅ | r7rs alias to `srfi/160/s8` |
| `scheme/vector/s16` | ✅ | r7rs alias to `srfi/160/s16` |
| `scheme/vector/s32` | ✅ | r7rs alias to `srfi/160/s32` |
| `scheme/vector/s64` | ✅ | r7rs alias to `srfi/160/s64` |
| `scheme/vector/u8` | ✅ | r7rs alias to `srfi/160/u8` |
| `scheme/vector/u16` | ✅ | r7rs alias to `srfi/160/u16` |
| `scheme/vector/u32` | ✅ | r7rs alias to `srfi/160/u32` |
| `scheme/vector/u64` | ✅ | r7rs alias to `srfi/160/u64` |
| `scheme/write` | ✅ | |
| `scheme/char/normalization` | ✅ | |

---

## srfi libraries (`srfi/*`)

| module | status | notes |
|--------|--------|-------|
| `srfi/1` | ✅ | list library |
| `srfi/1/immutable` | ✅ | |
| `srfi/2` | ✅ | and-let* |
| `srfi/6` | ✅ | basic string ports |
| `srfi/8` | ✅ | receive |
| `srfi/9` | ✅ | define-record-type |
| `srfi/11` | ✅ | let-values |
| `srfi/14` | ✅ | char-sets |
| `srfi/16` | ✅ | case-lambda |
| `srfi/18` | 🔒 | OS threads; posix-only, deliberately unsafe |
| `srfi/23` | ✅ | error |
| `srfi/26` | ✅ | cut/cute |
| `srfi/27` | ✅ | random numbers (PRNG, no OS seeding side-effects) |
| `srfi/33` | ✅ | bitwise ops (deprecated alias for srfi/151) |
| `srfi/35` | ✅ | conditions |
| `srfi/35/internal` | ✅ | |
| `srfi/38` | ✅ | write-with-shared-structure |
| `srfi/39` | ✅ | parameters (dynamic binding) |
| `srfi/41` | ✅ | streams |
| `srfi/46` | ✅ | basic syntax-rules extensions |
| `srfi/55` | ✅ | require-extension |
| `srfi/64` | 🔒 | testing framework (depends on scheme/eval) |
| `srfi/69` | ✅ | basic hash tables |
| `srfi/95` | ✅ | sort |
| `srfi/98` | 🌑 | env vars — shadow stubs always return `#f` |
| `srfi/99` | ✅ | records |
| `srfi/99/records` | ✅ | |
| `srfi/99/records/inspection` | ✅ | |
| `srfi/99/records/procedural` | ✅ | |
| `srfi/99/records/syntactic` | ✅ | |
| `srfi/101` | ✅ | random-access lists |
| `srfi/111` | ✅ | boxes |
| `srfi/113` | ✅ | sets and bags |
| `srfi/115` | ✅ | regexp |
| `srfi/116` | ✅ | immutable lists |
| `srfi/117` | ✅ | mutable queues |
| `srfi/121` | ✅ | generators |
| `srfi/124` | ✅ | ephemerons |
| `srfi/125` | ✅ | hash tables |
| `srfi/127` | ✅ | lazy sequences |
| `srfi/128` | ✅ | comparators |
| `srfi/129` | ✅ | titlecase |
| `srfi/130` | ✅ | string cursors |
| `srfi/132` | ✅ | sort libraries |
| `srfi/133` | ✅ | vector library |
| `srfi/134` | ✅ | immutable deques |
| `srfi/135` | ✅ | immutable texts |
| `srfi/135/kernel8` | ✅ | |
| `srfi/139` | ✅ | syntax parameters |
| `srfi/141` | ✅ | integer division |
| `srfi/142` | ✅ | bitwise ops (deprecated alias for srfi/151) |
| `srfi/143` | ✅ | fixnums |
| `srfi/144` | ✅ | flonums |
| `srfi/145` | ✅ | assumptions |
| `srfi/146` | ✅ | mappings |
| `srfi/146/hamt` | ✅ | |
| `srfi/146/hamt-map` | ✅ | |
| `srfi/146/hamt-misc` | ✅ | |
| `srfi/146/hash` | 🔒 | hash-map backing |
| `srfi/146/vector-edit` | ✅ | |
| `srfi/147` | ✅ | custom macro transformers |
| `srfi/151` | ✅ | bitwise ops |
| `srfi/154` | ✅ | first-class dynamic extents |
| `srfi/158` | ✅ | generators and accumulators |
| `srfi/159` | ✅ | show (earlier version of srfi/166); shares .scm files via `../166/` relative includes |
| `srfi/159/base` | ✅ | |
| `srfi/159/color` | ✅ | |
| `srfi/159/columnar` | ✅ | |
| `srfi/159/unicode` | ✅ | |
| `srfi/160/base` | ✅ | homogeneous numeric vectors |
| `srfi/160/c128` | ✅ | |
| `srfi/160/c64` | ✅ | |
| `srfi/160/f8` | ✅ | |
| `srfi/160/f16` | ✅ | |
| `srfi/160/f32` | ✅ | |
| `srfi/160/f64` | ✅ | |
| `srfi/160/mini` | ✅ | |
| `srfi/160/prims` | ✅ | C-backed via hand-written `uvprims.c` in chibi fork |
| `srfi/160/s8` | ✅ | |
| `srfi/160/s16` | ✅ | |
| `srfi/160/s32` | ✅ | |
| `srfi/160/s64` | ✅ | |
| `srfi/160/u8` | ✅ | |
| `srfi/160/u16` | ✅ | |
| `srfi/160/u32` | ✅ | |
| `srfi/160/u64` | ✅ | |
| `srfi/160/uvector` | ✅ | |
| `srfi/165` | ✅ | the environment monad |
| `srfi/166` | ✅ | monadic formatting |
| `srfi/166/base` | ✅ | |
| `srfi/166/color` | ✅ | |
| `srfi/166/columnar` | ✅ | |
| `srfi/166/pretty` | ✅ | |
| `srfi/166/unicode` | ✅ | |
| `srfi/179` | ✅ | nonempty intervals + generalized arrays |
| `srfi/179/base` | ✅ | |
| `srfi/188` | ✅ | splicing binding constructs |
| `srfi/193` | ❌ | command channel — not in VFS |
| `srfi/211/identifier-syntax` | ✅ | |
| `srfi/211/variable-transformer` | ✅ | |
| `srfi/219` | ✅ | define higher-order lambda |
| `srfi/227` | ✅ | optional arguments |
| `srfi/227/definition` | ✅ | re-exports `define-optionals` from `chibi/optional` |
| `srfi/229` | ✅ | tagged procedures |
| `srfi/231` | ✅ | revised intervals and generalized arrays (successor to srfi/179) |
| `srfi/231/base` | ✅ | |

---

## chibi internal modules (`chibi/*`)

these are chibi-specific, not r7rs standard. many are safe pure libs; some touch OS.

| module | status | notes |
|--------|--------|-------|
| `chibi/app` | ❌ | CLI app framework — reads env/args, needs shadow |
| `chibi/apropos` | ❌ | reflects on env/module contents |
| `chibi/assert` | ✅ | |
| `chibi/ast` | ✅ | AST introspection; internal dep (srfi/18, chibi/io etc) |
| `chibi/base64` | ✅ | pure encoder/decoder |
| `chibi/binary-record` | ✅ | binary record type macros — pure scheme |
| `chibi/bytevector` | ✅ | bytevector extras (IEEE-754 floats) |
| `chibi/channel` | ✅ | pure-scheme FIFO channel; embedded. depends on srfi/18 (threads, disabled) — in VFS but channel ops unavailable without thread support |
| `chibi/char-set` | ✅ | |
| `chibi/char-set/ascii` | ✅ | |
| `chibi/char-set/base` | ✅ | |
| `chibi/char-set/boundary` | ✅ | |
| `chibi/char-set/extras` | ✅ | |
| `chibi/char-set/full` | ✅ | |
| `chibi/config` | ❌ | reads config files — file i/o |
| `chibi/crypto/md5` | ✅ | pure hash |
| `chibi/crypto/rsa` | ✅ | RSA crypto — pure scheme |
| `chibi/crypto/sha2` | ✅ | pure hash; cond-expand takes srfi/151 + chibi/bytevector path |
| `chibi/csv` | ✅ | CSV parser |
| `chibi/diff` | ✅ | diff algorithm |
| `chibi/disasm` | ❌ | chibi bytecode disassembler — exposes internals |
| `chibi/doc` | ❌ | documentation extraction — file i/o |
| `chibi/edit-distance` | ✅ | edit distance algorithm |
| `chibi/emscripten` | ❌ | browser/JS interop — not applicable |
| `chibi/equiv` | ✅ | |
| `chibi/filesystem` | ✅ | sandbox stub (phase 1) — importable, all fns raise `[sandbox:chibi/filesystem]` error |
| `chibi/generic` | ✅ | generic functions |
| `chibi/heap-stats` | ❌ | GC heap introspection — internal |
| `chibi/highlight` | ✅ | syntax highlighting — pure scheme |
| `chibi/ieee-754` | ❌ | not in lib? (listed in original inventory but no .sld found) |
| `chibi/io` | ✅ | string/port i/o helpers; internal dep |
| `chibi/iset` | ✅ | |
| `chibi/iset/base` | ✅ | |
| `chibi/iset/constructors` | ✅ | |
| `chibi/iset/iterators` | ✅ | |
| `chibi/iset/optimize` | ✅ | integer set rebalancing + optimisation; pure scheme |
| `chibi/json` | ❌ | use `tein/json` instead |
| `chibi/log` | ❌ | logging — writes to stderr, file |
| `chibi/loop` | ✅ | loop macros |
| `chibi/match` | ✅ | pattern matching |
| `chibi/math/prime` | ✅ | prime factorisation |
| `chibi/memoize` | ✅ | in-memory LRU cache works; file-backed errors via shadowed deps (#105) |
| `chibi/mime` | ✅ | pure MIME parsing — base64, content-type, message folding |
| `chibi/modules` | ❌ | module reflection — exposes module internals |
| `chibi/monad/environment` | ✅ | environment monad |
| `chibi/net` | ✅ | sandbox stub (phase 1) — importable, all fns/consts stubbed |
| `chibi/net/http` | ✅ | sandbox stub (phase 1) |
| `chibi/net/http-server` | ✅ | sandbox stub (phase 1) |
| `chibi/net/server` | ✅ | sandbox stub (phase 1) |
| `chibi/net/server-util` | ✅ | sandbox stub (phase 1) |
| `chibi/net/servlet` | ✅ | sandbox stub (phase 1) |
| `chibi/optimize` | ❌ | compiler optimiser internals |
| `chibi/optimize/profile` | ❌ | |
| `chibi/optimize/rest` | ❌ | |
| `chibi/optional` | ✅ | |
| `chibi/parse` | ✅ | PEG parser |
| `chibi/parse/common` | ✅ | |
| `chibi/pathname` | ✅ | path manipulation |
| `chibi/process` | ✅ | sandbox stub (phase 1) — importable, all fns/consts stubbed (note: fn `exit` overlaps with tein/process) |
| `chibi/pty` | ❌ | pseudo-terminals — dangerous ⚠️ |
| `chibi/quoted-printable` | ✅ | MIME quoted-printable encoding |
| `chibi/regexp` | ✅ | |
| `chibi/regexp/pcre` | ❌ | PCRE backend — not in VFS |
| `chibi/reload` | ❌ | module reloading — file i/o |
| `chibi/repl` | ❌ | interactive REPL — use tein/reader |
| `chibi/scribble` | ❌ | scribble doc format — file i/o |
| `chibi/shell` | ✅ | sandbox stub (phase 1) — fns + macros all stubbed |
| `chibi/show` | ❌ | not in VFS — use `srfi/166` instead |
| `chibi/show/base` | ✅ | thin alias to `srfi/166/base` |
| `chibi/show/c` | ❌ | C pretty printer |
| `chibi/show/color` | ✅ | `alias-for (srfi 166 color)` |
| `chibi/show/column` | ✅ | `alias-for (srfi 166 columnar)` |
| `chibi/show/pretty` | ✅ | `alias-for (srfi 166 pretty)` |
| `chibi/show/shared` | ✅ | internal dep only |
| `chibi/show/unicode` | ✅ | `alias-for (srfi 166 unicode)` |
| `chibi/snow/*` | ❌ | snow package manager — file i/o + network ⚠️ |
| `chibi/string` | ✅ | |
| `chibi/stty` | ❌ | terminal control — OS ⚠️ |
| `chibi/sxml` | ✅ | SXML |
| `chibi/syntax-case` | ✅ | syntax-case macros |
| `chibi/system` | ✅ | sandbox stub (phase 1) — importable, all fns raise sandbox error |
| `chibi/tar` | ❌ | tar format — file i/o ⚠️ |
| `chibi/temp-file` | ✅ | sandbox stub (phase 1) — importable, fns raise sandbox error |
| `chibi/term/ansi` | ✅ | ANSI terminal escape codes |
| `chibi/term/edit-line` | ❌ | line editing — terminal i/o ⚠️ |
| `chibi/text` | ✅ | text editor operations |
| `chibi/text/base` | ✅ | (includes marks + movement) |
| `chibi/text/marks` | ❌ | included in chibi/text/base |
| `chibi/text/movement` | ❌ | included in chibi/text/base |
| `chibi/text/search` | ✅ | |
| `chibi/text/types` | ✅ | |
| `chibi/text/utf8` | ✅ | (uses portable fallback in tein) |
| `chibi/time` | ✅ | |
| `chibi/trace` | ❌ | execution tracing — debugging |
| `chibi/type-inference` | ❌ | type inference — compiler internal |
| `chibi/uri` | ✅ | URI parsing |
| `chibi/weak` | ✅ | weak references and ephemerons |
| `chibi/win32/process-win32` | ❌ | windows process creation — not applicable on linux |
| `chibi/zlib` | ❌ | zlib compression — C native, needs clib entry |

---

## tein modules (`tein/*`)

tein's own modules — always in VFS.

| module | status | notes |
|--------|--------|-------|
| `tein/docs` | ✅ | |
| `tein/file` | ✅ | sandboxed file i/o (FsPolicy) |
| `tein/foreign` | ✅ | |
| `tein/json` | ✅ | |
| `tein/load` | ✅ | sandboxed load (VFS only) |
| `tein/macro` | ✅ | macro expansion hook |
| `tein/process` | ✅ | neutered env/argv in sandbox |
| `tein/reader` | ✅ | reader dispatch hook |
| `tein/test` | ✅ | |
| `tein/time` | ✅ | |
| `tein/toml` | ✅ | |
| `tein/uuid` | ✅ | |

---

## summary

| category | ✅ safe | 🔒 unsafe | 🌑 shadow | ❌ not in VFS |
|----------|---------|----------|----------|--------------|
| scheme/* | 48 | 7 | 3 | 2 |
| srfi/* | 101 | 3 | 1 | 1 |
| chibi/* | 65 | 0 | 0 | 34 |
| tein/* | 12 | 0 | 0 | 0 |
| **total** | **226** | **10** | **4** | **37** |

### priority queue

**✅ shadow stubs done (phase 1 — error-on-call):**
- `chibi/filesystem`, `chibi/process`, `chibi/system`
- `chibi/shell`, `chibi/temp-file`
- `chibi/net`, `chibi/net/http`, `chibi/net/server`, `chibi/net/http-server`,
  `chibi/net/server-util`, `chibi/net/servlet`
- `chibi/channel` (embedded, not a stub — but depends on srfi/18 / threads)

**⚠️ still needs shadow/trampoline (not in VFS):**
- `chibi/stty`, `chibi/term/edit-line` — terminal i/o
- `chibi/tar` — file i/o
- `chibi/app`, `chibi/config`, `chibi/log` — env/args/logging
- `chibi/apropos`, `srfi/193` — env introspection / info leak
- `scheme/load` — arbitrary file loading (already blocked; use `tein/load`)
- `scheme/r5rs` — already blocked

**phase 2 (selective gating — not started):**
- selectively expose safe fns from stub modules with real FS/network policy checks
- e.g. `chibi/filesystem` `file-exists?`, `file-size`; `chibi/process` `current-process-id`

**intentionally excluded (not useful for embedding):**
- see appendix B for rationale per module

---

## appendix A: shadow stub rationale

modules added to the VFS as shadow stubs (error-on-call). importing succeeds; calling
any exported function raises `[sandbox:module/path] fn-name not available`. this lets
code that conditionally uses these modules load without crashing, while preventing
actual OS access. see #105 for future progressive gating (selectively unshadowing
safe operations with real implementations).

### phase 1 stubs (already implemented)

| module | why stubbed |
|--------|-------------|
| `chibi/filesystem` | POSIX filesystem ops: stat, mkdir, readlink, symlink, chmod, chown. gated by FsPolicy; #105 tracks selective unshadowing |
| `chibi/process` | process creation (`system`, `execute`), signals, fork. `exit` overlaps with `tein/process` |
| `chibi/system` | UID/GID queries, hostname, uname — OS identity information leak |
| `chibi/shell` | shell command execution via `shell`, `shell->string`, `shell-pipe` + macros |
| `chibi/temp-file` | creates files in `/tmp` — filesystem write outside policy control |
| `chibi/net` | BSD socket API: `open-net-io`, `make-listener-socket`, address resolution |
| `chibi/net/http` | HTTP client — network access |
| `chibi/net/server` | TCP server loop — network listener |
| `chibi/net/http-server` | HTTP server framework — network listener + filesystem serving |
| `chibi/net/server-util` | server utilities (logging, connection handling) |
| `chibi/net/servlet` | HTTP servlet framework — request/response handling with network + filesystem |

### phase 2 stubs (planned)

| module | why stubbed |
|--------|-------------|
| `chibi/stty` | terminal control: `stty`, `with-raw-io`, `get-terminal-width`. C-backed via `include-shared`; no pre-generated `.c` exists and the real impl is unsafe (raw ioctl) |
| `chibi/term/edit-line` | interactive line editor depending on `chibi/stty` for terminal mode switching |
| `chibi/log` | logging framework deeply coupled to OS: file locking (`file-lock`), process/user IDs for log prefixes, `open-output-file/append`. #105 could enable scoped log file writing |
| `chibi/app` | CLI application framework depending on `chibi/config` (filesystem) and `scheme/process-context` (argv/env). stubs let libraries that optionally import it still load |
| `chibi/config` | config file reader using `scheme/file` + `chibi/filesystem` (`file-directory?`). #105 could enable reading from allowed paths |
| `chibi/tar` | tar archive handling hard-wired to `chibi/filesystem` (15+ direct calls: `create-directory*`, `link-file`, `symbolic-link-file`, `directory-fold-tree`, stat ops). #105 could enable scoped extraction |
| `srfi/193` | SRFI-193 command-line: `command-line`, `command-name`, `script-file`, `script-directory`. leaks host argv and script path — information disclosure in sandbox |
| `chibi/apropos` | `apropos` / `apropos-list` enumerate all bindings in an environment — exposes internal module structure, information leak |

---

## appendix B: intentionally excluded modules

modules deliberately not added to the VFS. these expose chibi internals, target
inapplicable platforms, or have tein-native replacements.

| module | why excluded |
|--------|-------------|
| `chibi/disasm` | chibi bytecode disassembler — exposes VM internals; not useful outside chibi development |
| `chibi/heap-stats` | GC heap introspection — chibi-internal debugging tool |
| `chibi/modules` | module reflection (`module-exports`, `add-module!`, `delete-module!`) — exposes and mutates module system internals |
| `chibi/optimize/*` | compiler optimiser passes (`optimize`, `profile`, `rest`) — chibi compiler internals |
| `chibi/reload` | hot-reload modules from filesystem — arbitrary file loading, bypasses VFS |
| `chibi/repl` | interactive REPL — reads from stdin, writes to stdout, loads files. use `tein/reader` for reader dispatch |
| `chibi/trace` | execution tracing — debugging tool instrumenting chibi's eval, not meaningful in embedded context |
| `chibi/type-inference` | type inference for chibi's compiler — internal optimisation pass |
| `chibi/snow/*` | snow package manager — downloads and installs packages from network, full filesystem access |
| `chibi/emscripten` | emscripten/JS interop — not applicable outside browser/wasm target |
| `chibi/win32/*` | windows process creation — not applicable on linux; tein is linux-first |
| `chibi/doc` | documentation extraction — reads source files, writes output files |
| `chibi/scribble` | scribble document format — file i/o for document generation |
| `chibi/json` | chibi's JSON library — tein provides `(tein json)` with rust-backed implementation |
| `chibi/pty` | pseudo-terminal creation — dangerous OS primitive, not useful for embedded scheme |
| `chibi/show` | top-level show library — use `(srfi 166)` instead (same implementation, standard name) |
| `chibi/show/c` | C pretty-printer — niche formatting tool for C code output |
| `chibi/regexp/pcre` | PCRE regex backend — requires native libpcre; `chibi/regexp` (IrRegex) is already in VFS |
| `chibi/zlib` | zlib compression — requires native libz as clib. potential future feature if demand arises |
| `chibi/ieee-754` | listed in original chibi inventory but no `.sld` found in `lib/` — likely dead/removed |
| `chibi/text/marks` | text editor mark operations — included in `chibi/text/base`, not a standalone module |
| `chibi/text/movement` | text editor cursor movement — included in `chibi/text/base`, not a standalone module |
| `scheme/r5rs` | r5rs mega-bundle re-exporting `scheme/file`, `scheme/eval`, `scheme/load`, `scheme/repl`. blocked on #97 (sandboxed eval). tracked in #106 |
