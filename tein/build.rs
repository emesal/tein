// build script for compiling chibi-scheme from our fork
//
// fetches emesal/chibi-scheme (branch emesal-tein) into target/chibi-scheme/,
// then generates:
//   install.h       — chibi config with VFS module path (in OUT_DIR/chibi/)
//   tein_vfs_data.h — embedded .sld/.scm files for the virtual filesystem (in OUT_DIR)
//   tein_clibs.c    — static C library table for native-backed modules (in OUT_DIR)

use std::fs;
use std::path::Path;
use std::process::Command;

include!("src/vfs_registry.rs");

const CHIBI_REPO: &str = "https://github.com/emesal/chibi-scheme.git";
const CHIBI_BRANCH: &str = "emesal-tein";

/// fetch or update the chibi-scheme fork into `target/chibi-scheme/`.
///
/// clones on first build, then fetches + resets to branch tip on subsequent builds.
/// uses `target/chibi-scheme/` (two levels up from `tein/`) so it survives `cargo clean`
/// (which only removes `target/{debug,release,...}`) and is shared across profiles.
fn fetch_chibi() -> String {
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
        // if the dir exists but isn't a git repo (e.g. leftover from a cancelled build),
        // remove it so git clone can proceed cleanly
        if chibi_dir.exists() {
            std::fs::remove_dir_all(&chibi_dir)
                .expect("failed to remove stale chibi-scheme directory");
        }

        // initial clone — shallow single-branch for speed
        let clone = Command::new("git")
            .args([
                "clone",
                "--branch",
                CHIBI_BRANCH,
                "--single-branch",
                "--depth",
                "1",
                CHIBI_REPO,
                chibi_dir.to_str().expect("non-utf8 path"),
            ])
            .status()
            .expect("failed to run git clone");
        assert!(clone.success(), "git clone failed");
    }

    chibi_dir.to_str().expect("non-utf8 path").to_string()
}

/// validate that each embedded `.sld` file's `(include ...)` directives reference
/// only files already present in that entry's `files` list.
///
/// panics with a clear message if a referenced file is missing from the registry entry.
/// this catches registry drift when upstream chibi-scheme adds or renames included files.
fn validate_sld_includes(chibi_dir: &str) {
    for entry in VFS_REGISTRY
        .iter()
        .filter(|e| e.source == VfsSource::Embedded)
    {
        // find the .sld file in this entry's files list
        let sld_rel = match entry.files.iter().find(|f| f.ends_with(".sld")) {
            Some(f) => *f,
            None => continue, // no .sld, nothing to validate
        };

        let sld_path = format!("{chibi_dir}/{sld_rel}");
        let source = match fs::read_to_string(&sld_path) {
            Ok(s) => s,
            Err(e) => panic!("failed to read {sld_path}: {e}"),
        };

        // derive the directory containing the .sld to resolve relative includes
        let sld_dir = Path::new(sld_rel)
            .parent()
            .unwrap_or(Path::new(""))
            .to_str()
            .expect("non-utf8 sld dir");

        // collect all (include "...") references in the .sld
        // this is a line-based scan: look for `"filename.scm"` after `include`
        let referenced_files = collect_include_files(&source, sld_dir);

        // referenced files come back as paths relative to the chibi lib/ dir
        // (e.g. "tein/foreign.scm" from sld_dir="lib/tein" + file="foreign.scm"
        //  becomes "lib/tein/foreign.scm" after collection).
        // compare directly against the entry's files list.
        for ref_file in &referenced_files {
            if !entry.files.contains(&ref_file.as_str()) {
                panic!(
                    "registry validation failed for '{}': \
                     .sld references '{}' but it is not listed in the entry's files array.\n\
                     add '{}' to the files list for '{}'.",
                    entry.path, ref_file, ref_file, entry.path
                );
            }
        }
    }
}

