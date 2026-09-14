use std::collections::HashSet;

use proc_macro::TokenStream;
use syn::{
    DataEnum, DataStruct, Expr, Fields, FieldsNamed, FieldsUnnamed, Ident, LitStr, Path,
    PathArguments, Type, TypePath, spanned::Spanned,
};

pub(crate) fn replique_value_enum(name: &Ident, data: &DataEnum) -> TokenStream {
    derive_enum(name, data).unwrap_or_else(|e| e.to_compile_error().into())
}

pub(crate) fn replique_value_struct(name: &Ident, data: &DataStruct) -> TokenStream {
    let result = match &data.fields {
        Fields::Named(fields) => derive_struct_named(name, fields),
        Fields::Unnamed(fields) => derive_struct_unnamed(name, fields),
        Fields::Unit => Err(syn::Error::new(
            name.span(),
            "Only named or single value unnamed struct can derive `RepliqueValue`",
        )),
    };

    result.unwrap_or_else(|e| e.to_compile_error().into())
}

fn derive_enum(name: &Ident, data: &DataEnum) -> syn::Result<TokenStream> {
    let mut idents = Vec::new();
    let mut names = Vec::new();
    for v in &data.variants {
        if !matches!(v.fields, Fields::Unit) {
            return Err(syn::Error::new(
                v.ident.span(),
                "Only unit enum can derive `RepliqueValue`",
            ));
        }

        let attrs = VariantAttrs::parse(&v.attrs)?;

        names.push(
            attrs
                .alias
                .unwrap_or_else(|| LitStr::new(&v.ident.to_string(), v.ident.span())),
        );
        idents.push(&v.ident);
    }

    check_unique(&names)?;

    let type_name = LitStr::new(&name.to_string(), name.span());

    Ok(quote! {
        impl FromValue for #name {
            fn from_value(value: Value) -> Result<Self, ValueError> {
                let word = String::from_value(value)?;
                match word.as_str() {
                    #(#names => Ok(Self::#idents),)*
                    _ => Err(ValueError::Unknown { expected: #type_name, got: word }),
                }
            }
        }

        impl IntoValue for #name {
            fn into_value(self) -> Value {
                match self {
                    #(Self::#idents => Value::String(#names.into()),)*
                }
            }
        }
    }
    .into())
}

fn derive_struct_named(name: &Ident, fields: &FieldsNamed) -> syn::Result<TokenStream> {
    // Fields read from the dict, and the key each one answers to.
    let mut idents = Vec::new();
    let mut types = Vec::new();
    let mut names = Vec::new();

    // Ignored fields given a value of their own, which spare the struct the
    // `Default` bound the bare `ignore` needs.
    let mut default_idents = Vec::new();
    let mut default_exprs = Vec::new();

    let mut has_bare_ignore = false;

    for field in &fields.named {
        let ident = field.ident.clone().expect("named field");
        let attrs = FieldAttrs::parse(&field.attrs)?;

        if attrs.ignore.is_some() {
            match attrs.default {
                Some((_, expr)) => {
                    default_idents.push(ident);
                    default_exprs.push(expr);
                }
                None => has_bare_ignore = true,
            }
            continue;
        }

        if is_option(&field.ty) {
            return Err(syn::Error::new(
                field.ty.span(),
                "`Option` is not supported in a `RepliqueValue` dict: every key must be \
                 written in the dialogue. Use `#[replique(ignore)]` to leave the field out.",
            ));
        }

        names.push(
            attrs
                .alias
                .unwrap_or_else(|| LitStr::new(&ident.to_string(), ident.span())),
        );
        idents.push(ident);
        types.push(field.ty.clone());
    }

    check_unique(&names)?;

    let maybe_default = has_bare_ignore.then(|| quote! { ..Default::default() });

    Ok(quote! {
        impl FromValue for #name {
            fn from_value(value: Value) -> Result<Self, ValueError> {
                let Value::Dict(mut dict) = value else {
                    return Err(ValueError::wrong_type("dict", &value));
                };

                Ok(Self {
                    #(#idents: <#types as FromMaybeValue>::from_maybe_value(dict.remove(#names))
                        .map_err(|err| err.under_key(#names))?,)*
                    #(#default_idents: #default_exprs,)*
                    #maybe_default
                })
            }
        }

        impl IntoValue for #name {
            fn into_value(self) -> Value {
                Value::Dict(::std::collections::HashMap::from([
                    #((#names.to_owned(), self.#idents.into_value()),)*
                ]))
            }
        }
    }
    .into())
}

