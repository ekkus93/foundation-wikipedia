//! Recursive duplicate-object-key rejection for Enterprise JSON.
//! serde_json::Value alone keeps only the last repeated key, which can silently
//! overwrite upstream page, revision and source-provenance identities.

use std::fmt;

use serde::de::{Error as DeError, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::{Map, Number, Value};

struct StrictValue(Value);

impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: Deserializer<'de>>(decoder: D) -> Result<Self, D::Error> {
        decoder.deserialize_any(StrictVisitor)
    }
}

struct StrictVisitor;

impl<'de> Visitor<'de> for StrictVisitor {
    type Value = StrictValue;

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON value with no duplicate object keys")
    }

    fn visit_unit<E: DeError>(self) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Null))
    }

    fn visit_none<E: DeError>(self) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Null))
    }

    fn visit_some<D: Deserializer<'de>>(self, decoder: D) -> Result<Self::Value, D::Error> {
        StrictValue::deserialize(decoder)
    }

    fn visit_bool<E: DeError>(self, value: bool) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Bool(value)))
    }

    fn visit_i64<E: DeError>(self, value: i64) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Number(Number::from(value))))
    }

    fn visit_u64<E: DeError>(self, value: u64) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::Number(Number::from(value))))
    }

    fn visit_f64<E: DeError>(self, value: f64) -> Result<Self::Value, E> {
        Number::from_f64(value)
            .map(Value::Number)
            .map(StrictValue)
            .ok_or_else(|| E::custom("nonfinite JSON number"))
    }

    fn visit_str<E: DeError>(self, value: &str) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::String(value.to_owned())))
    }

    fn visit_string<E: DeError>(self, value: String) -> Result<Self::Value, E> {
        Ok(StrictValue(Value::String(value)))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        let mut values = Vec::new();
        while let Some(value) = seq.next_element::<StrictValue>()? {
            values.push(value.0);
        }
        Ok(StrictValue(Value::Array(values)))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut values = Map::new();
        while let Some((key, value)) = map.next_entry::<String, StrictValue>()? {
            if values.insert(key, value.0).is_some() {
                return Err(A::Error::custom("duplicate JSON object key"));
            }
        }
        Ok(StrictValue(Value::Object(values)))
    }
}

pub(crate) fn parse(input: &str) -> Result<Value, serde_json::Error> {
    serde_json::from_str::<StrictValue>(input).map(|value| value.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_nested_duplicate_keys_and_equivalent_escape_spellings() {
        for source in [
            r#"{"page":42,"page":43}"#,
            r#"{"version":{"id":1,"id":2}}"#,
            r#"{"values":[{"x":1,"x":2}]}"#,
            r#"{"page":42,"p\u0061ge":43}"#,
        ] {
            assert!(parse(source).is_err(), "{source}");
        }
    }

    #[test]
    fn accepts_ordinary_multilingual_json_and_exact_integer_values() {
        let source = r#"{"name":"München","nested":[null,true,-3,0.25,{"id":9223372036854775807}]}"#;
        assert_eq!(parse(source).unwrap(), serde_json::from_str::<Value>(source).unwrap());
    }
}
