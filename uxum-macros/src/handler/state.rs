use proc_macro2::TokenStream;
use quote::ToTokens;
use syn::{
    AngleBracketedGenericArguments, FnArg, GenericArgument, ItemFn, Path, PathArguments, Type,
    TypePath,
};

/// Type for detected state extractor
#[derive(Debug)]
pub(crate) struct StateType(Path);

impl ToTokens for StateType {
    fn to_tokens(&self, tokens: &mut TokenStream) {
        self.0.to_tokens(tokens)
    }
}

pub(crate) fn detect_state(handler: &ItemFn) -> Option<StateType> {
    handler.sig.inputs.iter().find_map(|input| match input {
        FnArg::Typed(arg_type) => match arg_type.ty.as_ref() {
            Type::Path(path) => {
                path.path
                    .segments
                    .last()
                    .and_then(|seg| match seg.ident.to_string().as_str() {
                        "State" => match &seg.arguments {
                            PathArguments::AngleBracketed(AngleBracketedGenericArguments {
                                args,
                                ..
                            }) if args.len() == 1 => match &args[0] {
                                GenericArgument::Type(Type::Path(TypePath { path, .. })) => {
                                    Some(StateType(path.clone()))
                                }
                                _ => None,
                            },
                            _ => None,
                        },
                        _ => None,
                    })
            }
            _ => None,
        },
        FnArg::Receiver(_) => None,
    })
}
