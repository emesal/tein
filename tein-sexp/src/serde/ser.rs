//! serde serializer for s-expressions
//!
//! serializes rust types into [`Sexp`] values following scheme conventions:
//! - maps/structs become alists: `((key . val) ...)`
//! - sequences become lists: `(1 2 3)`
//! - Option::None becomes `()`
//! - enum variants use tagged lists: `(variant-name value ...)`

use crate::ast::Sexp;
use crate::error::ParseError;
use serde::ser;

/// serialize a value to an s-expression AST node
pub fn to_sexp<T: ser::Serialize>(value: &T) -> Result<Sexp, ParseError> {
    value.serialize(Serializer)
}

/// serde serializer that produces [`Sexp`] values
struct Serializer;

impl ser::Serializer for Serializer {
    type Ok = Sexp;
    type Error = ParseError;
    type SerializeSeq = SeqSerializer;
    type SerializeTuple = SeqSerializer;
    type SerializeTupleStruct = SeqSerializer;
    type SerializeTupleVariant = TupleVariantSerializer;
    type SerializeMap = MapSerializer;
    type SerializeStruct = MapSerializer;
    type SerializeStructVariant = StructVariantSerializer;

    fn serialize_bool(self, v: bool) -> Result<Sexp, ParseError> {
        Ok(Sexp::boolean(v))
    }

    fn serialize_i8(self, v: i8) -> Result<Sexp, ParseError> {
        self.serialize_i64(v as i64)
    }

    fn serialize_i16(self, v: i16) -> Result<Sexp, ParseError> {
        self.serialize_i64(v as i64)
    }

    fn serialize_i32(self, v: i32) -> Result<Sexp, ParseError> {
        self.serialize_i64(v as i64)
    }

    fn serialize_i64(self, v: i64) -> Result<Sexp, ParseError> {
        Ok(Sexp::integer(v))
    }

    fn serialize_u8(self, v: u8) -> Result<Sexp, ParseError> {
        self.serialize_i64(v as i64)
    }

    fn serialize_u16(self, v: u16) -> Result<Sexp, ParseError> {
        self.serialize_i64(v as i64)
    }

    fn serialize_u32(self, v: u32) -> Result<Sexp, ParseError> {
        self.serialize_i64(v as i64)
    }

    fn serialize_u64(self, v: u64) -> Result<Sexp, ParseError> {
        if v <= i64::MAX as u64 {
            self.serialize_i64(v as i64)
        } else {
            // silent f64 truncation would corrupt data; explicit error is safer
            Err(ParseError::no_span(format!(
                "u64 value {v} exceeds i64::MAX and cannot be represented losslessly"
            )))
        }
    }

    fn serialize_i128(self, _v: i128) -> Result<Sexp, ParseError> {
        Err(ParseError::no_span(
            "i128 cannot be represented in s-expressions (i64 max)",
        ))
    }

    fn serialize_u128(self, _v: u128) -> Result<Sexp, ParseError> {
        Err(ParseError::no_span(
            "u128 cannot be represented in s-expressions (i64 max)",
        ))
    }

    fn serialize_f32(self, v: f32) -> Result<Sexp, ParseError> {
        self.serialize_f64(v as f64)
    }

    fn serialize_f64(self, v: f64) -> Result<Sexp, ParseError> {
        Ok(Sexp::float(v))
    }

    fn serialize_char(self, v: char) -> Result<Sexp, ParseError> {
        Ok(Sexp::char(v))
    }

