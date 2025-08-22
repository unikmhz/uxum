//! CORS [`tower`] layer.

use std::{str::FromStr, time::Duration};

use axum::http::{header, HeaderName, HeaderValue};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tower_http::cors::{Any, CorsLayer};

/// Error type returned by CORS module.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CorsError {
    /// Invalid CORS origin.
    #[error("Invalid CORS origin")]
    InvalidOrigin(header::InvalidHeaderValue),
    /// Invalid CORS header.
    #[error("Invalid CORS header")]
    InvalidHeader(header::InvalidHeaderName),
}

/// Allow either any value, or listed values.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum AnyOr<T> {
    #[default]
    Any,
    Some(Vec<T>),
}

/// CORS configuration for a handler.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[non_exhaustive]
pub struct CorsConfig {
    /// Control [`Access-Control-Allow-Origin`][mdn] header.
    ///
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Allow-Origin
    origins: AnyOr<String>,
    /// Control [`Access-Control-Allow-Credentials`][mdn] header.
    ///
    /// `false` excludes the header from response.
    ///
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Allow-Credentials
    #[serde(default)]
    credentials: bool,
    /// Control [`Access-Control-Allow-Headers`][mdn] header.
    ///
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Allow-Headers
    #[serde(default)]
    headers: Option<AnyOr<String>>,
    /// Control [`Access-Control-Max-Age`][mdn] header.
    ///
    /// [mdn]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Access-Control-Max-Age
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "humantime_serde"
    )]
    max_age: Option<Duration>,
}

impl CorsConfig {
    /// Create CORS [`tower`] layer.
    ///
    /// # Errors
    ///
    /// Returns `Err` if:
    /// * Origin cannot be transformed into HTTP header value encoding.
    /// * Some of header names cannot be transformed into HTTP header name encoding.
    pub fn make_layer(&self) -> Result<CorsLayer, CorsError> {
        let mut layer = CorsLayer::new();
        layer = match &self.origins {
            AnyOr::Any => layer.allow_origin(Any),
            AnyOr::Some(origins) => {
                let headers = origins
                    .iter()
                    .map(|origin| HeaderValue::from_str(origin))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(CorsError::InvalidOrigin)?;
                layer.allow_origin(headers)
            }
        };
        layer = match self.credentials {
            true => layer.allow_credentials(true),
            false => layer,
        };
        layer = match &self.headers {
            None => layer,
            Some(AnyOr::Any) => layer.allow_headers(Any),
            Some(AnyOr::Some(headers)) => {
                let headers = headers
                    .iter()
                    .map(|origin| HeaderName::from_str(origin))
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(CorsError::InvalidHeader)?;
                layer.allow_headers(headers)
            }
        };
        if let Some(max_age) = self.max_age {
            layer = layer.max_age(max_age);
        }
        Ok(layer)
    }
}

mod serde_impls {
    use std::{fmt, marker::PhantomData};

    use serde::{de, ser::SerializeSeq, Deserializer, Serializer};

    use super::*;

    #[doc(hidden)]
    struct AnyOrVisitor<T> {
        marker: PhantomData<T>,
    }

