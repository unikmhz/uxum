use proc_macro2::TokenStream;
use quote::{quote, ToTokens, TokenStreamExt};
use syn::{ItemFn, ReturnType, Type};

///
pub(crate) enum ResponseTemplate {
    Default,
    Typed(Type),
}

impl ToTokens for ResponseTemplate {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let new_tokens = match self {
            Self::Default => quote! {
                openapi3::Responses {
                    responses: okapi::map! {
                        "200".into() => openapi3::RefOr::Object(openapi3::Response {
                            description: "Empty response".into(), // TODO: allow customization
                            ..Default::default()
                        }),
                    },
                    ..Default::default()
                }
            },
            Self::Typed(inner) => quote! {
                <#inner as uxum::GetResponseSchemas>::get_responses(gen)
            },
        };
        tokens.append_all(new_tokens)
    }
}

///
pub(crate) fn detect_responses(handler: &ItemFn) -> ResponseTemplate {
    match &handler.sig.output {
        ReturnType::Default => ResponseTemplate::Default,
        ReturnType::Type(_, t) => ResponseTemplate::Typed(*t.clone()),
    }
}
