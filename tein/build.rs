// build script for compiling vendored chibi-scheme
//
// generates:
//   tein_vfs_data.h — embedded .sld/.scm files for the virtual filesystem
//   tein_clibs.c    — static C library table for native-backed modules
//   install.h       — chibi config with VFS module path

use std::fs;
use std::path::Path;

/// files embedded in the VFS for r7rs standard library support.
///
/// keys become `/vfs/lib/...` paths that chibi's module resolver finds.
/// order doesn't matter — the VFS is a flat lookup table.
const VFS_FILES: &[&str] = &[
    // bootstrap
    "lib/init-7.scm",
    "lib/meta-7.scm",
    // r7rs standard modules
    "lib/scheme/base.sld",
    "lib/scheme/write.sld",
    "lib/scheme/read.sld",
    "lib/scheme/lazy.sld",
    "lib/scheme/case-lambda.sld",
    "lib/scheme/cxr.sld",
    "lib/scheme/inexact.sld",
    "lib/scheme/complex.sld",
    "lib/scheme/char.sld",
    // scheme includes
    "lib/scheme/define-values.scm",
    "lib/scheme/extras.scm",
    "lib/scheme/misc-macros.scm",
    "lib/scheme/cxr.scm",
    "lib/scheme/inexact.scm",
    "lib/scheme/digit-value.scm",
    "lib/scheme/char/full.scm",
    "lib/scheme/char/special-casing.scm",
    "lib/scheme/char/case-offsets.scm",
    // chibi dependencies
    "lib/chibi/equiv.sld",
    "lib/chibi/equiv.scm",
    "lib/chibi/string.sld",
    "lib/chibi/string.scm",
    "lib/chibi/ast.sld",
    "lib/chibi/ast.scm",
    "lib/chibi/io.sld",
    "lib/chibi/io/io.scm",
    "lib/chibi/char-set/base.sld",
    "lib/chibi/char-set/full.sld",
    "lib/chibi/char-set/full.scm",
    "lib/chibi/iset/base.sld",
    "lib/chibi/iset/base.scm",
    // srfi dependencies
    "lib/srfi/9.sld",
    "lib/srfi/9.scm",
    "lib/srfi/11.sld",
    "lib/srfi/16.sld",
    "lib/srfi/38.sld",
    "lib/srfi/38.scm",
    "lib/srfi/39.sld",
    "lib/srfi/39/syntax.scm",
    "lib/srfi/39/syntax-no-threads.scm",
    "lib/srfi/69.sld",
    "lib/srfi/69/type.scm",
    "lib/srfi/69/interface.scm",
    "lib/srfi/151.sld",
    "lib/srfi/151/bitwise.scm",
];

/// C-backed modules that need static linking.
///
/// each entry: (path to .c file relative to chibi dir, init function suffix, table key).
/// the table key must match what `sexp_find_module_file_raw` constructs via the `/vfs/lib` path,
/// minus the `.so` extension (chibi's `sexp_find_static_library` strips `.so` before comparing).
const CLIB_ENTRIES: &[(&str, &str, &str)] = &[
    ("lib/chibi/ast.c", "chibi_ast", "/vfs/lib/chibi/ast"),
    ("lib/chibi/io/io.c", "chibi_io", "/vfs/lib/chibi/io/io"),
    (
        "lib/srfi/39/param.c",
        "srfi_39_param",
        "/vfs/lib/srfi/39/param",
    ),
    (
        "lib/srfi/69/hash.c",
        "srfi_69_hash",
        "/vfs/lib/srfi/69/hash",
    ),
    (
        "lib/srfi/151/bit.c",
        "srfi_151_bit",
        "/vfs/lib/srfi/151/bit",
    ),
];