    impl<'de, T: Deserialize<'de>> de::Visitor<'de> for AnyOrVisitor<T> {
        type Value = AnyOr<T>;

        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("'any' string or a sequence")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let mut vec = match seq.size_hint() {
                Some(hint) => Vec::with_capacity(hint),
                None => Vec::new(),
            };
            while let Some(el) = seq.next_element()? {
                vec.push(el);
            }
            Ok(AnyOr::Some(vec))
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            match v.to_ascii_lowercase().as_str() {
                "any" => Ok(AnyOr::Any),
                other => Err(E::custom(format_args!(
                    "expecting string 'any', got '{other}'"
                ))),
            }
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(AnyOr::Any)
        }
    }

    impl<'de, T: Deserialize<'de>> Deserialize<'de> for AnyOr<T> {
        fn deserialize<D>(deser: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            let visitor = AnyOrVisitor {
                marker: PhantomData,
            };
            deser.deserialize_any(visitor)
        }
    }

    impl<T: Serialize> Serialize for AnyOr<T> {
        fn serialize<S>(&self, ser: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            match self {
                Self::Any => ser.serialize_str("any"),
                Self::Some(vals) => {
                    let mut seq = ser.serialize_seq(Some(vals.len()))?;
                    for val in vals {
                        seq.serialize_element(val)?;
                    }
                    seq.end()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{from_str, json, to_value};

    use super::*;

    #[derive(Debug, Default, Deserialize, Serialize, PartialEq)]
    struct TestData {
        #[serde(default)]
        string_param: AnyOr<String>,
        #[serde(default)]
        int_param: AnyOr<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        opt_int_param: Option<AnyOr<i32>>,
    }

    /// Deserialize - all "any".
    #[test]
    fn anyor_de_all_any() {
        let serialized = r#"{
            "string_param": "any",
            "int_param": "Any",
            "opt_int_param": "ANY"
        }"#;
        let deserialized: TestData = from_str(serialized).unwrap();
        assert_eq!(
            deserialized,
            TestData {
                string_param: AnyOr::Any,
                int_param: AnyOr::Any,
                opt_int_param: Some(AnyOr::Any),
            }
        );
    }

    /// Deserialize - all "or".
    #[test]
    fn anyor_de_all_or() {
        let serialized = r#"{
            "string_param": ["v1", "v2"],
            "int_param": [11, 22],
            "opt_int_param": [33, 44]
        }"#;
        let deserialized: TestData = from_str(serialized).unwrap();
        assert_eq!(
            deserialized,
            TestData {
                string_param: AnyOr::Some(vec!["v1".into(), "v2".into()]),
                int_param: AnyOr::Some(vec![11, 22]),
                opt_int_param: Some(AnyOr::Some(vec![33, 44])),
            }
        );
    }

    /// Deserialize - default values.
    #[test]
    fn anyor_de_default() {
        let serialized = "{}";
        let deserialized: TestData = from_str(serialized).unwrap();
        assert_eq!(
            deserialized,
            TestData {
                string_param: AnyOr::Any,
                int_param: AnyOr::Any,
                opt_int_param: None,
            }
        );
    }

    /// Deserialize - invalid any string.
    #[test]
    fn anyor_de_invalid_any() {
        let serialized = r#"{
            "string_param": "whatever",
            "int_param": [11, 22],
            "opt_int_param": [33, 44]
        }"#;
        assert!(from_str::<TestData>(serialized).is_err());
    }

    /// Serialize - all "any".
    #[test]
    fn anyor_ser_all_any() {
        let deserialized = TestData {
            string_param: AnyOr::Any,
            int_param: AnyOr::Any,
            opt_int_param: Some(AnyOr::Any),
        };
        let serialized = to_value(deserialized).unwrap();
        assert_eq!(
            serialized,
            json!({
                "string_param": "any",
                "int_param": "any",
                "opt_int_param": "any"
            })
        );
    }

    /// Serialize - all "or".
    #[test]
    fn anyor_ser_all_or() {
        let deserialized = TestData {
            string_param: AnyOr::Some(vec!["v1".into(), "v2".into()]),
            int_param: AnyOr::Some(vec![11, 22]),
            opt_int_param: Some(AnyOr::Some(vec![33, 44])),
        };
        let serialized = to_value(deserialized).unwrap();
        assert_eq!(
            serialized,
            json!({
                "string_param": ["v1", "v2"],
                "int_param": [11, 22],
                "opt_int_param": [33, 44]
            })
        );
    }

    /// Serialize - default values.
    #[test]
    fn anyor_ser_default() {
        let deserialized = TestData {
            string_param: AnyOr::default(),
            int_param: AnyOr::default(),
            opt_int_param: Option::default(),
        };
        let serialized = to_value(deserialized).unwrap();
        assert_eq!(
            serialized,
            json!({
                "string_param": "any",
                "int_param": "any"
            })
        );
    }
}
