#![forbid(unsafe_code)]
#![deny(elided_lifetimes_in_paths, unreachable_pub)]

mod body;
mod case;
mod doc;
mod handler;
mod path;
mod util;

use darling::{ast::NestedMeta, FromMeta};
use proc_macro::TokenStream;
use proc_macro_error::{abort, abort_call_site, proc_macro_error};
use quote::{format_ident, quote};
use syn::{parse_macro_input, ItemFn};

use crate::{
    body::detect_request_body,
    case::ToCamelCase,
    handler::{HandlerData, HandlerMethod},
    path::format_path_for_spec,
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
    // dbg!(&attr_args);
    // dbg!(&data);
    // if input.sig.ident == "name_from_text_body" {
    //     dbg!(&input);
    // }

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
    let handler_spec = data.spec.generate_spec(
        &handler_name,
        &handler_path,
        &handler_method,
        &input,
        &request_body,
    );

    quote! {
        #input

        #[doc(hidden)]
        #[allow(missing_docs)]
        mod #mod_ident {
            use ::uxum::{
                reexport::{
                    axum::{
                        handler::HandlerWithoutStateExt,
                        routing::{self, MethodRouter},
                    },
                    http,
                    inventory,
                    okapi,
                    openapi3,
                    schemars,
                },
                apply_layers,
                HandlerConfig,
                HandlerExt,
            };

            use super::*;

            struct #handler_ident;

            #[automatically_derived]
            impl HandlerExt for #handler_ident {
                #[inline]
                fn name(&self) -> &'static str {
                    #handler_name
                }

                #[inline]
                fn path(&self) -> &'static str {
                    #handler_path
                }

                #[inline]
                fn spec_path(&self) -> &'static str {
                    #handler_spec_path
                }

                #[inline]
                fn method(&self) -> http::Method {
                    #handler_method
                }

                fn register_method(&self, mrtr: MethodRouter, cfg: Option<&HandlerConfig>) -> MethodRouter {
                    (match self.method() {
                        http::Method::GET => routing::get_service,
                        http::Method::HEAD => routing::head_service,
                        http::Method::POST => routing::post_service,
                        http::Method::PUT => routing::put_service,
                        http::Method::DELETE => routing::delete_service,
                        http::Method::OPTIONS => routing::options_service,
                        http::Method::TRACE => routing::trace_service,
                        http::Method::PATCH => routing::patch_service,
                        // axum::routing::MethodFilter does not support custom methods
                        other => panic!("Unsupported HTTP method: {other}", other = other),
                    })(apply_layers(self, super::#fn_ident.into_service(), cfg))
                }

                fn openapi_spec(&self, gen: &mut schemars::gen::SchemaGenerator) -> openapi3::Operation {
                    #handler_spec
                }
            }

            inventory::submit! { &#handler_ident as &dyn HandlerExt }
        }
    }.into()
}
