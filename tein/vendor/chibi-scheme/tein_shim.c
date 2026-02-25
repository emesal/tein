// tein shim layer - exports chibi macros as actual functions for ffi

#include <assert.h>
#include "include/chibi/sexp.h"
#include "include/chibi/eval.h"

// type checks
int tein_sexp_integerp(sexp x) { return sexp_integerp(x); }
int tein_sexp_flonump(sexp x) { return sexp_flonump(x); }
int tein_sexp_stringp(sexp x) { return sexp_stringp(x); }
int tein_sexp_symbolp(sexp x) { return sexp_symbolp(x); }
int tein_sexp_booleanp(sexp x) { return sexp_booleanp(x); }
int tein_sexp_nullp(sexp x) { return sexp_nullp(x); }
int tein_sexp_exceptionp(sexp x) { return sexp_exceptionp(x); }
int tein_sexp_pairp(sexp x) { return sexp_pairp(x); }

// value extraction
sexp_sint_t tein_sexp_unbox_fixnum(sexp x) { return sexp_unbox_fixnum(x); }
double tein_sexp_flonum_value(sexp x) { return (double)sexp_flonum_value(x); }
const char* tein_sexp_string_data(sexp x) { return sexp_string_data(x); }
sexp_uint_t tein_sexp_string_size(sexp x) { return sexp_string_size(x); }
sexp tein_sexp_symbol_to_string(sexp ctx, sexp sym) { return sexp_symbol_to_string(ctx, sym); }

// context
sexp tein_sexp_context_env(sexp ctx) { return sexp_context_env(ctx); }

// pair operations
sexp tein_sexp_car(sexp x) { return sexp_car(x); }
sexp tein_sexp_cdr(sexp x) { return sexp_cdr(x); }

// character operations
int tein_sexp_charp(sexp x) { return sexp_charp(x); }
int tein_sexp_unbox_character(sexp x) { return sexp_unbox_character(x); }
sexp tein_sexp_make_character(int n) { return sexp_make_character(n); }

// bytevector operations
int tein_sexp_bytesp(sexp x) { return sexp_bytesp(x); }
char* tein_sexp_bytes_data(sexp x) { return sexp_bytes_data(x); }
sexp_uint_t tein_sexp_bytes_length(sexp x) { return sexp_bytes_length(x); }
sexp tein_sexp_make_bytes(sexp ctx, sexp_uint_t len, unsigned char init) {
    return sexp_make_bytes(ctx, sexp_make_fixnum(len), sexp_make_fixnum(init));
}

// vector operations
int tein_sexp_vectorp(sexp x) { return sexp_vectorp(x); }
sexp_uint_t tein_sexp_vector_length(sexp x) { return sexp_vector_length(x); }
sexp* tein_sexp_vector_data(sexp x) { return sexp_vector_data(x); }

// port operations
int tein_sexp_portp(sexp x) { return sexp_portp(x); }
int tein_sexp_iportp(sexp x) { return sexp_iportp(x); }
int tein_sexp_oportp(sexp x) { return sexp_oportp(x); }

// exception details
sexp tein_sexp_exception_message(sexp x) { return sexp_exception_message(x); }
sexp tein_sexp_exception_irritants(sexp x) { return sexp_exception_irritants(x); }

// value construction (for rust→scheme)
sexp tein_sexp_make_fixnum(sexp_sint_t n) { return sexp_make_fixnum(n); }
sexp tein_sexp_make_flonum(sexp ctx, double f) { return sexp_make_flonum(ctx, f); }
sexp tein_sexp_make_boolean(int b) { return b ? SEXP_TRUE : SEXP_FALSE; }
sexp tein_get_void() { return SEXP_VOID; }

// foreign function registration
sexp tein_sexp_define_foreign(sexp ctx, sexp env, const char* name,
                              int num_args, const char* fname, sexp_proc1 f) {
    return sexp_define_foreign_aux(ctx, env, name, num_args, 0, fname, f, NULL);
}

