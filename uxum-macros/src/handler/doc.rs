use std::collections::VecDeque;

use syn::{Attribute, Expr, ExprLit, Lit, Meta};

#[derive(Default)]
pub(crate) struct DocData {
    ///
    pub(crate) title: Option<String>,
    ///
    pub(crate) description: Option<String>,
}

///
#[must_use]
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
