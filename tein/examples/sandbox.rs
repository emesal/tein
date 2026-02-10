// sandboxing example — step limits, restricted environments, timeouts

use std::time::Duration;
use tein::{Context, Error};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("--- step limits ---\n");

    // a context with a step limit terminates infinite loops
    let ctx = Context::builder().step_limit(5_000).build()?;

    println!("==> (+ 1 2 3) with 5000 step limit");
    let result = ctx.evaluate("(+ 1 2 3)")?;
    println!("    {}\n", result);

    println!("==> infinite loop with 5000 step limit");
    match ctx.evaluate("((lambda () (define (loop) (loop)) (loop)))") {
        Err(Error::StepLimitExceeded) => println!("    caught: step limit exceeded"),
        other => println!("    unexpected: {:?}", other),
    }

    println!("\n--- restricted environments ---\n");

    // arithmetic-only: no list ops, no io, no mutation
    let ctx = Context::builder()
        .preset(&tein::sandbox::ARITHMETIC)
        .build()?;

    println!("==> (+ 1 2) in arithmetic-only env");
    let result = ctx.evaluate("(+ 1 2)")?;
    println!("    {}", result);

    println!("\n==> (cons 1 2) in arithmetic-only env");
    match ctx.evaluate("(cons 1 2)") {
        Err(e) => println!("    blocked: {}", e),
        Ok(v) => println!("    unexpected: {}", v),
    }

    // core syntax always available, even in restricted envs
    println!("\n==> (define (sq x) (* x x)) (sq 7) in arithmetic-only env");
    let result = ctx.evaluate("(define (sq x) (* x x)) (sq 7)")?;
    println!("    {}", result);

    // pure_computation: arithmetic + math + lists + vectors + strings + chars + predicates
    let ctx = Context::builder().pure_computation().build()?;

    println!("\n==> (car (cons 1 2)) in pure_computation env");
    let result = ctx.evaluate("(car (cons 1 2))")?;
    println!("    {}", result);

    println!("\n==> (string? \"hello\") in pure_computation env");
    let result = ctx.evaluate("(string? \"hello\")")?;
    println!("    {}", result);

    println!("\n--- combining limits + restriction ---\n");

    // step limit + safe preset: can't escape, can't loop forever
    let ctx = Context::builder().safe().step_limit(50_000).build()?;

    println!("==> (define x (cons 1 2)) (set-car! x 99) (car x) in safe env");
    let result = ctx.evaluate("(define x (cons 1 2)) (set-car! x 99) (car x)")?;
    println!("    {}", result);

    println!("\n==> file io blocked in safe env");
    match ctx.evaluate("(open-input-file \"/etc/passwd\")") {
        Err(e) => println!("    blocked: {}", e),
        Ok(v) => println!("    unexpected: {}", v),
    }

    println!("\n--- wall-clock timeout ---\n");

    // TimeoutContext: dedicated thread with wall-clock deadline
    let ctx = Context::builder()
        .step_limit(1_000_000)
        .build_timeout(Duration::from_secs(5))?;

    println!("==> (+ 40 2) via TimeoutContext");
    let result = ctx.evaluate("(+ 40 2)")?;
    println!("    {}", result);

    // state persists between evaluations
    ctx.evaluate("(define answer 42)")?;
    let result = ctx.evaluate("answer")?;
    println!("\n==> state persists: answer = {}", result);

    println!("\n==> infinite loop with 500ms timeout");
    let ctx = Context::builder()
        .step_limit(10_000_000)
        .build_timeout(Duration::from_millis(500))?;

    match ctx.evaluate("((lambda () (define (loop) (loop)) (loop)))") {
        Err(Error::Timeout) => println!("    caught: evaluation timed out"),
        Err(Error::StepLimitExceeded) => println!("    caught: step limit exceeded"),
        other => println!("    unexpected: {:?}", other),
    }

    println!("\n--- parameterised file IO ---\n");

    // create a temp directory for the IO demo
    let io_dir = std::env::temp_dir().join("tein-sandbox-demo");
    std::fs::create_dir_all(&io_dir)?;
    let canon_dir = io_dir.canonicalize()?;
    let canon_str = canon_dir.to_str().unwrap();

    // file_read: allow reading only from our temp dir
    let ctx = Context::builder()
        .safe()
        .file_read(&[canon_str])
        .step_limit(100_000)
        .build()?;

    // write a test file from rust, then read it from scheme
    let test_file = io_dir.join("greeting.txt");
    std::fs::write(&test_file, "hello-from-tein")?;

    println!("==> reading file from allowed path");
    let code = format!(
        r#"(define p (open-input-file "{}")) (define r (read p)) (close-input-port p) r"#,
        test_file.display()
    );
    let result = ctx.evaluate(&code)?;
    println!("    read: {}", result);

    println!("\n==> reading file from denied path");
    match ctx.evaluate(r#"(open-input-file "/etc/passwd")"#) {
        Err(e) => println!("    blocked: {}", e),
        Ok(v) => println!("    unexpected: {}", v),
    }

    // file_write: allow writing only to our temp dir
    let ctx = Context::builder()
        .safe()
        .file_write(&[canon_str])
        .step_limit(100_000)
        .build()?;

    let output_file = io_dir.join("output.txt");
    println!("\n==> writing file to allowed path");
    let code = format!(
        r#"(define p (open-output-file "{}")) (write "hello" p) (close-output-port p)"#,
        output_file.display()
    );
    ctx.evaluate(&code)?;
    let contents = std::fs::read_to_string(&output_file)?;
    println!("    wrote: {}", contents);

    println!("\n==> writing file to denied path");
    match ctx.evaluate(r#"(open-output-file "/tmp/tein-nope.txt")"#) {
        Err(e) => println!("    blocked: {}", e),
        Ok(v) => println!("    unexpected: {}", v),
    }

    // cleanup
    std::fs::remove_dir_all(&io_dir).ok();

    println!("\ndone!");
    Ok(())
}
