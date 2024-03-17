use proc_macro2::TokenStream;
use quote::ToTokens;
use syn::{
    AngleBracketedGenericArguments, FnArg, GenericArgument, ItemFn, Path, PathArguments, Type,
    TypePath,
};

///
#[derive(Debug)]
pub(crate) struct QueryType(Path);

impl ToTokens for QueryType {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        self.0.to_tokens(tokens)
    }
}

/// Detect query string extractor inside handler function signature
pub(crate) fn detect_query_strings(handler: &ItemFn) -> Option<QueryType> {
    handler.sig.inputs.iter().find_map(|input| match input {
        FnArg::Typed(arg_type) => match arg_type.ty.as_ref() {
            Type::Path(path) => {
                path.path
                    .segments
                    .last()
                    .and_then(|seg| match seg.ident.to_string().as_str() {
                        "Query" => match &seg.arguments {
                            PathArguments::AngleBracketed(AngleBracketedGenericArguments {
                                args,
                                ..
                            }) if args.len() == 1 => match &args[0] {
                                GenericArgument::Type(Type::Path(TypePath { path, .. })) => {
                                    Some(QueryType(path.clone()))
                                }
                                _ => None,
                            },
                            _ => None,
                        },
                        _ => None,
                    })
            }
            // TODO: support other variants
            _ => None,
        },
        FnArg::Receiver(_) => None,
    })
}
