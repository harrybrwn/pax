use quote::{quote, quote_spanned};
use syn::{spanned::Spanned, DeriveInput};
use syn::{Data, DataEnum, DataStruct, Fields, FieldsNamed, FieldsUnnamed};

use crate::Attrs;

pub(crate) fn from_lua(input: DeriveInput) -> proc_macro::TokenStream {
    let ident = input.ident;
    let ident_str = ident.to_string();
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let tbl_ident = syn::Ident::new(TABLE_IDENT, proc_macro2::Span::call_site());
    match input.data {
        Data::Struct(DataStruct { fields, .. }) => {
            let self_expr = match fields {
                Fields::Named(FieldsNamed { named, .. }) => {
                    let table_gets = named.iter().filter_map(gen_table_access);
                    quote_spanned! {named.span()=>
                        Self {
                            #(
                                #table_gets,
                            )*
                        }
                    }
                }
                Fields::Unnamed(FieldsUnnamed { unnamed, .. }) => {
                    let table_gets = unnamed.iter().enumerate().filter_map(|(i, f)| {
                        let attrs = Attrs::from(f);
                        let lua_ix = i + 1;
                        if attrs.ignored {
                            Some(quote_spanned! {f.span()=>
                                ::core::default::Default::default()
                            })
                        } else {
                            Some(quote_spanned! {f.span()=>
                                #tbl_ident.get(#lua_ix)
                            })
                        }
                    });
                    quote! { Self(#(#table_gets?, )*)}
                }
                Fields::Unit => panic!("can't derive FromLua from unit struct"),
            };
            quote! {
                impl #impl_generics ::mlua::FromLua<'_> for #ident #ty_generics #where_clause {
                    fn from_lua(value: ::mlua::Value<'_>, lua: &'_ ::mlua::Lua) -> ::mlua::Result<Self> {
                        match value {
                            ::mlua::Value::Table(#tbl_ident) => {
                                Ok(#self_expr)
                            }
                            _ => Err(::mlua::Error::FromLuaConversionError {
                                from: value.type_name(),
                                to: #ident_str,
                                message: Some(String::from(concat!("\"", #ident_str, "\" must be a table"))),
                            }),
                        }
                    }
                }
            }.into()
        }
        Data::Enum(DataEnum { .. }) => unimplemented!(),
        Data::Union(_) => unimplemented!(),
    }
}

static TABLE_IDENT: &str = "table";

pub(crate) fn from_lua_table(input: DeriveInput) -> proc_macro2::TokenStream {
    let ident = input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    let mut has_lua_lifetime = false;
    for param in input.generics.params.iter() {
        match param {
            syn::GenericParam::Lifetime(syn::LifetimeParam { lifetime, .. }) => {
                if lifetime.ident == "lua" {
                    has_lua_lifetime = true;
                    break;
                }
            }
            _ => (),
        };
    }
    let lifetime = if has_lua_lifetime {
        syn::Lifetime::new("'lua", proc_macro2::Span::call_site())
    } else {
        syn::Lifetime::new("'_", proc_macro2::Span::call_site())
    };
    let tbl_ident = syn::Ident::new(TABLE_IDENT, proc_macro2::Span::call_site());
    match input.data {
        Data::Struct(DataStruct { fields, .. }) => {
            let self_expr = match fields {
                Fields::Named(FieldsNamed { named, .. }) => {
                    let table_gets = named.iter().filter_map(|f| gen_table_access(f));
                    quote_spanned! {named.span()=>
                        Self {
                            #(
                                #table_gets,
                            )*
                        }
                    }
                }
                Fields::Unnamed(FieldsUnnamed { unnamed, .. }) => {
                    let table_gets = unnamed.iter().enumerate().filter_map(|(i, f)| {
                        let attrs = Attrs::from(f);
                        let lua_ix = i + 1;
                        if attrs.ignored {
                            Some(quote_spanned! {f.span()=>
                                ::core::default::Default::default()
                            })
                        } else {
                            Some(quote_spanned! {f.span()=>
                                #tbl_ident.get(#lua_ix)
                            })
                        }
                    });
                    quote! { Self(#(#table_gets?, )*)}
                }
                Fields::Unit => panic!("can't derive FromLua from unit struct"),
            };
            quote! {
                impl #impl_generics #ident #ty_generics #where_clause {
                    #[allow(dead_code)]
                    pub fn from_lua_table(#tbl_ident: ::mlua::Table<#lifetime>, lua: &#lifetime ::mlua::Lua) -> ::mlua::Result<Self> {
                        Ok(#self_expr)
                    }
                }
            }.into()
        }
        Data::Enum(DataEnum { .. }) => unimplemented!(),
        Data::Union(_) => unimplemented!(),
    }
}

fn gen_table_access(f: &syn::Field) -> Option<proc_macro2::TokenStream> {
    let name = f.ident.clone()?;
    let attrs = Attrs::from(f);
    let str_name = name.to_string();
    let table_ident = syn::Ident::new(TABLE_IDENT, proc_macro2::Span::call_site());
    let table_access = if let Some(default) = attrs.lua_default {
        quote! {
            match #table_ident.get(#str_name) {
                Ok(v) => v,
                Err(::mlua::Error::FromLuaConversionError { from, .. }) if from == "nil" => #default,
                Err(e) => return Err(e),
            }
        }
    } else if attrs.ignored {
        quote!(::core::default::Default::default())
    } else {
        quote! {
            #table_ident.get(#str_name).map_err(|e| match &e {
                ::mlua::Error::FromLuaConversionError { from, to, message } => {
                    ::mlua::Error::FromLuaConversionError {
                        from,
                        to,
                        message: Some(match *from {
                            "nil" => match message {
                                None => String::from(concat!("\"", #str_name, "\"", " is required and should not be nil")),
                                Some(msg) => format!(
                                    concat!("\"", #str_name, "\"", " is required and should not be nil. {}"),
                                    msg,
                                ),
                            },
                            _ => match message {
                                None => String::from(concat!("error at field \"", #str_name, "\"")),
                                Some(msg) => format!(
                                    concat!("error at field \"", #str_name, "\": {}"),
                                    msg,
                                ),
                            }
                        }),
                    }
                }
                _ => e
            })?
        }
    };
    Some(quote_spanned! {f.span()=>
        #name: #table_access
    })
}
