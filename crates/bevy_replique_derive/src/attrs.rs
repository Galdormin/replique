//! What a `#[replique]` attribute says, and the checks every derive of this
//! crate makes on the names it ends up with.

use std::collections::HashSet;

use syn::{Attribute, LitStr, PathArguments, Type, TypePath};

/// The word a variant answers to, when `#[replique(alias = "...")]` gives it
/// one instead of its own name.
pub(crate) fn variant_alias(attrs: &[Attribute]) -> syn::Result<Option<LitStr>> {
    let mut alias = None;

    // Every `#[replique]` the variant carries, not just the first one.
    for attr in attrs.iter().filter(|a| a.path().is_ident("replique")) {
        attr.parse_nested_meta(|meta| {
            // #[replique(alias = "alice")]
            if meta.path.is_ident("alias") {
                if alias.is_some() {
                    return Err(meta.error("`alias` is already given"));
                }
                alias = Some(meta.value()?.parse()?);
                return Ok(());
            }
            Err(meta.error("unrecognized `replique` attribute"))
        })?;
    }

    Ok(alias)
}

/// Two fields or two variants answering to the same word would leave one of
/// them unreachable, and nothing else would say so.
pub(crate) fn check_unique(names: &[LitStr]) -> syn::Result<()> {
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

/// Whether a type is spelled `Option<_>`, which is as far as a derive can see:
/// an alias of one reads as any other type.
pub(crate) fn is_option(ty: &Type) -> bool {
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
