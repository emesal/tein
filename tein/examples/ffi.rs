// foreign function example — using #[tein_fn] proc macro

use tein::{Context, Value, tein_fn};

// 0 args: returns a greeting string
#[tein_fn]
fn greet() -> String {
    "hail from rust!".to_string()
}

// 1 arg: squares an integer
#[tein_fn]
fn square(n: i64) -> i64 {
    n * n
}

// 2 args: adds two integers
#[tein_fn]
fn rust_add(a: i64, b: i64) -> i64 {
    a + b
}

// error handling: safe division
#[tein_fn]
fn safe_div(a: i64, b: i64) -> Result<i64, String> {
    if b == 0 {
        Err("division by zero".to_string())
    } else {
        Ok(a / b)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = Context::new()?;

    // register rust functions in scheme using #[tein_fn] wrappers
    ctx.define_fn_variadic("greet", __tein_greet)?;
    ctx.define_fn_variadic("square", __tein_square)?;
    ctx.define_fn_variadic("rust-add", __tein_rust_add)?;
    ctx.define_fn_variadic("safe-div", __tein_safe_div)?;

    println!("--- rust→scheme ffi (via #[tein_fn]) ---\n");

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

    println!("\n==> (safe-div 10 3)");
    let result = ctx.evaluate("(safe-div 10 3)")?;
    println!("    {}", result);

    println!("\n==> (safe-div 10 0)");
    let result = ctx.evaluate("(safe-div 10 0)")?;
    println!("    {} ; error propagated!", result);

    println!("\n--- procedures as values ---\n");

    // get a scheme lambda and call it from rust
    let square_fn = ctx.evaluate("(lambda (x) (* x x))")?;
    println!("==> (lambda (x) (* x x))");
    println!("    {} ; callable from rust!", square_fn);

    let result = ctx.call(&square_fn, &[Value::Integer(12)])?;
    println!("\n==> call from rust with arg 12");
    println!("    {}", result);

    // get a builtin and call it
    let plus = ctx.evaluate("+")?;
    let result = ctx.call(&plus, &[Value::Integer(100), Value::Integer(200)])?;
    println!("\n==> call builtin + from rust with (100, 200)");
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
