#![forbid(unsafe_code)]
#![deny(elided_lifetimes_in_paths, unreachable_pub)]

mod case;
mod handler;
mod util;

use darling::{ast::NestedMeta, FromMeta};
use proc_macro::TokenStream;
use proc_macro_error::{abort, abort_call_site, proc_macro_error};
use quote::{format_ident, quote};
use syn::{parse_macro_input, ItemFn};

use crate::{
    case::ToCamelCase,
    handler::{
        body::detect_request_body,
        data::{HandlerData, HandlerMethod},
        path::format_path_for_spec,
    },
};

/// Attribute macro for declaring service endpoints
#[proc_macro_error]
#[proc_macro_attribute]
pub fn handler(args: TokenStream, input: TokenStream) -> TokenStream {
    let attr_args = match NestedMeta::parse_meta_list(args.into()) {
        Ok(meta) => meta,
        Err(err) => abort_call_site!("Unable to parse attributes: {}", err),
    };
    let input = parse_macro_input!(input as ItemFn);
    let fn_ident = &input.sig.ident;
    let handler_ident = format_ident!("{}HandlerMeta", fn_ident.to_camel_case());
    let mod_ident = format_ident!("_uxum_private_{}", fn_ident);

    let data = match HandlerData::from_list(&attr_args) {
        Ok(val) => val,
        Err(err) => abort!(
            input.sig.ident,
            "Unable to parse handler attributes: {}",
            err
        ),
    };

    let handler_name = data.name.unwrap_or_else(|| input.sig.ident.to_string());
    let handler_path = data.path.unwrap_or_else(|| format!("/{handler_name}"));
    let handler_spec_path = format_path_for_spec(&handler_path);
    let request_body = detect_request_body(&input);
    let handler_method = match data.method {
        Some(method) => method,
        None => {
            if request_body.is_some() {
                HandlerMethod::Post
            } else {
                HandlerMethod::Get
            }
        }
    };
    let permissions = data.permissions;
    let handler_spec = data.spec.generate_spec(
        &handler_name,
        &handler_path,
        &handler_method,
        &input,
        &request_body,
    );

    quote! {
        #[::uxum::reexport::tracing::instrument(name = "handler", skip_all, fields(name = #handler_name))]
        #[::uxum::reexport::axum::debug_handler]
        #input

        #[doc(hidden)]
        #[allow(missing_docs)]
        mod #mod_ident {
            use ::std::convert::Infallible;

            use ::uxum::{
                reexport::{
                    axum::{
                        body::Body,
                        handler::HandlerWithoutStateExt,
                    },
                    http,
                    hyper::{Request, Response},
                    inventory,
                    okapi,
                    openapi3,
                    schemars,
                    tower::util::BoxCloneService,
                },
                HandlerExt,
            };

            use super::*;

            struct #handler_ident;

            #[automatically_derived]
            impl HandlerExt for #handler_ident {
                #[inline]
                #[must_use]
                fn name(&self) -> &'static str {
                    #handler_name
                }

                #[inline]
                #[must_use]
                fn path(&self) -> &'static str {
                    #handler_path
                }

                #[inline]
                #[must_use]
                fn spec_path(&self) -> &'static str {
                    #handler_spec_path
                }

                #[inline]
                #[must_use]
                fn method(&self) -> http::Method {
                    #handler_method
                }

                #[inline]
                #[must_use]
                fn permissions(&self) -> &'static [&'static str] {
                    &[#(#permissions),*]
                }

                #[inline]
                #[must_use]
                fn service(&self) -> BoxCloneService<Request<Body>, Response<Body>, Infallible> {
                    BoxCloneService::new(super::#fn_ident.into_service())
                }

                #[inline]
                #[must_use]
                fn openapi_spec(&self, gen: &mut schemars::gen::SchemaGenerator) -> openapi3::Operation {
                    #handler_spec
                }
            }

            inventory::submit! { &#handler_ident as &dyn HandlerExt }
        }
    }.into()
}