/// extract file paths from `(include ...)` and `(include-ci ...)` directives.
///
/// only handles the `.scm` include form (not `include-shared`, which embeds C `.so`).
/// resolves relative to `sld_dir` (the directory containing the `.sld` file).
fn collect_include_files(source: &str, sld_dir: &str) -> Vec<String> {
    use tein_sexp::{SexpKind, parser};

    let mut result = Vec::new();

    let sexps = match parser::parse_all(source) {
        Ok(s) => s,
        Err(_) => return result, // parse error — skip validation for this file
    };

    // recursively walk all sexps collecting include strings
    fn walk(sexp: &tein_sexp::Sexp, sld_dir: &str, out: &mut Vec<String>) {
        if let SexpKind::List(items) = &sexp.kind {
            // check if this is (include "...") or (include-ci "...")
            if let Some(first) = items.first()
                && let SexpKind::Symbol(name) = &first.kind
                && (name == "include" || name == "include-ci")
            {
                for arg in items.iter().skip(1) {
                    if let SexpKind::String(file) = &arg.kind {
                        // resolve relative to sld_dir
                        let resolved = if sld_dir.is_empty() {
                            file.clone()
                        } else {
                            format!("{sld_dir}/{file}")
                        };
                        out.push(resolved);
                    }
                }
                return; // don't recurse further into include args
            }
            // recurse into all list items
            for item in items {
                walk(item, sld_dir, out);
            }
        }
    }

    for sexp in &sexps {
        walk(sexp, sld_dir, &mut result);
    }

    result
}

/// check whether a cargo feature is enabled at build time (mirrors sandbox.rs)
fn feature_enabled(feature: Option<&str>) -> bool {
    match feature {
        None => true,
        Some("json") => cfg!(feature = "json"),
        Some("toml") => cfg!(feature = "toml"),
        Some("uuid") => cfg!(feature = "uuid"),
        Some("time") => cfg!(feature = "time"),
        Some(f) => {
            // unknown feature name — conservatively include
            eprintln!("cargo:warning=unknown feature gate in VFS_REGISTRY: {f}");
            true
        }
    }
}

/// bootstrap files embedded in the VFS but not in the registry (not importable modules)
const BOOTSTRAP_FILES: &[&str] = &["lib/init-7.scm", "lib/meta-7.scm"];

/// hardcoded exports for dynamic modules (registered via `#[tein_module]`, no .sld to parse)
const DYNAMIC_MODULE_EXPORTS: &[(&str, &[&str])] = &[
    ("tein/uuid", &["make-uuid", "uuid?", "uuid-nil"]),
    (
        "tein/time",
        &["current-second", "current-jiffy", "jiffies-per-second"],
    ),
];

/// extract exported binding names from each module's `.sld` file.
///
/// for embedded modules: parses the `.sld` to find `(export ...)` forms.
/// for dynamic modules: uses the hardcoded [`DYNAMIC_MODULE_EXPORTS`] table.
///
/// `(rename old new)` export specs yield the external name `new`.
/// returns a vec of `(module_path, exports)` pairs (stable order: registry order).
fn extract_exports(chibi_dir: &str) -> Vec<(&'static str, Vec<String>)> {
    use tein_sexp::parser;

    let mut result = Vec::new();

    for entry in VFS_REGISTRY.iter() {
        if !feature_enabled(entry.feature) {
            continue;
        }

        match entry.source {
            VfsSource::Dynamic => {
                // look up hardcoded exports for this dynamic module
                if let Some((_, exports)) = DYNAMIC_MODULE_EXPORTS
                    .iter()
                    .find(|(path, _)| *path == entry.path)
                {
                    result.push((entry.path, exports.iter().map(|s| s.to_string()).collect()));
                }
                // dynamic modules with no hardcoded exports are silently skipped
            }
            VfsSource::Embedded => {
                // find the .sld file in the entry's files list
                let sld_rel = match entry.files.iter().find(|f| f.ends_with(".sld")) {
                    Some(f) => *f,
                    None => continue,
                };

                let sld_path = format!("{chibi_dir}/{sld_rel}");
                let source = match std::fs::read_to_string(&sld_path) {
                    Ok(s) => s,
                    Err(e) => panic!("extract_exports: failed to read {sld_path}: {e}"),
                };

                let sexps = match parser::parse_all(&source) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("cargo:warning=extract_exports: parse error in {sld_path}: {e}");
                        continue;
                    }
                };

                // collect all exports from (export ...) forms at the top level
                // (export ...) may appear inside (define-library ...)
                let exports = collect_exports_from_sexps(&sexps);
                result.push((entry.path, exports));
            }
        }
    }

    result
}

