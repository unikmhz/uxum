use darling::FromMeta;
use proc_macro2::TokenStream;
use quote::{quote, ToTokens, TokenStreamExt};

use crate::handler::spec::HandlerSpec;

/// Top-level handler parameters object
#[derive(Debug, Default, FromMeta)]
#[darling(default)]
pub(crate) struct HandlerData {
    /// Unique handler name
    #[darling(default)]
    pub(crate) name: Option<String>,
    /// HTTP URL path for handler
    #[darling(default)]
    pub(crate) path: Option<String>,
    /// HTTP method for handler
    #[darling(default)]
    pub(crate) method: Option<HandlerMethod>,
    /// Additional parameters for OpenAPI specification
    #[darling(default, flatten)]
    pub(crate) spec: HandlerSpec,
    /// RBAC permissions required to call this handler
    #[darling(default)]
    pub(crate) permissions: Vec<syn::LitStr>,
    /// Skip authentication for this method
    #[darling(default)]
    pub(crate) no_auth: bool,
}

/// Supported HTTP methods
#[derive(Debug, Default, FromMeta)]
#[darling(default, rename_all = "SCREAMING_SNAKE_CASE")]
pub(crate) enum HandlerMethod {
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
