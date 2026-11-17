//! Helpers for reading other services' replies into typed structs. Every
//! reply is deserialized into a struct that names the fields we use; these
//! keep one odd field from failing a whole reply.

use serde::de::{Deserialize, Deserializer, IgnoredAny};

/// A field read leniently: a value of the wrong type counts as missing,
/// instead of failing the whole reply. Use with `#[serde(default,
/// deserialize_with = "crate::wire::lenient")]` on an `Option<T>`.
pub fn lenient<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum Either<T> {
        Value(T),
        Other(IgnoredAny),
    }
    Ok(match Option::<Either<T>>::deserialize(deserializer)? {
        Some(Either::Value(value)) => Some(value),
        _ => None,
    })
}

/// A list element read leniently: one of the wrong shape is `None`, not a
/// failed list.
#[derive(Debug, Clone, PartialEq)]
pub struct Lenient<T>(pub Option<T>);

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Lenient<T> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum Either<T> {
            Value(T),
            Other(IgnoredAny),
        }
        Ok(Lenient(match Either::<T>::deserialize(deserializer)? {
            Either::Value(value) => Some(value),
            Either::Other(_) => None,
        }))
    }
}

/// The items of a list that had the right shape.
pub fn kept<T>(list: Vec<Lenient<T>>) -> impl Iterator<Item = T> {
    list.into_iter().filter_map(|item| item.0)
}

/// A number some services send as a number and some as a string.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(untagged)]
pub enum Number {
    Number(f64),
    Text(String),
}

impl Number {
    pub fn value(&self) -> Option<f64> {
        match self {
            Number::Number(n) => Some(*n),
            Number::Text(text) => text.trim().parse().ok(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(serde::Deserialize)]
    struct Reply {
        #[serde(default, deserialize_with = "lenient")]
        duration: Option<f64>,
        #[serde(default)]
        credits: Option<Number>,
    }

    #[test]
    fn odd_fields_do_not_fail_a_reply() {
        let reply: Reply = serde_json::from_str(r#"{ "duration": "long", "credits": "6.5" }"#).unwrap();
        assert_eq!(reply.duration, None);
        assert_eq!(reply.credits.and_then(|c| c.value()), Some(6.5));
        let reply: Reply = serde_json::from_str(r#"{ "duration": 3.5 }"#).unwrap();
        assert_eq!(reply.duration, Some(3.5));
    }
}