// foreign function registration (procedure-wrapped, supports variadic)
sexp tein_sexp_define_foreign_proc(sexp ctx, sexp env, const char* name,
                                   int num_args, int flags,
                                   const char* fname, sexp_proc1 f) {
    return sexp_define_foreign_proc_aux(ctx, env, name, num_args, flags, fname, f, NULL);
}

// interning symbols
sexp tein_sexp_intern(sexp ctx, const char* str, sexp_sint_t len) {
    return sexp_intern(ctx, str, len);
}

// constants - export as actual values
sexp tein_get_true() { return SEXP_TRUE; }
sexp tein_get_false() { return SEXP_FALSE; }
sexp tein_get_null() { return SEXP_NULL; }

// pair/list construction
sexp tein_sexp_cons(sexp ctx, sexp head, sexp tail) { return sexp_cons(ctx, head, tail); }

// vector construction
sexp tein_sexp_make_vector(sexp ctx, sexp_uint_t len, sexp dflt) {
    return sexp_make_vector(ctx, sexp_make_fixnum(len), dflt);
}

// vector element setting with bounds assertion
void tein_sexp_vector_set(sexp vec, sexp_uint_t i, sexp val) {
    /* bounds assertion: caller is trusted (Rust), but an incorrect index
     * would be a heap write OOB. assert catches this in debug builds. */
    assert(i < (sexp_uint_t)sexp_vector_length(vec));
    sexp_vector_data(vec)[i] = val;
}

// procedure/application support
int tein_sexp_procedurep(sexp x) { return sexp_procedurep(x); }
int tein_sexp_opcodep(sexp x) { return sexp_opcodep(x); }
int tein_sexp_applicablep(sexp x) { return sexp_applicablep(x); }

/* extract the name (scheme string) from an opcode/foreign-fn object */
sexp tein_sexp_opcode_name(sexp op) { return sexp_opcode_name(op); }

// multi-expression evaluation support
sexp tein_get_eof() { return SEXP_EOF; }
int tein_sexp_eofp(sexp x) { return x == SEXP_EOF; }
sexp tein_sexp_open_input_string(sexp ctx, sexp str) {
    return sexp_open_input_string(ctx, str);
}
sexp tein_sexp_read(sexp ctx, sexp port) { return sexp_read(ctx, port); }
sexp tein_sexp_evaluate(sexp ctx, sexp obj, sexp env) { return sexp_eval(ctx, obj, env); }

// gc preservation for rust-side references
void tein_sexp_preserve_object(sexp ctx, sexp x) { sexp_preserve_object(ctx, x); }
void tein_sexp_release_object(sexp ctx, sexp x) { sexp_release_object(ctx, x); }

// fuel control (step limiting)
//
// chibi's vm creates child contexts for each eval, so we can't set
// fuel on a single context. instead we use thread-local counters that
// the vm checks via a minimal patch in vm.c.
//
// the vm runs opcodes in small timeslices (default 500 ops). our
// patch in vm.c calls tein_fuel_consume_slice at each timeslice
// boundary, subtracting from the thread-local budget. when the
// budget is exhausted, the vm stops.
//
// two vm paths exist: with green threads (unix), the fuel check
// piggybacks on the existing scheduler timeslice loop. without
// green threads (windows), a standalone decrement loop provides
// the same fuel semantics.

// MSVC uses __declspec(thread), gcc/clang use __thread
#ifdef _MSC_VER
#define TEIN_THREAD_LOCAL __declspec(thread)
#else
#define TEIN_THREAD_LOCAL __thread
#endif

TEIN_THREAD_LOCAL sexp_sint_t tein_fuel_budget = -1;   // -1 = unlimited
TEIN_THREAD_LOCAL int tein_fuel_exhausted_flag = 0;

void tein_fuel_arm(sexp ctx, sexp_sint_t total_fuel) {
    (void)ctx;
    tein_fuel_budget = total_fuel;
    tein_fuel_exhausted_flag = 0;
}

