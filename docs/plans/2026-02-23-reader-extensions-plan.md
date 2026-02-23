# Reader Extensions Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add custom ports (rust Read/Write as scheme ports) and hash dispatch reader extensions (#x syntax) to tein.

**Architecture:** Two features in dependency order. Custom ports bridge rust `Read`/`Write` to chibi's custom port mechanism via thread-local trampoline (same pattern as ForeignStore). Hash dispatch patches chibi's reader to check a C-level dispatch table before its hardcoded `#` switch, exposed as `(tein reader)` VFS module.

**Tech Stack:** Rust, C (chibi-scheme vendored patches), scheme (VFS modules)

---

## Key existing patterns to follow

- **ForeignStore + FOREIGN_STORE_PTR + ForeignStoreGuard**: `tein/src/foreign.rs` (store), `tein/src/context.rs:18-52` (thread-local + guard), `tein/src/context.rs:58-80` (extern "C" trampoline)
- **define_fn_variadic**: `tein/src/context.rs:1005-1025` — registers variadic native fn
- **register_foreign_protocol**: `tein/src/context.rs:1146-1169` — lazy registration pattern with `has_foreign_protocol` flag
- **(tein foreign) VFS module**: `tein/vendor/chibi-scheme/lib/tein/foreign.sld` + `foreign.scm`, registered in `tein/build.rs:68-70`
- **TEIN_THREAD_LOCAL**: `tein/vendor/chibi-scheme/tein_shim.c:138-140` — cross-platform thread-local macro
- **Value::Port / as_port()**: `tein/src/value.rs:87,600-604` — opaque port value

## Critical C-level detail

chibi's custom port uses `fopencookie` (linux). the cookie's `sexp_cookie_reader` (port.c:63-82) calls `sexp_apply(ctx, read_proc, (buf 0 size))`. our read_proc is a scheme closure `(lambda (buf start end) (tein-port-read ID buf start end))` where `tein-port-read` is our extern "C" trampoline. the trampoline reads from the rust `Read` impl via `PORT_STORE_PTR` thread-local.

`sexp_make_custom_input_port` / `sexp_make_custom_output_port` are non-static in port.c (compiled into chibi_io static lib via io.c include). shimable via extern declaration.

---

## Feature 1: Custom Ports

### Task 1: PortStore scaffolding

**Files:**
- Create: `tein/src/port.rs`
- Modify: `tein/src/lib.rs` — add `mod port;`
- Modify: `tein/src/context.rs` — add `port_store: RefCell<PortStore>` field to Context, initialize in build()

**Step 1: Create `tein/src/port.rs`**

```rust
//! custom port bridge — rust Read/Write as scheme ports.
//!
//! stores rust `Read`/`Write` objects in a per-context map. chibi's custom
//! port callbacks dispatch through a thread-local pointer to find the
//! backing reader/writer. same pattern as `ForeignStore`.

use std::collections::HashMap;
use std::io::{Read, Write};

/// stored port object — either a reader or writer.
enum PortObject {
    Reader(Box<dyn Read>),
    Writer(Box<dyn Write>),
}

/// per-context store for custom port backing objects.
pub(crate) struct PortStore {
    ports: HashMap<u64, PortObject>,
    next_id: u64,
}

impl PortStore {
    pub(crate) fn new() -> Self {
        Self { ports: HashMap::new(), next_id: 1 }
    }

    pub(crate) fn insert_reader(&mut self, reader: Box<dyn Read>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.ports.insert(id, PortObject::Reader(reader));
        id
    }

    pub(crate) fn insert_writer(&mut self, writer: Box<dyn Write>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.ports.insert(id, PortObject::Writer(writer));
        id
    }

    pub(crate) fn get_reader(&mut self, id: u64) -> Option<&mut dyn Read> {
        match self.ports.get_mut(&id) {
            Some(PortObject::Reader(r)) => Some(r.as_mut()),
            _ => None,
        }
    }

    pub(crate) fn get_writer(&mut self, id: u64) -> Option<&mut dyn Write> {
        match self.ports.get_mut(&id) {
            Some(PortObject::Writer(w)) => Some(w.as_mut()),
            _ => None,
        }
    }
}
```

**Step 2: Wire into Context**

- `tein/src/lib.rs`: add `mod port;` after `mod foreign;`
- `tein/src/context.rs`: add `use crate::port::PortStore;`, add `port_store: RefCell<PortStore>` to Context struct, initialize `port_store: RefCell::new(PortStore::new())` in build()

**Step 3: Verify build**

Run: `cargo build`

**Step 4: Commit**

```
feat: add PortStore scaffolding for custom ports
```

---