fn derive_struct_unnamed(name: &Ident, fields: &FieldsUnnamed) -> syn::Result<TokenStream> {
    let [field] = &fields.unnamed.iter().collect::<Vec<_>>()[..] else {
        return Err(syn::Error::new(
            name.span(),
            "Only single value unnamed struct can derive `RepliqueValue`",
        ));
    };

    // Nothing here is a dict key, so an attribute would be read and dropped.
    if let Some(attr) = field.attrs.iter().find(|a| a.path().is_ident("replique")) {
        return Err(syn::Error::new(
            attr.span(),
            "`replique` means nothing on a single value unnamed struct",
        ));
    }

    let ty = &field.ty;
    Ok(quote! {
        impl FromValue for #name {
            fn from_value(value: Value) -> Result<Self, ValueError> {
                 Ok(Self(<#ty as FromValue>::from_value(value)?))
            }
        }

        impl IntoValue for #name {
            fn into_value(self) -> Value {
                self.0.into_value()
            }
        }
    }
    .into())
}

#[derive(Default)]
struct VariantAttrs {
    alias: Option<LitStr>,
}

impl VariantAttrs {
    fn parse(attrs: &[syn::Attribute]) -> syn::Result<Self> {
        let mut parsed = Self::default();

        // Every `#[replique]` the variant carries, not just the first one.
        for attr in attrs.iter().filter(|a| a.path().is_ident("replique")) {
            attr.parse_nested_meta(|meta| {
                // #[replique(alias = "alice")]
                if meta.path.is_ident("alias") {
                    if parsed.alias.is_some() {
                        return Err(meta.error("`alias` is already given"));
                    }
                    parsed.alias = Some(meta.value()?.parse()?);
                    return Ok(());
                }
                Err(meta.error("unrecognized `replique` attribute"))
            })?;
        }

        Ok(parsed)
    }
}

#[derive(Default)]
struct FieldAttrs {
    alias: Option<LitStr>,
    ignore: Option<Path>,
    default: Option<(Path, Expr)>,
}

impl FieldAttrs {
    fn parse(attrs: &[syn::Attribute]) -> syn::Result<Self> {
        let mut parsed = Self::default();

        // Every `#[replique]` the field carries, not just the first one.
        for attr in attrs.iter().filter(|a| a.path().is_ident("replique")) {
            attr.parse_nested_meta(|meta| {
                // #[replique(alias = "hp")]
                if meta.path.is_ident("alias") {
                    if parsed.alias.is_some() {
                        return Err(meta.error("`alias` is already given"));
                    }
                    parsed.alias = Some(meta.value()?.parse()?);
                    return Ok(());
                }
                // #[replique(ignore)]
                if meta.path.is_ident("ignore") {
                    parsed.ignore = Some(meta.path.clone());
                    return Ok(());
                }
                // #[replique(ignore, default = 18.36)]
                if meta.path.is_ident("default") {
                    if parsed.default.is_some() {
                        return Err(meta.error("`default` is already given"));
                    }
                    parsed.default = Some((meta.path.clone(), meta.value()?.parse()?));
                    return Ok(());
                }
                Err(meta.error("unrecognized `replique` attribute"))
            })?;
        }

        // Both are read once the whole attribute is, so their order is free.
        if let Some((path, _)) = &parsed.default
            && parsed.ignore.is_none()
        {
            return Err(syn::Error::new(
                path.span(),
                "`default` needs `ignore`: a field the dialogue writes takes its value from there",
            ));
        }

        if let (Some(alias), Some(_)) = (&parsed.alias, &parsed.ignore) {
            return Err(syn::Error::new(
                alias.span(),
                "`alias` means nothing on an ignored field: it answers to no key",
            ));
        }

        Ok(parsed)
    }
}

/// Two fields answering to the same key would leave one of them unreachable,
/// and the derived `FromValue` would refuse every dict without saying why.
fn check_unique(names: &[LitStr]) -> syn::Result<()> {
    let mut seen = HashSet::new();
    for name in names {
        if !seen.insert(name.value()) {
            return Err(syn::Error::new(
                name.span(),
                format!("`{}` is already used", name.value()),
            ));
        }
    }

    Ok(())
}

fn is_option(ty: &Type) -> bool {
    let Type::Path(TypePath {
        qself: None, path, ..
    }) = ty
    else {
        return false;
    };
    path.segments.last().is_some_and(|seg| {
        seg.ident == "Option" && matches!(seg.arguments, PathArguments::AngleBracketed(_))
    })
}
