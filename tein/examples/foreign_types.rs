// foreign type protocol — exposing rust types as first-class scheme objects
//
// demonstrates the full lifecycle: type definition, registration, construction,
// method dispatch (via convenience procs and universal foreign-call), introspection,
// and error handling.

use tein::{Context, ForeignType, MethodFn, Value};

// --- counter type ---
//
// a simple stateful counter. implement ForeignType to expose it to scheme.
// type_name() becomes the identity in error messages and predicates.
// methods() returns the dispatch table.

struct Counter {
    n: i64,
}

impl ForeignType for Counter {
    fn type_name() -> &'static str {
        "counter"
    }
    fn methods() -> &'static [(&'static str, MethodFn)] {
        &[
            ("increment", |obj, _ctx, args| {
                let c = obj.downcast_mut::<Counter>().unwrap();
                let step = match args.first() {
                    Some(Value::Integer(n)) => *n,
                    None => 1,
                    Some(v) => {
                        return Err(tein::Error::TypeError(format!(
                            "increment: expected integer step, got {}",
                            v
                        )));
                    }
                };
                c.n += step;
                Ok(Value::Integer(c.n))
            }),
            ("get", |obj, _ctx, _args| {
                let c = obj.downcast_ref::<Counter>().unwrap();
                Ok(Value::Integer(c.n))
            }),
            ("reset", |obj, _ctx, _args| {
                let c = obj.downcast_mut::<Counter>().unwrap();
                c.n = 0;
                Ok(Value::Unspecified)
            }),
        ]
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let ctx = Context::new_standard()?;

    // register the type — auto-generates: counter?, counter-increment,
    // counter-get, counter-reset convenience procs in the scheme env
    ctx.register_foreign_type::<Counter>()?;

    // for this demo, pass counter values from rust via foreign_value()
    // and demonstrate all the protocol operations
    println!("--- creating counter from rust ---\n");

    let c = ctx.foreign_value(Counter { n: 0 })?;
    println!("created:  {}", c);
    println!("type:     {:?}", c.foreign_type_name());
    println!("foreign?: {}", c.is_foreign());

    // borrow the inner value from rust
    {
        let inner = ctx.foreign_ref::<Counter>(&c)?;
        println!("n (rust): {}", inner.n);
    }

    println!("\n--- scheme method dispatch ---\n");

    // pass the value to scheme as a define
    // (round-trip via to_raw — the tagged-list wire format is transparent to scheme)
    let raw_c = unsafe { c.to_raw(ctx.raw_ctx())? };
    let _ = raw_c; // would use sexp_define to bind this in scheme env

    // directly call dispatch from rust using ctx.call:
    let get_fn = ctx.evaluate("counter-get")?;
    let result = ctx.call(&get_fn, std::slice::from_ref(&c))?;
    println!("(counter-get c) via ctx.call => {}", result);

    let inc_fn = ctx.evaluate("counter-increment")?;
    let _ = ctx.call(&inc_fn, std::slice::from_ref(&c))?;
    let _ = ctx.call(&inc_fn, std::slice::from_ref(&c))?;
    let _ = ctx.call(&inc_fn, std::slice::from_ref(&c))?;
    let result = ctx.call(&get_fn, std::slice::from_ref(&c))?;
    println!("after 3 increments => {}", result);

    // universal foreign-call works too
    let foreign_call_fn = ctx.evaluate("foreign-call")?;
    let result = ctx.call(
        &foreign_call_fn,
        &[c.clone(), Value::Symbol("get".to_string())],
    )?;
    println!("(foreign-call c 'get) => {}", result);

    // increment with a step argument
    let result = ctx.call(&inc_fn, &[c.clone(), Value::Integer(10)])?;
    println!("(counter-increment c 10) => {}", result);

    println!("\n--- introspection ---\n");

    let types = ctx.evaluate("(foreign-types)")?;
    println!("(foreign-types)              => {}", types);

    let methods = ctx.evaluate("(foreign-type-methods \"counter\")")?;
    println!("(foreign-type-methods counter) => {}", methods);

    let foreign_methods_fn = ctx.evaluate("foreign-methods")?;
    let obj_methods = ctx.call(&foreign_methods_fn, std::slice::from_ref(&c))?;
    println!("(foreign-methods c)            => {}", obj_methods);

    let foreign_type_fn = ctx.evaluate("foreign-type")?;
    let type_name = ctx.call(&foreign_type_fn, std::slice::from_ref(&c))?;
    println!("(foreign-type c)               => {}", type_name);

    println!("\n--- error messages ---\n");

    // wrong method name
    let err = ctx.call(
        &foreign_call_fn,
        &[c.clone(), Value::Symbol("oops".to_string())],
    );
    println!("(foreign-call c 'oops):  {}", err.unwrap_err());

    println!("\n--- display format ---\n");

    println!(
        "Value::Foreign display: {}",
        Value::Foreign {
            handle_id: 7,
            type_name: "counter".to_string()
        }
    );
    println!("wire format (scheme sees): (__tein-foreign \"counter\" 7)");

    println!("\n--- done ---");
    Ok(())
}