/// recursively collect export names from `(export ...)` forms in a list of sexps.
///
/// handles `(rename old new)` export specs by taking `new`.
/// recurses into `(define-library ...)` and other list forms to find nested exports.
fn collect_exports_from_sexps(sexps: &[tein_sexp::Sexp]) -> Vec<String> {
    use tein_sexp::SexpKind;

    let mut exports = Vec::new();

    fn walk(sexp: &tein_sexp::Sexp, out: &mut Vec<String>) {
        let items = match &sexp.kind {
            SexpKind::List(items) => items,
            _ => return,
        };

        let is_export = matches!(
            items.first().map(|s| &s.kind),
            Some(SexpKind::Symbol(name)) if name == "export"
        );

        if is_export {
            // collect each export spec
            for spec in items.iter().skip(1) {
                match &spec.kind {
                    SexpKind::Symbol(name) => {
                        out.push(name.clone());
                    }
                    SexpKind::List(rename_items) => {
                        // (rename old new) — take `new` (index 2)
                        let is_rename = matches!(
                            rename_items.first().map(|s| &s.kind),
                            Some(SexpKind::Symbol(n)) if n == "rename"
                        );
                        if is_rename
                            && let Some(SexpKind::Symbol(new_name)) =
                                rename_items.get(2).map(|s| &s.kind)
                        {
                            out.push(new_name.clone());
                        }
                    }
                    _ => {}
                }
            }
        } else {
            // recurse into all list children (handles define-library, cond-expand, etc.)
            for item in items {
                walk(item, out);
            }
        }
    }

    for sexp in sexps {
        walk(sexp, &mut exports);
    }

    exports
}

