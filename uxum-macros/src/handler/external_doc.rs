use darling::FromMeta;
use proc_macro2::TokenStream;
use quote::{quote, ToTokens, TokenStreamExt};

use crate::util::quote_option;

/// External documentation link.
#[derive(Debug, FromMeta)]
pub(crate) struct OpenApiExternalDoc {
    /// Description of the link.
    #[darling(default)]
    description: Option<String>,
    /// Link URL.
    url: String,
}

impl ToTokens for OpenApiExternalDoc {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let description = quote_option(&self.description);
        let url = &self.url;
        tokens.append_all(quote! {
            openapi3::ExternalDocs {
                description: #description,
                url: #url.into(),
                extensions: Default::default(),
            }
        });
    }
}