### Task 2: C shim + FFI for custom port creation

**Files:**
- Modify: `tein/vendor/chibi-scheme/tein_shim.c`
- Modify: `tein/src/ffi.rs`

**Step 1: Add shim wrappers to tein_shim.c**

Append after the existing functions:

```c
// --- custom port creation ---
// sexp_make_custom_input_port / sexp_make_custom_output_port are defined in
// lib/chibi/io/port.c (compiled via io.c into chibi_io static lib).
extern sexp sexp_make_custom_input_port(sexp ctx, sexp self,
                                         sexp read, sexp seek, sexp close);
extern sexp sexp_make_custom_output_port(sexp ctx, sexp self,
                                          sexp write, sexp seek, sexp close);

sexp tein_make_custom_input_port(sexp ctx, sexp read_proc) {
    return sexp_make_custom_input_port(ctx, SEXP_FALSE, read_proc, SEXP_FALSE, SEXP_FALSE);
}

sexp tein_make_custom_output_port(sexp ctx, sexp write_proc) {
    return sexp_make_custom_output_port(ctx, SEXP_FALSE, write_proc, SEXP_FALSE, SEXP_FALSE);
}
```

**Step 2: Add FFI declarations to ffi.rs**

In the `unsafe extern "C"` block, add:

```rust
pub fn tein_make_custom_input_port(ctx: sexp, read_proc: sexp) -> sexp;
pub fn tein_make_custom_output_port(ctx: sexp, write_proc: sexp) -> sexp;
```

Add safe wrappers:

```rust
/// create a custom input port backed by a scheme read procedure.
#[inline]
pub unsafe fn make_custom_input_port(ctx: sexp, read_proc: sexp) -> sexp {
    unsafe { tein_make_custom_input_port(ctx, read_proc) }
}

/// create a custom output port backed by a scheme write procedure.
#[inline]
pub unsafe fn make_custom_output_port(ctx: sexp, write_proc: sexp) -> sexp {
    unsafe { tein_make_custom_output_port(ctx, write_proc) }
}
```

**Step 3: Verify build**

Run: `cargo build`

**Step 4: Commit**

```
feat: add C shim + FFI for custom port creation
```

---

### Task 3: Read trampoline + open_input_port + test

**Files:**
- Modify: `tein/src/context.rs` — thread-local, guard, trampoline, open_input_port method, test

**Step 1: Write failing test**

```rust
#[test]
fn test_open_input_port_basic() {
    let ctx = Context::new_standard().expect("context");
    let reader = std::io::Cursor::new(b"(+ 1 2)");
    let port = ctx.open_input_port(reader);
    assert!(port.is_ok(), "open_input_port should succeed");
    assert!(port.unwrap().is_port(), "should return a Port value");
}
```

