//! Helpers for reading other services' replies into typed structs. Every
//! reply is deserialized into a struct that names the fields we use: a field
//! of the wrong type fails the reply, and fields the struct doesn't name are
//! dropped. Where a reply may leave something out, the field is an `Option`.

use serde::de::{Deserialize, Deserializer, Error as _};

// Spec: keel.JSN-3
/// A number some services send as a number and some as a string. A string
/// that is not a number fails the reply, as any other wrong type does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Number(pub f64);

impl<'de> Deserialize<'de> for Number {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum Either {
            Number(f64),
            Text(String),
        }
        match Either::deserialize(deserializer)? {
            Either::Number(n) => Ok(Number(n)),
            Either::Text(text) => text
                .trim()
                .parse()
                .map(Number)
                .map_err(|_| D::Error::custom(format!("not a number: {text:?}"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, serde::Deserialize)]
    struct Reply {
        #[serde(default)]
        duration: Option<f64>,
        #[serde(default)]
        credits: Option<Number>,
    }

    // Spec: keel.JSN-3
    #[test]
    fn replies_are_read_strictly_and_unnamed_fields_dropped() {
        let reply: Reply = serde_json::from_str(r#"{ "duration": 3.5, "credits": "6.5", "extra": [1] }"#).unwrap();
        assert_eq!(reply.duration, Some(3.5));
        assert_eq!(reply.credits, Some(Number(6.5)));
        let reply: Reply = serde_json::from_str("{}").unwrap();
        assert_eq!((reply.duration, reply.credits), (None, None), "a field left out is missing");
        assert!(serde_json::from_str::<Reply>(r#"{ "duration": "long" }"#).is_err(), "a wrong type fails");
        assert!(serde_json::from_str::<Reply>(r#"{ "credits": "many" }"#).is_err(), "a string that is no number fails");
    }
}
