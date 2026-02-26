//! managed context example — persistent and fresh modes
//!
//! demonstrates ThreadLocalContext with init closures, state
//! accumulation (persistent mode), and clean rebuilds (fresh mode).

use tein::Context;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // --- persistent mode ---
    println!("=== persistent mode ===");

    let ctx = Context::builder()
        .standard_env()
        .step_limit(1_000_000)
        .build_managed(|ctx| {
            ctx.evaluate("(define counter 0)")?;
            Ok(())
        })?;

    // state accumulates across calls
    ctx.evaluate("(set! counter (+ counter 1))")?;
    ctx.evaluate("(set! counter (+ counter 1))")?;
    let result = ctx.evaluate("counter")?;
    println!("counter after 2 increments: {}", result);

    // reset clears state and re-runs init
    ctx.reset()?;
    let result = ctx.evaluate("counter")?;
    println!("counter after reset: {}", result);

    // --- fresh mode ---
    println!("\n=== fresh mode ===");

    let ctx = Context::builder()
        .step_limit(1_000_000)
        .build_managed_fresh(|_ctx| {
            println!("  (init closure running)");
            Ok(())
        })?;

    // each evaluate gets a fresh context
    let r1 = ctx.evaluate("(+ 1 2)")?;
    let r2 = ctx.evaluate("(* 3 4)")?;
    println!("1 + 2 = {}", r1);
    println!("3 * 4 = {}", r2);

    println!("\ndone!");
    Ok(())
}