    fn serialize_str(self, v: &str) -> Result<Sexp, ParseError> {
        Ok(Sexp::string(v))
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Sexp, ParseError> {
        let items = v.iter().map(|&b| Sexp::integer(b as i64)).collect();
        Ok(Sexp::list(items))
    }

    fn serialize_none(self) -> Result<Sexp, ParseError> {
        Ok(Sexp::nil())
    }

    fn serialize_some<T: ser::Serialize + ?Sized>(self, value: &T) -> Result<Sexp, ParseError> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Sexp, ParseError> {
        Ok(Sexp::nil())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Sexp, ParseError> {
        Ok(Sexp::nil())
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<Sexp, ParseError> {
        Ok(Sexp::symbol(variant))
    }

    fn serialize_newtype_struct<T: ser::Serialize + ?Sized>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Sexp, ParseError> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ser::Serialize + ?Sized>(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Sexp, ParseError> {
        let inner = value.serialize(Serializer)?;
        Ok(Sexp::list(vec![Sexp::symbol(variant), inner]))
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<SeqSerializer, ParseError> {
        Ok(SeqSerializer {
            items: Vec::with_capacity(len.unwrap_or(0)),
        })
    }

    fn serialize_tuple(self, len: usize) -> Result<SeqSerializer, ParseError> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<SeqSerializer, ParseError> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<TupleVariantSerializer, ParseError> {
        Ok(TupleVariantSerializer {
            variant: variant.to_string(),
            items: Vec::with_capacity(len),
        })
    }

    fn serialize_map(self, len: Option<usize>) -> Result<MapSerializer, ParseError> {
        Ok(MapSerializer {
            entries: Vec::with_capacity(len.unwrap_or(0)),
            pending_key: None,
            use_symbol_keys: false,
        })
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<MapSerializer, ParseError> {
        Ok(MapSerializer {
            entries: Vec::with_capacity(len),
            pending_key: None,
            use_symbol_keys: true,
        })
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<StructVariantSerializer, ParseError> {
        Ok(StructVariantSerializer {
            variant: variant.to_string(),
            entries: Vec::with_capacity(len),
        })
    }
}

/// serializes a sequence/tuple as a list
pub struct SeqSerializer {
    items: Vec<Sexp>,
}

impl ser::SerializeSeq for SeqSerializer {
    type Ok = Sexp;
    type Error = ParseError;

    fn serialize_element<T: ser::Serialize + ?Sized>(
        &mut self,
        value: &T,
    ) -> Result<(), ParseError> {
        self.items.push(value.serialize(Serializer)?);
        Ok(())
    }

    fn end(self) -> Result<Sexp, ParseError> {
        Ok(Sexp::list(self.items))
    }
}

impl ser::SerializeTuple for SeqSerializer {
    type Ok = Sexp;
    type Error = ParseError;

    fn serialize_element<T: ser::Serialize + ?Sized>(
        &mut self,
        value: &T,
    ) -> Result<(), ParseError> {
        ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Sexp, ParseError> {
        ser::SerializeSeq::end(self)
    }
}

impl ser::SerializeTupleStruct for SeqSerializer {
    type Ok = Sexp;
    type Error = ParseError;

    fn serialize_field<T: ser::Serialize + ?Sized>(&mut self, value: &T) -> Result<(), ParseError> {
        ser::SerializeSeq::serialize_element(self, value)
    }

    fn end(self) -> Result<Sexp, ParseError> {
        ser::SerializeSeq::end(self)
    }
}

/// serializes a tuple variant as `(variant val1 val2 ...)`
pub struct TupleVariantSerializer {
    variant: String,
    items: Vec<Sexp>,
}

impl ser::SerializeTupleVariant for TupleVariantSerializer {
    type Ok = Sexp;
    type Error = ParseError;

    fn serialize_field<T: ser::Serialize + ?Sized>(&mut self, value: &T) -> Result<(), ParseError> {
        self.items.push(value.serialize(Serializer)?);
        Ok(())
    }

    fn end(self) -> Result<Sexp, ParseError> {
        let mut list = Vec::with_capacity(1 + self.items.len());
        list.push(Sexp::symbol(self.variant));
        list.extend(self.items);
        Ok(Sexp::list(list))
    }
}

/// serializes a map/struct as an alist: `((key . val) ...)`
pub struct MapSerializer {
    entries: Vec<Sexp>,
    pending_key: Option<Sexp>,
    /// true for structs (keys become symbols), false for maps (keys stay as-is)
    use_symbol_keys: bool,
}

impl ser::SerializeMap for MapSerializer {
    type Ok = Sexp;
    type Error = ParseError;

    fn serialize_key<T: ser::Serialize + ?Sized>(&mut self, key: &T) -> Result<(), ParseError> {
        self.pending_key = Some(key.serialize(Serializer)?);
        Ok(())
    }

    fn serialize_value<T: ser::Serialize + ?Sized>(&mut self, value: &T) -> Result<(), ParseError> {
        let key = self
            .pending_key
            .take()
            .expect("serialize_value called before serialize_key");
        let val = value.serialize(Serializer)?;
        self.entries.push(Sexp::dotted_list(vec![key], val));
        Ok(())
    }

    fn end(self) -> Result<Sexp, ParseError> {
        Ok(Sexp::list(self.entries))
    }
}

impl ser::SerializeStruct for MapSerializer {
    type Ok = Sexp;
    type Error = ParseError;

    fn serialize_field<T: ser::Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), ParseError> {
        let key_sexp = if self.use_symbol_keys {
            Sexp::symbol(key)
        } else {
            Sexp::string(key)
        };
        let val = value.serialize(Serializer)?;
        self.entries.push(Sexp::dotted_list(vec![key_sexp], val));
        Ok(())
    }

    fn end(self) -> Result<Sexp, ParseError> {
        Ok(Sexp::list(self.entries))
    }
}

/// serializes a struct variant as `(variant (field . val) ...)`
pub struct StructVariantSerializer {
    variant: String,
    entries: Vec<Sexp>,
}

impl ser::SerializeStructVariant for StructVariantSerializer {
    type Ok = Sexp;
    type Error = ParseError;

    fn serialize_field<T: ser::Serialize + ?Sized>(
        &mut self,
        key: &'static str,
        value: &T,
    ) -> Result<(), ParseError> {
        let val = value.serialize(Serializer)?;
        self.entries
            .push(Sexp::dotted_list(vec![Sexp::symbol(key)], val));
        Ok(())
    }

    fn end(self) -> Result<Sexp, ParseError> {
        let mut list = Vec::with_capacity(1 + self.entries.len());
        list.push(Sexp::symbol(self.variant));
        list.extend(self.entries);
        Ok(Sexp::list(list))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Serialize;
    use std::collections::BTreeMap;

    fn ser(value: &impl Serialize) -> String {
        let sexp = to_sexp(value).unwrap();
        sexp.to_string()
    }

    // --- primitives ---

    #[test]
    fn serialize_bool() {
        assert_eq!(ser(&true), "#t");
        assert_eq!(ser(&false), "#f");
    }

    #[test]
    fn serialize_integers() {
        assert_eq!(ser(&42i32), "42");
        assert_eq!(ser(&-7i64), "-7");
        assert_eq!(ser(&0u8), "0");
    }

    #[test]
    fn serialize_float() {
        assert_eq!(ser(&3.125f64), "3.125");
    }

    #[test]
    fn serialize_char() {
        assert_eq!(ser(&'a'), "#\\a");
        assert_eq!(ser(&' '), "#\\space");
    }

    #[test]
    fn serialize_string() {
        assert_eq!(ser(&"hello"), "\"hello\"");
        assert_eq!(ser(&String::from("world")), "\"world\"");
    }

    // --- option ---

    #[test]
    fn serialize_none() {
        assert_eq!(ser(&None::<i32>), "()");
    }

    #[test]
    fn serialize_some() {
        assert_eq!(ser(&Some(42)), "42");
    }

    // --- unit ---

    #[test]
    fn serialize_unit() {
        assert_eq!(ser(&()), "()");
    }

    // --- sequences ---

    #[test]
    fn serialize_vec() {
        assert_eq!(ser(&vec![1, 2, 3]), "(1 2 3)");
    }

    #[test]
    fn serialize_empty_vec() {
        assert_eq!(ser(&Vec::<i32>::new()), "()");
    }

    #[test]
    fn serialize_tuple() {
        assert_eq!(ser(&(1, "hello", true)), "(1 \"hello\" #t)");
    }

    // --- map ---

    #[test]
    fn serialize_map() {
        let mut m = BTreeMap::new();
        m.insert("name", "alice");
        m.insert("role", "admin");
        let result = ser(&m);
        assert_eq!(result, "((\"name\" . \"alice\") (\"role\" . \"admin\"))");
    }

    // --- struct ---

    #[derive(Serialize)]
    struct Point {
        x: i32,
        y: i32,
    }

    #[test]
    fn serialize_struct() {
        let p = Point { x: 1, y: 2 };
        assert_eq!(ser(&p), "((x . 1) (y . 2))");
    }

    #[derive(Serialize)]
    struct Config {
        name: String,
        debug: bool,
        count: i32,
    }

    #[test]
    fn serialize_struct_mixed_types() {
        let c = Config {
            name: "test".to_string(),
            debug: true,
            count: 42,
        };
        assert_eq!(ser(&c), "((name . \"test\") (debug . #t) (count . 42))");
    }

    // --- enums ---

    #[derive(Serialize)]
    #[allow(dead_code)]
    enum Color {
        Red,
        Green,
        Blue,
    }

    #[test]
    fn serialize_unit_variant() {
        assert_eq!(ser(&Color::Red), "Red");
        assert_eq!(ser(&Color::Green), "Green");
    }

    #[derive(Serialize)]
    enum Shape {
        Circle(f64),
        Rectangle(f64, f64),
        Labeled { name: String, sides: u32 },
    }

    #[test]
    fn serialize_newtype_variant() {
        assert_eq!(ser(&Shape::Circle(5.0)), "(Circle 5.0)");
    }

    #[test]
    fn serialize_tuple_variant() {
        assert_eq!(ser(&Shape::Rectangle(3.0, 4.0)), "(Rectangle 3.0 4.0)");
    }

    #[test]
    fn serialize_struct_variant() {
        let s = Shape::Labeled {
            name: "triangle".to_string(),
            sides: 3,
        };
        assert_eq!(ser(&s), "(Labeled (name . \"triangle\") (sides . 3))");
    }

    // --- nested ---

    #[derive(Serialize)]
    struct Nested {
        items: Vec<i32>,
        label: Option<String>,
    }

    #[test]
    fn serialize_nested_struct() {
        let n = Nested {
            items: vec![1, 2, 3],
            label: Some("test".to_string()),
        };
        assert_eq!(ser(&n), "((items . (1 2 3)) (label . \"test\"))");
    }

    #[test]
    fn serialize_nested_struct_none() {
        let n = Nested {
            items: vec![],
            label: None,
        };
        assert_eq!(ser(&n), "((items . ()) (label . ()))");
    }

    // --- bytes ---

    #[test]
    fn serialize_bytes() {
        use serde::Serializer;
        let sexp = super::Serializer.serialize_bytes(&[0x41, 0x42]).unwrap();
        assert_eq!(sexp.to_string(), "(65 66)");
    }

    // --- public api ---

    #[test]
    fn to_string_api() {
        let result = crate::serde::to_string(&42).unwrap();
        assert_eq!(result, "42");
    }

    #[test]
    fn to_string_pretty_api() {
        let result = crate::serde::to_string_pretty(&vec![1, 2, 3]).unwrap();
        assert_eq!(result, "(1 2 3)"); // short enough to stay compact
    }

    // --- i128/u128 errors ---

    #[test]
    fn serialize_i128_error_message() {
        let err = crate::serde::to_sexp(&42i128).unwrap_err();
        assert!(
            err.to_string().contains("i128"),
            "error should mention i128: {err}"
        );
    }

    #[test]
    fn serialize_u128_error_message() {
        let err = crate::serde::to_sexp(&42u128).unwrap_err();
        assert!(
            err.to_string().contains("u128"),
            "error should mention u128: {err}"
        );
    }

    // --- u64 overflow ---

    #[test]
    fn serialize_u64_max_errors() {
        let result = crate::serde::to_sexp(&u64::MAX);
        assert!(result.is_err(), "u64::MAX should error, not silently lose precision");
    }

    #[test]
    fn serialize_u64_fits_i64() {
        // values that fit in i64 should work fine
        let sexp = crate::serde::to_sexp(&(i64::MAX as u64)).unwrap();
        assert_eq!(sexp.to_string(), i64::MAX.to_string());
    }
}
