// build script for compiling vendored chibi-scheme

use std::fs;
use std::path::Path;

fn main() {
    let chibi_dir = "vendor/chibi-scheme";
    let include_dir = format!("{}/include", chibi_dir);

    // generate install.h from template
    generate_install_h(&include_dir);

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
        .include(&include_dir)
        .include(chibi_dir)
        .flag("-DSEXP_USE_DL=0") // disable dynamic loading
        .warnings(false); // chibi may have some warnings

    // add all source files
    for src in &sources {
        build.file(format!("{}/{}", chibi_dir, src));
    }

    build.compile("chibi");

    // tell cargo to rerun if chibi sources change
    println!("cargo:rerun-if-changed={}", chibi_dir);
    println!("cargo:rerun-if-changed={}", include_dir);
}

fn generate_install_h(include_dir: &str) {
    let install_h_path = Path::new(include_dir).join("chibi/install.h");

    // simple install.h with sensible defaults for embedded use
    let content = r#"#define sexp_so_extension ".so"
#define sexp_default_module_path ""
#define sexp_platform "unknown"
#define sexp_architecture "unknown"
#define sexp_version "0.11"
#define sexp_release_name "tein-embedded"
"#;

    fs::write(install_h_path, content).expect("failed to write install.h");
}
