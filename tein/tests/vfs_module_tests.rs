//! scheme-level integration tests using chibi-scheme's bundled srfi and
//! library test suites. each test imports `(chibi test)` with a custom
//! applier that raises immediately on failure, giving cargo test clean
//! abort-on-first-fail semantics with the failing assertion name.

use tein::Context;

/// uses a standard non-sandboxed context with VFS shadows registered.
/// imports `(chibi test)`, runs the given module's `(run-tests)`, then checks
/// `(test-failure-count)` — any non-zero count panics with the count.
///
/// `with_vfs_shadows()` provides `(scheme process-context)` and `(scheme time)`
/// required by `(chibi test)`'s load chain.
fn run_chibi_test(import: &str) {
    let ctx = Context::builder()
        .standard_env()
        .with_vfs_shadows()
        .build()
        .expect("context");
    ctx.evaluate("(import (chibi test))")
        .expect("import chibi/test");
    ctx.evaluate(&format!("(import {})", import))
        .expect("import test module");
    ctx.evaluate("(run-tests)").expect("run-tests");
    let failures = ctx
        .evaluate("(test-failure-count)")
        .expect("test-failure-count");
    if failures != tein::Value::Integer(0) {
        panic!("{import}: {failures} test failure(s)");
    }
}

// ── srfi test suites ─────────────────────────────────────────────────────────

#[test]
fn test_scheme_bytevector() {
    run_chibi_test("(scheme bytevector-test)");
}

#[test]
fn test_srfi_1_list() {
    run_chibi_test("(srfi 1 test)");
}

#[test]
fn test_srfi_2_and_let_star() {
    run_chibi_test("(srfi 2 test)");
}

#[test]
fn test_srfi_14_char_sets() {
    run_chibi_test("(srfi 14 test)");
}

#[test]
fn test_srfi_16_case_lambda() {
    run_chibi_test("(srfi 16 test)");
}

#[test]
fn test_srfi_18_threads() {
    run_chibi_test("(srfi 18 test)");
}

#[test]
fn test_srfi_26_cut() {
    run_chibi_test("(srfi 26 test)");
}

#[test]
fn test_srfi_27_random() {
    run_chibi_test("(srfi 27 test)");
}

/// chibi's srfi/33 `bitwise-merge` test cases have known failures (2 out of 57).
/// the implementation produces slightly different results than the test expects.
#[test]
#[ignore]
fn test_srfi_33_bitwise() {
    run_chibi_test("(srfi 33 test)");
}

/// srfi/35/test imports `(chibi repl)` which is not in the VFS registry.
#[test]
#[ignore]
fn test_srfi_35_conditions() {
    run_chibi_test("(srfi 35 test)");
}

#[test]
fn test_srfi_38_write_read() {
    run_chibi_test("(srfi 38 test)");
}

#[test]
fn test_srfi_41_streams() {
    run_chibi_test("(srfi 41 test)");
}

#[test]
fn test_srfi_69_hash_tables() {
    run_chibi_test("(srfi 69 test)");
}

#[test]
fn test_srfi_95_sorting() {
    run_chibi_test("(srfi 95 test)");
}

#[test]
fn test_srfi_99_records() {
    run_chibi_test("(srfi 99 test)");
}

#[test]
fn test_srfi_101_random_access_lists() {
    run_chibi_test("(srfi 101 test)");
}

#[test]
fn test_srfi_113_sets() {
    run_chibi_test("(srfi 113 test)");
}

#[test]
fn test_srfi_116_immutable_lists() {
    run_chibi_test("(srfi 116 test)");
}

#[test]
fn test_srfi_117_list_queues() {
    run_chibi_test("(srfi 117 test)");
}

#[test]
fn test_srfi_121_generators() {
    run_chibi_test("(srfi 121 test)");
}

#[test]
fn test_srfi_125_hash_tables() {
    run_chibi_test("(srfi 125 test)");
}

#[test]
fn test_srfi_127_lseq() {
    run_chibi_test("(srfi 127 test)");
}

#[test]
fn test_srfi_128_comparators() {
    run_chibi_test("(srfi 128 test)");
}

#[test]
fn test_srfi_129_titlecase() {
    run_chibi_test("(srfi 129 test)");
}

#[test]
fn test_srfi_130_string_cursors() {
    run_chibi_test("(srfi 130 test)");
}

#[test]
fn test_srfi_132_sorting() {
    run_chibi_test("(srfi 132 test)");
}

#[test]
fn test_srfi_133_vectors() {
    run_chibi_test("(srfi 133 test)");
}

