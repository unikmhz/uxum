mod handler;

use darling::{ast::NestedMeta, Error, FromMeta};
use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{parse_macro_input, ItemFn};

use crate::handler::HandlerData;

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
    let handler_ident = format_ident!("{}HandlerMeta", fn_ident);

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

        struct #handler_ident;

        #[automatically_derived]
        impl ::uxum::HandlerExt for #handler_ident {
            fn name(&self) -> &'static str {
                #handler_name
            }

            fn path(&self) -> &'static str {
                #handler_path
            }

            fn method(&self) -> http::Method {
                #handler_method
            }

            fn register_method(&self, mrtr: ::axum::routing::MethodRouter) -> ::axum::routing::MethodRouter {
                (match self.method() {
                    http::Method::GET => ::axum::routing::get,
                    http::Method::HEAD => ::axum::routing::head,
                    http::Method::POST => ::axum::routing::post,
                    http::Method::PUT => ::axum::routing::put,
                    http::Method::DELETE => ::axum::routing::delete,
                    http::Method::OPTIONS => ::axum::routing::options,
                    http::Method::TRACE => ::axum::routing::trace,
                    http::Method::PATCH => ::axum::routing::patch,
                    _other => {
                        // FIXME: add custom filter
                        ::axum::routing::get
                    }
                })(#fn_ident)
            }
        }

        inventory::submit! { &#handler_ident as &dyn ::uxum::HandlerExt }
    }.into()
}
