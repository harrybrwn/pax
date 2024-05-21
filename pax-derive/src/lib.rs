extern crate proc_macro;

mod from_lua;
mod into_lua;
mod userdata;

use from_lua::{from_lua, from_lua_table};
use into_lua::into_lua;

use proc_macro::TokenStream;
use quote::{quote, quote_spanned};
use syn::{
    parse_macro_input, spanned::Spanned, Data, DeriveInput, Fields, FieldsNamed, FieldsUnnamed,
    Ident, Index,
};

#[proc_macro_derive(IntoLua)]
pub fn derive_into_lua(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    into_lua(input)
}

#[proc_macro_derive(FromLua, attributes(lua_default, ignored))]
pub fn derive_from_lua(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    from_lua(input)
}

#[proc_macro_derive(FromLuaTable, attributes(lua_default, ignored))]
pub fn derive_from_lua_table(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    from_lua_table(input).into()
}

#[proc_macro_derive(UserData)]
pub fn derive_userdata(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    userdata::userdata(input, false).into()
}

#[proc_macro_derive(UserDataWithDefault)]
pub fn derive_userdata_with_default(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    userdata::userdata(input, true).into()
}

#[derive(Debug, Default)]
struct Attrs<'a> {
    lua_default: Option<&'a proc_macro2::TokenStream>,
    ignored: bool,
}

impl syn::parse::Parse for Attrs<'_> {
    fn parse(input: syn::parse::ParseStream) -> syn::Result<Self> {
        println!("Attrs::parse: {}", input.to_string());
        unimplemented!()
    }
}

impl syn::parse::Parser for Attrs<'_> {
    type Output = Self;
    fn parse2(self, tokens: proc_macro2::TokenStream) -> syn::Result<Self::Output> {
        println!("Attrs:parse2: {}", tokens.to_string());
        unimplemented!()
    }
}

// use with attr.parse_args_with(NoopParser)
struct NoopParser;

impl syn::parse::Parser for NoopParser {
    type Output = proc_macro2::TokenStream;
    fn parse2(self, tokens: proc_macro2::TokenStream) -> syn::Result<Self::Output> {
        Ok(tokens)
    }
}

impl<'a> From<&'a syn::Field> for Attrs<'a> {
    fn from(value: &'a syn::Field) -> Self {
        let mut res = Self::default();
        for attr in &value.attrs {
            if attr.path().is_ident("lua_default") {
                res.lua_default = get_tokens(attr).ok();
            } else if attr.path().is_ident("ignored") {
                res.ignored = true;
            } else {
                continue;
            }
        }
        res
    }
}

fn get_tokens<'a>(attr: &'a syn::Attribute) -> syn::Result<&'a proc_macro2::TokenStream> {
    use syn::{Meta, MetaList};
    match &attr.meta {
        Meta::Path(path) => Err(syn::Error::new(path.span(), "unexpected path")),
        Meta::NameValue(nv) => Err(syn::Error::new(nv.span(), "unexpected named value")),
        Meta::List(MetaList { tokens, .. }) => Ok(tokens),
    }
}

#[proc_macro_derive(LuaGettersSetters)]
pub fn derive_lua_getterssetters(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let ident = input.ident;
    let functions = match input.data {
        Data::Struct(sd) => match sd.fields {
            Fields::Named(FieldsNamed { named, .. }) => {
                let funcs = named.into_iter().filter(|f| f.ident.is_some()).map(|f| {
                    let name = f.ident.as_ref().unwrap();
                    let getter = Ident::new(&format!("get_{}", name.to_string()), proc_macro2::Span::call_site());
                    let setter = Ident::new(&format!("set_{}", name.to_string()), proc_macro2::Span::call_site());
                    let ty = f.ty.clone();
                    quote_spanned! {f.span()=>
                        #[allow(unused_variables)]
                        fn #getter<'lua>(lua: &'lua ::mlua::Lua, this: &Self) -> ::mlua::Result<#ty> {
                            Ok(this.#name.clone())
                        }
                        #[allow(unused_variables)]
                        fn #setter<'lua>(lua: &'lua ::mlua::Lua, this: &mut Self, val: #ty) -> ::mlua::Result<()> {
                            this.#name = val;
                            Ok(())
                        }
                    }
                });
                quote! {
                    #(
                        #funcs
                    )*
                }
            }
            Fields::Unnamed(FieldsUnnamed { unnamed, .. }) => {
                let funcs = unnamed.iter().enumerate().map(|(i, f)| {
                    let index = Index::from(i);
                    let getter = Ident::new(&format!("get_{}", i), proc_macro2::Span::call_site());
                    let setter = Ident::new(&format!("set_{}", i), proc_macro2::Span::call_site());
                    let ty = f.ty.clone();
                    quote_spanned! {f.span()=>
                        #[allow(unused_variables)]
                        fn #getter<'lua>(lua: &'lua ::mlua::Lua, this: &Self) -> ::mlua::Result<#ty> {
                            Ok(this.#index.clone())
                        }
                        #[allow(unused_variables)]
                        fn #setter<'lua>(lua: &'lua ::mlua::Lua, this: &mut Self, val: #ty) -> ::mlua::Result<()> {
                            this.#index = val;
                            Ok(())
                        }
                    }
                });
                quote! {
                    #(
                        #funcs
                    )*
                }
            }
            Fields::Unit => unimplemented!(),
        },
        _ => unimplemented!("only structs are supported"),
    };
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    quote! {
        impl #impl_generics #ident #ty_generics #where_clause {
            #functions
        }
    }
    .into()
}

#[cfg(test)]
mod tests {}
