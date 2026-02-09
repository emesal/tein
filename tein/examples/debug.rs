use tein::Context;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = Context::new()?;

    // test individual floats
    println!("==> 2.5");
    let a = ctx.evaluate("2.5")?;
    println!("    {} ({:?})", a, a);

    println!("\n==> 4.0");
    let b = ctx.evaluate("4.0")?;
    println!("    {} ({:?})", b, b);

    // test multiplication
    println!("\n==> (* 2.5 4.0)");
    let result = ctx.evaluate("(* 2.5 4.0)")?;
    println!("    {} ({:?})", result, result);

    // check if it's being read as integer
    println!("\n==> 10");
    let c = ctx.evaluate("10")?;
    println!("    {} ({:?})", c, c);

    Ok(())
}
