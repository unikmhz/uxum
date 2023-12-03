use std::collections::HashMap;

use darling::FromMeta;
use proc_macro2::TokenStream;
use quote::{quote, ToTokens, TokenStreamExt};

#[derive(Debug, FromMeta)]
pub struct OpenApiExternalDoc {
    ///
    #[darling(default)]
    pub description: Option<String>,
    ///
    pub url: String,
}

#[derive(Debug, FromMeta)]
pub struct OpenApiParameter {
    // TODO: OpenApiParameter
}

#[derive(Debug, Default, FromMeta)]
pub struct OpenApiResponses {
    // TODO: write
}

#[derive(Debug, FromMeta)]
pub struct OpenApiCallback {
    // TODO: write
}

#[derive(Debug, FromMeta)]
pub struct OpenApiSecurity {
    // TODO: write
}

#[derive(Debug, FromMeta)]
pub struct OpenApiServer {
    // TODO: write
}

#[derive(Debug, Default, FromMeta)]
pub struct HandlerSpec {
    #[darling(default, multiple, rename = "tag")]
    pub tags: Vec<String>,
    #[darling(default)]
    pub summary: Option<String>,
    #[darling(default)]
    pub description: Option<String>,
    #[darling(default)]
    pub external_docs: Option<OpenApiExternalDoc>,
    #[darling(default)]
    pub operation_id: Option<String>,
    #[darling(default, multiple)]
    pub parameters: Vec<OpenApiParameter>,
    // TODO: remove? request_body:
    #[darling(default)]
    pub responses: OpenApiResponses,
    #[darling(default)]
    pub callbacks: HashMap<String, OpenApiCallback>,
    #[darling(default)]
    pub deprecated: bool,
    #[darling(default, multiple)]
    pub security: Vec<OpenApiSecurity>,
    #[darling(default, multiple)]
    pub servers: Vec<OpenApiServer>,
}

#[derive(Debug, Default, FromMeta)]
#[darling(default, rename_all = "SCREAMING_SNAKE_CASE")]
pub enum HandlerMethod {
    #[default]
    Get,
    Head,
    Post,
    Put,
    Delete,
    Options,
    Trace,
    Patch,
}

impl ToTokens for HandlerMethod {
    fn to_tokens(&self, stream: &mut TokenStream) {
        let new_tokens: TokenStream = match self {
            Self::Get => quote! { ::uxum::reexport::http::Method::GET },
            Self::Head => quote! { ::uxum::reexport::http::Method::HEAD },
            Self::Post => quote! { ::uxum::reexport::http::Method::POST },
            Self::Put => quote! { ::uxum::reexport::http::Method::PUT },
            Self::Delete => quote! { ::uxum::reexport::http::Method::DELETE },
            Self::Options => quote! { ::uxum::reexport::http::Method::OPTIONS },
            Self::Trace => quote! { ::uxum::reexport::http::Method::TRACE },
            Self::Patch => quote! { ::uxum::reexport::http::Method::PATCH },
        };
        stream.append_all(new_tokens);
    }
}

#[derive(Debug, FromMeta)]
pub struct HandlerData {
    #[darling(default)]
    pub name: Option<String>,
    #[darling(default)]
    pub path: Option<String>,
    #[darling(default)]
    pub method: HandlerMethod,
    #[darling(default)]
    pub spec: HandlerSpec,
}
