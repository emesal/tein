//! scheme evaluation context

use crate::{
    Value,
    error::{Error, Result},
    ffi,
};
use std::ffi::CString;
use std::path::Path;

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
}

impl Context {
    /// create a new scheme context with default settings
    ///
    /// initializes a chibi-scheme context with:
    /// - 4mb initial heap
    /// - 128mb max heap
    /// - 1mb stack
    /// - r7rs standard environment
    pub fn new() -> Result<Self> {
        Self::with_sizes(4 * 1024 * 1024, 128 * 1024 * 1024, 1024 * 1024)
    }

    /// create a new scheme context with custom memory settings
    ///
    /// # arguments
    /// * `heap_size` - initial heap size in bytes
    /// * `heap_max` - maximum heap size in bytes
    /// * `_stack_size` - stack size in bytes (unused in chibi's current api)
    pub fn with_sizes(heap_size: usize, heap_max: usize, _stack_size: usize) -> Result<Self> {
        unsafe {
            let ctx = ffi::sexp_make_eval_context(
                std::ptr::null_mut(), // parent context
                std::ptr::null_mut(), // stack
                std::ptr::null_mut(), // env
                heap_size as ffi::sexp_uint_t,
                heap_max as ffi::sexp_uint_t,
            );

            if ctx.is_null() {
                return Err(Error::InitError("failed to create context".to_string()));
            }

            // note: we're not loading r7rs environment for now
            // this gives us a minimal scheme with basic primitives
            // TODO: figure out static library setup for r7rs

            Ok(Context { ctx })
        }
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

    /// register a 0-argument foreign function as a scheme primitive
    ///
    /// chibi calls with `(ctx, self, 0)`.
    pub fn define_fn0(
        &self,
        name: &str,
        f: unsafe extern "C" fn(ffi::sexp, ffi::sexp, ffi::sexp_sint_t) -> ffi::sexp,
    ) -> Result<()> {
        self.define_foreign(name, 0, f as *const std::ffi::c_void)
    }

    /// register a 1-argument foreign function as a scheme primitive
    ///
    /// chibi calls with `(ctx, self, 1, arg1)`.
    pub fn define_fn1(
        &self,
        name: &str,
        f: unsafe extern "C" fn(ffi::sexp, ffi::sexp, ffi::sexp_sint_t, ffi::sexp) -> ffi::sexp,
    ) -> Result<()> {
        self.define_foreign(name, 1, f as *const std::ffi::c_void)
    }

    /// register a 2-argument foreign function as a scheme primitive
    ///
    /// chibi calls with `(ctx, self, 2, arg1, arg2)`.
    pub fn define_fn2(
        &self,
        name: &str,
        f: unsafe extern "C" fn(
            ffi::sexp,
            ffi::sexp,
            ffi::sexp_sint_t,
            ffi::sexp,
            ffi::sexp,
        ) -> ffi::sexp,
    ) -> Result<()> {
        self.define_foreign(name, 2, f as *const std::ffi::c_void)
    }

    /// register a 3-argument foreign function as a scheme primitive
    ///
    /// chibi calls with `(ctx, self, 3, arg1, arg2, arg3)`.
    pub fn define_fn3(
        &self,
        name: &str,
        f: unsafe extern "C" fn(
            ffi::sexp,
            ffi::sexp,
            ffi::sexp_sint_t,
            ffi::sexp,
            ffi::sexp,
            ffi::sexp,
        ) -> ffi::sexp,
    ) -> Result<()> {
        self.define_foreign(name, 3, f as *const std::ffi::c_void)
    }

    /// internal: register a foreign function with chibi
    fn define_foreign(&self, name: &str, num_args: i32, f: *const std::ffi::c_void) -> Result<()> {
        let c_name = CString::new(name)
            .map_err(|_| Error::EvalError("function name contains null bytes".to_string()))?;

        unsafe {
            let env = ffi::sexp_context_env(self.ctx);
            // chibi stores the function pointer as sexp_proc1 and casts at call time
            // based on the opcode's num_args — this is safe because we match arity.
            // the transmute converts *const c_void to the expected Option<extern "C" fn>
            // that chibi's FFI registration expects.
            let f_typed: Option<
                unsafe extern "C" fn(ffi::sexp, ffi::sexp, ffi::sexp_sint_t) -> ffi::sexp,
            > = std::mem::transmute::<*const std::ffi::c_void, _>(f);
            let result = ffi::sexp_define_foreign(
                self.ctx,
                env,
                c_name.as_ptr(),
                num_args as std::os::raw::c_int,
                c_name.as_ptr(),
                f_typed,
            );

            if ffi::sexp_exceptionp(result) != 0 {
                return Err(Error::EvalError(format!(
                    "failed to define foreign function '{}'",
                    name
                )));
            }
        }

        Ok(())
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

    // --- foreign functions ---

    #[test]
    fn test_define_fn1() {
        unsafe extern "C" fn add_forty_two(
            _ctx: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            arg: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let n = crate::ffi::sexp_unbox_fixnum(arg);
                crate::ffi::sexp_make_fixnum(n + 42)
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn1("add42", add_forty_two)
            .expect("failed to define fn");
        let result = ctx.evaluate("(add42 8)").expect("failed to evaluate");
        match result {
            Value::Integer(n) => assert_eq!(n, 50),
            _ => panic!("expected integer, got {:?}", result),
        }
    }

    #[test]
    fn test_define_fn0_string() {
        unsafe extern "C" fn hello(
            ctx: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
        ) -> crate::ffi::sexp {
            unsafe {
                let s = "hello from rust";
                let c_str = std::ffi::CString::new(s).unwrap();
                crate::ffi::sexp_c_str(ctx, c_str.as_ptr(), s.len() as crate::ffi::sexp_sint_t)
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn0("hello", hello).expect("failed to define fn");
        let result = ctx.evaluate("(hello)").expect("failed to evaluate");
        match result {
            Value::String(s) => assert_eq!(s, "hello from rust"),
            _ => panic!("expected string, got {:?}", result),
        }
    }

    #[test]
    fn test_define_fn2() {
        unsafe extern "C" fn multiply(
            _ctx: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            a: crate::ffi::sexp,
            b: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let x = crate::ffi::sexp_unbox_fixnum(a);
                let y = crate::ffi::sexp_unbox_fixnum(b);
                crate::ffi::sexp_make_fixnum(x * y)
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn2("rust-mul", multiply)
            .expect("failed to define fn");
        let result = ctx.evaluate("(rust-mul 6 7)").expect("failed to evaluate");
        match result {
            Value::Integer(n) => assert_eq!(n, 42),
            _ => panic!("expected integer, got {:?}", result),
        }
    }

    #[test]
    fn test_foreign_fn_uses_scheme_values() {
        // a foreign fn that squares a number, then scheme uses it in an expression
        unsafe extern "C" fn square(
            _ctx: crate::ffi::sexp,
            _self: crate::ffi::sexp,
            _n: crate::ffi::sexp_sint_t,
            arg: crate::ffi::sexp,
        ) -> crate::ffi::sexp {
            unsafe {
                let n = crate::ffi::sexp_unbox_fixnum(arg);
                crate::ffi::sexp_make_fixnum(n * n)
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn1("square", square)
            .expect("failed to define fn");
        let result = ctx
            .evaluate("(+ (square 3) (square 4))")
            .expect("failed to evaluate");
        match result {
            Value::Integer(n) => assert_eq!(n, 25), // 9 + 16
            _ => panic!("expected integer, got {:?}", result),
        }
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
    fn test_as_integer() {
        let v = Value::Integer(42);
        assert_eq!(v.as_integer(), Some(42));
        assert_eq!(Value::Float(3.14).as_integer(), None);
    }

    #[test]
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
        ctx.define_fn0("get-test-list", get_test_list)
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
        ctx.define_fn0("get-test-pair", get_test_pair)
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
        ) -> crate::ffi::sexp {
            unsafe {
                let vec = Value::Vector(vec![Value::String("a".into()), Value::String("b".into())]);
                vec.to_raw(ctx_ptr)
                    .unwrap_or_else(|_| crate::ffi::get_void())
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn0("get-test-vector", get_test_vector)
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
        ctx.define_fn0("get-nested-list", get_nested_list)
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
        ) -> crate::ffi::sexp {
            unsafe {
                let empty = Value::List(vec![]);
                empty
                    .to_raw(ctx_ptr)
                    .unwrap_or_else(|_| crate::ffi::get_void())
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn0("get-empty-list", get_empty_list)
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
        ) -> crate::ffi::sexp {
            unsafe {
                let empty = Value::Vector(vec![]);
                empty
                    .to_raw(ctx_ptr)
                    .unwrap_or_else(|_| crate::ffi::get_void())
            }
        }

        let ctx = Context::new().expect("failed to create context");
        ctx.define_fn0("get-empty-vector", get_empty_vector)
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
}