Run: `cargo test test_open_input_port_basic` — expected FAIL (method doesn't exist)

**Step 2: Add PORT_STORE_PTR thread-local + PortStoreGuard**

In context.rs, after `FOREIGN_STORE_PTR`:

```rust
thread_local! {
    static PORT_STORE_PTR: Cell<*const RefCell<PortStore>> = const { Cell::new(std::ptr::null()) };
}

struct PortStoreGuard;

impl Drop for PortStoreGuard {
    fn drop(&mut self) {
        PORT_STORE_PTR.with(|c| c.set(std::ptr::null()));
    }
}
```

**Step 3: Add read trampoline**

```rust
/// extern "C" trampoline for custom input port reads.
///
/// called by chibi via sexp_apply when the custom port's buffer needs refilling.
/// args from scheme: (port-id buffer start end).
/// reads from the rust Read object in PortStore, copies bytes into the scheme
/// string buffer, returns fixnum byte count.
unsafe extern "C" fn port_read_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let id_sexp = ffi::sexp_car(args);
        let rest = ffi::sexp_cdr(args);
        let buf_sexp = ffi::sexp_car(rest);
        let rest2 = ffi::sexp_cdr(rest);
        let start_sexp = ffi::sexp_car(rest2);
        let rest3 = ffi::sexp_cdr(rest2);
        let end_sexp = ffi::sexp_car(rest3);

        let port_id = ffi::sexp_unbox_fixnum(id_sexp) as u64;
        let start = ffi::sexp_unbox_fixnum(start_sexp) as usize;
        let end = ffi::sexp_unbox_fixnum(end_sexp) as usize;
        let len = end - start;

        let store_ptr = PORT_STORE_PTR.with(|c| c.get());
        if store_ptr.is_null() {
            return ffi::sexp_make_fixnum(0);
        }
        let store = &*store_ptr;
        let mut store_ref = store.borrow_mut();
        let reader = match store_ref.get_reader(port_id) {
            Some(r) => r,
            None => return ffi::sexp_make_fixnum(0),
        };

        let mut tmp = vec![0u8; len];
        let bytes_read = match reader.read(&mut tmp) {
            Ok(n) => n,
            Err(_) => return ffi::sexp_make_fixnum(0),
        };

        let buf_data = ffi::sexp_string_data(buf_sexp) as *mut u8;
        std::ptr::copy_nonoverlapping(tmp.as_ptr(), buf_data.add(start), bytes_read);

        ffi::sexp_make_fixnum(bytes_read as ffi::sexp_sint_t)
    }
}
```

**Step 4: Add register_port_protocol + open_input_port**

Add `has_port_protocol: Cell<bool>` to Context struct, initialize `Cell::new(false)`.

```rust
fn register_port_protocol(&self) -> Result<()> {
    self.define_fn_variadic("tein-port-read", port_read_trampoline)?;
    Ok(())
}

/// wrap a rust `Read` as a scheme input port.
pub fn open_input_port(&self, reader: impl Read + 'static) -> Result<Value> {
    if !self.has_port_protocol.get() {
        self.register_port_protocol()?;
        self.has_port_protocol.set(true);
    }

    let port_id = self.port_store.borrow_mut().insert_reader(Box::new(reader));

    // create scheme closure capturing port ID
    let closure_code = format!(
        "(lambda (buf start end) (tein-port-read {} buf start end))",
        port_id
    );

    // need PORT_STORE_PTR set for the evaluate call
    PORT_STORE_PTR.with(|c| c.set(&self.port_store as *const _));
    let _guard = PortStoreGuard;

    let read_proc_val = self.evaluate(&closure_code)?;
    let raw_proc = read_proc_val.as_procedure()
        .ok_or_else(|| Error::EvalError("failed to create port read closure".into()))?;

    unsafe {
        let port = ffi::make_custom_input_port(self.ctx, raw_proc);
        if ffi::sexp_exceptionp(port) != 0 {
            return Err(Error::EvalError("failed to create custom input port".into()));
        }
        Value::from_raw(self.ctx, port)
    }
}
```

**Step 5: Set PORT_STORE_PTR in evaluate() and call()**

In `evaluate()`, after the FOREIGN_STORE_PTR line:
```rust
PORT_STORE_PTR.with(|c| c.set(&self.port_store as *const _));
let _port_guard = PortStoreGuard;
```

Same in `call()`.

**Step 6: Run test**

Run: `cargo test test_open_input_port_basic` — expected PASS

**Step 7: Commit**

```
feat: add read trampoline + open_input_port for custom ports
```

---

### Task 4: read() method

**Files:**
- Modify: `tein/src/context.rs`

**Step 1: Write failing test**

```rust
#[test]
fn test_read_from_custom_port() {
    let ctx = Context::new_standard().expect("context");
    let reader = std::io::Cursor::new(b"42");
    let port = ctx.open_input_port(reader).expect("open port");
    let val = ctx.read(&port).expect("read");
    assert_eq!(val, Value::Integer(42));
}
```

Run: `cargo test test_read_from_custom_port` — FAIL

**Step 2: Implement read()**

```rust
/// read one s-expression from a port.
///
/// returns the parsed but unevaluated expression.
/// returns `Value::Unspecified` at end-of-input (EOF).
pub fn read(&self, port: &Value) -> Result<Value> {
    let raw_port = port.as_port()
        .ok_or_else(|| Error::TypeError(format!("expected port, got {}", port)))?;

    PORT_STORE_PTR.with(|c| c.set(&self.port_store as *const _));
    let _port_guard = PortStoreGuard;
    FOREIGN_STORE_PTR.with(|c| c.set(&self.foreign_store as *const _));
    let _foreign_guard = ForeignStoreGuard;

    unsafe {
        let result = ffi::sexp_read(self.ctx, raw_port);
        if ffi::sexp_eofp(result) != 0 {
            return Ok(Value::Unspecified);
        }
        if ffi::sexp_exceptionp(result) != 0 {
            let msg = ffi::exception_message(self.ctx, result);
            return Err(Error::EvalError(msg));
        }
        Value::from_raw(self.ctx, result)
    }
}
```

**Step 3: Run test**

Run: `cargo test test_read_from_custom_port` — PASS

**Step 4: Commit**

```
feat: add Context::read() for reading s-expressions from ports
```

---

### Task 5: evaluate_port()

**Files:**
- Modify: `tein/src/context.rs`

**Step 1: Write failing tests**

```rust
#[test]
fn test_evaluate_port_single() {
    let ctx = Context::new_standard().expect("context");
    let port = ctx.open_input_port(std::io::Cursor::new(b"(+ 1 2)")).expect("port");
    let result = ctx.evaluate_port(&port).expect("eval");
    assert_eq!(result, Value::Integer(3));
}

#[test]
fn test_evaluate_port_multiple() {
    let ctx = Context::new_standard().expect("context");
    let port = ctx.open_input_port(std::io::Cursor::new(b"(define x 10) (+ x 5)")).expect("port");
    let result = ctx.evaluate_port(&port).expect("eval");
    assert_eq!(result, Value::Integer(15));
}

#[test]
fn test_evaluate_port_empty() {
    let ctx = Context::new_standard().expect("context");
    let port = ctx.open_input_port(std::io::Cursor::new(b"")).expect("port");
    let result = ctx.evaluate_port(&port).expect("eval");
    assert_eq!(result, Value::Unspecified);
}
```

Run: `cargo test test_evaluate_port` — FAIL

**Step 2: Implement evaluate_port()**

```rust
/// read and evaluate all expressions from a port.
///
/// reads s-expressions one at a time, evaluating each in sequence.
/// returns the result of the last expression evaluated, or
/// `Value::Unspecified` if the port was empty.
pub fn evaluate_port(&self, port: &Value) -> Result<Value> {
    let raw_port = port.as_port()
        .ok_or_else(|| Error::TypeError(format!("expected port, got {}", port)))?;

    PORT_STORE_PTR.with(|c| c.set(&self.port_store as *const _));
    let _port_guard = PortStoreGuard;
    FOREIGN_STORE_PTR.with(|c| c.set(&self.foreign_store as *const _));
    let _foreign_guard = ForeignStoreGuard;
    self.arm_fuel();

    unsafe {
        let env = ffi::sexp_context_env(self.ctx);
        let mut last = Value::Unspecified;

        loop {
            let expr = ffi::sexp_read(self.ctx, raw_port);
            if ffi::sexp_eofp(expr) != 0 {
                break;
            }
            if ffi::sexp_exceptionp(expr) != 0 {
                let msg = ffi::exception_message(self.ctx, expr);
                return Err(Error::EvalError(msg));
            }
            let result = ffi::sexp_evaluate(self.ctx, expr, env);
            self.check_fuel()?;
            if ffi::sexp_exceptionp(result) != 0 {
                let msg = ffi::exception_message(self.ctx, result);
                return Err(Error::EvalError(msg));
            }
            last = Value::from_raw(self.ctx, result)?;
        }
        Ok(last)
    }
}
```

**Step 3: Run tests**

Run: `cargo test test_evaluate_port` — all 3 PASS

**Step 4: Commit**

```
feat: add Context::evaluate_port() for read+eval from ports
```

---

### Task 6: Output ports

**Files:**
- Modify: `tein/src/context.rs` — write trampoline, register in port protocol, open_output_port

**Step 1: Write failing test**

```rust
#[test]
fn test_output_port_write() {
    use std::sync::{Arc, Mutex};

    struct SharedWriter(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for SharedWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }

    let buf: Arc<Mutex<Vec<u8>>> = Arc::new(Mutex::new(Vec::new()));
    let ctx = Context::new_standard().expect("context");
    let port = ctx.open_output_port(SharedWriter(buf.clone())).expect("port");

    ctx.call(
        &ctx.evaluate("display").expect("display"),
        &[Value::String("hello".into()), port],
    ).expect("display call");

    let output = buf.lock().unwrap();
    assert_eq!(&*output, b"hello");
}
```

Run: `cargo test test_output_port_write` — FAIL

**Step 2: Add write trampoline**

Mirror of read trampoline but writes from scheme string to rust Writer:

```rust
unsafe extern "C" fn port_write_trampoline(
    ctx: ffi::sexp,
    _self: ffi::sexp,
    _n: ffi::sexp_sint_t,
    args: ffi::sexp,
) -> ffi::sexp {
    unsafe {
        let id_sexp = ffi::sexp_car(args);
        let rest = ffi::sexp_cdr(args);
        let buf_sexp = ffi::sexp_car(rest);
        let rest2 = ffi::sexp_cdr(rest);
        let start_sexp = ffi::sexp_car(rest2);
        let rest3 = ffi::sexp_cdr(rest2);
        let end_sexp = ffi::sexp_car(rest3);

        let port_id = ffi::sexp_unbox_fixnum(id_sexp) as u64;
        let start = ffi::sexp_unbox_fixnum(start_sexp) as usize;
        let end = ffi::sexp_unbox_fixnum(end_sexp) as usize;
        let len = end - start;

        let store_ptr = PORT_STORE_PTR.with(|c| c.get());
        if store_ptr.is_null() {
            return ffi::sexp_make_fixnum(0);
        }
        let store = &*store_ptr;
        let mut store_ref = store.borrow_mut();
        let writer = match store_ref.get_writer(port_id) {
            Some(w) => w,
            None => return ffi::sexp_make_fixnum(0),
        };

        let buf_data = ffi::sexp_string_data(buf_sexp) as *const u8;
        let slice = std::slice::from_raw_parts(buf_data.add(start), len);
        match writer.write(slice) {
            Ok(n) => ffi::sexp_make_fixnum(n as ffi::sexp_sint_t),
            Err(_) => ffi::sexp_make_fixnum(0),
        }
    }
}
```

**Step 3: Register in port protocol + add open_output_port**

Add `tein-port-write` to `register_port_protocol`. Implement `open_output_port` (mirror of open_input_port but uses `insert_writer` and `make_custom_output_port`).

**Step 4: Run test**

Run: `cargo test test_output_port_write` — PASS

**Step 5: Commit**

```
feat: add output port support (write trampoline + open_output_port)
```

---

### Task 7: Port edge cases + scheme-side tests

**Files:**
- Modify: `tein/src/context.rs` — additional tests

**Step 1: Write and verify tests**

```rust
#[test]
fn test_port_read_multiple_sexps() {
    let ctx = Context::new_standard().expect("context");
    let port = ctx.open_input_port(std::io::Cursor::new(b"1 2 3")).expect("port");
    assert_eq!(ctx.read(&port).unwrap(), Value::Integer(1));
    assert_eq!(ctx.read(&port).unwrap(), Value::Integer(2));
    assert_eq!(ctx.read(&port).unwrap(), Value::Integer(3));
    assert_eq!(ctx.read(&port).unwrap(), Value::Unspecified); // EOF
}

#[test]
fn test_port_recognized_by_scheme() {
    let ctx = Context::new_standard().expect("context");
    let port = ctx.open_input_port(std::io::Cursor::new(b"42")).expect("port");
    let is_port = ctx.call(
        &ctx.evaluate("input-port?").expect("fn"),
        &[port],
    ).expect("call");
    assert_eq!(is_port, Value::Boolean(true));
}

#[test]
fn test_port_scheme_read() {
    let ctx = Context::new_standard().expect("context");
    let port = ctx.open_input_port(std::io::Cursor::new(b"(list 1 2 3)")).expect("port");
    let read_fn = ctx.evaluate("read").expect("read fn");
    let expr = ctx.call(&read_fn, &[port.clone()]).expect("read");
    // expr is unevaluated: (list 1 2 3)
    let result = ctx.call(&ctx.evaluate("eval").expect("eval"), &[expr]).expect("eval");
    assert_eq!(result, Value::List(vec![Value::Integer(1), Value::Integer(2), Value::Integer(3)]));
}
```

Run: `cargo test test_port` — all PASS

**Step 2: Commit**

```
test: add edge case tests for custom ports
```

---

### Task 8: Update lib.rs re-exports + docs

**Files:**
- Modify: `tein/src/lib.rs` — ensure no new public types needed (PortStore is internal)
- Modify: `AGENTS.md` — add port.rs to architecture, add custom port data flow
- Modify: `DEVELOPMENT.md` — add custom ports section

**Step 1: Update AGENTS.md**

Add to architecture file listing:
```
  port.rs    — PortStore, Read/Write bridge via thread-local trampoline (custom ports)
```

Add custom port flow description.

**Step 2: Verify**

Run: `cargo test && cargo clippy && cargo fmt --check`

**Step 3: Commit**

```
docs: add custom port architecture to AGENTS.md
```

---

## Feature 2: Hash Dispatch Reader Extensions

### Task 9: C dispatch table in tein_shim.c

**Files:**
- Modify: `tein/vendor/chibi-scheme/tein_shim.c`

**Step 1: Add dispatch table infrastructure**

Append to tein_shim.c:

```c
// --- reader dispatch table ---
// maps ASCII chars to scheme procedures for custom #x syntax.
// thread-local so each context thread has independent dispatch state.

#define TEIN_READER_DISPATCH_SIZE 128

TEIN_THREAD_LOCAL sexp tein_reader_dispatch[TEIN_READER_DISPATCH_SIZE];
TEIN_THREAD_LOCAL int tein_reader_dispatch_init = 0;

static void tein_reader_dispatch_ensure_init(void) {
    if (!tein_reader_dispatch_init) {
        for (int i = 0; i < TEIN_READER_DISPATCH_SIZE; i++)
            tein_reader_dispatch[i] = SEXP_FALSE;
        tein_reader_dispatch_init = 1;
    }
}

static int tein_reader_char_reserved(int c) {
    switch (c) {
    case 'b': case 'B': case 'o': case 'O': case 'd': case 'D':
    case 'x': case 'X': case 'e': case 'E': case 'i': case 'I':
    case 'f': case 'F': case 't': case 'T': case 'u': case 'U':
    case 'v': case 'V': case 's': case 'S': case 'c': case 'C':
    case '0': case '1': case '2': case '3': case '4':
    case '5': case '6': case '7': case '8': case '9':
    case ';': case '|': case '!': case '\\': case '(': case '\'':
    case '`': case ',':
        return 1;
    default:
        return 0;
    }
}

