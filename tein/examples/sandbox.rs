// sandboxing example — module sets, step limits, timeouts, file IO policies

use std::time::Duration;
use tein::{Context, Error, sandbox::Modules};

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

    println!("\n--- module-based sandboxing ---\n");

    // Modules::Safe: conservative safe set — scheme/base, scheme/write, scheme/read,
    // srfi/*, tein/* (excluding eval/repl/process). import only reads from VFS.
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::Safe)
        .build()?;

    println!("==> (import (scheme base)) (+ 1 2) in Modules::Safe");
    ctx.evaluate("(import (scheme base))")?;
    let result = ctx.evaluate("(+ 1 2)")?;
    println!("    {}", result);

    println!("\n==> (import (scheme eval)) blocked in Modules::Safe");
    match ctx.evaluate("(import (scheme eval))") {
        Err(e) => println!("    blocked: {}", e),
        Ok(v) => println!("    unexpected: {}", v),
    }

    // Modules::Only: explicit module list with transitive deps resolved
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::only(&["scheme/base"]))
        .build()?;

    println!("\n==> (import (scheme base)) (define (sq x) (* x x)) (sq 7) in Modules::only");
    ctx.evaluate("(import (scheme base))")?;
    let result = ctx.evaluate("(define (sq x) (* x x)) (sq 7)")?;
    println!("    {}", result);

    println!("\n==> (cons 1 2) after scheme/base import");
    let result = ctx.evaluate("(cons 1 2)")?;
    println!("    {}", result);

    // Modules::None: syntax + import only. UX stubs inform about missing bindings.
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::None)
        .build()?;

    println!("\n==> (+ 1 2) in Modules::None — UX stub hints at providing module");
    match ctx.evaluate("(+ 1 2)") {
        Err(e) => println!("    blocked: {}", e),
        Ok(v) => println!("    unexpected: {}", v),
    }

    println!("\n--- combining limits + sandboxing ---\n");

    // step limit + Modules::Safe: import first (VFS loading costs many steps),
    // then the step limit guards user computation.
    // tip: set a generous limit to cover module loading, tighten for eval-time.
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::Safe)
        .step_limit(5_000_000)
        .build()?;
    ctx.evaluate("(import (scheme base))")?;

    println!("==> mutation works in Modules::Safe after import");
    let result = ctx.evaluate("(define x (cons 1 2)) (set-car! x 99) (car x)")?;
    println!("    {}", result);

    println!("\n==> file io not accessible without file_read() policy");
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
    // open-input-file wrapper is injected directly — no (scheme file) import needed
    let ctx = Context::builder()
        .standard_env()
        .sandboxed(Modules::Safe)
        .file_read(&[canon_str])
        .step_limit(5_000_000)
        .build()?;

    // write a test file from rust, then read it from scheme
    let test_file = io_dir.join("greeting.txt");
    std::fs::write(&test_file, "hello-from-tein")?;

    println!("==> reading file from allowed path");
    ctx.evaluate("(import (scheme base))")?;
    ctx.evaluate("(import (scheme read))")?;
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
        .standard_env()
        .sandboxed(Modules::Safe)
        .file_write(&[canon_str])
        .step_limit(5_000_000)
        .build()?;

    ctx.evaluate("(import (scheme base))")?;
    let output_file = io_dir.join("output.txt");
    println!("\n==> writing file to allowed path");
    let code = format!(
        r#"(define p (open-output-file "{}")) (write-char #\X p) (close-output-port p)"#,
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
