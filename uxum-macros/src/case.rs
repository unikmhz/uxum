use syn::Ident;

pub(crate) fn camel_case(input: impl AsRef<str>) -> String {
    input
        .as_ref()
        .split(&['_', '-', ' '])
        .map(|word| {
            let mut ch = word.chars();
            match ch.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(ch).collect(),
            }
        })
        .collect()
}

/// String conversion to camel case
pub(crate) trait ToCamelCase {
    /// Resulting type
    type CamelCased;

    /// Convert string-like type to camel case
    fn to_camel_case(&self) -> Self::CamelCased;
}

impl ToCamelCase for Ident {
    type CamelCased = Ident;

    fn to_camel_case(&self) -> Self::CamelCased {
        let camel_cased = camel_case(self.to_string());
        Ident::new(&camel_cased, self.span())
    }
}
