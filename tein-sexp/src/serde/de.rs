//! serde deserializer for s-expressions
//!
//! deserializes s-expression text or [`Sexp`] values into rust types.
//! self-describing format: the deserializer examines the s-expression
//! to determine which visitor method to call.

use crate::ast::{Sexp, SexpKind};
use crate::error::ParseError;
use crate::parser;
use serde::de;
use std::fmt;

/// deserialize a value from s-expression text
///
/// # Examples
///
/// ```
/// use tein_sexp::serde::from_str;
///
/// let v: Vec<i32> = from_str("(1 2 3)").unwrap();
/// assert_eq!(v, vec![1, 2, 3]);
/// ```
pub fn from_str<'de, T: de::Deserialize<'de>>(input: &str) -> Result<T, ParseError> {
    let sexp = parser::parse(input)?;
    from_sexp(&sexp)
}

/// deserialize a value from an [`Sexp`] node
///
/// # Examples
///
/// ```
/// use tein_sexp::{Sexp, serde::from_sexp};
///
/// let sexp = Sexp::integer(42);
/// let n: i32 = from_sexp(&sexp).unwrap();
/// assert_eq!(n, 42);
/// ```
pub fn from_sexp<'de, T: de::Deserialize<'de>>(sexp: &Sexp) -> Result<T, ParseError> {
    T::deserialize(SexpDeserializer { sexp })
}

/// serde deserializer wrapping a reference to an [`Sexp`]
struct SexpDeserializer<'a> {
    sexp: &'a Sexp,
}

impl<'a> SexpDeserializer<'a> {
    /// create a parse error pointing at this node's span
    fn error(&self, msg: impl fmt::Display) -> ParseError {
        ParseError::new(msg.to_string(), self.sexp.span)
    }
}

impl<'de, 'a> de::Deserializer<'de> for SexpDeserializer<'a> {
    type Error = ParseError;

    fn deserialize_any<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, ParseError> {
        match &self.sexp.kind {
            SexpKind::Integer(n) => visitor.visit_i64(*n),
            SexpKind::Float(f) => visitor.visit_f64(*f),
            SexpKind::String(s) => visitor.visit_string(s.clone()),
            SexpKind::Symbol(s) => visitor.visit_string(s.clone()),
            SexpKind::Boolean(b) => visitor.visit_bool(*b),
            SexpKind::Char(c) => visitor.visit_char(*c),
            SexpKind::Nil => visitor.visit_unit(),
            SexpKind::List(items) => {
                // heuristic: if all items are dotted pairs with symbol/string key, treat as map
                if self.sexp.is_alist() {
                    visitor.visit_map(AlistMapAccess::new(items))
                } else {
                    visitor.visit_seq(SexpSeqAccess::new(items))
                }
            }
            SexpKind::DottedList(items, tail) => {
                // flatten dotted list into a sequence including the tail
                let mut all = items.clone();
                all.push(*tail.clone());
                visitor.visit_seq(SexpSeqAccess::new_owned(all))
            }
            SexpKind::Vector(items) => visitor.visit_seq(SexpSeqAccess::new(items)),
            // numeric tower: bignums serialize as strings, rational/complex as maps
            SexpKind::Bignum(s) => visitor.visit_string(s.clone()),
            SexpKind::Rational(n, d) => {
                let entries = vec![
                    Sexp::dotted_list(vec![Sexp::string("numerator")], *n.clone()),
                    Sexp::dotted_list(vec![Sexp::string("denominator")], *d.clone()),
                ];
                visitor.visit_map(OwnedAlistMapAccess::new(entries))
            }
            SexpKind::Complex(r, i) => {
                let entries = vec![
                    Sexp::dotted_list(vec![Sexp::string("real")], *r.clone()),
                    Sexp::dotted_list(vec![Sexp::string("imag")], *i.clone()),
                ];
                visitor.visit_map(OwnedAlistMapAccess::new(entries))
            }
            SexpKind::Bytevector(bytes) => {
                let items: Vec<Sexp> = bytes.iter().map(|&b| Sexp::integer(b as i64)).collect();
                visitor.visit_seq(SexpSeqAccess::new_owned(items))
            }
        }
    }

    fn deserialize_bool<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, ParseError> {
        match &self.sexp.kind {
            SexpKind::Boolean(b) => visitor.visit_bool(*b),
            _ => Err(self.error("expected boolean")),
        }
    }

