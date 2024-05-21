use quote::{quote, quote_spanned};
use syn::{spanned::Spanned, Data, DataEnum, DataStruct, DeriveInput, Fields};

pub(crate) fn userdata(input: DeriveInput, defaultable: bool) -> proc_macro2::TokenStream {
    let ident = &input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let items = match &input.data {
        Data::Struct(DataStruct { .. }) => unimplemented!(),
        Data::Enum(DataEnum { variants, .. }) => {
            let res = variants
                .into_iter()
                .enumerate()
                .map(|(i, f)| {
                    let span = f.span();
                    match f.fields {
                        Fields::Unit => {
                            let ident = &f.ident;
                            let ident_str = ident.to_string();
                            let ident_str_lower = ident_str.to_lowercase();
                            (
                                quote_spanned! {span=>
                                    fields.add_field(#ident_str, #i)
                                },
                                quote_spanned!(span=> #i => Ok(Self::#ident)),
                                quote_spanned!(span=> #ident_str_lower => Ok(Self::#ident)),
                                quote_spanned!(span=> Self::#ident => #ident_str_lower),
                            )
                        }
                        _ => {
                            let ident_str = f.ident.to_string();
                            let e = quote_spanned! {span=>
                                compile_error!(concat!("only unit enums are supported, ", #ident_str, " is not a unit variant"));
                            };
                            (
                                e, // we only need one to trigger the compile error
                                proc_macro2::TokenStream::new(),
                                proc_macro2::TokenStream::new(),
                                proc_macro2::TokenStream::new(),
                            )
                        }
                    }
                });
            res
        }
        Data::Union(_) => unimplemented!("unions not supported"),
    };
    let (fields, match_nums, match_strs, to_strs) = quad_unzip(items);
    let impl_tryfrom = try_from_lua_string(&input, match_strs);
    let impl_fromlua = impl_from_lua(&input, match_nums, defaultable);
    let extra_funcs = match &input.data {
        Data::Enum(DataEnum { variants, .. }) => {
            let variant_str = variants.iter().map(|v| v.ident.to_string());
            quote! {
                methods.add_function("is_enum", |_lua, ()| Ok(true));
                methods.add_function("variants", |_lua, ()| {
                    Ok(vec![
                        #(
                            #variant_str,
                        )*
                    ])
                });
            }
        }
        _ => quote! {
            methods.add_function("is_enum", |_lua, ()| Ok(false));
        },
    };

    quote! {
        impl #impl_generics ::mlua::UserData for #ident #ty_generics #where_clause {
            fn add_fields<'lua, F: ::mlua::UserDataFields<'lua, Self>>(fields: &mut F) {
                #(
                    #fields;
                )*
            }
            fn add_methods<'lua, M: ::mlua::prelude::LuaUserDataMethods<'lua, Self>>(methods: &mut M) {
                #extra_funcs
            }
        }
        impl #impl_generics Into<&str> for #ident #ty_generics #where_clause {
            fn into(self) -> &'static str {
                match self {
                    #(
                        #to_strs,
                    )*
                }
            }
        }
        #impl_tryfrom
        #impl_fromlua
    }
    .into()
}

fn try_from_lua_string(
    input: &DeriveInput,
    match_strs: Vec<proc_macro2::TokenStream>,
) -> proc_macro2::TokenStream {
    let ident = &input.ident;
    let ident_str = ident.to_string();
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    quote! {
        impl #impl_generics TryFrom<::mlua::String<'_>> for #ident #ty_generics #where_clause {
            type Error = ::mlua::Error;
            fn try_from(value: ::mlua::String<'_>) -> Result<Self, Self::Error> {
                let s = value.to_str()?;
                match s.to_lowercase().as_str() {
                    #(
                        #match_strs,
                    )*
                    _ => Err(::mlua::Error::FromLuaConversionError {
                        from: "string",
                        to: #ident_str,
                        message: Some(format!(
                            concat!("failed to convert \"{}\" to \"", #ident_str, "\""),
                            s,
                        )),
                    }),
                }
            }
        }
    }
}

