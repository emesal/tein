//! scheme evaluation context

use crate::{
    Value,
    error::{Error, Result},
    ffi,
    sandbox::Preset,
};
use std::ffi::CString;
use std::path::Path;

// --- default sizes ---

const DEFAULT_HEAP_SIZE: usize = 4 * 1024 * 1024;
const DEFAULT_HEAP_MAX: usize = 128 * 1024 * 1024;

/// builder for configuring a scheme context before creation
///
/// provides a fluent api for setting heap sizes, step limits,
/// and environment restrictions (sandboxing).
///
/// # examples
///
/// ```
/// use tein::Context;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // default context
/// let ctx = Context::new()?;
///
/// // configured context
/// let ctx = Context::builder()
///     .heap_size(8 * 1024 * 1024)
///     .step_limit(100_000)
///     .build()?;
/// # Ok(())
/// # }
/// ```
pub struct ContextBuilder {
    heap_size: usize,
    heap_max: usize,
    step_limit: Option<u64>,
    allowed_primitives: Option<Vec<&'static str>>,
}

impl ContextBuilder {
    /// set the initial heap size in bytes (default: 4mb)
    pub fn heap_size(mut self, size: usize) -> Self {
        self.heap_size = size;
        self
    }

    /// set the maximum heap size in bytes (default: 128mb)
    pub fn heap_max(mut self, size: usize) -> Self {
        self.heap_max = size;
        self
    }

    /// set the maximum number of vm steps per evaluation call
    ///
    /// when the limit is reached, evaluation returns `Error::StepLimitExceeded`.
    /// fuel resets before each `evaluate()` or `call()` invocation.
    pub fn step_limit(mut self, limit: u64) -> Self {
        self.step_limit = Some(limit);
        self
    }

    /// add all primitives from a preset to the allowlist
    ///
    /// activating any preset switches the context to restricted mode:
    /// only explicitly allowed primitives (plus core syntax) are available.
    /// presets are additive — calling this multiple times combines them.
    pub fn preset(mut self, preset: &Preset) -> Self {
        let list = self.allowed_primitives.get_or_insert_with(Vec::new);
        for name in preset.primitives {
            if !list.contains(name) {
                list.push(name);
            }
        }
        self
    }

    /// add individual primitives to the allowlist
    ///
    /// like `preset()`, activates restricted mode. additive with presets.
    pub fn allow(mut self, names: &[&'static str]) -> Self {
        let list = self.allowed_primitives.get_or_insert_with(Vec::new);
        for name in names {
            if !list.contains(name) {
                list.push(name);
            }
        }
        self
    }

    /// convenience: allow arithmetic + math + lists + vectors + strings + characters + type predicates
    ///
    /// suitable for pure computation with no side effects or mutation.
    pub fn pure_computation(self) -> Self {
        use crate::sandbox::*;
        self.preset(&ARITHMETIC)
            .preset(&MATH)
            .preset(&LISTS)
            .preset(&VECTORS)
            .preset(&STRINGS)
            .preset(&CHARACTERS)
            .preset(&TYPE_PREDICATES)
    }

    /// convenience: pure_computation + mutation + string_ports + stdout_only + exceptions
    ///
    /// suitable for most sandboxed use cases that don't need file/network io.
    pub fn safe(self) -> Self {
        use crate::sandbox::*;
        self.pure_computation()
            .preset(&MUTATION)
            .preset(&STRING_PORTS)
            .preset(&STDOUT_ONLY)
            .preset(&EXCEPTIONS)
    }

    /// check if a step limit has been configured
    pub(crate) fn has_step_limit(&self) -> bool {
        self.step_limit.is_some()
    }

    /// build the configured context
    pub fn build(self) -> Result<Context> {
        unsafe {
            let ctx = ffi::sexp_make_eval_context(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                self.heap_size as ffi::sexp_uint_t,
                self.heap_max as ffi::sexp_uint_t,
            );

            if ctx.is_null() {
                return Err(Error::InitError("failed to create context".to_string()));
            }

            // apply environment restrictions if presets are active
            if let Some(ref allowed) = self.allowed_primitives {
                let primitive_env = ffi::sexp_context_env(ctx);
                let version = ffi::sexp_make_fixnum(7);
                let null_env = ffi::sexp_make_null_env(ctx, version);

                if ffi::sexp_exceptionp(null_env) != 0 {
                    ffi::sexp_destroy_context(ctx);
                    return Err(Error::InitError(
                        "failed to create null environment".to_string(),
                    ));
                }

                // copy allowed primitives from the full env into the restricted env
                let undefined = ffi::get_void();
                for name in allowed {
                    let c_name = CString::new(*name).map_err(|_| {
                        ffi::sexp_destroy_context(ctx);
                        Error::InitError(format!("primitive name contains null bytes: {}", name))
                    })?;
                    let sym =
                        ffi::sexp_intern(ctx, c_name.as_ptr(), name.len() as ffi::sexp_sint_t);
                    let val = ffi::sexp_env_ref(ctx, primitive_env, sym, undefined);
                    if val != undefined {
                        ffi::sexp_env_define(ctx, null_env, sym, val);
                    }
                }

                ffi::sexp_context_env_set(ctx, null_env);
            }

            Ok(Context {
                ctx,
                step_limit: self.step_limit,
            })
        }
    }
}

/// a scheme evaluation context
///
/// this is the main entry point for evaluating scheme code.
/// each context maintains its own heap and environment.
///
/// # examples
///
/// ```
/// use tein::{Context, Value};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let ctx = Context::new()?;
/// let result = ctx.evaluate("(+ 1 2 3)")?;
/// assert_eq!(result, Value::Integer(6));
/// # Ok(())
/// # }
/// ```
pub struct Context {
    ctx: ffi::sexp,
    step_limit: Option<u64>,
}

impl Context {
    /// create a new scheme context with default settings
    ///
    /// initializes a chibi-scheme context with:
    /// - 4mb initial heap
    /// - 128mb max heap
    /// - full primitive environment (no restrictions)
    /// - no step limit
    pub fn new() -> Result<Self> {
        Self::builder().build()
    }

    /// create a builder for configuring a context
    pub fn builder() -> ContextBuilder {
        ContextBuilder {
            heap_size: DEFAULT_HEAP_SIZE,
            heap_max: DEFAULT_HEAP_MAX,
            step_limit: None,
            allowed_primitives: None,
        }
    }

    /// set fuel before an evaluation call (if step limit is configured)
    fn arm_fuel(&self) {
        if let Some(limit) = self.step_limit {
            unsafe {
                ffi::fuel_arm(self.ctx, limit as ffi::sexp_sint_t);
            }
        }
    }