int tein_reader_char_is_reserved(int c) {
    return tein_reader_char_reserved(c);
}

int tein_reader_dispatch_set(int c, sexp proc) {
    tein_reader_dispatch_ensure_init();
    if (c < 0 || c >= TEIN_READER_DISPATCH_SIZE) return -2;
    if (tein_reader_char_reserved(c)) return -1;
    tein_reader_dispatch[c] = proc;
    return 0;
}

int tein_reader_dispatch_unset(int c) {
    tein_reader_dispatch_ensure_init();
    if (c < 0 || c >= TEIN_READER_DISPATCH_SIZE) return -2;
    tein_reader_dispatch[c] = SEXP_FALSE;
    return 0;
}

sexp tein_reader_dispatch_get(int c) {
    tein_reader_dispatch_ensure_init();
    if (c < 0 || c >= TEIN_READER_DISPATCH_SIZE) return SEXP_FALSE;
    return tein_reader_dispatch[c];
}

sexp tein_reader_dispatch_chars(sexp ctx) {
    tein_reader_dispatch_ensure_init();
    sexp result = SEXP_NULL;
    for (int i = TEIN_READER_DISPATCH_SIZE - 1; i >= 0; i--) {
        if (tein_reader_dispatch[i] != SEXP_FALSE)
            result = sexp_cons(ctx, sexp_make_character(i), result);
    }
    return result;
}

