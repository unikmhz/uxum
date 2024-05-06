use axum::http::StatusCode;
use okapi::openapi3;
use schemars::gen::SchemaGenerator;

/// Object for documenting handler responses as OpenAPI schema
pub struct ResponseSchema {
    /// HTTP status code
    pub status: StatusCode,
    /// OpenAPI response spec
    pub response: openapi3::Response,
}

pub trait GetResponseSchemas {
    /// Iterator over all available responses
    type ResponseIter: IntoIterator<Item = ResponseSchema>;

    /// Get all available responses
    #[must_use]
    fn get_response_schemas(gen: &mut schemars::gen::SchemaGenerator) -> Self::ResponseIter;

    /// Convert responses into [`openapi3`] object
    #[must_use]
    fn get_responses(gen: &mut schemars::gen::SchemaGenerator) -> openapi3::Responses {
        openapi3::Responses {
            responses: Self::get_response_schemas(gen)
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

    impl GetResponseSchemas for String {
        type ResponseIter = [ResponseSchema; 1];
        fn get_response_schemas(gen: &mut SchemaGenerator) -> Self::ResponseIter {
            [ResponseSchema {
                status: StatusCode::OK,
                response: openapi3::Response {
                    description: "UTF-8 string response".into(), // TODO: allow customization
                    content: okapi::map! {
                        "text/plain; charset=utf-8".into() => openapi3::MediaType {
                            schema: Some(gen.subschema_for::<String>().into_object()),
                            ..Default::default()
                        },
                    },
                    ..Default::default()
                },
            }]
        }
    }

    impl GetResponseSchemas for str {
        type ResponseIter = [ResponseSchema; 1];
        fn get_response_schemas(gen: &mut SchemaGenerator) -> Self::ResponseIter {
            <String as GetResponseSchemas>::get_response_schemas(gen)
        }
    }

    impl<'a, T> GetResponseSchemas for &'a T
    where
        T: GetResponseSchemas + ?Sized,
    {
        type ResponseIter = T::ResponseIter;
        fn get_response_schemas(gen: &mut SchemaGenerator) -> Self::ResponseIter {
            T::get_response_schemas(gen)
        }
    }

    impl<T> GetResponseSchemas for Box<T>
    where
        T: GetResponseSchemas,
    {
        type ResponseIter = T::ResponseIter;
        fn get_response_schemas(gen: &mut schemars::gen::SchemaGenerator) -> Self::ResponseIter {
            T::get_response_schemas(gen)
        }
    }

    impl<T> GetResponseSchemas for Json<T>
    where
        T: JsonSchema,
    {
        type ResponseIter = [ResponseSchema; 1];
        fn get_response_schemas(gen: &mut SchemaGenerator) -> Self::ResponseIter {
            [ResponseSchema {
                status: StatusCode::OK,
                response: openapi3::Response {
                    description: "Serialized JSON".into(), // FIXME: get from schema
                    content: okapi::map! {
                        "application/json".into() => openapi3::MediaType {
                            schema: Some(gen.subschema_for::<T>().into_object()),
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
        fn get_response_schemas(gen: &mut SchemaGenerator) -> Self::ResponseIter {
            T::get_response_schemas(gen)
                .into_iter()
                // FIXME: This will overwrite T's responses if E has responses with same status
                // codes.
                .chain(E::get_response_schemas(gen))
                .collect()
        }
    }
}