    /// check if fuel was exhausted after an evaluation call, then disarm
    fn check_fuel(&self) -> Result<()> {
        if self.step_limit.is_some() {
            unsafe {
                let exhausted = ffi::fuel_exhausted(self.ctx) != 0;
                ffi::fuel_disarm(self.ctx);
                if exhausted {
                    return Err(Error::StepLimitExceeded);
                }
            }
        }
        Ok(())
    }

    /// evaluate one or more scheme expressions
    ///
    /// evaluates all expressions in the string sequentially, returning the
    /// result of the last expression. this enables natural scripting patterns
    /// like defining values and then using them.
    ///
    /// # examples
    ///
    /// ```
    /// use tein::{Context, Value};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let ctx = Context::new()?;
    ///
    /// // single expression
    /// let result = ctx.evaluate("(+ 1 2 3)")?;
    /// assert_eq!(result, Value::Integer(6));
    ///
    /// // multiple expressions - returns the last result
    /// let result = ctx.evaluate("(define x 5) (+ x 3)")?;
    /// assert_eq!(result, Value::Integer(8));
    /// # Ok(())
    /// # }
    /// ```
    pub fn evaluate(&self, code: &str) -> Result<Value> {
        let c_str = CString::new(code)
            .map_err(|_| Error::EvalError("code contains null bytes".to_string()))?;

        self.arm_fuel();

        unsafe {
            let env = ffi::sexp_context_env(self.ctx);

            // create a scheme string from the code
            let scheme_str =
                ffi::sexp_c_str(self.ctx, c_str.as_ptr(), code.len() as ffi::sexp_sint_t);

            // open an input port on the string
            let port = ffi::sexp_open_input_string(self.ctx, scheme_str);
            if ffi::sexp_exceptionp(port) != 0 {
                return Value::from_raw(self.ctx, port);
            }

            // read and evaluate expressions until EOF
            let mut result = ffi::get_void();
            loop {
                let expr = ffi::sexp_read(self.ctx, port);

                // EOF means we're done
                if ffi::sexp_eofp(expr) != 0 {
                    break;
                }

                // read error
                if ffi::sexp_exceptionp(expr) != 0 {
                    return Value::from_raw(self.ctx, expr);
                }

                // evaluate the expression
                result = ffi::sexp_evaluate(self.ctx, expr, env);

                // check fuel exhaustion before exception status
                // (fuel exhaustion returns a normal-looking value, not an exception)
                self.check_fuel()?;

                // evaluation error
                if ffi::sexp_exceptionp(result) != 0 {
                    return Value::from_raw(self.ctx, result);
                }
            }

            Value::from_raw(self.ctx, result)
        }
    }

    /// load and evaluate a scheme file
    ///
    /// reads the file contents and evaluates all expressions sequentially,
    /// returning the result of the last expression. this is the file-based
    /// equivalent of [`evaluate`](Self::evaluate).
    ///
    /// # examples
    ///
    /// ```no_run
    /// use tein::{Context, Value};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let ctx = Context::new()?;
    ///
    /// // load a config file that defines values and returns a result
    /// let result = ctx.load_file("config.scm")?;
    ///
    /// // load a prelude for side effects (defines), ignore result
    /// let _ = ctx.load_file("prelude.scm")?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # errors
    ///
    /// returns [`Error::IoError`] if the file cannot be read, or evaluation
    /// errors if the scheme code is invalid.
    pub fn load_file<P: AsRef<Path>>(&self, path: P) -> Result<Value> {
        let contents = std::fs::read_to_string(path.as_ref())?;
        self.evaluate(&contents)
    }

    /// register a foreign function as a scheme primitive
    ///
    /// all arguments are passed as a single scheme list via the `args` parameter.
    /// this is the universal registration method — use `#[scheme_fn]` for ergonomic
    /// wrappers that handle argument extraction and return conversion automatically.
    ///
    /// the function receives all arguments as a single scheme list in the `args`
    /// parameter. chibi passes `(ctx, self, nargs, args)` where args is a proper
    /// list of all actual arguments.
    ///
    /// this uses `sexp_define_foreign_proc_aux` with `SEXP_PROC_VARIADIC`,
    /// which wraps the opcode in a real procedure object.
    ///
    /// # examples
    ///
    /// ```
    /// use tein::{Context, Value, raw};
    ///
    /// // sum all integer arguments
    /// unsafe extern "C" fn sum_all(
    ///     ctx: raw::sexp, _self: raw::sexp, _n: raw::sexp_sint_t, args: raw::sexp,
    /// ) -> raw::sexp {
    ///     unsafe {
    ///         let mut total: i64 = 0;
    ///         let mut current = args;
    ///         while raw::sexp_pairp(current) != 0 {
    ///             total += raw::sexp_unbox_fixnum(raw::sexp_car(current)) as i64;
    ///             current = raw::sexp_cdr(current);
    ///         }
    ///         raw::sexp_make_fixnum(total as raw::sexp_sint_t)
    ///     }
    /// }
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let ctx = Context::new()?;
    /// ctx.define_fn_variadic("sum-all", sum_all)?;
    /// let result = ctx.evaluate("(sum-all 1 2 3 4 5)")?;
    /// assert_eq!(result, Value::Integer(15));
    /// # Ok(())
    /// # }
    /// ```
    pub fn define_fn_variadic(
        &self,
        name: &str,
        f: unsafe extern "C" fn(ffi::sexp, ffi::sexp, ffi::sexp_sint_t, ffi::sexp) -> ffi::sexp,
    ) -> Result<()> {
        let c_name = CString::new(name)
            .map_err(|_| Error::EvalError("function name contains null bytes".to_string()))?;

        unsafe {
            let env = ffi::sexp_context_env(self.ctx);
            let f_typed: Option<
                unsafe extern "C" fn(ffi::sexp, ffi::sexp, ffi::sexp_sint_t) -> ffi::sexp,
            > = std::mem::transmute::<*const std::ffi::c_void, _>(f as *const std::ffi::c_void);
            let result = ffi::sexp_define_foreign_proc(
                self.ctx,
                env,
                c_name.as_ptr(),
                0, // num_args = 0 (variadic handles its own arity)
                ffi::SEXP_PROC_VARIADIC,
                c_name.as_ptr(),
                f_typed,
            );

            if ffi::sexp_exceptionp(result) != 0 {
                return Err(Error::EvalError(format!(
                    "failed to define variadic function '{}'",
                    name
                )));
            }
        }

        Ok(())
    }

