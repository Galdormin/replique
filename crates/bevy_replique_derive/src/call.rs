//! `#[replique_function]` and `#[replique_command]`, which write down what the
//! dialogue calls a system next to the system itself.
//!
//! Both do the same thing, and differ only in the trait they implement and the
//! method that trait registers with — hence one [`expand`] taking a [`Kind`].

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use syn::{Expr, ItemFn, Lit, LitStr, Meta};

#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Function,
    Command,
}

impl Kind {
    fn trait_path(self) -> TokenStream2 {
        match self {
            Kind::Function => quote! { ::bevy_replique::function::RepliqueFunction },
            Kind::Command => quote! { ::bevy_replique::command::RepliqueCommand },
        }
    }

    fn register_call(self) -> TokenStream2 {
        match self {
            Kind::Function => quote! {
                <::bevy::app::App as ::bevy_replique::function::DialogueFunctionAppExt>
                    ::add_dialogue_function_named
            },
            Kind::Command => quote! {
                <::bevy::app::App as ::bevy_replique::command::DialogueCommandAppExt>
                    ::add_dialogue_command_named
            },
        }
    }

    fn attribute(self) -> &'static str {
        match self {
            Kind::Function => "replique_function",
            Kind::Command => "replique_command",
        }
    }
}

pub(crate) fn expand(kind: Kind, args: TokenStream, item: TokenStream) -> TokenStream {
    let original = TokenStream2::from(item.clone());
    let refuse = |err: syn::Error| {
        let err = err.to_compile_error();
        TokenStream::from(quote! { #err #original })
    };

    let item = match syn::parse::<ItemFn>(item) {
        Ok(item) => item,
        Err(err) => return refuse(err),
    };

    let name = match dialogue_name(kind, args, &item) {
        Ok(name) => name,
        Err(err) => return refuse(err),
    };

    let doc = doc_of(&item);
    let ident = &item.sig.ident;
    let vis = &item.vis;

    let attrs = item
        .attrs
        .iter()
        .filter(|attr| !attr.path().is_ident("doc"));

    let mut sig = item.sig.clone();
    sig.ident = format_ident!("system");
    let block = &item.block;

    let trait_path = kind.trait_path();
    let register_call = kind.register_call();

    quote! {
        #[doc = #doc]
        #[allow(non_camel_case_types)]
        #vis struct #ident;

        #[allow(unreachable_pub)]
        impl #ident {
            #(#attrs)*
            pub #sig #block
        }

        impl #trait_path for #ident {
            const NAME: &'static str = #name;
            const DOC: &'static str = #doc;

            fn register(self, app: &mut ::bevy::app::App) {
                #register_call(app, Self::NAME, Self::system);
            }
        }
    }
    .into()
}

fn dialogue_name(kind: Kind, args: TokenStream, item: &ItemFn) -> syn::Result<LitStr> {
    let mut name = None;

    // `syn::meta::parser` answers an empty `args` without ever calling this,
    // so `#[replique_function]` bare is not a case to handle here.
    let parser = syn::meta::parser(|meta| {
        // #[replique_function(name = "addScene")]
        if meta.path.is_ident("name") {
            if name.is_some() {
                return Err(meta.error("`name` is already given"));
            }
            name = Some(meta.value()?.parse::<LitStr>()?);
            return Ok(());
        }

        Err(meta.error(format!("unrecognized `{}` argument", kind.attribute())))
    });

    syn::parse::Parser::parse(parser, args)?;

    Ok(name.unwrap_or_else(|| LitStr::new(&item.sig.ident.to_string(), item.sig.ident.span())))
}

fn doc_of(item: &ItemFn) -> String {
    item.attrs
        .iter()
        .filter(|attr| attr.path().is_ident("doc"))
        .filter_map(|attr| match &attr.meta {
            Meta::NameValue(doc) => match &doc.value {
                Expr::Lit(expr) => match &expr.lit {
                    Lit::Str(line) => Some(line.value().trim().to_owned()),
                    _ => None,
                },
                _ => None,
            },
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n")
}
