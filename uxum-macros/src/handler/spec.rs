use std::collections::HashMap;

use darling::FromMeta;
use proc_macro2::TokenStream;
use quote::quote;
use syn::ItemFn;

use crate::{
    handler::{
        body::RequestBody,
        data::HandlerMethod,
        doc::extract_docstring,
        external_doc::OpenApiExternalDoc,
        path::extract_path_params,
        path_param::OpenApiPathParameter,
        response::detect_responses,
    },
    util::quote_option,
};

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
        request_body: &Option<RequestBody>,
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
                openapi3::Parameter {
                    name: #elem.into(),
                    location: "path".into(),
                    description: #descr,
                    required: true,
                    deprecated: #deprecated,
                    allow_empty_value: #allow_empty,
                    value: openapi3::ParameterValue::Schema {
                        style: None,
                        explode: None,
                        allow_reserved: false,
                        // FIXME: subschema
                        schema: gen.subschema_for::<#value_type>().into_object(),
                        example: None,
                        examples: None,
                    },
                    extensions: Default::default(),
                }
            }
        });

        let request_body = quote_option(request_body);
        let responses = detect_responses(handler);

        quote! {
            openapi3::Operation {
                tags: vec![#(#tags.into()),*],
                summary: #summary,
                description: #description,
                external_docs: #docs,
                operation_id: Some(#name.into()),
                parameters: vec![#(#path_params.into()),*],
                request_body: #request_body,
                responses: #responses,
                callbacks: Default::default(), // TODO: fill?
                deprecated: #deprecated,
                security: None,
                servers: None,
                extensions: Default::default(),
            }
        }
    }
}
