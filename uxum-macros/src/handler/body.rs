use proc_macro2::TokenStream;
use quote::{quote, ToTokens, TokenStreamExt};
use syn::{
    AngleBracketedGenericArguments, FnArg, GenericArgument, ItemFn, Path, PathArguments, Type,
    TypePath,
};

/// Type of detected request body.
pub(crate) enum RequestBody {
    /// UTF-8 string.
    String,
    /// Binary data.
    Bytes,
    /// HTTP form input.
    Form,
    /// Some type serialized as JSON.
    Json(Path),
}

impl ToTokens for RequestBody {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let media_type = self.media_type();
        let schema = match self {
            Self::String => quote! { gen.subschema_for::<String>().into_object() },
            Self::Bytes => quote! { gen.subschema_for::<bytes::Bytes>().into_object() },
            Self::Form => return, // TODO: write this.
            Self::Json(path) => quote! { gen.subschema_for::<#path>().into_object() },
        };
        tokens.append_all(quote! {
            openapi3::RequestBody {
                description: None,
                content: okapi::map! {
                    #media_type.into() => openapi3::MediaType {
                        schema: Some(#schema),
                        example: None,
                        examples: None,
                        encoding: Default::default(),
                        extensions: Default::default(),
                    },
                },
                required: true, // TODO: optional request bodies.
                extensions: Default::default(),
            }
        })
    }
}

impl RequestBody {
    /// Get MIME type based on request body type.
    #[must_use]
    fn media_type(&self) -> &'static str {
        match self {
            Self::String => mime::TEXT_PLAIN_UTF_8.as_ref(),
            Self::Bytes => mime::APPLICATION_OCTET_STREAM.as_ref(),
            Self::Form => mime::APPLICATION_WWW_FORM_URLENCODED.as_ref(),
            Self::Json(_) => mime::APPLICATION_JSON.as_ref(),
        }
    }
}

/// Detect request body extractor inside handler function signature.
#[must_use]
pub(crate) fn detect_request_body(handler: &ItemFn) -> Option<RequestBody> {
    handler.sig.inputs.iter().find_map(|input| match input {
        FnArg::Typed(arg_type) => match arg_type.ty.as_ref() {
            Type::Path(path) => {
                path.path
                    .segments
                    .last()
                    .and_then(|seg| match seg.ident.to_string().as_str() {
                        // TODO: support other extractors.
                        "String" => Some(RequestBody::String),
                        "Bytes" => Some(RequestBody::Bytes),
                        // TODO: type inside Form.
                        "Form" => Some(RequestBody::Form),
                        "Json" => match &seg.arguments {
                            PathArguments::AngleBracketed(AngleBracketedGenericArguments {
                                args,
                                ..
                            }) if args.len() == 1 => match &args[0] {
                                GenericArgument::Type(Type::Path(TypePath { path, .. })) => {
                                    Some(RequestBody::Json(path.clone()))
                                }
                                _ => None,
                            },
                            _ => None,
                        },
                        _ => None,
                    })
            }
            // TODO: support other variants.
            _ => None,
        },
        FnArg::Receiver(_) => None,
    })
}
