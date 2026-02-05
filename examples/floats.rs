use tein::Context;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = Context::new()?;

    println!("==> 3.14");
    let result = ctx.evaluate("3.14")?;
    println!("    {}", result);

    println!("\n==> (* 2.5 4.0)");
    let result = ctx.evaluate("(* 2.5 4.0)")?;
    println!("    {}", result);

    println!("\n==> (/ 22.0 7.0)");
    let result = ctx.evaluate("(/ 22.0 7.0)")?;
    println!("    {}", result);

    Ok(())
}
