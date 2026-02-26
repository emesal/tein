//! custom port bridge — rust Read/Write as scheme ports.
//!
//! stores rust `Read`/`Write` objects in a per-context map. chibi's custom
//! port callbacks dispatch through a thread-local pointer to find the
//! backing reader/writer. same pattern as `ForeignStore`.
//!
//! handle IDs are generated via xorshift64 seeded from `SystemTime` —
//! they are unpredictable, preventing a scheme program from guessing IDs
//! to access ports it doesn't hold a reference to.

use std::cell::Cell;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::time::{SystemTime, UNIX_EPOCH};

thread_local! {
    /// xorshift64 state for unpredictable handle ID generation.
    /// seeded from SystemTime on first use to prevent sequential ID guessing.
    static XOR_STATE: Cell<u64> = const { Cell::new(0) };
}

/// Generate the next unpredictable handle ID via xorshift64.
///
/// On first call the state is seeded from `SystemTime` (or a fixed fallback).
/// IDs are never 0 — if the PRNG produces 0, a fixed non-zero value is used.
fn next_handle_id() -> u64 {
    XOR_STATE.with(|state| {
        let mut s = state.get();
        if s == 0 {
            // seed from wall clock; any non-zero value works
            s = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0xdead_beef_cafe_f00d);
            if s == 0 {
                s = 0xdead_beef_cafe_f00d;
            }
        }
        // xorshift64 — state must never be 0
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        if s == 0 {
            s = 1;
        }
        state.set(s);
        // mask to chibi fixnum range: SEXP_MAX_FIXNUM = 2^62 - 1 on 64-bit
        // (SEXP_FIXNUM_BITS = 1, so max positive fixnum is i64::MAX >> 1).
        // port IDs are passed through scheme as fixnum literals; values outside
        // this range would corrupt the encoding. ensure non-zero after masking.
        let id = s & (i64::MAX as u64 >> 1);
        if id == 0 { 1 } else { id }
    })
}

/// stored port object — either a reader or writer.
enum PortObject {
    Reader(Box<dyn Read>),
    Writer(Box<dyn Write>),
}

/// per-context store for custom port backing objects.
pub(crate) struct PortStore {
    ports: HashMap<u64, PortObject>,
}

impl PortStore {
    pub(crate) fn new() -> Self {
        Self {
            ports: HashMap::new(),
        }
    }

    pub(crate) fn insert_reader(&mut self, reader: Box<dyn Read>) -> u64 {
        let id = next_handle_id();
        self.ports.insert(id, PortObject::Reader(reader));
        id
    }

    pub(crate) fn insert_writer(&mut self, writer: Box<dyn Write>) -> u64 {
        let id = next_handle_id();
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
