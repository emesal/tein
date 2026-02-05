// basic example of using tein

use tein::{Context, Value};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // create a scheme context
    let ctx = Context::new()?;

    // evaluate some expressions
    println!("==> (+ 1 2 3)");
    let result = ctx.evaluate("(+ 1 2 3)")?;
    println!("    {}", result);

    println!("\n==> (* 6 7)");
    let result = ctx.evaluate("(* 6 7)")?;
    println!("    {}", result);

    println!("\n==> (- 100 42)");
    let result = ctx.evaluate("(- 100 42)")?;
    println!("    {}", result);

    println!("\n==> \"hello from scheme!\"");
    let result = ctx.evaluate(r#""hello from scheme!""#)?;
    println!("    {}", result);

    println!("\n==> #t");
    let result = ctx.evaluate("#t")?;
    println!("    {}", result);

    println!("\n==> (quote (a b c))");
    let result = ctx.evaluate("(quote (a b c))")?;
    println!("    {}", result);

    println!("\n==> (quote (1 2 3))");
    let result = ctx.evaluate("(quote (1 2 3))")?;
    println!("    {}", result);

    println!("\n==> (quote ())");
    let result = ctx.evaluate("(quote ())")?;
    println!("    {}", result);

    println!("\n==> (quote (nested (list (structure))))");
    let result = ctx.evaluate("(quote (nested (list (structure))))")?;
    println!("    {}", result);

    println!("\n==> (cons 1 2) ; improper list");
    let result = ctx.evaluate("(cons 1 2)")?;
    println!("    {}", result);

    println!("\n==> (cons 1 (cons 2 3)) ; improper list");
    let result = ctx.evaluate("(cons 1 (cons 2 3))")?;
    println!("    {}", result);

    // pattern matching on values
    println!("\n==> Type checking:");
    let num = ctx.evaluate("42")?;
    match num {
        Value::Integer(n) => println!("    Got integer: {}", n),
        _ => println!("    Unexpected type!"),
    }

    let list = ctx.evaluate("(quote (x y z))")?;
    match list {
        Value::List(items) => println!("    Got list with {} items", items.len()),
        _ => println!("    Unexpected type!"),
    }

    Ok(())
}
