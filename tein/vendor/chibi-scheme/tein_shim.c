// tein shim layer - exports chibi macros as actual functions for ffi

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

// vector operations
int tein_sexp_vectorp(sexp x) { return sexp_vectorp(x); }
sexp_uint_t tein_sexp_vector_length(sexp x) { return sexp_vector_length(x); }
sexp* tein_sexp_vector_data(sexp x) { return sexp_vector_data(x); }

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

// vector element setting (direct write, no bounds check)
void tein_sexp_vector_set(sexp vec, sexp_uint_t i, sexp val) {
    sexp_vector_data(vec)[i] = val;
}

// procedure/application support
int tein_sexp_procedurep(sexp x) { return sexp_procedurep(x); }
int tein_sexp_opcodep(sexp x) { return sexp_opcodep(x); }
int tein_sexp_applicablep(sexp x) { return sexp_applicablep(x); }

// multi-expression evaluation support
sexp tein_get_eof() { return SEXP_EOF; }
int tein_sexp_eofp(sexp x) { return x == SEXP_EOF; }
sexp tein_sexp_open_input_string(sexp ctx, sexp str) {
    return sexp_open_input_string(ctx, str);
}
sexp tein_sexp_read(sexp ctx, sexp port) { return sexp_read(ctx, port); }
sexp tein_sexp_evaluate(sexp ctx, sexp obj, sexp env) { return sexp_eval(ctx, obj, env); }

// fuel control (step limiting)
//
// chibi's vm creates child contexts for each eval, so we can't set
// fuel on a single context. instead we use thread-local counters that
// the vm checks via a minimal patch in vm.c.
//
// the vm's existing fuel/refuel mechanism runs in small timeslices
// (default 500 ops). our patch subtracts each timeslice from the
// thread-local budget. when the budget is exhausted, it zeroes
// refuel so the vm stops.

__thread sexp_sint_t tein_fuel_budget = -1;   // -1 = unlimited
__thread int tein_fuel_exhausted_flag = 0;

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
// returns the next timeslice size, or 0 to stop the vm.
sexp_sint_t tein_fuel_consume_slice(sexp ctx, sexp_sint_t slice_used) {
    if (tein_fuel_budget < 0) {
        // unlimited — return default quantum
        return SEXP_DEFAULT_QUANTUM;
    }
    tein_fuel_budget -= slice_used;
    if (tein_fuel_budget <= 0) {
        tein_fuel_exhausted_flag = 1;
        sexp_context_refuel(ctx) = 0;
        return 0;
    }
    sexp_sint_t next = (tein_fuel_budget < SEXP_DEFAULT_QUANTUM)
        ? tein_fuel_budget : SEXP_DEFAULT_QUANTUM;
    sexp_context_refuel(ctx) = next;
    return next;
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
