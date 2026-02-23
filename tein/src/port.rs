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
        Self {
            ports: HashMap::new(),
            next_id: 1,
        }
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
