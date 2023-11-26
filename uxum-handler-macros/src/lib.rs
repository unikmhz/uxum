mod case;
mod handler;

use darling::{ast::NestedMeta, Error, FromMeta};
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, ItemFn};

use crate::{case::ToCamelCase, handler::HandlerData};

// TODO: #[proc_macro_error] ?

#[proc_macro_attribute]
pub fn handler(args: TokenStream, input: TokenStream) -> TokenStream {
    let attr_args = match NestedMeta::parse_meta_list(args.into()) {
        Ok(meta) => meta,
        Err(err) => {
            return TokenStream::from(Error::from(err).write_errors());
        }
    };
    let input = parse_macro_input!(input as ItemFn);
    let fn_ident = &input.sig.ident;
    let handler_ident = format_ident!("{}HandlerMeta", fn_ident.to_camel_case());
    let mod_ident = format_ident!("_uxum_private_{}", fn_ident);

    let data = match HandlerData::from_list(&attr_args) {
        Ok(val) => val,
        Err(err) => {
            return TokenStream::from(err.write_errors());
        }
    };

    let handler_name = data.name.unwrap_or_else(|| input.sig.ident.to_string());
    let handler_path = data.path.unwrap_or_else(|| format!("/{handler_name}"));
    let handler_method = data.method;

    // dbg!(&attr_args);
    // dbg!(&data);
    // dbg!(&input);

    quote! {
        #input

        #[doc(hidden)]
        mod #mod_ident {
            use ::uxum::{
                reexport::{
                    axum::{
                        body::Body,
                        handler::HandlerWithoutStateExt,
                        routing::{self, MethodRouter},
                        BoxError,
                    },
                    okapi::openapi3,
                    tower::util::BoxCloneService,
                },
                apply_layers,
                HandlerConfig,
                HandlerExt,
            };

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
                fn method(&self) -> http::Method {
                    #handler_method
                }

                fn register_method(&self, mrtr: MethodRouter<(), Body, BoxError>, cfg: Option<&HandlerConfig>) -> MethodRouter<(), Body, BoxError> {
                    (match self.method() {
                        http::Method::GET => routing::get_service,
                        http::Method::HEAD => routing::head_service,
                        http::Method::POST => routing::post_service,
                        http::Method::PUT => routing::put_service,
                        http::Method::DELETE => routing::delete_service,
                        http::Method::OPTIONS => routing::options_service,
                        http::Method::TRACE => routing::trace_service,
                        http::Method::PATCH => routing::patch_service,
                        _other => {
                            // FIXME: add custom filter
                            routing::get_service
                        }
                    })(apply_layers(self, super::#fn_ident.into_service(), cfg))
                }

                fn openapi_spec(&self) -> Option<openapi3::Operation> {
                    // TODO: write this
                    None
                }
            }

            inventory::submit! { &#handler_ident as &dyn HandlerExt }
        }
    }.into()
}