    fn deserialize_i8<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, ParseError> {
        self.deserialize_i64(visitor)
    }

    fn deserialize_i16<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, ParseError> {
        self.deserialize_i64(visitor)
    }

    fn deserialize_i32<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, ParseError> {
        self.deserialize_i64(visitor)
    }

    fn deserialize_i64<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, ParseError> {
        match &self.sexp.kind {
            SexpKind::Integer(n) => visitor.visit_i64(*n),
            _ => Err(self.error("expected integer")),
        }
    }

    fn deserialize_i128<V: de::Visitor<'de>>(self, _visitor: V) -> Result<V::Value, ParseError> {
        Err(self.error("i128 cannot be deserialized from s-expressions (i64 max)"))
    }

    fn deserialize_u8<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, ParseError> {
        self.deserialize_u64(visitor)
    }

    fn deserialize_u16<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, ParseError> {
        self.deserialize_u64(visitor)
    }

    fn deserialize_u32<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, ParseError> {
        self.deserialize_u64(visitor)
    }

    fn deserialize_u64<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, ParseError> {
        match &self.sexp.kind {
            SexpKind::Integer(n) if *n >= 0 => visitor.visit_u64(*n as u64),
            SexpKind::Integer(_) => Err(self.error("expected non-negative integer")),
            _ => Err(self.error("expected integer")),
        }
    }

    fn deserialize_u128<V: de::Visitor<'de>>(self, _visitor: V) -> Result<V::Value, ParseError> {
        Err(self.error("u128 cannot be deserialized from s-expressions (i64 max)"))
    }

    fn deserialize_f32<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, ParseError> {
        self.deserialize_f64(visitor)
    }

    fn deserialize_f64<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, ParseError> {
        match &self.sexp.kind {
            SexpKind::Float(f) => visitor.visit_f64(*f),
            SexpKind::Integer(n) => visitor.visit_f64(*n as f64),
            _ => Err(self.error("expected float")),
        }
    }

    fn deserialize_char<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, ParseError> {
        match &self.sexp.kind {
            SexpKind::Char(c) => visitor.visit_char(*c),
            _ => Err(self.error("expected char")),
        }
    }

    fn deserialize_str<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, ParseError> {
        self.deserialize_string(visitor)
    }

    fn deserialize_string<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, ParseError> {
        match &self.sexp.kind {
            SexpKind::String(s) => visitor.visit_string(s.clone()),
            SexpKind::Symbol(s) => visitor.visit_string(s.clone()),
            _ => Err(self.error("expected string or symbol")),
        }
    }

    fn deserialize_bytes<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, ParseError> {
        self.deserialize_byte_buf(visitor)
    }

    fn deserialize_byte_buf<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, ParseError> {
        // bytes are serialized as a list of integers
        match &self.sexp.kind {
            SexpKind::List(items) => {
                let mut bytes = Vec::with_capacity(items.len());
                for item in items {
                    match &item.kind {
                        SexpKind::Integer(n) if *n >= 0 && *n <= 255 => bytes.push(*n as u8),
                        _ => return Err(ParseError::new("expected byte value (0-255)", item.span)),
                    }
                }
                visitor.visit_byte_buf(bytes)
            }
            _ => Err(self.error("expected list of bytes")),
        }
    }

    fn deserialize_option<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, ParseError> {
        match &self.sexp.kind {
            SexpKind::Nil => visitor.visit_none(),
            _ => visitor.visit_some(self),
        }
    }

    fn deserialize_unit<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, ParseError> {
        match &self.sexp.kind {
            SexpKind::Nil => visitor.visit_unit(),
            _ => Err(self.error("expected ()")),
        }
    }

    fn deserialize_unit_struct<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, ParseError> {
        self.deserialize_unit(visitor)
    }

    fn deserialize_newtype_struct<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, ParseError> {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, ParseError> {
        match &self.sexp.kind {
            SexpKind::List(items) => visitor.visit_seq(SexpSeqAccess::new(items)),
            SexpKind::Vector(items) => visitor.visit_seq(SexpSeqAccess::new(items)),
            SexpKind::Nil => visitor.visit_seq(SexpSeqAccess::new(&[])),
            _ => Err(self.error("expected list or vector")),
        }
    }

    fn deserialize_tuple<V: de::Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, ParseError> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_tuple_struct<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, ParseError> {
        self.deserialize_seq(visitor)
    }

    fn deserialize_map<V: de::Visitor<'de>>(self, visitor: V) -> Result<V::Value, ParseError> {
        match &self.sexp.kind {
            SexpKind::List(items) => visitor.visit_map(AlistMapAccess::new(items)),
            SexpKind::Nil => visitor.visit_map(AlistMapAccess::new(&[])),
            _ => Err(self.error("expected alist")),
        }
    }

    fn deserialize_struct<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, ParseError> {
        self.deserialize_map(visitor)
    }

    fn deserialize_enum<V: de::Visitor<'de>>(
        self,
        _name: &'static str,
        _variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, ParseError> {
        match &self.sexp.kind {
            // unit variant: bare symbol
            SexpKind::Symbol(_) => visitor.visit_enum(EnumAccess { sexp: self.sexp }),
            // other variants: tagged list (variant ...)
            SexpKind::List(items) if !items.is_empty() => {
                visitor.visit_enum(EnumAccess { sexp: self.sexp })
            }
            _ => Err(self.error("expected symbol or tagged list for enum")),
        }
    }

    fn deserialize_identifier<V: de::Visitor<'de>>(
        self,
        visitor: V,
    ) -> Result<V::Value, ParseError> {
        self.deserialize_string(visitor)
    }

    fn deserialize_ignored_any<V: de::Visitor<'de>>(
        self,
        visitor: V,
    ) -> Result<V::Value, ParseError> {
        self.deserialize_any(visitor)
    }
}