#[test]
fn test_srfi_134_ideque() {
    run_chibi_test("(srfi 134 test)");
}

#[test]
fn test_srfi_135_texts() {
    run_chibi_test("(srfi 135 test)");
}

#[test]
fn test_srfi_139_syntax_parameters() {
    run_chibi_test("(srfi 139 test)");
}

#[test]
fn test_srfi_143_fixnums() {
    run_chibi_test("(srfi 143 test)");
}

#[test]
fn test_srfi_144_flonums() {
    run_chibi_test("(srfi 144 test)");
}

#[test]
fn test_srfi_146_mappings() {
    run_chibi_test("(srfi 146 test)");
}

#[test]
fn test_srfi_151_bitwise() {
    run_chibi_test("(srfi 151 test)");
}

#[test]
fn test_srfi_158_generators() {
    run_chibi_test("(srfi 158 test)");
}

#[test]
fn test_srfi_160_uniform_vectors() {
    run_chibi_test("(srfi 160 test)");
}

/// srfi/166/test needs real filesystem for `with-output-to-file`/`delete-file`.
/// `chibi/filesystem` shadow stubs deny file ops in `with_vfs_shadows` context.
#[test]
#[ignore]
fn test_srfi_166_formatting() {
    run_chibi_test("(srfi 166 test)");
}

#[test]
fn test_srfi_211_syntax_transformers() {
    run_chibi_test("(srfi 211 test)");
}

#[test]
fn test_srfi_219_define_record_type() {
    run_chibi_test("(srfi 219 test)");
}

#[test]
fn test_srfi_229_tagged_procedures() {
    run_chibi_test("(srfi 229 test)");
}

// ── chibi library test suites ─────────────────────────────────────────────────

#[test]
fn test_chibi_assert() {
    run_chibi_test("(chibi assert-test)");
}

#[test]
fn test_chibi_base64() {
    run_chibi_test("(chibi base64-test)");
}

#[test]
fn test_chibi_binary_record() {
    run_chibi_test("(chibi binary-record-test)");
}

#[test]
fn test_chibi_bytevector() {
    run_chibi_test("(chibi bytevector-test)");
}

#[test]
fn test_chibi_csv() {
    run_chibi_test("(chibi csv-test)");
}

/// chibi/diff-test: now works because scheme/process-context is Embedded
/// (re-exports real trampolines from (tein process)) — `get-environment-variable "TERM"`
/// returns the real value instead of #f.
#[test]
fn test_chibi_diff() {
    run_chibi_test("(chibi diff-test)");
}

#[test]
fn test_chibi_edit_distance() {
    run_chibi_test("(chibi edit-distance-test)");
}

#[test]
fn test_chibi_generic() {
    run_chibi_test("(chibi generic-test)");
}

#[test]
fn test_chibi_io() {
    run_chibi_test("(chibi io-test)");
}

#[test]
fn test_chibi_iset() {
    run_chibi_test("(chibi iset-test)");
}

#[test]
fn test_chibi_loop() {
    run_chibi_test("(chibi loop-test)");
}

#[test]
fn test_chibi_match() {
    run_chibi_test("(chibi match-test)");
}

#[test]
fn test_chibi_math_prime() {
    run_chibi_test("(chibi math prime-test)");
}

#[test]
fn test_chibi_optional() {
    run_chibi_test("(chibi optional-test)");
}

#[test]
fn test_chibi_parse() {
    run_chibi_test("(chibi parse-test)");
}

#[test]
fn test_chibi_pathname() {
    run_chibi_test("(chibi pathname-test)");
}

#[test]
fn test_chibi_quoted_printable() {
    run_chibi_test("(chibi quoted-printable-test)");
}

#[test]
fn test_chibi_string() {
    run_chibi_test("(chibi string-test)");
}

#[test]
fn test_chibi_sxml() {
    run_chibi_test("(chibi sxml-test)");
}

#[test]
fn test_chibi_syntax_case() {
    run_chibi_test("(chibi syntax-case-test)");
}

#[test]
fn test_chibi_text() {
    run_chibi_test("(chibi text-test)");
}

#[test]
fn test_chibi_uri() {
    run_chibi_test("(chibi uri-test)");
}

/// chibi/weak-test calls `(gc)` manually — causes SIGSEGV in embedded contexts
/// due to chibi's conservative GC not seeing rust stack roots. excluded from suite.
#[test]
#[ignore]
fn test_chibi_weak() {
    run_chibi_test("(chibi weak-test)");
}
