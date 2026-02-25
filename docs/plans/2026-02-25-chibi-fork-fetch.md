# chibi-scheme fork fetch implementation plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** replace vendored chibi-scheme directory with build-time git fetch from our fork (`emesal/chibi-scheme`, branch `emesal-tein`).

**Architecture:** build.rs clones/fetches the fork into `target/chibi-scheme/` (project-local, survives `cargo clean`). generated files (install.h, tein_vfs_data.h, tein_clibs.c) write into `OUT_DIR` instead of the source tree. the `tein/vendor/chibi-scheme/` directory is deleted entirely — all chibi files (including tein_shim.c, lib/tein/*, and the eval.c/sexp.c/vm.c patches) live in the fork's `emesal-tein` branch.

**Tech Stack:** rust build script, `cc` crate, git CLI (invoked from build.rs via `std::process::Command`)

---

## pre-work: push tein files to the fork

before any code changes, the fork's `emesal-tein` branch must contain all tein-specific files that currently live in `tein/vendor/chibi-scheme/`. this is a manual git operation on the fork repo, not a tein-dev code change.

**files to ensure exist on `emesal-tein`:**
- `tein_shim.c` (at repo root alongside other .c files)
- `lib/tein/foreign.sld`, `lib/tein/foreign.scm`
- `lib/tein/reader.sld`, `lib/tein/reader.scm`
- `lib/tein/macro.sld`, `lib/tein/macro.scm`

**patches already applied to `emesal-tein`:**
- `eval.c` — VFS module lookup (A), VFS load (B), VFS open-input-file (C), macro expansion hook (D)
- `sexp.c` — reader dispatch table check
- `vm.c` — fuel budget consumption

verify with: `cd /path/to/chibi-fork && git diff main..emesal-tein --stat`

all tein_shim.c and lib/tein/ files must be present before proceeding. if not, copy them from `tein/vendor/chibi-scheme/` and commit to the fork.

---

### task 1: add git fetch helper to build.rs

**files:**
- modify: `tein/build.rs`
- modify: `tein/Cargo.toml` (no new deps needed — using `std::process::Command`)

**step 1: add constants and fetch function to build.rs**

add at the top of build.rs, after the existing `use` statements:

```rust
use std::process::Command;

const CHIBI_REPO: &str = "https://github.com/emesal/chibi-scheme.git";
const CHIBI_BRANCH: &str = "emesal-tein";
```

add a new function before `main()`:

```rust
/// fetch or update the chibi-scheme fork into `target/chibi-scheme/`.
///
/// clones on first build, then fetches + resets to branch tip on subsequent builds.
/// uses `target/chibi-scheme/` so it survives `cargo clean` (which only removes
/// `target/{debug,release,...}`) and is shared across profiles.
fn fetch_chibi() -> String {
    // resolve workspace target dir (two levels up from tein/build.rs → workspace root)
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");
    let workspace_root = Path::new(&manifest_dir)
        .parent()
        .expect("tein crate must be in a workspace");
    let chibi_dir = workspace_root.join("target").join("chibi-scheme");

    if chibi_dir.join(".git").exists() {
        // fetch latest and reset to branch tip
        let fetch = Command::new("git")
            .args(["fetch", "origin", CHIBI_BRANCH])
            .current_dir(&chibi_dir)
            .status()
            .expect("failed to run git fetch");
        assert!(fetch.success(), "git fetch failed");

        let reset = Command::new("git")
            .args(["reset", "--hard", &format!("origin/{CHIBI_BRANCH}")])
            .current_dir(&chibi_dir)
            .status()
            .expect("failed to run git reset");
        assert!(reset.success(), "git reset failed");
    } else {
        // initial clone
        let clone = Command::new("git")
            .args([
                "clone",
                "--branch", CHIBI_BRANCH,
                "--single-branch",
                "--depth", "1",
                CHIBI_REPO,
                chibi_dir.to_str().expect("non-utf8 path"),
            ])
            .status()
            .expect("failed to run git clone");
        assert!(clone.success(), "git clone failed");
    }

    chibi_dir.to_str().expect("non-utf8 path").to_string()
}
```

**step 2: run build to verify it compiles (fetch function isn't called yet)**

run: `cd /home/fey/projects/tein/tein-dev && cargo build 2>&1 | tail -5`
expected: builds successfully (fetch_chibi is dead code for now, may get a warning)

**step 3: commit**

```bash
git add tein/build.rs
git commit -m "build: add git fetch helper for chibi-scheme fork"
```

---

### task 2: update main() to use fetched chibi dir

**files:**
- modify: `tein/build.rs`

**step 1: update main() to call fetch_chibi() and use its path**

replace the first two lines of `main()`:

```rust
// old:
let chibi_dir = "vendor/chibi-scheme";
let include_dir = format!("{chibi_dir}/include");
```

with:

```rust
let chibi_dir = fetch_chibi();
let include_dir = format!("{chibi_dir}/include");
```

**step 2: redirect generated files to OUT_DIR**

currently `generate_vfs_data` and `generate_clibs` write into the chibi source tree. since that tree is now a git checkout, we should write generated files into `OUT_DIR` and add that as an include path.

in `main()`, after the `let include_dir = ...` line, add:

```rust
let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");
```

update `generate_vfs_data` call:

```rust
generate_vfs_data(&chibi_dir, &out_dir);
```

update `generate_clibs` call:

```rust
generate_clibs(&chibi_dir, &out_dir);
```

update the cc::Build to include `OUT_DIR` (for tein_vfs_data.h) and compile tein_clibs.c from there:

in the `sources` array, change `"tein_clibs.c"` handling. the sources array should only contain files in `chibi_dir`. handle the generated tein_clibs.c separately:

```rust
let sources = [
    "sexp.c",
    "bignum.c",
    "gc.c",
    "gc_heap.c",
    "opcodes.c",
    "vm.c",
    "eval.c",
    "simplify.c",
    "tein_shim.c",
];
```

then after the `for src in &sources` loop that adds files, add:

```rust
build.file(format!("{out_dir}/tein_clibs.c"));
```

add `OUT_DIR` as an include path for the build (so tein_shim.c can find tein_vfs_data.h):

```rust
build.include(&out_dir);
```

**step 3: update generate_vfs_data signature and output path**

change signature from:

```rust
fn generate_vfs_data(chibi_dir: &str) {
    let out_path = Path::new(chibi_dir).join("tein_vfs_data.h");
```

to:

```rust
fn generate_vfs_data(chibi_dir: &str, out_dir: &str) {
    let out_path = Path::new(out_dir).join("tein_vfs_data.h");
```

**step 4: update generate_clibs signature and output path**

change signature from:

```rust
fn generate_clibs(chibi_dir: &str) {
    let out_path = Path::new(chibi_dir).join("tein_clibs.c");
```

to:

```rust
fn generate_clibs(chibi_dir: &str, out_dir: &str) {
    let out_path = Path::new(out_dir).join("tein_clibs.c");
```

**step 5: update generate_install_h to write to OUT_DIR**

install.h should also go into `OUT_DIR` now. update `main()` call:

```rust
generate_install_h(&include_dir, &out_dir);
```

BUT WAIT — install.h needs to be at `chibi/install.h` relative to an include path. so we need to create the subdirectory:

```rust
fn generate_install_h(include_dir: &str, out_dir: &str) {
    let chibi_out = Path::new(out_dir).join("chibi");
    fs::create_dir_all(&chibi_out).expect("failed to create chibi/ in OUT_DIR");
    let install_h_path = chibi_out.join("install.h");
```

and the `OUT_DIR` include (already added above) will let `#include <chibi/install.h>` resolve there. BUT — the fetched repo's `include/chibi/` dir also has headers (sexp.h, features.h, etc.) and the compiler searches include paths in order. we need `OUT_DIR` to be searched BEFORE the repo's include dir so our generated install.h wins. update the build order:

```rust
build
    .include(&out_dir)       // generated install.h first
    .include(&include_dir)   // then repo headers
    .include(&chibi_dir)
```

remove the old `include_dir` parameter from `generate_install_h` since it no longer writes there.

**step 6: update rerun-if-changed directives**

the rerun-if-changed paths need to use `chibi_dir` (which is now an absolute path, that's fine). update the last few:

```rust
println!("cargo:rerun-if-changed={include_dir}/chibi/sexp.h");
println!("cargo:rerun-if-changed={include_dir}/chibi/features.h");
println!("cargo:rerun-if-changed=build.rs");
```

these already use `include_dir` which derives from `chibi_dir`, so they'll resolve correctly. no change needed here — just verify.

**step 7: verify tein_shim.c includes tein_vfs_data.h correctly**

check how tein_shim.c includes the VFS header. if it uses `#include "tein_vfs_data.h"` (quoted), the `-I OUT_DIR` flag will find it. if it uses `#include <tein_vfs_data.h>`, same thing. either way works with the OUT_DIR include.

run: `grep tein_vfs_data tein/vendor/chibi-scheme/tein_shim.c` to verify the include style.

**step 8: build and test**

run: `cd /home/fey/projects/tein/tein-dev && cargo build 2>&1 | tail -10`
expected: successful build, fetching chibi into `target/chibi-scheme/`

run: `cd /home/fey/projects/tein/tein-dev && cargo test 2>&1 | tail -20`
expected: all 196+ tests pass

**step 9: commit**

```bash
git add tein/build.rs
git commit -m "build: fetch chibi-scheme from fork, generate into OUT_DIR"
```

---

### task 3: remove vendored chibi-scheme directory

**files:**
- delete: `tein/vendor/chibi-scheme/` (entire directory)
- modify: `.gitignore`

**step 1: verify the build works from fetched source (not vendor)**

confirm `target/chibi-scheme/` exists and was used:

run: `ls /home/fey/projects/tein/tein-dev/target/chibi-scheme/tein_shim.c`
expected: file exists

run: `cd /home/fey/projects/tein/tein-dev && cargo test 2>&1 | tail -10`
expected: all tests pass

**step 2: delete vendor directory**

```bash
rm -rf tein/vendor/
```

**step 3: update .gitignore**

remove lines referencing the old vendor location:

```
# generated files (build.rs)
tein/vendor/chibi-scheme/include/chibi/install.h
tein/vendor/chibi-scheme/tein_vfs_data.h
tein/vendor/chibi-scheme/tein_clibs.c
```

add line for the fetched chibi cache:

```
# fetched chibi-scheme (build.rs clones from fork)
/target/chibi-scheme/
```

keep the existing `/vendor/` ignore for the workspace-root stray.

**step 4: clean build to verify nothing depends on vendor**

run: `cd /home/fey/projects/tein/tein-dev && cargo clean && cargo build 2>&1 | tail -10`
expected: clone + build succeeds

run: `cd /home/fey/projects/tein/tein-dev && cargo test 2>&1 | tail -20`
expected: all tests pass

**step 5: commit**

```bash
git add -A
git commit -m "build: remove vendored chibi-scheme, fetch from fork at build time"
```

---

### task 4: update documentation

**files:**
- modify: `AGENTS.md` (architecture section)
- modify: `ARCHITECTURE.md` (if it references vendor/)

**step 1: update AGENTS.md architecture section**

in the architecture tree, replace:

```
vendor/chibi-scheme/
```

with:

```
target/chibi-scheme/   — fetched from emesal/chibi-scheme (branch emesal-tein) by build.rs
```

add a note in the architecture section about the build flow:

```
**chibi-scheme source**: fetched at build time from https://github.com/emesal/chibi-scheme (branch `emesal-tein`) into `target/chibi-scheme/`. all chibi patches and tein-specific C/scheme files live in the fork. generated files (install.h, tein_vfs_data.h, tein_clibs.c) are written to `OUT_DIR`.
```

update references from `vendor/chibi-scheme/tein_shim.c` to just `tein_shim.c` (in the fork), etc.

**step 2: update ARCHITECTURE.md**

check for and update any references to `vendor/chibi-scheme/`.

**step 3: update the comment at top of build.rs**

```rust
// build script for compiling chibi-scheme from our fork
//
// fetches emesal/chibi-scheme (branch emesal-tein) into target/chibi-scheme/,
// then generates:
//   install.h       — chibi config with VFS module path (in OUT_DIR)
//   tein_vfs_data.h — embedded .sld/.scm files for the virtual filesystem (in OUT_DIR)
//   tein_clibs.c    — static C library table for native-backed modules (in OUT_DIR)
```

**step 4: commit**

```bash
git add AGENTS.md ARCHITECTURE.md tein/build.rs
git commit -m "docs: update architecture for fork-based chibi fetch"
```

---

## notes

- **offline builds**: this approach requires network access on first build (or after deleting `target/chibi-scheme/`). subsequent builds only need network if the branch has new commits. if offline and cache exists, the fetch will fail but the cached version remains usable — consider adding a fallback that skips fetch if the dir exists and fetch fails.
- **CI**: CI environments will need git access to github.com. shallow clone + single-branch keeps it fast.
- **`cargo clean`**: only removes `target/{debug,release,...}`, not `target/chibi-scheme/`. a full `rm -rf target/` would require re-clone.
