//! scheme evaluation context

use crate::{error::{Error, Result}, ffi, Value};
use std::ffi::CString;

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

    /// evaluate a scheme expression string
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
    pub fn evaluate(&self, code: &str) -> Result<Value> {
        let c_str = CString::new(code).map_err(|_| {
            Error::EvalError("code contains null bytes".to_string())
        })?;

        unsafe {
            let env = ffi::sexp_context_env(self.ctx);
            let result = ffi::sexp_eval_string(
                self.ctx,
                c_str.as_ptr(),
                code.len() as ffi::sexp_sint_t,
                env,
            );

            Value::from_raw(self.ctx, result)
        }
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
        f: unsafe extern "C" fn(ffi::sexp, ffi::sexp, ffi::sexp_sint_t, ffi::sexp, ffi::sexp) -> ffi::sexp,
    ) -> Result<()> {
        self.define_foreign(name, 2, f as *const std::ffi::c_void)
    }

    /// register a 3-argument foreign function as a scheme primitive
    ///
    /// chibi calls with `(ctx, self, 3, arg1, arg2, arg3)`.
    pub fn define_fn3(
        &self,
        name: &str,
        f: unsafe extern "C" fn(ffi::sexp, ffi::sexp, ffi::sexp_sint_t, ffi::sexp, ffi::sexp, ffi::sexp) -> ffi::sexp,
    ) -> Result<()> {
        self.define_foreign(name, 3, f as *const std::ffi::c_void)
    }

    /// internal: register a foreign function with chibi
    fn define_foreign(&self, name: &str, num_args: i32, f: *const std::ffi::c_void) -> Result<()> {
        let c_name = CString::new(name).map_err(|_| {
            Error::EvalError("function name contains null bytes".to_string())
        })?;

        unsafe {
            let env = ffi::sexp_context_env(self.ctx);
            // chibi stores the function pointer as sexp_proc1 and casts at call time
            // based on the opcode's num_args — this is safe because we match arity.
            // the transmute converts *const c_void to the expected Option<extern "C" fn>
            // that chibi's FFI registration expects.
            let f_typed: Option<unsafe extern "C" fn(ffi::sexp, ffi::sexp, ffi::sexp_sint_t) -> ffi::sexp> =
                std::mem::transmute::<*const std::ffi::c_void, _>(f);
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
                    "failed to define foreign function '{}'", name
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

    #[test]
    fn test_string_evaluation() {
        let ctx = Context::new().expect("failed to create context");
        let result = ctx.evaluate(r#""hello world""#).expect("failed to evaluate");
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
        let result = ctx.evaluate("(quote (a (b c) d))").expect("failed to evaluate");
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
        let result = ctx.evaluate("(make-vector 3 0)").expect("failed to evaluate");
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
        let result = ctx.evaluate("(make-vector 0 #f)").expect("failed to evaluate");
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
            "error message too generic: {}", msg
        );
    }

    #[test]
    fn test_error_on_undefined() {
        let ctx = Context::new().expect("failed to create context");
        let err = ctx.evaluate("undefined-variable").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("undefined"), "expected 'undefined' in: {}", msg);
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
        ctx.define_fn1("add42", add_forty_two).expect("failed to define fn");
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
        ctx.define_fn2("rust-mul", multiply).expect("failed to define fn");
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
        ctx.define_fn1("square", square).expect("failed to define fn");
        let result = ctx.evaluate("(+ (square 3) (square 4))").expect("failed to evaluate");
        match result {
            Value::Integer(n) => assert_eq!(n, 25), // 9 + 16
            _ => panic!("expected integer, got {:?}", result),
        }
    }

    // --- value display ---

    #[test]
    fn test_display_roundtrip() {
        let cases = [
            (Value::Integer(42), "42"),
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
}