/// generate `tein_exports.rs` in `OUT_DIR` — module path → exported binding names.
///
/// emits a `const MODULE_EXPORTS: &[(&str, &[&str])]` for use in sandbox.rs.
fn generate_exports_rs(out_dir: &str, exports: &[(&'static str, Vec<String>)]) {
    let out_path = std::path::Path::new(out_dir).join("tein_exports.rs");
    let mut out = String::with_capacity(64 * 1024);

    out.push_str("// generated by build.rs — do not edit\n\n");
    out.push_str("#[allow(dead_code)] // used in context.rs starting task 7\n");
    out.push_str("/// auto-generated module path → exported binding names\n");
    out.push_str("const MODULE_EXPORTS: &[(&str, &[&str])] = &[\n");

    for (path, syms) in exports {
        out.push_str("    (\"");
        out.push_str(path);
        out.push_str("\", &[");
        for (i, sym) in syms.iter().enumerate() {
            if i > 0 {
                out.push_str(", ");
            }
            out.push('"');
            out.push_str(sym);
            out.push('"');
        }
        out.push_str("]),\n");
    }

    out.push_str("];\n");
    std::fs::write(&out_path, &out).expect("failed to write tein_exports.rs");
}

fn main() {
    let chibi_dir = fetch_chibi();
    let include_dir = format!("{chibi_dir}/include");
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR not set");

    // build the combined VFS file list from the registry (replaces VFS_FILES + feature gates)
    let mut vfs_files: Vec<&str> = BOOTSTRAP_FILES.to_vec();
    vfs_files.extend(
        VFS_REGISTRY
            .iter()
            .filter(|e| e.source == VfsSource::Embedded && feature_enabled(e.feature))
            .flat_map(|e| e.files.iter().copied()),
    );

    // validate that .sld files reference only files present in their entry's files list
    validate_sld_includes(&chibi_dir);

    // generate install.h (with VFS module path) into OUT_DIR/chibi/
    generate_install_h(&out_dir);

    // generate VFS data header (embedded .sld/.scm files) into OUT_DIR
    generate_vfs_data(&chibi_dir, &out_dir, &vfs_files);

    // generate static C library table into OUT_DIR
    generate_clibs(&chibi_dir, &out_dir);

    // extract module exports and generate tein_exports.rs
    let exports = extract_exports(&chibi_dir);
    generate_exports_rs(&out_dir, &exports);

    // core chibi-scheme source files (excluding main.c which has main())
    let sources = [
        "sexp.c",
        "bignum.c",
        "gc.c",
        "gc_heap.c",
        "opcodes.c",
        "vm.c",
        "eval.c",
        "simplify.c",
        "tein_shim.c", // our ffi shim layer
    ];

    let mut build = cc::Build::new();

    build
        .include(&out_dir) // generated install.h (chibi/install.h) wins over repo's
        .include(&include_dir) // repo headers (sexp.h, features.h, etc.)
        .include(&chibi_dir)
        // SAFETY-CRITICAL: SEXP_USE_DL=0 disables dynamic loading, which:
        // 1. eliminates the dlopen attack surface
        // 2. prevents scheme code from registering types with C-level finalisers
        //    — this is the ONLY mitigation for chibi GC finaliser bugs (M19-M21 in
        //    chibi-scheme-review.md): resurrection → use-after-free, re-entrant GC
        //    from allocating finalisers, and half-collected referenced objects.
        //    also mitigates NULL-self finaliser call (M11).
        // 3. disables SEXP_USE_IMAGE_LOADING (derived: DL && 64-bit && ...), which
        //    mitigates image loading buffer overflows (M23-M24) and image version
        //    check bug (M9).
        // if this flag is ever changed, all of the above bugs become exploitable.
        // additionally, SEXP_USE_LIMITED_MALLOC (default 0) must stay disabled —
        // it has an unsynchronised global counter that races under concurrency (M10).
        .flag("-DSEXP_USE_DL=0")
        .flag("-DSEXP_STATIC_LIBRARY") // static link (prevents dllimport on win32)
        .flag("-DSEXP_USE_STATIC_LIBS=1") // enable static library lookup in eval.c
        .flag("-DSEXP_USE_STATIC_LIBS_NO_INCLUDE=1") // we define sexp_static_libraries ourselves
        .warnings(false); // chibi may have warnings

    // debug-chibi feature: GC instrumentation for diagnosing heap corruption.
    // HEADER_MAGIC adds a 4-byte sentinel to every sexp — caught on GC traversal.
    // SAFE_GC_MARK validates pointer bounds before marking — catches wild pointers.
    #[cfg(feature = "debug-chibi")]
    {
        build.flag("-DSEXP_USE_HEADER_MAGIC=1");
        build.flag("-DSEXP_USE_SAFE_GC_MARK=1");
    }

    // include paths for C files referenced by the static library table.
    // ast.c uses `#include <chibi/eval.h>` (covered by include_dir).
    // io/io.c includes port.c via relative path.
    // these extra -I paths ensure nested includes resolve correctly.
    for extra_include in &[
        format!("{chibi_dir}/lib/chibi/io"),
        format!("{chibi_dir}/lib/chibi"),
        format!("{chibi_dir}/lib/srfi/39"),
        format!("{chibi_dir}/lib/srfi/69"),
        format!("{chibi_dir}/lib/srfi/151"),
        format!("{chibi_dir}/lib/tein"),
    ] {
        build.include(extra_include);
    }

    for src in &sources {
        build.file(format!("{chibi_dir}/{src}"));
    }
    // generated tein_clibs.c lives in OUT_DIR
    build.file(format!("{out_dir}/tein_clibs.c"));

    build.compile("chibi");

    // rerun triggers
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={include_dir}/chibi/sexp.h");
    println!("cargo:rerun-if-changed={include_dir}/chibi/features.h");
    for src in &sources {
        println!("cargo:rerun-if-changed={chibi_dir}/{src}");
    }
    for f in &vfs_files {
        println!("cargo:rerun-if-changed={chibi_dir}/{f}");
    }
    for entry in VFS_REGISTRY.iter().filter_map(|e| e.clib.as_ref()) {
        println!("cargo:rerun-if-changed={chibi_dir}/{}", entry.source);
    }
}

/// generate install.h with VFS module path sentinel.
///
/// written into `OUT_DIR/chibi/install.h` so `#include <chibi/install.h>` resolves
/// to our version (OUT_DIR is searched before the repo's include/ dir).
fn generate_install_h(out_dir: &str) {
    let chibi_out = Path::new(out_dir).join("chibi");
    fs::create_dir_all(&chibi_out).expect("failed to create chibi/ in OUT_DIR");
    let install_h_path = chibi_out.join("install.h");

    // "/vfs/lib" as the module path — chibi appends "/" + filename to construct
    // paths like "/vfs/lib/init-7.scm", "/vfs/lib/scheme/base.sld" etc.
    // note: can't use "vfs://..." because colon is a path list separator in chibi
    let content = r#"#define sexp_so_extension ".so"
#define sexp_default_module_path "/vfs/lib"
#define sexp_platform "unknown"
#define sexp_architecture "unknown"
#define sexp_version "0.11"
#define sexp_release_name "tein-embedded"
"#;

    fs::write(install_h_path, content).expect("failed to write install.h");
}

/// generate tein_vfs_data.h — embedded scheme files as C string constants
///
/// produces a lookup table mapping `"vfs://lib/..."` keys to file contents.
/// all bytes are escaped as `\xNN` for safe C embedding (no encoding issues).
fn generate_vfs_data(chibi_dir: &str, out_dir: &str, vfs_files: &[&str]) {
    let out_path = Path::new(out_dir).join("tein_vfs_data.h");
    let mut out = String::with_capacity(1024 * 1024);

    out.push_str("// generated by build.rs — do not edit\n\n");

    // emit each file as a C string constant, chunked to stay within MSVC's
    // 16380-char string literal limit (C2026). adjacent string literals are
    // concatenated by the C preprocessor, so this is fully portable.
    const CHUNK_BYTES: usize = 1000; // each source byte → 4 chars (\xNN), so 4000 chars/chunk
    for (i, rel_path) in vfs_files.iter().enumerate() {
        let full_path = format!("{chibi_dir}/{rel_path}");
        let content = fs::read(&full_path)
            .unwrap_or_else(|e| panic!("failed to read VFS file {full_path}: {e}"));

        out.push_str(&format!("static const char tein_vfs_content_{i}[] =\n"));
        for chunk in content.chunks(CHUNK_BYTES) {
            out.push('"');
            for &byte in chunk {
                out.push_str(&format!("\\x{byte:02x}"));
            }
            out.push_str("\"\n");
        }
        out.push_str(";\n");
    }

    // emit the lookup table
    out.push_str("\nstruct tein_vfs_entry {\n");
    out.push_str("    const char *key;\n");
    out.push_str("    const char *content;\n");
    out.push_str("    unsigned int length;\n");
    out.push_str("};\n\n");

    out.push_str("static const struct tein_vfs_entry tein_vfs_table[] = {\n");
    for (i, rel_path) in vfs_files.iter().enumerate() {
        let full_path = format!("{chibi_dir}/{rel_path}");
        let len = fs::metadata(&full_path)
            .unwrap_or_else(|e| panic!("failed to stat VFS file {full_path}: {e}"))
            .len();
        out.push_str(&format!(
            "    {{ \"/vfs/{rel_path}\", tein_vfs_content_{i}, {len}u }},\n"
        ));
    }
    out.push_str("    { NULL, NULL, 0 }\n");
    out.push_str("};\n");

    fs::write(&out_path, &out).expect("failed to write tein_vfs_data.h");
}

/// generate tein_clibs.c — static C library table for native-backed modules
///
/// uses the `#define sexp_init_library / #include / #undef` pattern to give
/// each C library a unique init function name, then builds the lookup table
/// that chibi's `sexp_find_static_library` searches.
fn generate_clibs(chibi_dir: &str, out_dir: &str) {
    let out_path = Path::new(out_dir).join("tein_clibs.c");
    let mut out = String::with_capacity(4096);

    let clib_entries: Vec<&ClibEntry> = VFS_REGISTRY
        .iter()
        .filter_map(|e| e.clib.as_ref())
        .collect();

    out.push_str("// generated by build.rs — do not edit\n\n");
    out.push_str("#include <chibi/eval.h>\n\n");

    // include each C library with a unique init function name
    for entry in &clib_entries {
        out.push_str(&format!(
            "#define sexp_init_library sexp_init_lib_{}\n",
            entry.init_suffix
        ));
        out.push_str(&format!("#include \"{chibi_dir}/{}\"\n", entry.source));
        out.push_str("#undef sexp_init_library\n\n");
    }

    // the lookup table that chibi's eval.c searches via sexp_find_static_library.
    // init functions are already defined by the #include pattern above.
    out.push_str("\nstruct sexp_library_entry_t tein_static_libraries_array[] = {\n");
    for entry in &clib_entries {
        out.push_str(&format!(
            "    {{ \"{}\", (sexp_init_proc)sexp_init_lib_{} }},\n",
            entry.vfs_key, entry.init_suffix
        ));
    }
    out.push_str("    { NULL, NULL }\n");
    out.push_str("};\n\n");
    out.push_str(
        "struct sexp_library_entry_t *sexp_static_libraries = tein_static_libraries_array;\n",
    );

    fs::write(&out_path, &out).expect("failed to write tein_clibs.c");
}
