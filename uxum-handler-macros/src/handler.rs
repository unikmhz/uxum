use darling::FromMeta;
use proc_macro2::TokenStream;
use quote::{quote, ToTokens, TokenStreamExt};

use crate::util::quote_option;

///
#[derive(Debug, FromMeta)]
pub struct OpenApiExternalDoc {
    ///
    #[darling(default)]
    pub description: Option<String>,
    ///
    pub url: String,
}

impl ToTokens for OpenApiExternalDoc {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let description = quote_option(&self.description);
        let url = &self.url;
        tokens.append_all(quote! {
            ::uxum::reexport::openapi3::ExternalDocs {
                description: #description,
                url: #url,
                extensions: Default::default(),
            }
        });
    }
}

///
#[derive(Debug, FromMeta)]
pub struct OpenApiParameter {
    // TODO: OpenApiParameter
}

///
#[derive(Debug, FromMeta)]
pub struct OpenApiSecurity {
    // TODO: write
}

///
#[derive(Debug, FromMeta)]
pub struct OpenApiServer {
    ///
    pub url: String,
    ///
    #[darling(default)]
    pub description: Option<String>,
}

impl ToTokens for OpenApiServer {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let url = &self.url;
        let description = quote_option(&self.description);
        tokens.append_all(quote! {
            ::uxum::reexport::openapi3::Server {
                url: #url,
                description: #description,
                variables: Default::default(),
                extensions: Default::default(),
            }
        });
    }
}

///
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
    #[darling(default)]
    pub deprecated: bool,
    #[darling(default, multiple)]
    pub security: Vec<OpenApiSecurity>,
    #[darling(default, multiple, rename = "server")]
    pub servers: Vec<OpenApiServer>,
}

impl ToTokens for HandlerSpec {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let summary = quote_option(&self.summary);
        let description = quote_option(&self.description);
        let operation_id = quote_option(&self.operation_id);
        let deprecated = self.deprecated;
        tokens.append_all(quote! {
            ::uxum::reexport::openapi3::Operation {
                tags: vec![], // TODO: fill
                summary: #summary,
                description: #description,
                external_docs: None, // TODO: fill
                operation_id: #operation_id,
                parameters: vec![], // TODO: fill
                request_body: None, // TODO: fill
                responses: ::uxum::reexport::openapi3::Responses {
                    default: None, // TODO: fill
                    responses: Default::default(), // TODO: fill
                    extensions: Default::default(),
                },
                callbacks: Default::default(), // TODO: fill?
                deprecated: #deprecated,
                security: None,
                servers: None,
                extensions: Default::default(), // TODO: fill?
            }
        });
    }
}

///
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

///
#[derive(Debug, FromMeta)]
pub struct HandlerData {
    ///
    #[darling(default)]
    pub name: Option<String>,
    ///
    #[darling(default)]
    pub path: Option<String>,
    ///
    #[darling(default)]
    pub method: HandlerMethod,
    ///
    #[darling(default)]
    pub spec: HandlerSpec,
}