// --- sequence access ---

/// provides sequential access to list/vector elements
enum SexpSeqAccess<'a> {
    /// borrowed from the original sexp
    Borrowed { items: &'a [Sexp], index: usize },
    /// owned (for flattened dotted lists)
    Owned { items: Vec<Sexp>, index: usize },
}

impl<'a> SexpSeqAccess<'a> {
    fn new(items: &'a [Sexp]) -> Self {
        Self::Borrowed { items, index: 0 }
    }

    fn new_owned(items: Vec<Sexp>) -> Self {
        Self::Owned { items, index: 0 }
    }

    fn next_item(&mut self) -> Option<&Sexp> {
        match self {
            Self::Borrowed { items, index } => {
                if *index < items.len() {
                    let item = &items[*index];
                    *index += 1;
                    Some(item)
                } else {
                    None
                }
            }
            Self::Owned { items, index } => {
                if *index < items.len() {
                    let item = &items[*index];
                    *index += 1;
                    Some(item)
                } else {
                    None
                }
            }
        }
    }
}

impl<'de, 'a> de::SeqAccess<'de> for SexpSeqAccess<'a> {
    type Error = ParseError;

    fn next_element_seed<T: de::DeserializeSeed<'de>>(
        &mut self,
        seed: T,
    ) -> Result<Option<T::Value>, ParseError> {
        match self.next_item() {
            Some(item) => seed.deserialize(SexpDeserializer { sexp: item }).map(Some),
            None => Ok(None),
        }
    }
}

// --- map access (alists) ---

/// provides map access to alists: `((key . val) ...)`
struct AlistMapAccess<'a> {
    items: &'a [Sexp],
    index: usize,
}

impl<'a> AlistMapAccess<'a> {
    fn new(items: &'a [Sexp]) -> Self {
        Self { items, index: 0 }
    }
}

impl<'de, 'a> de::MapAccess<'de> for AlistMapAccess<'a> {
    type Error = ParseError;

    fn next_key_seed<K: de::DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, ParseError> {
        if self.index >= self.items.len() {
            return Ok(None);
        }
        let entry = &self.items[self.index];
        let key = alist_key(entry)?;
        seed.deserialize(SexpDeserializer { sexp: key }).map(Some)
    }

    fn next_value_seed<V: de::DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, ParseError> {
        let entry = &self.items[self.index];
        self.index += 1;
        let val = alist_value(entry)?;
        seed.deserialize(SexpDeserializer { sexp: val })
    }
}

/// owned variant of [`AlistMapAccess`] for synthetic alist entries (e.g. rational/complex).
struct OwnedAlistMapAccess {
    items: Vec<Sexp>,
    index: usize,
}

impl OwnedAlistMapAccess {
    fn new(items: Vec<Sexp>) -> Self {
        Self { items, index: 0 }
    }
}

impl<'de> de::MapAccess<'de> for OwnedAlistMapAccess {
    type Error = ParseError;

