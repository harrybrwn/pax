use quote::{quote, quote_spanned};
use syn::{spanned::Spanned, DeriveInput};
use syn::{Data, DataEnum, DataStruct, Fields, FieldsNamed, FieldsUnnamed, Index};

use crate::Attrs;

pub(crate) fn into_lua(input: DeriveInput) -> proc_macro::TokenStream {
    let ident = input.ident;
    let (impl_generics, ty_generics, where_clause) = input.generics.split_for_impl();
    match input.data {
        Data::Struct(DataStruct { fields, .. }) => {
            let table_sets: Vec<_> = match fields {
                Fields::Named(FieldsNamed { named, .. }) => named
                    .iter()
                    .filter_map(|f| {
                        let attrs = Attrs::from(f);
                        if attrs.ignored {
                            return None;
                        }
                        let name = f.ident.clone().unwrap();
                        let str_name = name.to_string();
                        Some(quote_spanned! {f.span()=>
                            tbl.set(#str_name, self.#name)?
                        })
                    })
                    .collect(),
                Fields::Unnamed(FieldsUnnamed { unnamed, .. }) => {
                    unnamed.iter().enumerate().filter_map(|(i, f)| {
                        let attrs = Attrs::from(f);
                        if attrs.ignored {
                            return None;
                        }
                        let lua_ix = i + 1;
                        let index = Index::from(i);
                        Some(quote_spanned! {f.span()=>
                            tbl.set(#lua_ix, self.#index)?
                        })
                    })
                }
                .collect(),
                Fields::Unit => panic!("can't derive FromLua from unit struct"),
            };
            let n_fields = table_sets.len(); // used for preallocation
            quote! {
                impl #impl_generics ::mlua::IntoLua<'_> for #ident #ty_generics #where_clause {
                    fn into_lua(self, lua: &'_ ::mlua::Lua) -> ::mlua::prelude::LuaResult<::mlua::prelude::LuaValue<'_>> {
                        let tbl = lua.create_table_with_capacity(0, #n_fields)?;
                        #(
                            #table_sets;
                        )*
                        Ok(::mlua::prelude::LuaValue::Table(tbl))
                    }
                }
            }.into()
        }
        Data::Enum(DataEnum { .. }) => unimplemented!(),
        Data::Union(_) => unimplemented!(),
    }
}