void tein_fuel_disarm(sexp ctx) {
    (void)ctx;
    tein_fuel_budget = -1;
    tein_fuel_exhausted_flag = 0;
}

int tein_fuel_exhausted(sexp ctx) {
    (void)ctx;
    return tein_fuel_exhausted_flag;
}

// called from vm.c at the timeslice boundary (when local fuel hits 0).
// subtracts the consumed slice from the thread-local budget and returns
// the next timeslice size, or 0 to stop the vm.
sexp_sint_t tein_fuel_consume_slice(sexp_sint_t slice_used) {
    if (tein_fuel_budget < 0) {
        // unlimited — return default quantum
        return SEXP_DEFAULT_QUANTUM;
    }
    tein_fuel_budget -= slice_used;
    if (tein_fuel_budget <= 0) {
        tein_fuel_exhausted_flag = 1;
        return 0;
    }
    return (tein_fuel_budget < SEXP_DEFAULT_QUANTUM)
        ? tein_fuel_budget : SEXP_DEFAULT_QUANTUM;
}

// error construction (for policy violation exceptions).
// len is unused; sexp_user_exception copies msg as a nul-terminated c string.
sexp tein_make_error(sexp ctx, const char* msg, sexp_sint_t len) {
    (void)len;
    return sexp_user_exception(ctx, SEXP_FALSE, msg, SEXP_NULL);
}

// --- module import policy ---
//
// controls which modules can be loaded via sexp_find_module_file_raw.
// 0 = unrestricted (all modules allowed), 1 = vfs-only (only /vfs/lib/ paths).
// set from rust before loading the standard env in sandboxed contexts.

TEIN_THREAD_LOCAL int tein_module_policy = 0;

// check if a module path is allowed under the current policy.
// called from eval.c patch A (sexp_find_module_file_raw).
int tein_module_allowed(const char *path) {
    if (tein_module_policy == 0) return 1;
    return strncmp(path, "/vfs/lib/", 9) == 0;
}

// set the module policy. called from rust ffi.
void tein_module_policy_set(int policy) {
    tein_module_policy = policy;
}

// environment manipulation (sandboxing)
sexp tein_sexp_make_null_env(sexp ctx, sexp version) { return sexp_make_null_env(ctx, version); }
sexp tein_sexp_make_primitive_env(sexp ctx, sexp version) { return sexp_make_primitive_env(ctx, version); }
sexp tein_sexp_env_define(sexp ctx, sexp env, sexp sym, sexp val) {
    sexp_env_define(ctx, env, sym, val);
    return SEXP_VOID;
}
sexp tein_sexp_env_ref(sexp ctx, sexp env, sexp sym, sexp dflt) {
    return sexp_env_ref(ctx, env, sym, dflt);
}
void tein_sexp_context_env_set(sexp ctx, sexp env) { sexp_context_env(ctx) = env; }

// --- virtual filesystem (VFS) for embedded scheme files ---
//
// the VFS allows sexp_load_standard_env to find init-7.scm, meta-7.scm,
// and all module .sld/.scm files without filesystem access. the data is
// embedded at compile time by build.rs into tein_vfs_data.h.
//
// two entry points, called from patched eval.c:
//   tein_vfs_find()   — maps a relative path to a full vfs:// path
//   tein_vfs_lookup() — returns embedded content for a full vfs:// path

#include "tein_vfs_data.h"

#include <string.h>
#include <stdlib.h>

// look up embedded content by full VFS path (e.g. "/vfs/lib/init-7.scm").
// returns the static content string and sets *out_length, or NULL if not found.
const char* tein_vfs_lookup(const char *full_path, unsigned int *out_length) {
    for (int i = 0; tein_vfs_table[i].key != NULL; i++) {
        if (strcmp(tein_vfs_table[i].key, full_path) == 0) {
            *out_length = tein_vfs_table[i].length;
            return tein_vfs_table[i].content;
        }
    }
    return NULL;
}

// --- standard ports ---
//
// wraps sexp_load_standard_ports to bind stdin/stdout/stderr in an env.
// needed after sexp_load_standard_env for IO to work.