    /// call a scheme procedure from rust
    ///
    /// invokes a `Value::Procedure` (lambda, named function, or builtin)
    /// with the given arguments and returns the result.
    ///
    /// # examples
    ///
    /// ```
    /// use tein::{Context, Value};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let ctx = Context::new()?;
    /// let add = ctx.evaluate("+")?;
    /// let result = ctx.call(&add, &[Value::Integer(2), Value::Integer(3)])?;
    /// assert_eq!(result, Value::Integer(5));
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # errors
    ///
    /// returns [`Error::TypeError`] if `proc` is not a `Value::Procedure`,
    /// or [`Error::EvalError`] if the scheme call raises an exception.
    pub fn call(&self, proc: &Value, args: &[Value]) -> Result<Value> {
        let raw_proc = proc
            .as_procedure()
            .ok_or_else(|| Error::TypeError(format!("expected procedure, got {}", proc)))?;

        self.arm_fuel();

        unsafe {
            // build scheme list from args (reverse-iterate with cons, like to_raw does for lists)
            let mut arg_list = ffi::get_null();
            for arg in args.iter().rev() {
                let raw_arg = arg.to_raw(self.ctx)?;
                arg_list = ffi::sexp_cons(self.ctx, raw_arg, arg_list);
            }

            let result = ffi::sexp_apply_proc(self.ctx, raw_proc, arg_list);

            // check fuel before exception status
            self.check_fuel()?;

            if ffi::sexp_exceptionp(result) != 0 {
                return Value::from_raw(self.ctx, result);
            }

            Value::from_raw(self.ctx, result)
        }
    }

    /// get the raw context pointer for advanced ffi use
    ///
    /// # safety
    /// the returned pointer is only valid for the lifetime of this context.
    /// do not call `sexp_destroy_context` on it.
    pub fn raw_ctx(&self) -> ffi::sexp {
        self.ctx
    }
}

impl Drop for Context {
    fn drop(&mut self) {
        unsafe {
            if !self.ctx.is_null() {
                ffi::sexp_destroy_context(self.ctx);
            }
        }
    }
}

// context is intentionally !Send + !Sync:
// chibi-scheme contexts are not thread-safe, and the raw sexp pointer
// provides !Send + !Sync by default. users who need multi-threaded
// evaluation should create one context per thread.

#[cfg(test)]
mod tests {
    use super::*;

    // --- basic types ---

    #[test]
    fn test_basic_arithmetic() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("(+ 1 2 3)").expect("failed to evaluate");
        match result {
            Value::Integer(n) => assert_eq!(n, 6),
            _ => panic!("expected integer, got {:?}", result),
        }
    }

    // --- multi-expression evaluation ---

