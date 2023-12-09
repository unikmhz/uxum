use std::collections::VecDeque;

use syn::{punctuated::Punctuated, token::Comma, Attribute, Expr, ExprLit, FnArg, Lit, Meta, Type};

///
pub(crate) enum RequestBody {
    ///
    Form,
    ///
    Json,
}

///
pub(crate) fn detect_request_body(inputs: Punctuated<FnArg, Comma>) -> Option<RequestBody> {
    inputs.iter().find_map(|input| match input {
        FnArg::Typed(arg_type) => match arg_type.ty.as_ref() {
            Type::Path(path) => {
                path.path
                    .segments
                    .last()
                    .and_then(|seg| match seg.ident.to_string().as_str() {
                        // TODO: support other extractors
                        "Form" => Some(RequestBody::Form),
                        "Json" => Some(RequestBody::Json),
                        _ => None,
                    })
            }
            // TODO: support other variants
            _ => None,
        },
        FnArg::Receiver(_) => None,
    })
}

#[derive(Default)]
pub(crate) struct DocData {
    ///
    pub(crate) title: Option<String>,
    ///
    pub(crate) description: Option<String>,
}

///
pub(crate) fn extract_docstring(attrs: &[Attribute]) -> DocData {
    let mut literals: VecDeque<_> = attrs
        .iter()
        .filter_map(|attr| match &attr.meta {
            Meta::NameValue(name_val) if name_val.path.is_ident("doc") => match &name_val.value {
                Expr::Lit(ExprLit {
                    lit: Lit::Str(s), ..
                }) => Some(s.value()),
                _ => None,
            },
            _ => None,
        })
        .flat_map(|chunk| {
            chunk
                .split('\n')
                .map(|line| line.trim().to_string())
                .collect::<Vec<_>>()
        })
        .collect();
    DocData {
        title: literals.pop_front(),
        description: if literals.is_empty() {
            None
        } else {
            Some(
                literals
                    .into_iter()
                    .skip_while(|l| l.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        },
    }
}