void tein_reader_dispatch_clear(void) {
    tein_reader_dispatch_ensure_init();
    for (int i = 0; i < TEIN_READER_DISPATCH_SIZE; i++)
        tein_reader_dispatch[i] = SEXP_FALSE;
}
```

**Step 2: Verify build**

Run: `cargo build`

**Step 3: Commit**

```
feat: add reader dispatch table to tein_shim.c
```

---

### Task 10: Patch sexp.c # dispatch

**Files:**
- Modify: `tein/vendor/chibi-scheme/sexp.c` — lines 3511-3512

**Step 1: Patch the reader**

Replace at line 3511-3512:
```c
  case '#':
    switch (c1=sexp_read_char(ctx, in)) {
```

With:
```c
  case '#':
    c1=sexp_read_char(ctx, in);
    /* tein patch: check user-registered dispatch before built-in # syntax */
    {
      extern sexp tein_reader_dispatch_get(int c);
      sexp _tein_dispatch = tein_reader_dispatch_get(c1);
      if (_tein_dispatch != SEXP_FALSE && sexp_applicablep(_tein_dispatch)) {
        res = sexp_apply1(ctx, _tein_dispatch, in);
        break;
      }
    }
    switch (c1) {
```

**Step 2: Verify build**

Run: `cargo build`

**Step 3: Commit**

```
feat: patch sexp.c to check reader dispatch table before # switch
```

---

### Task 11: FFI bindings for reader dispatch

**Files:**
- Modify: `tein/src/ffi.rs`

**Step 1: Add declarations**

```rust
pub fn tein_reader_dispatch_set(c: std::ffi::c_int, proc: sexp) -> std::ffi::c_int;
pub fn tein_reader_dispatch_unset(c: std::ffi::c_int) -> std::ffi::c_int;
pub fn tein_reader_dispatch_get(c: std::ffi::c_int) -> sexp;
pub fn tein_reader_dispatch_chars(ctx: sexp) -> sexp;
pub fn tein_reader_dispatch_clear();
pub fn tein_reader_char_is_reserved(c: std::ffi::c_int) -> std::ffi::c_int;
```

Add safe wrappers with clear docstrings.

**Step 2: Verify build**

Run: `cargo build`

**Step 3: Commit**

```
feat: add FFI bindings for reader dispatch table
```

---

### Task 12: Native dispatch functions + (tein reader) VFS module

**Files:**
- Modify: `tein/src/context.rs` — native wrapper fns, register_reader_protocol
- Create: `tein/vendor/chibi-scheme/lib/tein/reader.sld`
- Create: `tein/vendor/chibi-scheme/lib/tein/reader.scm`
- Modify: `tein/build.rs` — add to VFS_FILES

**Step 1: Write failing test**

```rust
#[test]
fn test_reader_dispatch_basic() {
    let ctx = Context::new_standard().expect("context");
    ctx.evaluate("(import (tein reader))").expect("import");
    ctx.evaluate("(set-reader! #\\j (lambda (port) 'j-value))").expect("set-reader");
    let result = ctx.evaluate("#j").expect("eval #j");
    assert_eq!(result, Value::Symbol("j-value".into()));
}
```

Run: `cargo test test_reader_dispatch_basic` — FAIL

**Step 2: Add extern "C" wrapper functions in context.rs**

- `reader_set_wrapper`: extracts char + proc from args, calls `ffi::tein_reader_dispatch_set`, returns void or error
- `reader_unset_wrapper`: extracts char, calls `ffi::tein_reader_dispatch_unset`
- `reader_chars_wrapper`: calls `ffi::tein_reader_dispatch_chars`

LLM-friendly error messages:
- reserved char: `"reader dispatch #X is reserved by r7rs and cannot be overridden"`
- wrong arg type: `"set-reader!: first argument must be a character, got ..."`
- wrong arg count: `"set-reader!: expected (set-reader! char proc), got N arguments"`

**Step 3: Add register_reader_protocol**

```rust
fn register_reader_protocol(&self) -> Result<()> {
    self.define_fn_variadic("tein-reader-set!", reader_set_wrapper)?;
    self.define_fn_variadic("tein-reader-unset!", reader_unset_wrapper)?;
    self.define_fn_variadic("tein-reader-dispatch-chars", reader_chars_wrapper)?;
    Ok(())
}
```

Always register in build() for standard_env contexts (cheap, 3 function registrations).

**Step 4: Create VFS module files**

`tein/vendor/chibi-scheme/lib/tein/reader.sld`:
```scheme
(define-library (tein reader)
  (import (scheme base))
  (export set-reader! unset-reader! reader-dispatch-chars)
  (include "reader.scm"))
```

`tein/vendor/chibi-scheme/lib/tein/reader.scm`:
```scheme
;;; (tein reader) — custom reader dispatch extensions
;;;
;;; register custom #x reader syntax via set-reader!. handlers are
;;; scheme procedures that receive the input port and return a value.
;;; native dispatch fns (tein-reader-set! etc.) are registered from rust.

(define (set-reader! ch proc)
  (tein-reader-set! ch proc))

(define (unset-reader! ch)
  (tein-reader-unset! ch))

(define (reader-dispatch-chars)
  (tein-reader-dispatch-chars))
```

**Step 5: Register in build.rs VFS_FILES**

After the tein/foreign entries:
```rust
"lib/tein/reader.sld",
"lib/tein/reader.scm",
```

**Step 6: Run test**

Run: `cargo test test_reader_dispatch_basic` — PASS

**Step 7: Commit**

```
feat: add (tein reader) VFS module with set-reader!/unset-reader!/reader-dispatch-chars
```

---

### Task 13: Reader dispatch tests — errors, edge cases, introspection

**Files:**
- Modify: `tein/src/context.rs` — tests

**Step 1: Write and run tests**

```rust
#[test]
fn test_reader_dispatch_reserved_char() {
    let ctx = Context::new_standard().expect("context");
    ctx.evaluate("(import (tein reader))").expect("import");
    let result = ctx.evaluate("(set-reader! #\\t (lambda (port) 42))");
    assert!(result.is_err());
    let msg = format!("{}", result.unwrap_err());
    assert!(msg.contains("reserved"), "expected 'reserved' in: {}", msg);
}

#[test]
fn test_reader_dispatch_handler_reads_port() {
    let ctx = Context::new_standard().expect("context");
    ctx.evaluate("(import (tein reader))").expect("import");
    ctx.evaluate(
        "(set-reader! #\\j (lambda (port) (list 'json (read port))))"
    ).expect("set");
    let result = ctx.evaluate("#j(1 2 3)").expect("eval");
    let list = result.as_list().expect("list");
    assert_eq!(list.len(), 2);
    assert_eq!(list[0], Value::Symbol("json".into()));
}

#[test]
fn test_reader_dispatch_unset() {
    let ctx = Context::new_standard().expect("context");
    ctx.evaluate("(import (tein reader))").expect("import");
    ctx.evaluate("(set-reader! #\\j (lambda (port) 'j))").expect("set");
    assert_eq!(ctx.evaluate("#j").unwrap(), Value::Symbol("j".into()));
    ctx.evaluate("(unset-reader! #\\j)").expect("unset");
    assert!(ctx.evaluate("#j").is_err());
}

#[test]
fn test_reader_dispatch_chars_introspection() {
    let ctx = Context::new_standard().expect("context");
    ctx.evaluate("(import (tein reader))").expect("import");
    ctx.evaluate("(set-reader! #\\j (lambda (port) 'j))").expect("set j");
    ctx.evaluate("(set-reader! #\\p (lambda (port) 'p))").expect("set p");
    let chars = ctx.evaluate("(reader-dispatch-chars)").expect("chars");
    let list = chars.as_list().expect("list");
    assert_eq!(list.len(), 2);
}

#[test]
fn test_reader_dispatch_multiple_chars() {
    // handler reads further to distinguish sub-syntax
    let ctx = Context::new_standard().expect("context");
    ctx.evaluate("(import (tein reader))").expect("import");
    ctx.evaluate(
        "(set-reader! #\\j
           (lambda (port)
             (let ((next (read-char port)))
               (cond
                 ((char=? next #\\s) (list 'json (read port)))
                 ((char=? next #\\w) (list 'jwt (read port)))
                 (else (error \"unknown #j sub-dispatch\" next))))))"
    ).expect("set");
    let json = ctx.evaluate("#js(1 2 3)").expect("json");
    assert_eq!(json.as_list().unwrap()[0], Value::Symbol("json".into()));
    let jwt = ctx.evaluate("#jw\"token\"").expect("jwt");
    assert_eq!(jwt.as_list().unwrap()[0], Value::Symbol("jwt".into()));
}
```

Run: `cargo test test_reader_dispatch` — all PASS

**Step 2: Commit**

```
test: add reader dispatch edge cases and introspection tests
```

---

### Task 14: Rust-side register_reader convenience API

**Files:**
- Modify: `tein/src/context.rs`

**Step 1: Write failing test**

```rust
#[test]
fn test_register_reader_from_rust() {
    let ctx = Context::new_standard().expect("context");
    let handler = ctx.evaluate("(lambda (port) 42)").expect("handler");
    ctx.register_reader('j', &handler).expect("register");
    let result = ctx.evaluate("#j").expect("eval");
    assert_eq!(result, Value::Integer(42));
}

#[test]
fn test_register_reader_reserved_from_rust() {
    let ctx = Context::new_standard().expect("context");
    let handler = ctx.evaluate("(lambda (port) 42)").expect("handler");
    let err = ctx.register_reader('t', &handler).unwrap_err();
    assert!(format!("{}", err).contains("reserved"));
}
```

**Step 2: Implement**

```rust
/// register a reader dispatch handler for `#ch` syntax.
///
/// the handler must be a scheme procedure taking one argument (the input port)
/// and returning a value. reserved r7rs characters cannot be overridden.
pub fn register_reader(&self, ch: char, handler: &Value) -> Result<()> {
    let raw_proc = handler.as_procedure()
        .ok_or_else(|| Error::TypeError("handler must be a procedure".into()))?;
    let c = ch as i32;
    unsafe {
        let result = ffi::tein_reader_dispatch_set(c as std::ffi::c_int, raw_proc);
        match result {
            0 => Ok(()),
            -1 => Err(Error::EvalError(format!(
                "reader dispatch #{} is reserved by r7rs and cannot be overridden", ch
            ))),
            _ => Err(Error::EvalError("character out of ASCII range".into())),
        }
    }
}
```

**Step 3: Run tests**

Run: `cargo test test_register_reader` — PASS

**Step 4: Commit**

```
feat: add Context::register_reader() convenience API
```

---

### Task 15: Final docs + cleanup

**Files:**
- Modify: `AGENTS.md` — add reader dispatch flow, update file listing
- Modify: `TODO.md` — mark reader extensions complete
- Modify: `tein/src/lib.rs` — verify re-exports are complete

**Step 1: Update AGENTS.md**

Add `port.rs` to file listing. Add reader dispatch data flow. Add `(tein reader)` to VFS module listing.

**Step 2: Update TODO.md**

Mark "custom reader extensions" as `[x]` complete.

**Step 3: Final verification**

Run: `cargo test && cargo clippy && cargo fmt --check`

Expected: all tests pass, no warnings, format clean.

**Step 4: Commit**

```
docs: add reader extensions to architecture docs and roadmap
```

---

## Verification

After all tasks complete:

```bash
cargo test                    # all tests pass (existing + new)
cargo clippy                  # no warnings
cargo fmt --check             # format clean
cargo test test_port          # custom port tests specifically
cargo test test_reader        # reader dispatch tests specifically
cargo test test_register      # rust-side registration tests
```