fn main() {
    let chibi_dir = "vendor/chibi-scheme";
    let include_dir = format!("{chibi_dir}/include");

    // generate install.h (with VFS module path)
    generate_install_h(&include_dir);

    // generate VFS data header (embedded .sld/.scm files)
    generate_vfs_data(chibi_dir);

    // generate static C library table
    generate_clibs(chibi_dir);

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
        "tein_shim.c",  // our ffi shim layer
        "tein_clibs.c", // generated static library table
    ];

    let mut build = cc::Build::new();

    build
        .include(&include_dir)
        .include(chibi_dir)
        .flag("-DSEXP_USE_DL=0") // disable dynamic loading
        .flag("-DSEXP_STATIC_LIBRARY") // static link (prevents dllimport on win32)
        .flag("-DSEXP_USE_STATIC_LIBS=1") // enable static library lookup in eval.c
        .flag("-DSEXP_USE_STATIC_LIBS_NO_INCLUDE=1") // we define sexp_static_libraries ourselves
        .warnings(false); // chibi may have warnings

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
    ] {
        build.include(extra_include);
    }

    for src in &sources {
        build.file(format!("{chibi_dir}/{src}"));
    }

    build.compile("chibi");

    // rerun triggers
    for src in &sources {
        println!("cargo:rerun-if-changed={chibi_dir}/{src}");
    }
    for f in VFS_FILES {
        println!("cargo:rerun-if-changed={chibi_dir}/{f}");
    }
    for &(c_file, _, _) in CLIB_ENTRIES {
        println!("cargo:rerun-if-changed={chibi_dir}/{c_file}");
    }
    println!("cargo:rerun-if-changed={include_dir}/chibi/sexp.h");
    println!("cargo:rerun-if-changed={include_dir}/chibi/features.h");
    println!("cargo:rerun-if-changed=build.rs");
}

/// generate install.h with VFS module path sentinel
fn generate_install_h(include_dir: &str) {
    let install_h_path = Path::new(include_dir).join("chibi/install.h");

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
fn generate_vfs_data(chibi_dir: &str) {
    let out_path = Path::new(chibi_dir).join("tein_vfs_data.h");
    let mut out = String::with_capacity(1024 * 1024);

    out.push_str("// generated by build.rs — do not edit\n\n");

    // emit each file as a C string constant, chunked to stay within MSVC's
    // 16380-char string literal limit (C2026). adjacent string literals are
    // concatenated by the C preprocessor, so this is fully portable.
    const CHUNK_BYTES: usize = 1000; // each source byte → 4 chars (\xNN), so 4000 chars/chunk
    for (i, rel_path) in VFS_FILES.iter().enumerate() {
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
    for (i, rel_path) in VFS_FILES.iter().enumerate() {
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
fn generate_clibs(chibi_dir: &str) {
    let out_path = Path::new(chibi_dir).join("tein_clibs.c");
    let mut out = String::with_capacity(4096);

    out.push_str("// generated by build.rs — do not edit\n\n");
    out.push_str("#include <chibi/eval.h>\n\n");

    // include each C library with a unique init function name
    for &(c_file, suffix, _) in CLIB_ENTRIES {
        out.push_str(&format!(
            "#define sexp_init_library sexp_init_lib_{suffix}\n"
        ));
        out.push_str(&format!("#include \"{c_file}\"\n"));
        out.push_str("#undef sexp_init_library\n\n");
    }

    // the lookup table that chibi's eval.c searches via sexp_find_static_library.
    // init functions are already defined by the #include pattern above.
    out.push_str("\nstruct sexp_library_entry_t tein_static_libraries_array[] = {\n");
    for &(_, suffix, key) in CLIB_ENTRIES {
        out.push_str(&format!(
            "    {{ \"{key}\", (sexp_init_proc)sexp_init_lib_{suffix} }},\n"
        ));
    }
    out.push_str("    { NULL, NULL }\n");
    out.push_str("};\n\n");
    out.push_str(
        "struct sexp_library_entry_t *sexp_static_libraries = tein_static_libraries_array;\n",
    );

    fs::write(&out_path, &out).expect("failed to write tein_clibs.c");
}
