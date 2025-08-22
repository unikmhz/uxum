use darling::FromMeta;
use proc_macro2::TokenStream;
use quote::{quote, ToTokens, TokenStreamExt};

use crate::util::quote_option;

/// Available service endpoint.
#[derive(Debug, FromMeta)]
#[allow(dead_code)]
pub(crate) struct OpenApiServer {
    /// Server URL.
    url: String,
    /// Server description.
    #[darling(default)]
    description: Option<String>,
}

impl ToTokens for OpenApiServer {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let url = &self.url;
        let description = quote_option(&self.description);
        tokens.append_all(quote! {
            openapi3::Server {
                url: #url.into(),
                description: #description,
                variables: Default::default(),
                extensions: Default::default(),
            }
        });
    }
}
