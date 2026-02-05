// foreign function example — registering rust functions in scheme

use tein::{Context, raw};

// 0 args: returns a greeting string
unsafe extern "C" fn greet(ctx: raw::sexp, _self: raw::sexp, _n: raw::sexp_sint_t) -> raw::sexp {
    unsafe {
        let s = "hail from rust!";
        let c_str = std::ffi::CString::new(s).unwrap();
        raw::sexp_c_str(ctx, c_str.as_ptr(), s.len() as raw::sexp_sint_t)
    }
}

// 1 arg: squares an integer
unsafe extern "C" fn square(
    _ctx: raw::sexp,
    _self: raw::sexp,
    _n: raw::sexp_sint_t,
    arg: raw::sexp,
) -> raw::sexp {
    unsafe {
        let n = raw::sexp_unbox_fixnum(arg);
        raw::sexp_make_fixnum(n * n)
    }
}

// 2 args: adds two integers
unsafe extern "C" fn rust_add(
    _ctx: raw::sexp,
    _self: raw::sexp,
    _n: raw::sexp_sint_t,
    a: raw::sexp,
    b: raw::sexp,
) -> raw::sexp {
    unsafe {
        let x = raw::sexp_unbox_fixnum(a);
        let y = raw::sexp_unbox_fixnum(b);
        raw::sexp_make_fixnum(x + y)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = Context::new()?;

    // register rust functions in scheme
    ctx.define_fn0("greet", greet)?;
    ctx.define_fn1("square", square)?;
    ctx.define_fn2("rust-add", rust_add)?;

    println!("--- rust→scheme ffi ---\n");

    println!("==> (greet)");
    let result = ctx.evaluate("(greet)")?;
    println!("    {}", result);

    println!("\n==> (square 7)");
    let result = ctx.evaluate("(square 7)")?;
    println!("    {}", result);

    println!("\n==> (+ (square 3) (square 4))");
    let result = ctx.evaluate("(+ (square 3) (square 4))")?;
    println!("    {} ; pythagorean!", result);

    println!("\n==> (rust-add 100 200)");
    let result = ctx.evaluate("(rust-add 100 200)")?;
    println!("    {}", result);

    println!("\n--- vectors ---\n");

    println!("==> (make-vector 5 0)");
    let result = ctx.evaluate("(make-vector 5 0)")?;
    println!("    {}", result);

    println!("\n--- error messages ---\n");

    println!("==> (car 42)");
    match ctx.evaluate("(car 42)") {
        Ok(val) => println!("    {}", val),
        Err(e) => println!("    {}", e),
    }

    println!("\n==> undefined-sym");
    match ctx.evaluate("undefined-sym") {
        Ok(val) => println!("    {}", val),
        Err(e) => println!("    {}", e),
    }

    Ok(())
}