fn impl_from_lua(
    input: &DeriveInput,
    match_nums: Vec<proc_macro2::TokenStream>,
    defaultable: bool,
) -> proc_macro2::TokenStream {
    let ident = &input.ident;
    let ident_str = ident.to_string();
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let extra = if defaultable {
        quote!(::mlua::Value::Nil => Ok(::core::default::Default::default()),)
    } else {
        proc_macro2::TokenStream::new()
    };
    quote! {
        impl #impl_generics ::mlua::FromLua<'_> for #ident #ty_generics #where_clause {
            fn from_lua(value: ::mlua::Value<'_>, lua: &'_ ::mlua::Lua) -> ::mlua::Result<Self> {
                match value {
                    ::mlua::Value::String(string) => Self::try_from(string),
                    #extra
                    ::mlua::Value::Number(n) => match n as usize {
                        #(
                            #match_nums,
                        )*
                        _ => Err(::mlua::Error::FromLuaConversionError {
                            from: "number",
                            to: #ident_str,
                            message: Some(format!(concat!("{} is too large to convert to a ", #ident_str), n)),
                        }),
                    },
                    ::mlua::Value::Integer(n) => match n as usize {
                        #(
                            #match_nums,
                        )*
                        _ => Err(::mlua::Error::FromLuaConversionError {
                            from: "integer",
                            to: #ident_str,
                            message: Some(format!(concat!("{} is too large to convert to a ", #ident_str), n)),
                        }),
                    },
                    _ => Err(::mlua::Error::FromLuaConversionError {
                        from: value.type_name(),
                        to: #ident_str,
                        message: None,
                    }),
                }
            }
        }
    }
}

fn quad_unzip<I, T>(iter: I) -> (Vec<T>, Vec<T>, Vec<T>, Vec<T>)
where
    I: Sized + Iterator<Item = (T, T, T, T)>,
{
    let all: Vec<_> = iter.collect();
    let mut va = Vec::with_capacity(all.len());
    let mut vb = Vec::with_capacity(all.len());
    let mut vc = Vec::with_capacity(all.len());
    let mut vd = Vec::with_capacity(all.len());
    for (a, b, c, d) in all {
        va.push(a);
        vb.push(b);
        vc.push(c);
        vd.push(d);
    }
    (va, vb, vc, vd)
}

#[cfg(test)]
mod tests {
    use super::userdata;
    use syn::{parse_str, DeriveInput};

    macro_rules! ident_eq {
        ($(&)? $tok:expr, $s:literal) => {
            match &$tok {
                proc_macro2::TokenTree::Ident(i) => assert_eq!(
                    *i,
                    proc_macro2::Ident::new($s, proc_macro2::Span::call_site())
                ),
                _ => panic!(),
            }
        };
    }

    macro_rules! punct_eq {
        ($(&)? $tok:expr, $c:literal) => {
            match &$tok {
                proc_macro2::TokenTree::Punct(p) => assert_eq!(p.as_char(), $c),
                _ => panic!(),
            }
        };
        ($(&)? $tok:expr, $c:literal, $spacing:expr) => {
            match &$tok {
                proc_macro2::TokenTree::Punct(p) => {
                    assert_eq!(*p, proc_macro2::Punct::new($c, $spacing))
                }
                _ => panic!(),
            }
        };
    }

    #[test]
    fn testing() {
        let input: DeriveInput = parse_str(
            r#"
            enum Letters {
                A,
                B,
                C,
            }
        "#,
        )
        .unwrap();
        let out = userdata(input, false);
        // println!("{:?}", out);
        // println!("====================================================");
        // println!("{}", out.to_string());
        let tokens: Vec<_> = out.into_iter().collect();
        ident_eq!(tokens[0], "impl");
        punct_eq!(tokens[1], ':');
        punct_eq!(tokens[2], ':');
        ident_eq!(tokens[3], "mlua");
        punct_eq!(tokens[4], ':');
        punct_eq!(tokens[5], ':');
        ident_eq!(tokens[6], "UserData");
        ident_eq!(tokens[7], "for");
        ident_eq!(tokens[8], "Letters");
    }
}
