use proc_macro2::TokenStream;
use quote::{quote_spanned, ToTokens};
use syn::spanned::Spanned;

pub(crate) fn quote_option<T: ToTokens + Spanned>(value: &Option<T>) -> TokenStream {
    let span = value.span();
    value.as_ref().map_or(
        quote_spanned!(span=> None),
        |v| quote_spanned!(span=> Some(#v.into())),
    )
}
