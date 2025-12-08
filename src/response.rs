//! OpenAPI schema generation for handler responses.

use axum::http::StatusCode;
use okapi::openapi3;
use schemars::r#gen::SchemaGenerator;

/// Object for documenting handler responses as OpenAPI schema.
pub struct ResponseSchema {
    /// HTTP status code.
    pub status: StatusCode,
    /// OpenAPI response spec.
    pub response: openapi3::Response,
}

/// Trait used to generate OpenAPI schemas from handler response types.
pub trait GetResponseSchemas {
    /// Iterator over all available responses.
    type ResponseIter: IntoIterator<Item = ResponseSchema>;

    /// Get all available responses.
    #[must_use]
    fn get_response_schemas(r#gen: &mut schemars::r#gen::SchemaGenerator) -> Self::ResponseIter;

    /// Convert responses into [`openapi3::Responses`] object.
    #[must_use]
    fn get_responses(r#gen: &mut schemars::r#gen::SchemaGenerator) -> openapi3::Responses {
        openapi3::Responses {
            responses: Self::get_response_schemas(r#gen)
                .into_iter()
                .map(|sch| {
                    (
                        sch.status.as_u16().to_string(),
                        openapi3::RefOr::Object(sch.response),
                    )
                })
                .collect(),
            ..Default::default()
        }
    }
}

mod impls {
    use axum::Json;
    use okapi::schemars;
    use schemars::JsonSchema;

    use super::*;

    macro_rules! type_alias {
        ($new:ty, $old:ty) => {
            impl GetResponseSchemas for $new {
                type ResponseIter = <$old as GetResponseSchemas>::ResponseIter;

                fn get_response_schemas(r#gen: &mut SchemaGenerator) -> Self::ResponseIter {
                    <$old as GetResponseSchemas>::get_response_schemas(r#gen)
                }
            }
        };
    }

    impl GetResponseSchemas for String {
        type ResponseIter = [ResponseSchema; 1];

        fn get_response_schemas(r#gen: &mut SchemaGenerator) -> Self::ResponseIter {
            [ResponseSchema {
                status: StatusCode::OK,
                response: openapi3::Response {
                    description: "UTF-8 string response".into(), // TODO: allow customization.
                    content: okapi::map! {
                        "text/plain; charset=utf-8".into() => openapi3::MediaType {
                            schema: Some(r#gen.subschema_for::<String>().into_object()),
                            ..Default::default()
                        },
                    },
                    ..Default::default()
                },
            }]
        }
    }

    type_alias!(str, String);
    type_alias!(std::borrow::Cow<'static, str>, String);

    impl GetResponseSchemas for bytes::Bytes {
        type ResponseIter = [ResponseSchema; 1];

        fn get_response_schemas(r#gen: &mut SchemaGenerator) -> Self::ResponseIter {
            [ResponseSchema {
                status: StatusCode::OK,
                response: openapi3::Response {
                    description: "Binary response".into(), // TODO: allow customization.
                    content: okapi::map! {
                        "application/octet-stream".into() => openapi3::MediaType {
                            schema: Some(r#gen.subschema_for::<bytes::Bytes>().into_object()),
                            ..Default::default()
                        },
                    },
                    ..Default::default()
                },
            }]
        }
    }

    type_alias!(bytes::BytesMut, bytes::Bytes);
    type_alias!([u8], bytes::Bytes);
    type_alias!(Vec<u8>, bytes::Bytes);

    impl GetResponseSchemas for () {
        type ResponseIter = [ResponseSchema; 1];

        fn get_response_schemas(_gen: &mut SchemaGenerator) -> Self::ResponseIter {
            [ResponseSchema {
                status: StatusCode::OK,
                response: openapi3::Response {
                    description: "Empty response body".into(),
                    ..Default::default()
                },
            }]
        }
    }

    impl<T> GetResponseSchemas for &'_ T
    where
        T: GetResponseSchemas + ?Sized,
    {
        type ResponseIter = T::ResponseIter;
        fn get_response_schemas(r#gen: &mut SchemaGenerator) -> Self::ResponseIter {
            T::get_response_schemas(r#gen)
        }
    }

    impl<T> GetResponseSchemas for Box<T>
    where
        T: GetResponseSchemas,
    {
        type ResponseIter = T::ResponseIter;

        fn get_response_schemas(
            r#gen: &mut schemars::r#gen::SchemaGenerator,
        ) -> Self::ResponseIter {
            T::get_response_schemas(r#gen)
        }
    }

    impl<T> GetResponseSchemas for Json<T>
    where
        T: JsonSchema,
    {
        type ResponseIter = [ResponseSchema; 1];

        fn get_response_schemas(r#gen: &mut SchemaGenerator) -> Self::ResponseIter {
            [ResponseSchema {
                status: StatusCode::OK,
                response: openapi3::Response {
                    description: "Serialized JSON".into(), // FIXME: get from schema.
                    content: okapi::map! {
                        "application/json".into() => openapi3::MediaType {
                            schema: Some(r#gen.subschema_for::<T>().into_object()),
                            ..Default::default()
                        },
                    },
                    ..Default::default()
                },
            }]
        }
    }

    impl<T, E> GetResponseSchemas for Result<T, E>
    where
        T: GetResponseSchemas,
        E: GetResponseSchemas,
    {
        type ResponseIter = Vec<ResponseSchema>;

        fn get_response_schemas(r#gen: &mut SchemaGenerator) -> Self::ResponseIter {
            T::get_response_schemas(r#gen)
                .into_iter()
                // FIXME: This will overwrite T's responses if E has responses with same status
                // codes.
                .chain(E::get_response_schemas(r#gen))
                .collect()
        }
    }

    impl GetResponseSchemas for std::convert::Infallible {
        type ResponseIter = [ResponseSchema; 0];

        fn get_response_schemas(_gen: &mut SchemaGenerator) -> Self::ResponseIter {
            []
        }
    }
}