    fn next_key_seed<K: de::DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, ParseError> {
        if self.index >= self.items.len() {
            return Ok(None);
        }
        let key = alist_key(&self.items[self.index])?;
        seed.deserialize(SexpDeserializer { sexp: key }).map(Some)
    }

    fn next_value_seed<V: de::DeserializeSeed<'de>>(
        &mut self,
        seed: V,
    ) -> Result<V::Value, ParseError> {
        let val = alist_value(&self.items[self.index])?;
        self.index += 1;
        seed.deserialize(SexpDeserializer { sexp: val })
    }
}

/// extract the key from an alist entry (dotted pair)
fn alist_key(entry: &Sexp) -> Result<&Sexp, ParseError> {
    match &entry.kind {
        SexpKind::DottedList(items, _) if items.len() == 1 => Ok(&items[0]),
        SexpKind::List(items) if items.len() == 2 => Ok(&items[0]),
        _ => Err(ParseError::new(
            "expected dotted pair (key . value) in alist",
            entry.span,
        )),
    }
}

/// extract the value from an alist entry (dotted pair)
fn alist_value(entry: &Sexp) -> Result<&Sexp, ParseError> {
    match &entry.kind {
        SexpKind::DottedList(_, tail) => Ok(tail.as_ref()),
        SexpKind::List(items) if items.len() == 2 => Ok(&items[1]),
        _ => Err(ParseError::new(
            "expected dotted pair (key . value) in alist",
            entry.span,
        )),
    }
}

// --- enum access ---

/// handles enum deserialization from symbols and tagged lists
struct EnumAccess<'a> {
    sexp: &'a Sexp,
}

impl<'de, 'a> de::EnumAccess<'de> for EnumAccess<'a> {
    type Error = ParseError;
    type Variant = VariantAccess<'a>;

    fn variant_seed<V: de::DeserializeSeed<'de>>(
        self,
        seed: V,
    ) -> Result<(V::Value, Self::Variant), ParseError> {
        match &self.sexp.kind {
            // unit variant: bare symbol
            SexpKind::Symbol(_) => {
                let variant = seed.deserialize(SexpDeserializer { sexp: self.sexp })?;
                Ok((variant, VariantAccess { sexp: self.sexp }))
            }
            // tagged list: (variant-name value ...)
            SexpKind::List(items) if !items.is_empty() => {
                let tag = &items[0];
                let variant = seed.deserialize(SexpDeserializer { sexp: tag })?;
                Ok((variant, VariantAccess { sexp: self.sexp }))
            }
            _ => Err(ParseError::new("expected enum variant", self.sexp.span)),
        }
    }
}

/// handles the payload of an enum variant
struct VariantAccess<'a> {
    sexp: &'a Sexp,
}

impl<'de, 'a> de::VariantAccess<'de> for VariantAccess<'a> {
    type Error = ParseError;

    fn unit_variant(self) -> Result<(), ParseError> {
        match &self.sexp.kind {
            SexpKind::Symbol(_) => Ok(()),
            SexpKind::List(items) if items.len() == 1 => Ok(()),
            _ => Err(ParseError::new("expected unit variant", self.sexp.span)),
        }
    }

