use std::collections::HashMap;

use darling::FromMeta;
use proc_macro2::TokenStream;
use quote::{quote, ToTokens, TokenStreamExt};
use syn::ItemFn;

use crate::{
    parse::{detect_request_body, extract_docstring, RequestBody},
    path::extract_path_params,
    util::quote_option,
};

///
#[derive(Debug, FromMeta)]
pub(crate) struct HandlerData {
    ///
    #[darling(default)]
    pub(crate) name: Option<String>,
    ///
    #[darling(default)]
    pub(crate) path: Option<String>,
    ///
    #[darling(default)]
    pub(crate) method: HandlerMethod,
    ///
    #[darling(default)]
    pub(crate) spec: HandlerSpec,
}

///
#[derive(Debug, Default, FromMeta)]
pub(crate) struct HandlerSpec {
    ///
    #[darling(default, multiple, rename = "tag")]
    tags: Vec<String>,
    ///
    #[darling(default)]
    summary: Option<String>,
    ///
    #[darling(default)]
    description: Option<String>,
    ///
    #[darling(default)]
    docs: Option<OpenApiExternalDoc>,
    ///
    #[darling(default)]
    path_params: HashMap<String, OpenApiPathParameter>,
    ///
    #[darling(default)]
    deprecated: bool,
}

impl HandlerSpec {
    ///
    pub(crate) fn generate_spec(
        &self,
        name: &str,
        path: &str,
        _method: &HandlerMethod,
        handler: &ItemFn,
    ) -> TokenStream {
        let tags = &self.tags;
        let docs = quote_option(&self.docs);
        let deprecated = self.deprecated;

        let docstring = extract_docstring(&handler.attrs);
        let summary = quote_option(&self.summary.as_ref().or(docstring.title.as_ref()));
        let description =
            quote_option(&self.description.as_ref().or(docstring.description.as_ref()));

        let path_params = extract_path_params(path).map(|elem| {
            let param = self.path_params.get(elem).cloned().unwrap_or_default();
            let descr = quote_option(&param.description);
            let deprecated = param.deprecated;
            let allow_empty = param.allow_empty;
            // TODO: sense from extractors
            // FIXME: unwrap
            let value_type = param
                .value_type
                .unwrap_or(syn::Path::from_string("String").unwrap());
            quote! {
                ::uxum::reexport::openapi3::Parameter {
                    name: #elem.into(),
                    location: "path".into(),
                    description: #descr,
                    required: true,
                    deprecated: #deprecated,
                    allow_empty_value: #allow_empty,
                    value: ::uxum::reexport::openapi3::ParameterValue::Schema {
                        style: None,
                        explode: None,
                        allow_reserved: false,
                        // FIXME: subschema
                        schema: ::uxum::reexport::schemars::schema_for!(#value_type).schema,
                        example: None,
                        examples: None,
                    },
                    extensions: Default::default(),
                }
            }
        });

        quote! {
            ::uxum::reexport::openapi3::Operation {
                tags: vec![#(#tags.into()),*],
                summary: #summary,
                description: #description,
                external_docs: #docs,
                operation_id: Some(#name.into()),
                parameters: vec![#(#path_params.into()),*],
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
                extensions: Default::default(),
            }
        }
    }
}

///
#[derive(Debug, FromMeta)]
pub(crate) struct OpenApiExternalDoc {
    ///
    #[darling(default)]
    description: Option<String>,
    ///
    url: String,
}

impl ToTokens for OpenApiExternalDoc {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let description = quote_option(&self.description);
        let url = &self.url;
        tokens.append_all(quote! {
            ::uxum::reexport::openapi3::ExternalDocs {
                description: #description,
                url: #url.into(),
                extensions: Default::default(),
            }
        });
    }
}

///
#[derive(Clone, Debug, Default, FromMeta)]
struct OpenApiPathParameter {
    ///
    #[darling(default)]
    description: Option<String>,
    ///
    #[darling(default)]
    deprecated: bool,
    ///
    #[darling(default)]
    allow_empty: bool,
    ///
    #[darling(default)]
    value_type: Option<syn::Path>,
}

///
#[derive(Debug, FromMeta)]
pub(crate) struct OpenApiServer {
    ///
    url: String,
    ///
    #[darling(default)]
    description: Option<String>,
}

impl ToTokens for OpenApiServer {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        let url = &self.url;
        let description = quote_option(&self.description);
        tokens.append_all(quote! {
            ::uxum::reexport::openapi3::Server {
                url: #url.into(),
                description: #description,
                variables: Default::default(),
                extensions: Default::default(),
            }
        });
    }
}

///
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