sexp tein_sexp_load_standard_ports(sexp ctx, sexp env) {
    return sexp_load_standard_ports(ctx, env, stdin, stdout, stderr, 1);
}

// --- env copy helper for sandbox + standard env ---
//
// copies a single named binding from src_env to dst_env, searching both
// direct bindings and rename bindings (needed because sexp_load_standard_env
// stores most bindings as renames via the module system).
//
// walks the full env parent chain. for rename entries, the synclo key is
// unwrapped to compare against the bare symbol name.
//
// returns 1 if the binding was found and copied, 0 otherwise.

int tein_env_copy_named(sexp ctx, sexp src_env, sexp dst_env,
                        const char *name, sexp_sint_t name_len) {
    sexp sym = sexp_intern(ctx, name, name_len);
    sexp val = SEXP_VOID;
    int found = 0;

    // first try: direct lookup via sexp_env_ref (handles direct bindings
    // and parent chain, but NOT rename-to-bare matching)
    val = sexp_env_ref(ctx, src_env, sym, SEXP_VOID);
    if (val != SEXP_VOID) {
        sexp_env_define(ctx, dst_env, sym, val);
        return 1;
    }

    // second try: scan rename bindings for synclos whose underlying
    // expression matches our bare symbol.
    // note: sexp_envp(NULL) segfaults because sexp_pointerp(NULL) is true
    // (SEXP_POINTER_TAG == 0), so we guard against NULL explicitly.
    /* iteration limit guards against corrupted environment with cyclic parent chain.
     * chibi should never produce cycles, but defence-in-depth warrants a hard stop. */
    int env_walk_limit = 65536;
    sexp env = src_env;
    while (env && sexp_envp(env) && env_walk_limit-- > 0) {
#if SEXP_USE_RENAME_BINDINGS
        sexp ls;
        for (ls = sexp_env_renames(env); sexp_pairp(ls); ls = sexp_env_next_cell(ls)) {
            sexp key = sexp_car(ls);
            // rename keys are syntactic closures wrapping the original symbol
            if (sexp_synclop(key) && sexp_synclo_expr(key) == sym) {
                // found it — the value is in cdr of the rename cell,
                // which is itself a binding cell (car=key, cdr=value)
                sexp cell = sexp_cdr(ls);
                if (sexp_pairp(cell)) {
                    val = sexp_cdr(cell);
                } else {
                    val = cell;
                }
                // define using the bare symbol so it's accessible without
                // the module system's rename machinery
                sexp_env_define(ctx, dst_env, sym, val);
                return 1;
            }
        }
#endif
        env = sexp_env_parent(env);
    }

    return 0;
}

// --- custom port creation ---
// sexp_make_custom_input_port / sexp_make_custom_output_port are defined in
// lib/chibi/io/port.c (compiled via io.c into chibi_io static lib).
extern sexp sexp_make_custom_input_port(sexp ctx, sexp self,
                                         sexp read, sexp seek, sexp close);
extern sexp sexp_make_custom_output_port(sexp ctx, sexp self,
                                          sexp write, sexp seek, sexp close);

sexp tein_make_custom_input_port(sexp ctx, sexp read_proc) {
    return sexp_make_custom_input_port(ctx, SEXP_FALSE, read_proc, SEXP_FALSE, SEXP_FALSE);
}

sexp tein_make_custom_output_port(sexp ctx, sexp write_proc) {
    return sexp_make_custom_output_port(ctx, SEXP_FALSE, write_proc, SEXP_FALSE, SEXP_FALSE);
}

// --- reader dispatch table ---
/* reader dispatch table — 128 entries, ASCII-only.
 * maps ASCII chars (byte value 0–127) to scheme procedures for custom #x syntax.
 * characters with codepoint >= 128 are not dispatchable; the Rust API
 * documents this limitation in register_reader().
 * thread-local so each context thread has independent dispatch state. */

#define TEIN_READER_DISPATCH_SIZE 128

