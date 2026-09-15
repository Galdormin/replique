use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use syn::{DataEnum, DataStruct, Field, Fields, Ident, LitStr, spanned::Spanned};

use crate::attrs::{check_unique, is_option, variant_alias};

pub(crate) fn replique_args_enum(name: &Ident, data: &DataEnum) -> TokenStream {
    derive_enum(name, data).unwrap_or_else(|e| e.to_compile_error().into())
}

pub(crate) fn replique_args_struct(name: &Ident, data: &DataStruct) -> TokenStream {
    derive_struct(name, data).unwrap_or_else(|e| e.to_compile_error().into())
}

fn derive_enum(name: &Ident, data: &DataEnum) -> syn::Result<TokenStream> {
    let mut idents = Vec::new();
    let mut names = Vec::new();
    let mut arms = Vec::new();

    for variant in &data.variants {
        // The word is the argument 0 of the call, so what the variant holds starts at 1.
        let body = Body::read(&variant.fields, 1)?;
        let alias = variant_alias(&variant.attrs)?;

        arms.push(body);
        names.push(
            alias.unwrap_or_else(|| LitStr::new(&variant.ident.to_string(), variant.ident.span())),
        );
        idents.push(&variant.ident);
    }

    check_unique(&names)?;

    let type_name = LitStr::new(&name.to_string(), name.span());
    let arity = arms.iter().map(Body::arity_check);
    let read = arms.iter().map(|body| &body.read);
    let construct = arms.iter().map(|body| &body.construct);

    Ok(quote! {
        impl FromDialogueArgs for #name {
            fn from_dialogue_args(args: DialogueArgs) -> Result<Self, DialogueArgsError> {
                let __got = args.0.len();
                let mut __values = args.0.into_iter();

                let __word = String::from_maybe_value(__values.next())
                    .map_err(|source| DialogueArgsError::Argument { index: 0, source })?;

                match __word.as_str() {
                    #(#names => {
                        #arity
                        #read
                        Ok(Self::#idents #construct)
                    })*
                    _ => Err(DialogueArgsError::Argument {
                        index: 0,
                        source: ValueError::Unknown { expected: #type_name, got: __word },
                    }),
                }
            }
        }
    }
    .into())
}

fn derive_struct(name: &Ident, data: &DataStruct) -> syn::Result<TokenStream> {
    if matches!(data.fields, Fields::Unit) {
        return Err(syn::Error::new(
            name.span(),
            "A unit struct reads no argument at all, which `In<()>` already is",
        ));
    }

    let body = Body::read(&data.fields, 0)?;
    let arity = body.arity_check();
    let values = body.takes_args.then(|| {
        quote! { let mut __values = args.0.into_iter(); }
    });
    let Body {
        read, construct, ..
    } = &body;

    Ok(quote! {
        impl FromDialogueArgs for #name {
            fn from_dialogue_args(args: DialogueArgs) -> Result<Self, DialogueArgsError> {
                let __got = args.0.len();
                #arity
                #values
                #read
                Ok(Self #construct)
            }
        }
    }
    .into())
}

/// The reading of one shape of call
struct Body {
    /// One `let` per field, in the order the fields are written.
    read: TokenStream2,
    /// What follows the name of the struct or of the variant.
    construct: TokenStream2,
    /// How many arguments the shape takes, `None` when a variadic field
    /// leaves that open.
    arity: Option<usize>,
    /// Whether anything at all is read from the call.
    takes_args: bool,
}

impl Body {
    /// `first` is the argument the first field of the shape reads:
    /// 0 for a struct, 1 for a variant, whose ident is the arg 0.
    fn read(fields: &Fields, first: usize) -> syn::Result<Self> {
        let variadic_somewhere = fields.iter().any(|field| marked(field, "variadic"));

        let mut read = TokenStream2::new();
        let mut locals = Vec::new();
        let mut index = first;
        let mut variadic_seen = false;
        let mut takes_args = false;

        for (position, field) in fields.iter().enumerate() {
            let local = field
                .ident
                .clone()
                .unwrap_or_else(|| format_ident!("__arg{position}"));
            let ty = &field.ty;

            let variadic = marked(field, "variadic");

            if variadic_seen {
                return Err(syn::Error::new(
                    field.span(),
                    "`#[variadic]` takes every argument left, so no field after it can read one",
                ));
            }

            if variadic_somewhere && is_option(ty) {
                return Err(syn::Error::new(
                    ty.span(),
                    "`Option` and `#[variadic]` cannot be told apart in a call",
                ));
            }

            takes_args = true;

            if variadic {
                variadic_seen = true;
                read.extend(quote! {
                    let #local = __values
                        .enumerate()
                        .map(|(__offset, __value)| {
                            FromValue::from_value(__value).map_err(|source| {
                                DialogueArgsError::Argument { index: #index + __offset, source }
                            })
                        })
                        .collect::<Result<#ty, DialogueArgsError>>()?;
                });
                locals.push(local);
                continue;
            }

            read.extend(quote! {
                let #local = <#ty as FromMaybeValue>::from_maybe_value(__values.next())
                    .map_err(|source| DialogueArgsError::Argument { index: #index, source })?;
            });
            locals.push(local);
            index += 1;
        }

        let construct = match fields {
            Fields::Named(_) => quote! { { #(#locals),* } },
            Fields::Unnamed(_) => quote! { ( #(#locals),* ) },
            Fields::Unit => quote! {},
        };

        Ok(Self {
            read,
            construct,
            arity: (!variadic_seen).then_some(index),
            takes_args,
        })
    }

    /// A call longer than the shape holds is refused before anything is read.
    /// A variadic field has no such bound, so it has no check either.
    fn arity_check(&self) -> Option<TokenStream2> {
        self.arity.map(|arity| {
            quote! {
                if __got > #arity {
                    return Err(DialogueArgsError::TooManyArgs { expected: #arity, got: __got });
                }
            }
        })
    }
}

/// `#[variadic]` is present without `replique`
fn marked(field: &Field, name: &str) -> bool {
    field.attrs.iter().any(|attr| attr.path().is_ident(name))
}