    #[test]
    fn test_multi_expression_define_and_use() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx
            .evaluate("(define x 5) (+ x 3)")
            .expect("failed to evaluate");
        assert_eq!(result, Value::Integer(8));
    }

    #[test]
    fn test_multi_expression_returns_last() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("1 2 3").expect("failed to evaluate");
        assert_eq!(result, Value::Integer(3));
    }

    #[test]
    fn test_multi_expression_with_procedure() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx
            .evaluate("(define (square x) (* x x)) (square 7)")
            .expect("failed to evaluate");
        assert_eq!(result, Value::Integer(49));
    }

    #[test]
    fn test_multi_expression_error_stops_early() {
        let ctx = Context::new().expect("failed to create context");
        // error in first expression should prevent second from running
        let err = ctx.evaluate("(car 42) (+ 1 2)").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("pair"), "expected pair error, got: {}", msg);
    }

    #[test]
    fn test_empty_input() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("").expect("failed to evaluate");
        // empty input returns void/unspecified
        assert!(result.is_unspecified());
    }

    #[test]
    fn test_whitespace_only() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("   \n\t  ").expect("failed to evaluate");
        assert!(result.is_unspecified());
    }

    #[test]
    fn test_string_evaluation() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx
            .evaluate(r#""hello world""#)
            .expect("failed to evaluate");
        match result {
            Value::String(s) => assert_eq!(s, "hello world"),
            _ => panic!("expected string, got {:?}", result),
        }
    }

    #[test]
    fn test_boolean() {
        let ctx = Context::new().expect("failed to create context");
        let t = ctx.evaluate("#t").expect("failed to evaluate");
        let f = ctx.evaluate("#f").expect("failed to evaluate");
        assert!(matches!(t, Value::Boolean(true)));
        assert!(matches!(f, Value::Boolean(false)));
    }

    #[test]
    fn test_float() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("3.14").expect("failed to evaluate");
        match result {
            #[allow(clippy::approx_constant)]
            Value::Float(f) => assert!((f - 3.14).abs() < 1e-10),
            _ => panic!("expected float, got {:?}", result),
        }
    }

    #[test]
    fn test_symbol() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("(quote foo)").expect("failed to evaluate");
        match result {
            Value::Symbol(s) => assert_eq!(s, "foo"),
            _ => panic!("expected symbol, got {:?}", result),
        }
    }

    #[test]
    fn test_unspecified() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("(define x 5)").expect("failed to evaluate");
        assert_eq!(result, Value::Unspecified);
    }

    #[test]
    fn test_nil() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("(quote ())").expect("failed to evaluate");
        assert!(matches!(result, Value::Nil));
    }

    // --- lists and pairs ---

    #[test]
    fn test_proper_list() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("(quote (1 2 3))").expect("failed to evaluate");
        match result {
            Value::List(items) => {
                assert_eq!(items.len(), 3);
                assert!(matches!(items[0], Value::Integer(1)));
                assert!(matches!(items[1], Value::Integer(2)));
                assert!(matches!(items[2], Value::Integer(3)));
            }
            _ => panic!("expected list, got {:?}", result),
        }
    }

    #[test]
    fn test_dotted_pair() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate("(cons 1 2)").expect("failed to evaluate");
        match result {
            Value::Pair(car, cdr) => {
                assert!(matches!(*car, Value::Integer(1)));
                assert!(matches!(*cdr, Value::Integer(2)));
            }
            _ => panic!("expected pair, got {:?}", result),
        }
    }

    #[test]
    fn test_nested_list() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx
            .evaluate("(quote (a (b c) d))")
            .expect("failed to evaluate");
        match result {
            Value::List(items) => {
                assert_eq!(items.len(), 3);
                assert!(matches!(&items[1], Value::List(inner) if inner.len() == 2));
            }
            _ => panic!("expected list, got {:?}", result),
        }
    }

    // --- vectors ---

    #[test]
    fn test_vector() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx
            .evaluate("(make-vector 3 0)")
            .expect("failed to evaluate");
        match result {
            Value::Vector(items) => {
                assert_eq!(items.len(), 3);
                for item in &items {
                    assert!(matches!(item, Value::Integer(0)));
                }
            }
            _ => panic!("expected vector, got {:?}", result),
        }
    }

    #[test]
    fn test_vector_display() {
        let v = Value::Vector(vec![
            Value::Integer(1),
            Value::Integer(2),
            Value::Integer(3),
        ]);
        assert_eq!(format!("{}", v), "#(1 2 3)");
    }

    #[test]
    fn test_empty_vector() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx
            .evaluate("(make-vector 0 #f)")
            .expect("failed to evaluate");
        match result {
            Value::Vector(items) => assert_eq!(items.len(), 0),
            _ => panic!("expected empty vector, got {:?}", result),
        }
    }

    // --- error messages ---

    #[test]
    fn test_error_message_detail() {
        let ctx = Context::new().expect("failed to create context");
        let err = ctx.evaluate("(car 42)").unwrap_err();
        let msg = format!("{}", err);
        // should contain more than just "scheme exception occurred"
        assert!(
            msg.len() > "scheme evaluation error: ".len() + 5,
            "error message too generic: {}",
            msg
        );
    }

    #[test]
    fn test_error_on_undefined() {
        let ctx = Context::new().expect("failed to create context");
        let err = ctx.evaluate("undefined-variable").unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("undefined"),
            "expected 'undefined' in: {}",
            msg
        );
    }

    // --- foreign functions (using define_fn_variadic) ---

    #[test]
    fn test_foreign_fn_integer() {
        unsafe extern "C" fn add_forty_two(
            _ctx: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let n = crate::ffi::sexp_unbox_fixnum(crate::ffi::sexp_car(args));
                crate::ffi::sexp_make_fixnum(n + 42)
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn_variadic("add42", add_forty_two)
            .expect("failed to define fn");
        let result = ctx.evaluate("(add42 8)").expect("failed to evaluate");
        assert_eq!(result, Value::Integer(50));
    }

    #[test]
    fn test_foreign_fn_string() {
        unsafe extern "C" fn hello_fn(
            ctx: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            _args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let s = "hello from rust";
                let c_str = std::ffi::CString::new(s).unwrap();
                crate::ffi::sexp_c_str(ctx, c_str.as_ptr(), s.len() as crate::ffi::sexp_sint_t)
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn_variadic("hello", hello_fn)
            .expect("failed to define fn");
        let result = ctx.evaluate("(hello)").expect("failed to evaluate");
        assert_eq!(result, Value::String("hello from rust".to_string()));
    }

    #[test]
    fn test_foreign_fn_two_args() {
        unsafe extern "C" fn multiply(
            _ctx: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let a = crate::ffi::sexp_unbox_fixnum(crate::ffi::sexp_car(args));
                let rest = crate::ffi::sexp_cdr(args);
                let b = crate::ffi::sexp_unbox_fixnum(crate::ffi::sexp_car(rest));
                crate::ffi::sexp_make_fixnum(a * b)
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn_variadic("rust-mul", multiply)
            .expect("failed to define fn");
        let result = ctx.evaluate("(rust-mul 6 7)").expect("failed to evaluate");
        assert_eq!(result, Value::Integer(42));
    }

    #[test]
    fn test_foreign_fn_uses_scheme_values() {
        unsafe extern "C" fn square(
            _ctx: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let n = crate::ffi::sexp_unbox_fixnum(crate::ffi::sexp_car(args));
                crate::ffi::sexp_make_fixnum(n * n)
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn_variadic("square", square)
            .expect("failed to define fn");
        let result = ctx
            .evaluate("(+ (square 3) (square 4))")
            .expect("failed to evaluate");
        assert_eq!(result, Value::Integer(25)); // 9 + 16
    }

    // --- gc pinning (deeply nested structures) ---

    #[test]
    fn test_deeply_nested_list() {
        let ctx = Context::new().expect("failed to create context");
        // build a 100-deep nested list: (1 (1 (1 ... (1) ...)))
        let mut code = String::from("(quote ");
        for _ in 0..100 {
            code.push_str("(1 ");
        }
        code.push_str("()");
        for _ in 0..100 {
            code.push(')');
        }
        code.push(')');
        let result = ctx
            .evaluate(&code)
            .expect("failed to evaluate deeply nested list");
        // outermost should be a list
        assert!(
            matches!(result, Value::List(_)),
            "expected list, got {:?}",
            result
        );
    }

    #[test]
    fn test_deeply_nested_vector() {
        let ctx = Context::new().expect("failed to create context");
        // build 100-deep nested vector from a single expression:
        // (make-vector 1 (make-vector 1 (make-vector 1 ... 42 ...)))
        // this creates a true tree (no structural sharing) so extraction is O(n).
        let depth = 100;
        let mut code = String::new();
        for _ in 0..depth {
            code.push_str("(make-vector 1 ");
        }
        code.push_str("42");
        for _ in 0..depth {
            code.push(')');
        }
        let result = ctx
            .evaluate(&code)
            .expect("failed to evaluate nested vector");
        assert!(
            matches!(result, Value::Vector(_)),
            "expected vector, got {:?}",
            result
        );
    }

    #[test]
    fn test_mixed_nested_structures() {
        let ctx = Context::new().expect("failed to create context");
        // list containing vectors containing lists
        let result = ctx
            .evaluate("(quote ((1 2) (3 4)))")
            .expect("failed to evaluate");
        match &result {
            Value::List(items) => {
                assert_eq!(items.len(), 2);
                assert!(matches!(&items[0], Value::List(inner) if inner.len() == 2));
                assert!(matches!(&items[1], Value::List(inner) if inner.len() == 2));
            }
            _ => panic!("expected list, got {:?}", result),
        }

        // vector inside list
        ctx.evaluate("(define test-vec (make-vector 3 99))")
            .expect("define vec");
        let result = ctx
            .evaluate("(cons test-vec (quote ()))")
            .expect("eval cons");
        match &result {
            Value::List(items) => {
                assert_eq!(items.len(), 1);
                assert!(matches!(&items[0], Value::Vector(v) if v.len() == 3));
            }
            _ => panic!("expected list containing vector, got {:?}", result),
        }
    }

    // --- typed extraction helpers ---

    #[test]
    #[allow(clippy::approx_constant)]
    fn test_as_integer() {
        let v = Value::Integer(42);
        assert_eq!(v.as_integer(), Some(42));
        assert_eq!(Value::Float(3.14).as_integer(), None);
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn test_as_float() {
        let v = Value::Float(2.718);
        assert!((v.as_float().unwrap() - 2.718).abs() < 1e-10);
        assert_eq!(Value::Integer(42).as_float(), None);
    }

    #[test]
    fn test_as_string() {
        let v = Value::String("hello".into());
        assert_eq!(v.as_string(), Some("hello"));
        assert_eq!(Value::Symbol("hello".into()).as_string(), None);
    }

    #[test]
    fn test_as_symbol() {
        let v = Value::Symbol("foo".into());
        assert_eq!(v.as_symbol(), Some("foo"));
        assert_eq!(Value::String("foo".into()).as_symbol(), None);
    }

    #[test]
    fn test_as_bool() {
        assert_eq!(Value::Boolean(true).as_bool(), Some(true));
        assert_eq!(Value::Boolean(false).as_bool(), Some(false));
        assert_eq!(Value::Integer(1).as_bool(), None);
    }

    #[test]
    fn test_as_list() {
        let v = Value::List(vec![Value::Integer(1), Value::Integer(2)]);
        let items = v.as_list().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].as_integer(), Some(1));
        assert_eq!(Value::Vector(vec![]).as_list(), None);
    }

    #[test]
    fn test_as_pair() {
        let v = Value::Pair(Box::new(Value::Integer(1)), Box::new(Value::Integer(2)));
        let (car, cdr) = v.as_pair().unwrap();
        assert_eq!(car.as_integer(), Some(1));
        assert_eq!(cdr.as_integer(), Some(2));
        assert_eq!(Value::List(vec![]).as_pair(), None);
    }

    #[test]
    fn test_as_vector() {
        let v = Value::Vector(vec![Value::Integer(1), Value::Integer(2)]);
        let items = v.as_vector().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(Value::List(vec![]).as_vector(), None);
    }

    #[test]
    fn test_is_nil() {
        assert!(Value::Nil.is_nil());
        assert!(!Value::List(vec![]).is_nil());
    }

    #[test]
    fn test_is_unspecified() {
        assert!(Value::Unspecified.is_unspecified());
        assert!(!Value::Nil.is_unspecified());
    }

    // --- to_raw round-trip tests ---

    #[test]
    fn test_list_to_raw_roundtrip() {
        unsafe extern "C" fn get_test_list(
            ctx_ptr: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            _args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let list = Value::List(vec![
                    Value::Integer(1),
                    Value::Integer(2),
                    Value::Integer(3),
                ]);
                list.to_raw(ctx_ptr)
                    .unwrap_or_else(|_| crate::ffi::get_void())
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn_variadic("get-test-list", get_test_list)
            .expect("define fn");
        let result = ctx.evaluate("(get-test-list)").expect("eval");
        match result {
            Value::List(items) => {
                assert_eq!(items.len(), 3);
                assert_eq!(items[0].as_integer(), Some(1));
                assert_eq!(items[1].as_integer(), Some(2));
                assert_eq!(items[2].as_integer(), Some(3));
            }
            _ => panic!("expected list, got {:?}", result),
        }
    }

    #[test]
    fn test_pair_to_raw_roundtrip() {
        unsafe extern "C" fn get_test_pair(
            ctx_ptr: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            _args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let pair = Value::Pair(
                    Box::new(Value::Symbol("key".into())),
                    Box::new(Value::Integer(42)),
                );
                pair.to_raw(ctx_ptr)
                    .unwrap_or_else(|_| crate::ffi::get_void())
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn_variadic("get-test-pair", get_test_pair)
            .expect("define fn");
        let result = ctx.evaluate("(get-test-pair)").expect("eval");
        match result {
            Value::Pair(car, cdr) => {
                assert_eq!(car.as_symbol(), Some("key"));
                assert_eq!(cdr.as_integer(), Some(42));
            }
            _ => panic!("expected pair, got {:?}", result),
        }
    }

    #[test]
    fn test_vector_to_raw_roundtrip() {
        unsafe extern "C" fn get_test_vector(
            ctx_ptr: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            _args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let vec = Value::Vector(vec![Value::String("a".into()), Value::String("b".into())]);
                vec.to_raw(ctx_ptr)
                    .unwrap_or_else(|_| crate::ffi::get_void())
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn_variadic("get-test-vector", get_test_vector)
            .expect("define fn");
        let result = ctx.evaluate("(get-test-vector)").expect("eval");
        match result {
            Value::Vector(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].as_string(), Some("a"));
                assert_eq!(items[1].as_string(), Some("b"));
            }
            _ => panic!("expected vector, got {:?}", result),
        }
    }

    #[test]
    fn test_nested_list_to_raw_roundtrip() {
        unsafe extern "C" fn get_nested_list(
            ctx_ptr: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            _args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let nested = Value::List(vec![
                    Value::Integer(1),
                    Value::List(vec![Value::Integer(2), Value::Integer(3)]),
                    Value::Integer(4),
                ]);
                nested
                    .to_raw(ctx_ptr)
                    .unwrap_or_else(|_| crate::ffi::get_void())
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn_variadic("get-nested-list", get_nested_list)
            .expect("define fn");
        let result = ctx.evaluate("(get-nested-list)").expect("eval");
        match &result {
            Value::List(items) => {
                assert_eq!(items.len(), 3);
                assert_eq!(items[0].as_integer(), Some(1));
                assert!(matches!(&items[1], Value::List(inner) if inner.len() == 2));
                assert_eq!(items[2].as_integer(), Some(4));
            }
            _ => panic!("expected list, got {:?}", result),
        }
    }

    #[test]
    fn test_empty_list_to_raw() {
        unsafe extern "C" fn get_empty_list(
            ctx_ptr: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            _args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let empty = Value::List(vec![]);
                empty
                    .to_raw(ctx_ptr)
                    .unwrap_or_else(|_| crate::ffi::get_void())
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn_variadic("get-empty-list", get_empty_list)
            .expect("define fn");
        let result = ctx.evaluate("(get-empty-list)").expect("eval");
        assert!(
            result.is_nil(),
            "empty list should become nil, got {:?}",
            result
        );
    }

    #[test]
    fn test_empty_vector_to_raw() {
        unsafe extern "C" fn get_empty_vector(
            ctx_ptr: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            _args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let empty = Value::Vector(vec![]);
                empty
                    .to_raw(ctx_ptr)
                    .unwrap_or_else(|_| crate::ffi::get_void())
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn_variadic("get-empty-vector", get_empty_vector)
            .expect("define fn");
        let result = ctx.evaluate("(get-empty-vector)").expect("eval");
        match result {
            Value::Vector(items) => assert_eq!(items.len(), 0),
            _ => panic!("expected empty vector, got {:?}", result),
        }
    }

    // --- value display ---

    #[test]
    fn test_display_roundtrip() {
        let cases = [
            (Value::Integer(42), "42"),
            #[allow(clippy::approx_constant)]
            (Value::Float(3.14), "3.14"),
            (Value::String("hi".into()), "\"hi\""),
            (Value::Symbol("foo".into()), "foo"),
            (Value::Boolean(true), "#t"),
            (Value::Boolean(false), "#f"),
            (Value::Nil, "()"),
            (Value::Unspecified, "#<unspecified>"),
        ];
        for (val, expected) in &cases {
            assert_eq!(format!("{}", val), *expected, "for {:?}", val);
        }
    }

    // --- file loading ---

    #[test]
    fn test_load_file_basic() {
        let dir = std::env::temp_dir();
        let path = dir.join("tein_test_basic.scm");
        std::fs::write(&path, "(+ 1 2 3)").expect("write test file");

        let ctx = Context::new().expect("create context");
        let result = ctx.load_file(&path).expect("load file");
        assert_eq!(result, Value::Integer(6));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_load_file_multi_expression() {
        let dir = std::env::temp_dir();
        let path = dir.join("tein_test_multi.scm");
        std::fs::write(&path, "(define x 10)\n(define y 20)\n(+ x y)").expect("write test file");

        let ctx = Context::new().expect("create context");
        let result = ctx.load_file(&path).expect("load file");
        assert_eq!(result, Value::Integer(30));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_load_file_defines_persist() {
        let dir = std::env::temp_dir();
        let path = dir.join("tein_test_persist.scm");
        std::fs::write(&path, "(define (square x) (* x x))").expect("write test file");

        let ctx = Context::new().expect("create context");
        let _ = ctx.load_file(&path).expect("load file");

        // definition from file should be available for subsequent evaluation
        let result = ctx.evaluate("(square 7)").expect("eval");
        assert_eq!(result, Value::Integer(49));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_load_file_not_found() {
        let ctx = Context::new().expect("create context");
        let err = ctx.load_file("/nonexistent/path/to/file.scm").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("io error"), "expected io error, got: {}", msg);
    }

    #[test]
    fn test_load_file_syntax_error() {
        let dir = std::env::temp_dir();
        let path = dir.join("tein_test_syntax.scm");
        std::fs::write(&path, "(define x").expect("write test file"); // unclosed paren

        let ctx = Context::new().expect("create context");
        let err = ctx.load_file(&path).unwrap_err();
        // should be an eval error, not io error
        let msg = format!("{}", err);
        assert!(
            !msg.contains("io error"),
            "expected eval error, got io: {}",
            msg
        );

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_load_file_empty() {
        let dir = std::env::temp_dir();
        let path = dir.join("tein_test_empty.scm");
        std::fs::write(&path, "").expect("write test file");

        let ctx = Context::new().expect("create context");
        let result = ctx.load_file(&path).expect("load file");
        assert!(result.is_unspecified());

        std::fs::remove_file(&path).ok();
    }

    // --- procedures as values ---

    #[test]
    fn test_evaluate_lambda_returns_procedure() {
        let ctx = Context::new().expect("create context");
        let result = ctx.evaluate("(lambda (x) (* x x))").expect("eval lambda");
        assert!(
            result.is_procedure(),
            "expected procedure, got {:?}",
            result
        );
    }

    #[test]
    fn test_call_lambda() {
        let ctx = Context::new().expect("create context");
        let square = ctx.evaluate("(lambda (x) (* x x))").expect("eval lambda");
        let result = ctx
            .call(&square, &[Value::Integer(7)])
            .expect("call lambda");
        assert_eq!(result, Value::Integer(49));
    }

    #[test]
    fn test_call_named_procedure() {
        let ctx = Context::new().expect("create context");
        ctx.evaluate("(define (add a b) (+ a b))")
            .expect("define add");
        let add = ctx.evaluate("add").expect("get add");
        assert!(add.is_procedure());
        let result = ctx
            .call(&add, &[Value::Integer(3), Value::Integer(4)])
            .expect("call add");
        assert_eq!(result, Value::Integer(7));
    }

    #[test]
    fn test_call_builtin_procedure() {
        let ctx = Context::new().expect("create context");
        // + is a builtin opcode, should come back as Procedure via sexp_applicablep
        let plus = ctx.evaluate("+").expect("get +");
        assert!(
            plus.is_procedure(),
            "expected procedure for +, got {:?}",
            plus
        );
        let result = ctx
            .call(&plus, &[Value::Integer(10), Value::Integer(20)])
            .expect("call +");
        assert_eq!(result, Value::Integer(30));
    }

    #[test]
    fn test_call_with_non_procedure_returns_type_error() {
        let ctx = Context::new().expect("create context");
        let not_proc = Value::Integer(42);
        let err = ctx.call(&not_proc, &[]).unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("type error"),
            "expected type error, got: {}",
            msg
        );
    }

    #[test]
    fn test_call_wrong_arity_propagates_exception() {
        let ctx = Context::new().expect("create context");
        let square = ctx.evaluate("(lambda (x) (* x x))").expect("eval lambda");
        // call with 2 args when it expects 1
        let err = ctx
            .call(&square, &[Value::Integer(1), Value::Integer(2)])
            .unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("error"), "expected exception, got: {}", msg);
    }

    #[test]
    fn test_call_zero_args() {
        let ctx = Context::new().expect("create context");
        let thunk = ctx.evaluate("(lambda () 42)").expect("eval thunk");
        let result = ctx.call(&thunk, &[]).expect("call thunk");
        assert_eq!(result, Value::Integer(42));
    }

    #[test]
    fn test_extract_builtin_via_define() {
        // (define f +) f → Procedure
        let ctx = Context::new().expect("create context");
        let f = ctx.evaluate("(define f +) f").expect("eval");
        assert!(f.is_procedure(), "expected procedure, got {:?}", f);
        let result = ctx
            .call(&f, &[Value::Integer(1), Value::Integer(2)])
            .expect("call f");
        assert_eq!(result, Value::Integer(3));
    }

    #[test]
    fn test_roundtrip_rust_fn_as_procedure() {
        // register rust fn, get it back as procedure, call from rust
        unsafe extern "C" fn double_it(
            _ctx: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let n = crate::ffi::sexp_unbox_fixnum(crate::ffi::sexp_car(args));
                crate::ffi::sexp_make_fixnum(n * 2)
            }
        }

        let ctx = Context::new().expect("create context");
        ctx.define_fn_variadic("double-it", double_it)
            .expect("define fn");
        let proc = ctx.evaluate("double-it").expect("get proc");
        assert!(proc.is_procedure(), "expected procedure, got {:?}", proc);
        let result = ctx
            .call(&proc, &[Value::Integer(21)])
            .expect("call double-it");
        assert_eq!(result, Value::Integer(42));
    }

    #[test]
    fn test_procedure_display() {
        let ctx = Context::new().expect("create context");
        let proc = ctx.evaluate("(lambda (x) x)").expect("eval lambda");
        assert_eq!(format!("{}", proc), "#<procedure>");
    }

    #[test]
    fn test_procedure_equality() {
        let ctx = Context::new().expect("create context");
        // same lambda bound to a variable — same object
        ctx.evaluate("(define f (lambda (x) x))").expect("define f");
        let f1 = ctx.evaluate("f").expect("get f");
        let f2 = ctx.evaluate("f").expect("get f again");
        assert_eq!(f1, f2, "same binding should yield same procedure");

        // different lambdas are different objects
        let g = ctx.evaluate("(lambda (x) x)").expect("different lambda");
        assert_ne!(f1, g, "different lambdas should not be equal");
    }

    // --- variadic foreign functions ---

    #[test]
    fn test_variadic_sum() {
        unsafe extern "C" fn sum_all(
            _ctx: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let mut total: i64 = 0;
                let mut current = args;
                while crate::ffi::sexp_pairp(current) != 0 {
                    total += crate::ffi::sexp_unbox_fixnum(crate::ffi::sexp_car(current)) as i64;
                    current = crate::ffi::sexp_cdr(current);
                }
                crate::ffi::sexp_make_fixnum(total as crate::ffi::sexp_sint_t)
            }
        }

        let ctx = Context::new().expect("create context");
        ctx.define_fn_variadic("sum-all", sum_all)
            .expect("define fn");
        let result = ctx.evaluate("(sum-all 1 2 3 4 5)").expect("eval");
        assert_eq!(result, Value::Integer(15));
    }

    #[test]
    fn test_variadic_zero_args() {
        unsafe extern "C" fn constant(
            _ctx: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            _args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe { crate::ffi::sexp_make_fixnum(42) }
        }

        let ctx = Context::new().expect("create context");
        ctx.define_fn_variadic("constant", constant)
            .expect("define fn");
        let result = ctx.evaluate("(constant)").expect("eval");
        assert_eq!(result, Value::Integer(42));
    }

    #[test]
    fn test_variadic_many_args() {
        unsafe extern "C" fn count_args(
            _ctx: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let mut count: i64 = 0;
                let mut current = args;
                while crate::ffi::sexp_pairp(current) != 0 {
                    count += 1;
                    current = crate::ffi::sexp_cdr(current);
                }
                crate::ffi::sexp_make_fixnum(count as crate::ffi::sexp_sint_t)
            }
        }

        let ctx = Context::new().expect("create context");
        ctx.define_fn_variadic("count-args", count_args)
            .expect("define fn");
        let result = ctx
            .evaluate("(count-args 1 2 3 4 5 6 7 8 9 10 11 12)")
            .expect("eval");
        assert_eq!(result, Value::Integer(12));
    }

    #[test]
    fn test_variadic_mixed_types() {
        // returns a string describing the types of all args
        unsafe extern "C" fn describe_types(
            ctx: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let mut desc = std::string::String::new();
                let mut current = args;
                while crate::ffi::sexp_pairp(current) != 0 {
                    let item = crate::ffi::sexp_car(current);
                    if !desc.is_empty() {
                        desc.push(' ');
                    }
                    if crate::ffi::sexp_integerp(item) != 0 {
                        desc.push_str("int");
                    } else if crate::ffi::sexp_stringp(item) != 0 {
                        desc.push_str("str");
                    } else if crate::ffi::sexp_booleanp(item) != 0 {
                        desc.push_str("bool");
                    } else {
                        desc.push_str("other");
                    }
                    current = crate::ffi::sexp_cdr(current);
                }
                let c_str = std::ffi::CString::new(desc.as_str()).unwrap();
                crate::ffi::sexp_c_str(ctx, c_str.as_ptr(), desc.len() as crate::ffi::sexp_sint_t)
            }
        }

        let ctx = Context::new().expect("create context");
        ctx.define_fn_variadic("describe-types", describe_types)
            .expect("define fn");
        let result = ctx
            .evaluate(r#"(describe-types 1 "hello" #t 42)"#)
            .expect("eval");
        assert_eq!(result, Value::String("int str bool int".to_string()));
    }

    // --- phase 1: builder + step limits ---

    #[test]
    fn test_builder_default() {
        let ctx = Context::builder().build().expect("builder default");
        let result = ctx.evaluate("(+ 1 2 3)").expect("should work");
        assert_eq!(result, Value::Integer(6));
    }

    #[test]
    fn test_builder_custom_heap() {
        let ctx = Context::builder()
            .heap_size(8 * 1024 * 1024)
            .heap_max(64 * 1024 * 1024)
            .build()
            .expect("builder custom heap");
        let result = ctx.evaluate("(+ 1 1)").expect("should work");
        assert_eq!(result, Value::Integer(2));
    }

    #[test]
    fn test_step_limit_infinite_loop() {
        let ctx = Context::builder()
            .step_limit(1000)
            .build()
            .expect("builder");
        let err = ctx
            .evaluate("((lambda () (define (loop) (loop)) (loop)))")
            .unwrap_err();
        assert!(
            matches!(err, Error::StepLimitExceeded),
            "expected StepLimitExceeded, got: {}",
            err
        );
    }

    #[test]
    fn test_step_limit_short_computation_succeeds() {
        let ctx = Context::builder()
            .step_limit(100_000)
            .build()
            .expect("builder");
        let result = ctx.evaluate("(+ 1 2 3)").expect("should work");
        assert_eq!(result, Value::Integer(6));
    }

    #[test]
    fn test_no_step_limit_backwards_compat() {
        let ctx = Context::new().expect("context");
        let result = ctx
            .evaluate("(define (fib n) (if (< n 2) n (+ (fib (- n 1)) (fib (- n 2))))) (fib 10)")
            .expect("should work");
        assert_eq!(result, Value::Integer(55));
    }

    #[test]
    fn test_fuel_resets_between_evaluations() {
        let ctx = Context::builder()
            .step_limit(100_000)
            .build()
            .expect("builder");
        let r1 = ctx.evaluate("(+ 1 2)").expect("first");
        assert_eq!(r1, Value::Integer(3));
        let r2 = ctx.evaluate("(* 3 4)").expect("second");
        assert_eq!(r2, Value::Integer(12));
    }

    #[test]
    fn test_call_respects_step_limit() {
        let ctx = Context::builder()
            .step_limit(1000)
            .build()
            .expect("builder");
        let looper = ctx
            .evaluate("(lambda () ((lambda () (define (loop) (loop)) (loop))))")
            .expect("lambda");
        let err = ctx.call(&looper, &[]).unwrap_err();
        assert!(
            matches!(err, Error::StepLimitExceeded),
            "expected StepLimitExceeded, got: {}",
            err
        );
    }

    // --- phase 2: restricted environments + presets ---

    #[test]
    fn test_arithmetic_only_env() {
        let ctx = Context::builder()
            .preset(&crate::sandbox::ARITHMETIC)
            .build()
            .expect("builder");
        let result = ctx.evaluate("(+ 1 2)").expect("should work");
        assert_eq!(result, Value::Integer(3));
        let err = ctx.evaluate("(cons 1 2)");
        assert!(
            err.is_err(),
            "cons should be undefined in arithmetic-only env"
        );
    }

    #[test]
    fn test_syntax_forms_always_available() {
        let ctx = Context::builder()
            .preset(&crate::sandbox::ARITHMETIC)
            .build()
            .expect("builder");
        let result = ctx
            .evaluate("(define x 5) (if #t (+ x 1) 0)")
            .expect("should work");
        assert_eq!(result, Value::Integer(6));

        let result = ctx
            .evaluate("((lambda (a b) (+ a b)) 3 4)")
            .expect("lambda");
        assert_eq!(result, Value::Integer(7));

        let result = ctx.evaluate("(begin (+ 1 1) (+ 2 2))").expect("begin");
        assert_eq!(result, Value::Integer(4));

        let result = ctx.evaluate("(quote hello)").expect("quote");
        assert_eq!(result, Value::Symbol("hello".into()));
    }

    #[test]
    fn test_preset_composition() {
        let ctx = Context::builder()
            .preset(&crate::sandbox::ARITHMETIC)
            .preset(&crate::sandbox::LISTS)
            .build()
            .expect("builder");
        let result = ctx.evaluate("(+ 1 2)").expect("arithmetic");
        assert_eq!(result, Value::Integer(3));
        let result = ctx.evaluate("(car (cons 1 2))").expect("lists");
        assert_eq!(result, Value::Integer(1));
    }

    #[test]
    fn test_allow_individual_primitives() {
        let ctx = Context::builder()
            .allow(&["+", "-"])
            .build()
            .expect("builder");
        let result = ctx.evaluate("(+ 10 (- 5 3))").expect("should work");
        assert_eq!(result, Value::Integer(12));
        let err = ctx.evaluate("(* 2 3)");
        assert!(err.is_err(), "* should be undefined");
    }

    #[test]
    fn test_no_preset_full_env() {
        let ctx = Context::builder().build().expect("builder");
        let result = ctx
            .evaluate("(cons 1 (cons 2 (quote ())))")
            .expect("should work");
        assert_eq!(
            result,
            Value::List(vec![Value::Integer(1), Value::Integer(2)])
        );
    }

    #[test]
    fn test_pure_computation_convenience() {
        let ctx = Context::builder()
            .pure_computation()
            .build()
            .expect("builder");
        let r = ctx.evaluate("(+ 1 2)").expect("arithmetic");
        assert_eq!(r, Value::Integer(3));
        let r = ctx.evaluate("(car (cons 1 2))").expect("lists");
        assert_eq!(r, Value::Integer(1));
        let r = ctx.evaluate("(string? \"hello\")").expect("strings");
        assert_eq!(r, Value::Boolean(true));
    }

    #[test]
    fn test_safe_convenience() {
        let ctx = Context::builder().safe().build().expect("builder");
        let r = ctx
            .evaluate("(define x (cons 1 2)) (set-car! x 99) (car x)")
            .expect("should work");
        assert_eq!(r, Value::Integer(99));
    }

    #[test]
    fn test_foreign_fn_works_in_restricted_env() {
        unsafe extern "C" fn add100(
            _ctx: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            args: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let n = crate::ffi::sexp_unbox_fixnum(crate::ffi::sexp_car(args));
                crate::ffi::sexp_make_fixnum(n + 100)
            }
        }

        let ctx = Context::builder()
            .preset(&crate::sandbox::ARITHMETIC)
            .build()
            .expect("builder");
        ctx.define_fn_variadic("add100", add100).expect("define fn");
        let result = ctx.evaluate("(add100 5)").expect("should work");
        assert_eq!(result, Value::Integer(105));
    }

    #[test]
    fn test_file_io_absent_in_safe_preset() {
        let ctx = Context::builder().safe().build().expect("builder");
        let err = ctx.evaluate("(open-input-file \"/etc/passwd\")");
        assert!(err.is_err(), "file io should be unavailable in safe preset");
    }

    // --- phase 3: timeout context ---

    #[test]
    fn test_timeout_basic() {
        let ctx = Context::builder()
            .step_limit(1_000_000)
            .build_timeout(std::time::Duration::from_secs(5))
            .expect("build_timeout");
        let result = ctx.evaluate("(+ 1 2 3)").expect("should work");
        assert_eq!(result, Value::Integer(6));
    }

    #[test]
    fn test_timeout_infinite_loop() {
        let ctx = Context::builder()
            .step_limit(10_000)
            .build_timeout(std::time::Duration::from_millis(500))
            .expect("build_timeout");
        let err = ctx
            .evaluate("((lambda () (define (loop) (loop)) (loop)))")
            .unwrap_err();
        assert!(
            matches!(err, Error::Timeout | Error::StepLimitExceeded),
            "expected Timeout or StepLimitExceeded, got: {}",
            err
        );
    }

    #[test]
    fn test_timeout_multiple_sequential() {
        let ctx = Context::builder()
            .step_limit(1_000_000)
            .build_timeout(std::time::Duration::from_secs(5))
            .expect("build_timeout");
        let r1 = ctx.evaluate("(+ 1 2)").expect("first");
        let r2 = ctx.evaluate("(* 3 4)").expect("second");
        assert_eq!(r1, Value::Integer(3));
        assert_eq!(r2, Value::Integer(12));
    }

    #[test]
    fn test_timeout_state_persists() {
        let ctx = Context::builder()
            .step_limit(1_000_000)
            .build_timeout(std::time::Duration::from_secs(5))
            .expect("build_timeout");
        ctx.evaluate("(define x 42)").expect("define");
        let result = ctx.evaluate("x").expect("lookup");
        assert_eq!(result, Value::Integer(42));
    }

    #[test]
    fn test_build_timeout_without_step_limit_fails() {
        let err = Context::builder()
            .build_timeout(std::time::Duration::from_secs(1))
            .unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("step_limit"),
            "expected step_limit error, got: {}",
            msg
        );
    }

    #[test]
    fn test_timeout_drop_cleans_up() {
        let ctx = Context::builder()
            .step_limit(1_000_000)
            .build_timeout(std::time::Duration::from_secs(5))
            .expect("build_timeout");
        let _ = ctx.evaluate("(+ 1 1)");
        drop(ctx);
    }
}
