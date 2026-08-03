use serde::de::{self, Deserializer, Unexpected, Visitor};
use serde::Deserialize;
use std::fmt;

struct F64OrString;

impl<'de> Visitor<'de> for F64OrString {
    type Value = f64;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a number or a string containing a number")
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<f64, E> {
        Ok(v)
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<f64, E> {
        Ok(v as f64)
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<f64, E> {
        Ok(v as f64)
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<f64, E> {
        v.trim()
            .parse::<f64>()
            .map_err(|_| de::Error::invalid_value(Unexpected::Str(v), &self))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<f64, E> {
        self.visit_str(&v)
    }
}

pub fn flex_f64<'de, D>(deserializer: D) -> Result<f64, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(F64OrString)
}

pub fn flex_f64_opt<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(opt.and_then(|v| {
        v.as_f64()
            .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
    }))
}

struct U64OrString;

impl<'de> Visitor<'de> for U64OrString {
    type Value = u64;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a u64 or a string containing a u64")
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<u64, E> {
        u64::try_from(v).map_err(|_| de::Error::invalid_value(Unexpected::Signed(v), &self))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<u64, E> {
        Ok(v)
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<u64, E> {
        v.trim()
            .parse::<u64>()
            .map_err(|_| de::Error::invalid_value(Unexpected::Str(v), &self))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<u64, E> {
        self.visit_str(&v)
    }
}

pub fn flex_u64<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    deserializer.deserialize_any(U64OrString)
}

pub fn flex_u32<'de, D>(deserializer: D) -> Result<u32, D::Error>
where
    D: Deserializer<'de>,
{
    let value = flex_u64(deserializer)?;
    u32::try_from(value)
        .map_err(|_| de::Error::invalid_value(Unexpected::Unsigned(value), &"a u32 within range"))
}