TEIN_THREAD_LOCAL sexp tein_reader_dispatch[TEIN_READER_DISPATCH_SIZE];
TEIN_THREAD_LOCAL int tein_reader_dispatch_init = 0;

static void tein_reader_dispatch_ensure_init(void) {
    if (!tein_reader_dispatch_init) {
        for (int i = 0; i < TEIN_READER_DISPATCH_SIZE; i++)
            tein_reader_dispatch[i] = SEXP_FALSE;
        tein_reader_dispatch_init = 1;
    }
}

static int tein_reader_char_reserved(int c) {
    switch (c) {
    case 'b': case 'B': case 'o': case 'O': case 'd': case 'D':
    case 'x': case 'X': case 'e': case 'E': case 'i': case 'I':
    case 'f': case 'F': case 't': case 'T': case 'u': case 'U':
    case 'v': case 'V': case 's': case 'S': case 'c': case 'C':
    case '0': case '1': case '2': case '3': case '4':
    case '5': case '6': case '7': case '8': case '9':
    case ';': case '|': case '!': case '\\': case '(': case '\'':
    case '`': case ',':
        return 1;
    default:
        return 0;
    }
}

int tein_reader_char_is_reserved(int c) {
    return tein_reader_char_reserved(c);
}

int tein_reader_dispatch_set(int c, sexp proc) {
    tein_reader_dispatch_ensure_init();
    if (c < 0 || c >= TEIN_READER_DISPATCH_SIZE) return -2;
    if (tein_reader_char_reserved(c)) return -1;
    tein_reader_dispatch[c] = proc;
    return 0;
}

int tein_reader_dispatch_unset(int c) {
    tein_reader_dispatch_ensure_init();
    if (c < 0 || c >= TEIN_READER_DISPATCH_SIZE) return -2;
    tein_reader_dispatch[c] = SEXP_FALSE;
    return 0;
}

sexp tein_reader_dispatch_get(int c) {
    tein_reader_dispatch_ensure_init();
    if (c < 0 || c >= TEIN_READER_DISPATCH_SIZE) return SEXP_FALSE;
    return tein_reader_dispatch[c];
}

sexp tein_reader_dispatch_chars(sexp ctx) {
    tein_reader_dispatch_ensure_init();
    sexp result = SEXP_NULL;
    for (int i = TEIN_READER_DISPATCH_SIZE - 1; i >= 0; i--) {
        if (tein_reader_dispatch[i] != SEXP_FALSE)
            result = sexp_cons(ctx, sexp_make_character(i), result);
    }
    return result;
}

void tein_reader_dispatch_clear(void) {
    tein_reader_dispatch_ensure_init();
    for (int i = 0; i < TEIN_READER_DISPATCH_SIZE; i++)
        tein_reader_dispatch[i] = SEXP_FALSE;
}

// ─── macro expansion hook ───────────────────────────────────────────
// thread-local hook called after each macro expansion. when set (not
// SEXP_FALSE), the hook receives (name unexpanded expanded env) and
// its return value replaces the expanded form (replace-and-reanalyze).
// the active flag prevents recursion when the hook body uses macros.

TEIN_THREAD_LOCAL sexp tein_macro_expand_hook = SEXP_FALSE;
TEIN_THREAD_LOCAL int tein_macro_expand_hook_active = 0;

void tein_macro_expand_hook_set(sexp ctx, sexp proc) {
    if (tein_macro_expand_hook != SEXP_FALSE)
        sexp_release_object(ctx, tein_macro_expand_hook);
    tein_macro_expand_hook = proc;
    if (proc != SEXP_FALSE)
        sexp_preserve_object(ctx, proc);
}

sexp tein_macro_expand_hook_get(void) {
    return tein_macro_expand_hook;
}

void tein_macro_expand_hook_clear(sexp ctx) {
    if (tein_macro_expand_hook != SEXP_FALSE)
        sexp_release_object(ctx, tein_macro_expand_hook);
    tein_macro_expand_hook = SEXP_FALSE;
    tein_macro_expand_hook_active = 0;
}
