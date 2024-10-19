use std::collections::HashMap;

use darling::FromMeta;
use proc_macro2::TokenStream;
use quote::quote;
use syn::ItemFn;

use crate::{
    handler::{
        body::RequestBody, data::HandlerMethod, doc::extract_docstring,
        external_doc::OpenApiExternalDoc, path::extract_path_params,
        path_param::OpenApiPathParameter, query::detect_query_strings, response::detect_responses,
    },
    util::quote_option,
};

/// Handler attributes related to OpenAPI specification.
#[derive(Debug, Default, FromMeta)]
pub(crate) struct HandlerSpec {
    /// Tags assigned to this handler.
    #[darling(default)]
    tags: Vec<syn::LitStr>,
    /// Handler summary.
    ///
    /// Taken from first line of docstring if not explicitly specified.
    #[darling(default)]
    summary: Option<String>,
    /// Handler description.
    ///
    /// Taken from the docstring lines after the first, if not explicitly specified.
    #[darling(default)]
    description: Option<String>,
    /// Documentation links.
    #[darling(default)]
    docs: Option<OpenApiExternalDoc>,
    /// Schema for path parameters.
    #[darling(default)]
    path_params: HashMap<String, OpenApiPathParameter>,
    /// Deprecation flag.
    #[darling(default)]
    deprecated: bool,
}

impl HandlerSpec {
    /// Generate OpenAPI operation schema code.
    #[must_use]
    pub(crate) fn generate_schema(
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
            // TODO: sense from extractors.
            // FIXME: unwrap.
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
                        // FIXME: subschema.
                        schema: gen.subschema_for::<#value_type>().into_object(),
                        example: None,
                        examples: None,
                    },
                    extensions: Default::default(),
                }
            }
        });

        let query_params = detect_query_strings(handler)
            .map(|qt| {
                quote! {
                    .into_iter()
                    .chain({
                        let mut params = Vec::new();
                        let query_schema = schemars::schema_for!(#qt);
                        if let Some(query_object) = query_schema.schema.object {
                            for (key, param) in query_object.properties.into_iter() {
                                let obj = match &param {
                                    schemars::schema::Schema::Object(obj) => obj,
                                    _ => continue,
                                };
                                let meta = obj.metadata.as_ref();
                                params.push(openapi3::Parameter {
                                    name: key.into(),
                                    location: "query".into(),
                                    description: meta.and_then(|m| m.description.clone()),
                                    required: true, // FIXME: not always required.
                                    deprecated: meta.map(|m| m.deprecated).unwrap_or_default(),
                                    allow_empty_value: false,
                                    value: openapi3::ParameterValue::Schema {
                                        style: None,
                                        explode: None,
                                        allow_reserved: false,
                                        schema: param.into(),
                                        example: None, // TODO: maybe extract examples?
                                        examples: None,
                                    },
                                    extensions: Default::default(),
                                }.into());
                            }
                        }
                        params
                    })
                    .collect()
                }
            })
            .unwrap_or_else(|| quote! {});

        let request_body = quote_option(request_body);
        let responses = detect_responses(handler);

        quote! {
            openapi3::Operation {
                tags: vec![#(#tags.into()),*],
                summary: #summary,
                description: #description,
                external_docs: #docs,
                operation_id: Some(#name.into()),
                parameters: vec![#(#path_params.into()),*] #query_params,
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
