//! Misc utility functions and traits.

pub(crate) mod env;
pub(crate) mod fs;

use std::{
    convert::Infallible,
    future::Future,
    ops::{Deref, DerefMut},
    pin::Pin,
    task::{Context, Poll, ready},
};

use axum::{
    http::Request,
    response::{IntoResponse, IntoResponseParts, Response, ResponseParts},
};
use pin_project::pin_project;
use tower::{Layer, Service};

/// Helper function used for default boolean values in [`serde`].
///
/// Always returns `true`.
#[must_use]
#[inline]
pub(crate) fn default_true() -> bool {
    true
}

/// Helper function used for default boolean values in [`serde`].
///
/// Always returns `false`.
#[must_use]
#[inline]
pub(crate) fn default_false() -> bool {
    false
}

/// Response layer for adding an extension.
#[derive(Debug, Clone, Copy, Default)]
#[must_use]
#[non_exhaustive]
pub struct ResponseExtension<T>(pub T);

impl<T> Deref for ResponseExtension<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for ResponseExtension<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> IntoResponseParts for ResponseExtension<T>
where
    T: Clone + Send + Sync + 'static,
{
    type Error = Infallible;

    fn into_response_parts(self, mut res: ResponseParts) -> Result<ResponseParts, Self::Error> {
        res.extensions_mut().insert(self.0);
        Ok(res)
    }
}

impl<T> IntoResponse for ResponseExtension<T>
where
    T: Clone + Send + Sync + 'static,
{
    fn into_response(self) -> Response {
        let mut res = ().into_response();
        res.extensions_mut().insert(self.0);
        res
    }
}

impl<S, T> Layer<S> for ResponseExtension<T>
where
    T: Clone + Send + Sync + 'static,
{
    type Service = AddResponseExtension<S, T>;

    fn layer(&self, inner: S) -> Self::Service {
        AddResponseExtension {
            inner,
            value: self.0.clone(),
        }
    }
}

/// Middleware for adding extensions to response.
#[derive(Clone, Copy, Debug)]
pub struct AddResponseExtension<S, T> {
    /// Inner service.
    pub(crate) inner: S,
    /// Value to insert as a response extension.
    pub(crate) value: T,
}

impl<B, S, T, U> Service<Request<B>> for AddResponseExtension<S, T>
where
    S: Service<Request<B>, Response = Response<U>>,
    T: Clone + Send + Sync + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ResponseExtensionFuture<S::Future, T>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        ResponseExtensionFuture {
            inner: self.inner.call(req),
            value: self.value.clone(),
        }
    }
}

/// Response future for [`AddResponseExtension`].
#[pin_project]
pub struct ResponseExtensionFuture<F, T> {
    /// Inner future.
    #[pin]
    inner: F,
    /// Value to insert as a response extension.
    value: T,
}

impl<F, T, U, E> Future for ResponseExtensionFuture<F, T>
where
    F: Future<Output = Result<Response<U>, E>>,
    T: Clone + Send + Sync + 'static,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let resp_result = ready!(this.inner.poll(cx));
        Poll::Ready(resp_result.map(|mut resp| {
            resp.extensions_mut().insert(this.value.clone());
            resp
        }))
    }
}

/// Helper enum for deserialization.
#[derive(Clone, Debug, PartialEq)]
#[repr(transparent)]
pub struct OptVec<T>(Vec<T>);

impl<T> Deref for OptVec<T> {
    type Target = Vec<T>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for OptVec<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> AsRef<[T]> for OptVec<T> {
    fn as_ref(&self) -> &[T] {
        &self.0
    }
}

mod serde_impls {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    use super::*;

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OptVecInner<T> {
        Scalar(Option<T>),
        Vector(Vec<T>),
    }

    impl<'de, T: Deserialize<'de>> Deserialize<'de> for OptVec<T> {
        fn deserialize<D: Deserializer<'de>>(deser: D) -> Result<Self, D::Error> {
            match <OptVecInner<T> as Deserialize<'de>>::deserialize(deser) {
                Ok(OptVecInner::Scalar(Some(el))) => Ok(OptVec(vec![el])),
                Ok(OptVecInner::Scalar(None)) => Ok(OptVec(Vec::new())),
                Ok(OptVecInner::Vector(vec)) => Ok(OptVec(vec)),
                Err(err) => Err(err),
            }
        }
    }

    impl<T: Serialize> Serialize for OptVec<T> {
        fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
            match self.0.len() {
                0 => ser.serialize_none(),
                1 => self.0[0].serialize(ser),
                _ => self.0.serialize(ser),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use serde_json::{from_str, json, to_value};

    use super::*;

    #[derive(Debug, Deserialize, PartialEq, Serialize)]
    struct TestStruct {
        field: OptVec<i32>,
    }

    #[test]
    fn optvec_null() {
        let serialized = r#"{
            "field": null
        }"#;
        let deserialized: TestStruct = from_str(serialized).unwrap();
        assert_eq!(
            deserialized,
            TestStruct {
                field: OptVec(Vec::new()),
            },
        );
        let serialized = to_value(deserialized).unwrap();
        assert_eq!(
            serialized,
            json! {{
                "field": null
            }}
        );
    }

    #[test]
    fn optvec_single() {
        let serialized = r#"{
            "field": 123
        }"#;
        let deserialized: TestStruct = from_str(serialized).unwrap();
        assert_eq!(
            deserialized,
            TestStruct {
                field: OptVec(vec![123]),
            },
        );
        let serialized = to_value(deserialized).unwrap();
        assert_eq!(
            serialized,
            json! {{
                "field": 123
            }}
        );
    }

    #[test]
    fn optvec_multiple() {
        let serialized = r#"{
            "field": [123, 234]
        }"#;
        let deserialized: TestStruct = from_str(serialized).unwrap();
        assert_eq!(
            deserialized,
            TestStruct {
                field: OptVec(vec![123, 234]),
            },
        );
        let serialized = to_value(deserialized).unwrap();
        assert_eq!(
            serialized,
            json! {{
                "field": [123, 234]
            }}
        );
    }
}
