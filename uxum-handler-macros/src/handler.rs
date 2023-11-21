use std::collections::HashMap;

use darling::FromMeta;
use proc_macro2::TokenStream;
use quote::{quote, ToTokens, TokenStreamExt};

#[derive(Debug, FromMeta)]
pub struct OpenApiExternalDoc {
    #[darling(default)]
    description: Option<String>,
    url: String,
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
    tags: Vec<String>,
    #[darling(default)]
    summary: Option<String>,
    #[darling(default)]
    description: Option<String>,
    #[darling(default)]
    external_docs: Option<OpenApiExternalDoc>,
    #[darling(default)]
    operation_id: Option<String>,
    #[darling(default, multiple)]
    parameters: Vec<OpenApiParameter>,
    //request_body:
    #[darling(default)]
    responses: OpenApiResponses,
    #[darling(default)]
    callbacks: HashMap<String, OpenApiCallback>,
    #[darling(default)]
    deprecated: bool,
    #[darling(default, multiple)]
    security: Vec<OpenApiSecurity>,
    #[darling(default, multiple)]
    servers: Vec<OpenApiServer>,
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
            Self::Get => quote! { ::http::Method::GET },
            Self::Head => quote! { ::http::Method::HEAD },
            Self::Post => quote! { ::http::Method::POST },
            Self::Put => quote! { ::http::Method::PUT },
            Self::Delete => quote! { ::http::Method::DELETE },
            Self::Options => quote! { ::http::Method::OPTIONS },
            Self::Trace => quote! { ::http::Method::TRACE },
            Self::Patch => quote! { ::http::Method::PATCH },
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
