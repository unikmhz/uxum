use convert_case::{Case, Casing};
use syn::Ident;

/// String conversion to camel case
pub(crate) trait ToCamelCase {
    /// Resulting type
    type CamelCased;

    /// Convert string-like type to camel case
    fn to_camel_case(&self) -> Self::CamelCased;
}

/// String conversion to snake case
pub(crate) trait ToSnakeCase {
    /// Resulting type
    type SnakeCased;

    /// Convert string-like type to snake case
    fn to_snake_case(&self) -> Self::SnakeCased;
}

impl ToCamelCase for Ident {
    type CamelCased = Ident;

    fn to_camel_case(&self) -> Self::CamelCased {
        let camel_cased = self.to_string().to_case(Case::UpperCamel);
        Ident::new(&camel_cased, self.span())
    }
}

impl ToSnakeCase for Ident {
    type SnakeCased = Ident;

    fn to_snake_case(&self) -> Self::SnakeCased {
        let snake_cased = self.to_string().to_case(Case::Snake);
        Ident::new(&snake_cased, self.span())
    }
}