    fn newtype_variant_seed<T: de::DeserializeSeed<'de>>(
        self,
        seed: T,
    ) -> Result<T::Value, ParseError> {
        match &self.sexp.kind {
            SexpKind::List(items) if items.len() == 2 => {
                seed.deserialize(SexpDeserializer { sexp: &items[1] })
            }
            _ => Err(ParseError::new(
                "expected (variant value) for newtype variant",
                self.sexp.span,
            )),
        }
    }

    fn tuple_variant<V: de::Visitor<'de>>(
        self,
        _len: usize,
        visitor: V,
    ) -> Result<V::Value, ParseError> {
        match &self.sexp.kind {
            SexpKind::List(items) if items.len() >= 2 => {
                visitor.visit_seq(SexpSeqAccess::new(&items[1..]))
            }
            _ => Err(ParseError::new(
                "expected (variant val ...) for tuple variant",
                self.sexp.span,
            )),
        }
    }

    fn struct_variant<V: de::Visitor<'de>>(
        self,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, ParseError> {
        match &self.sexp.kind {
            SexpKind::List(items) if items.len() >= 2 => {
                // items[0] is the variant tag, items[1..] are the alist entries
                visitor.visit_map(AlistMapAccess::new(&items[1..]))
            }
            _ => Err(ParseError::new(
                "expected (variant (field . val) ...) for struct variant",
                self.sexp.span,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};
    use std::collections::BTreeMap;

    // --- primitives ---

    #[test]
    fn deserialize_integer() {
        assert_eq!(from_str::<i32>("42").unwrap(), 42);
        assert_eq!(from_str::<i64>("-7").unwrap(), -7);
    }

    #[test]
    fn deserialize_float() {
        let f: f64 = from_str("3.125").unwrap();
        assert!((f - 3.125).abs() < f64::EPSILON);
    }

    #[test]
    fn deserialize_bool() {
        assert!(from_str::<bool>("#t").unwrap());
        assert!(!from_str::<bool>("#f").unwrap());
    }

    #[test]
    fn deserialize_string() {
        assert_eq!(from_str::<String>("\"hello\"").unwrap(), "hello");
    }

    #[test]
    fn deserialize_symbol_as_string() {
        assert_eq!(from_str::<String>("foo").unwrap(), "foo");
    }

    #[test]
    fn deserialize_char() {
        assert_eq!(from_str::<char>("#\\a").unwrap(), 'a');
        assert_eq!(from_str::<char>("#\\space").unwrap(), ' ');
    }

    #[test]
    fn deserialize_unit() {
        from_str::<()>("()").unwrap();
    }

    // --- option ---

    #[test]
    fn deserialize_none() {
        assert_eq!(from_str::<Option<i32>>("()").unwrap(), None);
    }

    #[test]
    fn deserialize_some() {
        assert_eq!(from_str::<Option<i32>>("42").unwrap(), Some(42));
    }

    // --- sequences ---

    #[test]
    fn deserialize_vec() {
        assert_eq!(from_str::<Vec<i32>>("(1 2 3)").unwrap(), vec![1, 2, 3]);
    }

    #[test]
    fn deserialize_empty_vec() {
        assert_eq!(from_str::<Vec<i32>>("()").unwrap(), Vec::<i32>::new());
    }

    #[test]
    fn deserialize_tuple() {
        let t: (i32, String, bool) = from_str("(42 \"hello\" #t)").unwrap();
        assert_eq!(t, (42, "hello".to_string(), true));
    }

    // --- map ---

    #[test]
    fn deserialize_map() {
        let m: BTreeMap<String, String> =
            from_str("((\"name\" . \"alice\") (\"role\" . \"admin\"))").unwrap();
        assert_eq!(m.get("name").unwrap(), "alice");
        assert_eq!(m.get("role").unwrap(), "admin");
    }

    // --- struct ---

    #[derive(Debug, Deserialize, PartialEq)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[test]
    fn deserialize_struct() {
        let p: Point = from_str("((x . 1) (y . 2))").unwrap();
        assert_eq!(p, Point { x: 1, y: 2 });
    }

    #[derive(Debug, Deserialize, PartialEq)]
    struct Config {
        name: String,
        debug: bool,
        count: i32,
    }

    #[test]
    fn deserialize_struct_mixed() {
        let c: Config = from_str("((name . \"test\") (debug . #t) (count . 42))").unwrap();
        assert_eq!(
            c,
            Config {
                name: "test".to_string(),
                debug: true,
                count: 42,
            }
        );
    }

    // --- enums ---

    #[derive(Debug, Deserialize, PartialEq)]
    enum Color {
        Red,
        Green,
        Blue,
    }

    #[test]
    fn deserialize_unit_variant() {
        assert_eq!(from_str::<Color>("Red").unwrap(), Color::Red);
        assert_eq!(from_str::<Color>("Green").unwrap(), Color::Green);
    }

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    enum Shape {
        Circle(f64),
        Rectangle(f64, f64),
        Labeled { name: String, sides: u32 },
    }

    #[test]
    fn deserialize_newtype_variant() {
        let s: Shape = from_str("(Circle 5.0)").unwrap();
        assert_eq!(s, Shape::Circle(5.0));
    }

    #[test]
    fn deserialize_tuple_variant() {
        let s: Shape = from_str("(Rectangle 3.0 4.0)").unwrap();
        assert_eq!(s, Shape::Rectangle(3.0, 4.0));
    }

    #[test]
    fn deserialize_struct_variant() {
        let s: Shape = from_str("(Labeled (name . \"triangle\") (sides . 3))").unwrap();
        assert_eq!(
            s,
            Shape::Labeled {
                name: "triangle".to_string(),
                sides: 3,
            }
        );
    }

    // --- nested ---

    #[derive(Debug, Deserialize, PartialEq)]
    struct Nested {
        items: Vec<i32>,
        label: Option<String>,
    }

    #[test]
    fn deserialize_nested() {
        let n: Nested = from_str("((items . (1 2 3)) (label . \"test\"))").unwrap();
        assert_eq!(
            n,
            Nested {
                items: vec![1, 2, 3],
                label: Some("test".to_string()),
            }
        );
    }

    #[test]
    fn deserialize_nested_none() {
        let n: Nested = from_str("((items . ()) (label . ()))").unwrap();
        assert_eq!(
            n,
            Nested {
                items: vec![],
                label: None,
            }
        );
    }

    // --- round-trips (serialize → deserialize) ---

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct RoundTrip {
        name: String,
        values: Vec<i32>,
        active: bool,
    }

    #[test]
    fn round_trip_struct() {
        let original = RoundTrip {
            name: "test".to_string(),
            values: vec![1, 2, 3],
            active: true,
        };
        let text = crate::serde::to_string(&original).unwrap();
        let restored: RoundTrip = from_str(&text).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn round_trip_primitives() {
        // integers
        let text = crate::serde::to_string(&42i32).unwrap();
        assert_eq!(from_str::<i32>(&text).unwrap(), 42);

        // strings
        let text = crate::serde::to_string(&"hello").unwrap();
        assert_eq!(from_str::<String>(&text).unwrap(), "hello");

        // booleans
        let text = crate::serde::to_string(&true).unwrap();
        assert!(from_str::<bool>(&text).unwrap());
    }

    #[test]
    fn round_trip_enum() {
        let original = Shape::Rectangle(3.0, 4.0);
        let text = crate::serde::to_string(&original).unwrap();
        let restored: Shape = from_str(&text).unwrap();
        assert_eq!(original, restored);
    }

    #[test]
    fn round_trip_option() {
        let some_val = Some(42i32);
        let text = crate::serde::to_string(&some_val).unwrap();
        let restored: Option<i32> = from_str(&text).unwrap();
        assert_eq!(restored, some_val);

        let none_val: Option<i32> = None;
        let text = crate::serde::to_string(&none_val).unwrap();
        let restored: Option<i32> = from_str(&text).unwrap();
        assert_eq!(restored, none_val);
    }

    // --- error messages ---

    #[test]
    fn error_includes_span() {
        let err = from_str::<i32>("\"not an int\"").unwrap_err();
        assert!(err.to_string().contains("line"), "error: {err}");
    }

    #[test]
    fn error_type_mismatch() {
        let err = from_str::<bool>("42").unwrap_err();
        assert!(err.to_string().contains("expected boolean"), "error: {err}");
    }

    // --- from_sexp ---

    #[test]
    fn from_sexp_direct() {
        let sexp = Sexp::integer(42);
        assert_eq!(from_sexp::<i32>(&sexp).unwrap(), 42);
    }

    // --- integer coercion to float ---

    #[test]
    fn integer_to_float_coercion() {
        // when deserialize_f64 is called, integers should coerce
        assert_eq!(from_str::<f64>("42").unwrap(), 42.0);
    }

    // --- Sexp as serde value type ---

    #[test]
    fn sexp_as_deserialize_target() {
        let sexp: Sexp = from_str("(1 2 3)").unwrap();
        assert_eq!(
            sexp,
            Sexp::list(vec![Sexp::integer(1), Sexp::integer(2), Sexp::integer(3)])
        );
    }

    #[test]
    fn sexp_as_serialize_source() {
        let sexp = Sexp::list(vec![Sexp::symbol("hello"), Sexp::integer(42)]);
        let text = crate::serde::to_string(&sexp).unwrap();
        // symbols serialize as strings through the serde data model
        assert_eq!(text, "(\"hello\" 42)");
    }

    #[test]
    fn sexp_round_trip_nested() {
        // dotted lists flatten to sequences through the serde data model, and
        // symbols become strings — structural fidelity is intentionally limited.
        // use to_sexp/from_sexp directly for lossless Sexp↔Sexp conversion.
        //
        // dotted list (name . test) → serialises as ("name" "test") → deserialises as List
        let original = Sexp::list(vec![
            Sexp::string("config"),
            Sexp::dotted_list(vec![Sexp::string("name")], Sexp::string("test")),
            Sexp::boolean(true),
        ]);
        let text = crate::serde::to_string(&original).unwrap();
        let restored: Sexp = from_str(&text).unwrap();
        // dotted list becomes a flat list after the round-trip
        let expected = Sexp::list(vec![
            Sexp::string("config"),
            Sexp::list(vec![Sexp::string("name"), Sexp::string("test")]),
            Sexp::boolean(true),
        ]);
        assert_eq!(restored, expected);
    }

    #[test]
    fn sexp_in_struct_field() {
        use serde::{Deserialize, Serialize};
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Wrapper {
            tag: String,
            data: Sexp,
        }
        let w = Wrapper {
            tag: "test".to_string(),
            data: Sexp::list(vec![Sexp::integer(1), Sexp::integer(2)]),
        };
        let text = crate::serde::to_string(&w).unwrap();
        let restored: Wrapper = from_str(&text).unwrap();
        assert_eq!(w, restored);
    }

    // --- i128/u128 errors ---

    #[test]
    fn deserialize_i128_error_message() {
        let err = from_str::<i128>("42").unwrap_err();
        assert!(
            err.to_string().contains("i128"),
            "error should mention i128: {err}"
        );
    }

    #[test]
    fn deserialize_u128_error_message() {
        let err = from_str::<u128>("42").unwrap_err();
        assert!(
            err.to_string().contains("u128"),
            "error should mention u128: {err}"
        );
    }

    // --- alist with string keys ---

    #[test]
    fn round_trip_btreemap_string_keys() {
        use std::collections::BTreeMap;
        let mut m = BTreeMap::new();
        m.insert("name".to_string(), "alice".to_string());
        m.insert("role".to_string(), "admin".to_string());
        let text = crate::serde::to_string(&m).unwrap();
        let restored: BTreeMap<String, String> = from_str(&text).unwrap();
        assert_eq!(m, restored);
    }

    #[test]
    fn deserialize_any_string_key_alist_as_map() {
        // string-keyed alists must be detected by deserialize_any, not just typed deserialize_map.
        // this catches the regression where is_alist only recognised symbol keys.
        use serde::Deserialize;
        use std::collections::HashMap;

        // the outer map uses deserialize_map (typed), but the value uses deserialize_any
        // because the field type is dynamic. wrap in an untagged enum to force deserialize_any path.
        #[derive(Debug, Deserialize, PartialEq)]
        #[serde(untagged)]
        enum Dynamic {
            Map(HashMap<String, String>),
            Other(String),
        }

        // a string-keyed alist — serialised by BTreeMap<String, String>
        let text = r#"(("name" . "alice") ("role" . "admin"))"#;
        let result: Dynamic = from_str(text).unwrap();
        match result {
            Dynamic::Map(m) => {
                assert_eq!(m.get("name").unwrap(), "alice");
                assert_eq!(m.get("role").unwrap(), "admin");
            }
            other => panic!("expected Map variant, got {other:?}"),
        }
    }

    // --- serde attribute compatibility ---

    #[test]
    fn serde_rename_field() {
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Renamed {
            #[serde(rename = "full-name")]
            name: String,
        }
        let r = Renamed {
            name: "alice".to_string(),
        };
        let text = crate::serde::to_string(&r).unwrap();
        assert!(text.contains("full-name"), "got: {text}");
        let restored: Renamed = from_str(&text).unwrap();
        assert_eq!(r, restored);
    }

    #[test]
    fn serde_default_missing_field() {
        #[derive(Debug, Deserialize, PartialEq)]
        struct WithDefault {
            name: String,
            #[serde(default)]
            count: i32,
        }
        let d: WithDefault = from_str("((name . \"alice\"))").unwrap();
        assert_eq!(d.name, "alice");
        assert_eq!(d.count, 0);
    }

    #[test]
    fn serde_skip_serializing_if() {
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Sparse {
            name: String,
            #[serde(skip_serializing_if = "Option::is_none")]
            email: Option<String>,
        }
        let s = Sparse {
            name: "alice".to_string(),
            email: None,
        };
        let text = crate::serde::to_string(&s).unwrap();
        assert!(!text.contains("email"), "should skip None: {text}");
    }

    #[test]
    fn serde_flatten() {
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Base {
            name: String,
        }
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Extended {
            #[serde(flatten)]
            base: Base,
            age: i32,
        }
        let e = Extended {
            base: Base {
                name: "alice".to_string(),
            },
            age: 30,
        };
        let text = crate::serde::to_string(&e).unwrap();
        let restored: Extended = from_str(&text).unwrap();
        assert_eq!(e, restored);
    }

    #[test]
    fn serde_internally_tagged_enum() {
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        #[serde(tag = "type")]
        enum Event {
            Click { x: i32, y: i32 },
            Keypress { key: String },
        }
        let e = Event::Click { x: 10, y: 20 };
        let text = crate::serde::to_string(&e).unwrap();
        let restored: Event = from_str(&text).unwrap();
        assert_eq!(e, restored);
    }

    #[test]
    fn serde_adjacently_tagged_enum() {
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        #[serde(tag = "t", content = "c")]
        enum Msg {
            Text(String),
            Number(i32),
        }
        let m = Msg::Text("hello".to_string());
        let text = crate::serde::to_string(&m).unwrap();
        let restored: Msg = from_str(&text).unwrap();
        assert_eq!(m, restored);
    }

    // --- edge cases ---

    #[test]
    fn round_trip_special_floats() {
        // NaN
        let text = crate::serde::to_string(&f64::NAN).unwrap();
        let restored: f64 = from_str(&text).unwrap();
        assert!(restored.is_nan());

        // infinity
        let text = crate::serde::to_string(&f64::INFINITY).unwrap();
        assert_eq!(from_str::<f64>(&text).unwrap(), f64::INFINITY);

        let text = crate::serde::to_string(&f64::NEG_INFINITY).unwrap();
        assert_eq!(from_str::<f64>(&text).unwrap(), f64::NEG_INFINITY);
    }

    #[test]
    fn round_trip_empty_struct() {
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Empty {}
        let e = Empty {};
        let text = crate::serde::to_string(&e).unwrap();
        let restored: Empty = from_str(&text).unwrap();
        assert_eq!(e, restored);
    }

    #[test]
    fn round_trip_newtype_struct() {
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Wrapper(i32);
        let w = Wrapper(42);
        let text = crate::serde::to_string(&w).unwrap();
        let restored: Wrapper = from_str(&text).unwrap();
        assert_eq!(w, restored);
    }

    #[test]
    fn round_trip_tuple_struct() {
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Pair(i32, String);
        let p = Pair(42, "hello".to_string());
        let text = crate::serde::to_string(&p).unwrap();
        let restored: Pair = from_str(&text).unwrap();
        assert_eq!(p, restored);
    }

    #[test]
    fn round_trip_unit_struct() {
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        struct Marker;
        let m = Marker;
        let text = crate::serde::to_string(&m).unwrap();
        let restored: Marker = from_str(&text).unwrap();
        assert_eq!(m, restored);
    }

    #[test]
    fn round_trip_unicode_strings() {
        let text = crate::serde::to_string(&"héllo wörld 🌍").unwrap();
        let restored: String = from_str(&text).unwrap();
        assert_eq!(restored, "héllo wörld 🌍");
    }

    #[test]
    fn round_trip_escaped_strings() {
        let s = "line1\nline2\ttab\\backslash\"quote";
        let text = crate::serde::to_string(&s).unwrap();
        let restored: String = from_str(&text).unwrap();
        assert_eq!(restored, s);
    }

    #[test]
    fn round_trip_integer_map_keys() {
        let mut m = BTreeMap::new();
        m.insert(1i32, "one".to_string());
        m.insert(2, "two".to_string());
        let text = crate::serde::to_string(&m).unwrap();
        let restored: BTreeMap<i32, String> = from_str(&text).unwrap();
        assert_eq!(m, restored);
    }

    #[test]
    fn serde_untagged_enum() {
        #[derive(Debug, Serialize, Deserialize, PartialEq)]
        #[serde(untagged)]
        enum StringOrInt {
            Int(i32),
            Str(String),
        }
        let a = StringOrInt::Int(42);
        let text_a = crate::serde::to_string(&a).unwrap();
        let restored_a: StringOrInt = from_str(&text_a).unwrap();
        assert_eq!(a, restored_a);

        let b = StringOrInt::Str("hello".to_string());
        let text_b = crate::serde::to_string(&b).unwrap();
        let restored_b: StringOrInt = from_str(&text_b).unwrap();
        assert_eq!(b, restored_b);
    }
}
