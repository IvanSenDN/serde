//! Derive macro for `DeserializeIn` trait.

use crate::internals::ast::{Container, Data, Field, Style};
use crate::internals::{replace_receiver, Ctxt, Derive};
use crate::{dummy, private};
use proc_macro2::{Span, TokenStream};
use quote::quote;
use syn::Ident;

pub fn expand_derive_deserialize_in(input: &mut syn::DeriveInput) -> syn::Result<TokenStream> {
    replace_receiver(input);

    let ctxt = Ctxt::new();
    let Some(cont) = Container::from_ast(&ctxt, input, Derive::Deserialize, &private.ident())
    else {
        return Err(ctxt.check().unwrap_err());
    };
    ctxt.check()?;

    let ident = &cont.ident;
    let generics = &cont.generics;
    let (_, ty_generics, _) = generics.split_for_impl();

    let alloc_param = find_allocator_param(generics);

    let body = match &cont.data {
        Data::Struct(style, fields) => {
            deserialize_struct(*style, fields, alloc_param.as_ref(), &cont.ident)
        }
        Data::Enum(_) => {
            quote! { compile_error!("DeserializeIn for enums is not yet implemented") }
        }
    };

    let impl_block = if let Some(ref alloc_ident) = alloc_param {
        let other_params: Vec<_> = generics
            .params
            .iter()
            .filter(|p| {
                if let syn::GenericParam::Type(tp) = p {
                    tp.ident != *alloc_ident
                } else {
                    true
                }
            })
            .collect();

        let other_params_tokens = if other_params.is_empty() {
            quote! {}
        } else {
            quote! { #(#other_params,)* }
        };

        quote! {
            #[automatically_derived]
            impl<'__de, #other_params_tokens #alloc_ident> _serde::de::DeserializeIn<'__de, #alloc_ident> for #ident #ty_generics
            where
                #alloc_ident: ::core::alloc::Allocator + ::core::marker::Copy,
            {
                fn deserialize_in<__D>(__deserializer: __D, __alloc: #alloc_ident) -> ::core::result::Result<Self, __D::Error>
                where
                    __D: _serde::Deserializer<'__de>,
                {
                    #body
                }
            }
        }
    } else {
        quote! {
            compile_error!("DeserializeIn requires a type parameter with `Allocator` bound")
        }
    };

    Ok(dummy::wrap_in_const_for_allocator_api(
        cont.attrs.custom_serde_path(),
        impl_block,
    ))
}

fn find_allocator_param(generics: &syn::Generics) -> Option<Ident> {
    for param in &generics.params {
        if let syn::GenericParam::Type(type_param) = param {
            for bound in &type_param.bounds {
                if let syn::TypeParamBound::Trait(trait_bound) = bound {
                    let path = &trait_bound.path;
                    if path
                        .segments
                        .last()
                        .map(|s| s.ident == "Allocator")
                        .unwrap_or(false)
                    {
                        return Some(type_param.ident.clone());
                    }
                }
            }
        }
    }
    None
}

fn deserialize_struct(
    style: Style,
    fields: &[Field],
    alloc_param: Option<&Ident>,
    struct_ident: &Ident,
) -> TokenStream {
    match style {
        Style::Struct => deserialize_struct_named(fields, alloc_param, struct_ident),
        _ => quote! { compile_error!("Only named structs are supported for DeserializeIn") },
    }
}

fn deserialize_struct_named(
    fields: &[Field],
    alloc_param: Option<&Ident>,
    struct_ident: &Ident,
) -> TokenStream {
    let alloc_param = match alloc_param {
        Some(p) => p,
        None => return quote! { compile_error!("alloc_param required") },
    };

    let struct_name_str = struct_ident.to_string();

    // Collect field info
    let field_data: Vec<_> = fields
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let ident = f
                .original
                .ident
                .clone()
                .unwrap_or_else(|| Ident::new(&format!("__field{}", i), Span::call_site()));
            let name_str = f.attrs.name().deserialize_name().to_string();
            let ty = &f.ty;
            (ident, name_str, ty)
        })
        .collect();

    let field_count = field_data.len();

    // Field enum variants
    let field_enum_variants: Vec<TokenStream> = (0..field_count)
        .map(|i| {
            let variant = Ident::new(&format!("__Field{}", i), Span::call_site());
            quote! { #variant }
        })
        .collect();

    // Match arms for field names
    let field_match_arms: Vec<TokenStream> = field_data
        .iter()
        .enumerate()
        .map(|(i, (_, name_str, _))| {
            let variant = Ident::new(&format!("__Field{}", i), Span::call_site());
            quote! { #name_str => ::core::result::Result::Ok(__Field::#variant) }
        })
        .collect();

    // Variable declarations
    let field_vars: Vec<TokenStream> = (0..field_count)
        .map(|i| {
            let var = Ident::new(&format!("__field{}", i), Span::call_site());
            quote! { let mut #var: ::core::option::Option<_> = ::core::option::Option::None; }
        })
        .collect();

    // Visit map arms
    let visit_map_arms: Vec<TokenStream> = field_data
        .iter()
        .enumerate()
        .map(|(i, (ident, _, ty))| {
            let variant = Ident::new(&format!("__Field{}", i), Span::call_site());
            let var = Ident::new(&format!("__field{}", i), Span::call_site());
            quote! {
                __Field::#variant => {
                    if ::core::option::Option::is_some(&#var) {
                        return ::core::result::Result::Err(<__A::Error as _serde::de::Error>::duplicate_field(stringify!(#ident)));
                    }
                    #var = ::core::option::Option::Some(
                        _serde::de::MapAccess::next_value_seed(
                            &mut __map,
                            __Seed::<#ty, #alloc_param> {
                                alloc: self.__alloc.clone(),
                                marker: ::core::marker::PhantomData,
                            }
                        )?
                    );
                }
            }
        })
        .collect();

    // Struct construction
    let field_unwraps: Vec<TokenStream> = field_data
        .iter()
        .enumerate()
        .map(|(i, (ident, name_str, _))| {
            let var = Ident::new(&format!("__field{}", i), Span::call_site());
            quote! {
                #ident: #var.ok_or_else(|| <__A::Error as _serde::de::Error>::missing_field(#name_str))?
            }
        })
        .collect();

    // Field names array
    let field_names_array: Vec<TokenStream> = field_data
        .iter()
        .map(|(_, name_str, _)| quote! { #name_str })
        .collect();

    quote! {
        struct __Seed<__T, __A> {
            alloc: __A,
            marker: ::core::marker::PhantomData<__T>,
        }

        impl<'__de, __T, __A> _serde::de::DeserializeSeed<'__de> for __Seed<__T, __A>
        where
            __T: _serde::de::DeserializeIn<'__de, __A>,
            __A: ::core::alloc::Allocator + ::core::marker::Copy,
        {
            type Value = __T;

            fn deserialize<__D>(self, deserializer: __D) -> ::core::result::Result<Self::Value, __D::Error>
            where
                __D: _serde::Deserializer<'__de>,
            {
                _serde::de::DeserializeIn::deserialize_in(deserializer, self.alloc)
            }
        }

        #[allow(non_camel_case_types)]
        enum __Field {
            #(#field_enum_variants,)*
            __ignore,
        }

        impl<'__de> _serde::Deserialize<'__de> for __Field {
            fn deserialize<__D>(__deserializer: __D) -> ::core::result::Result<Self, __D::Error>
            where
                __D: _serde::Deserializer<'__de>,
            {
                struct __FieldVisitor;

                impl<'__de> _serde::de::Visitor<'__de> for __FieldVisitor {
                    type Value = __Field;

                    fn expecting(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                        ::core::fmt::Formatter::write_str(f, "field identifier")
                    }

                    fn visit_str<__E>(self, v: &str) -> ::core::result::Result<Self::Value, __E>
                    where
                        __E: _serde::de::Error,
                    {
                        match v {
                            #(#field_match_arms,)*
                            _ => ::core::result::Result::Ok(__Field::__ignore),
                        }
                    }
                }

                _serde::Deserializer::deserialize_identifier(__deserializer, __FieldVisitor)
            }
        }

        struct __Visitor<#alloc_param> {
            __alloc: #alloc_param,
        }

        impl<'__de, #alloc_param> _serde::de::Visitor<'__de> for __Visitor<#alloc_param>
        where
            #alloc_param: ::core::alloc::Allocator + ::core::marker::Copy,
        {
            type Value = #struct_ident<#alloc_param>;

            fn expecting(&self, f: &mut ::core::fmt::Formatter) -> ::core::fmt::Result {
                ::core::fmt::Formatter::write_str(f, #struct_name_str)
            }

            fn visit_map<__A>(self, mut __map: __A) -> ::core::result::Result<Self::Value, __A::Error>
            where
                __A: _serde::de::MapAccess<'__de>,
            {
                #(#field_vars)*

                while let ::core::option::Option::Some(__key) = _serde::de::MapAccess::next_key::<__Field>(&mut __map)? {
                    match __key {
                        #(#visit_map_arms)*
                        __Field::__ignore => {
                            let _ = _serde::de::MapAccess::next_value::<_serde::de::IgnoredAny>(&mut __map)?;
                        }
                    }
                }

                ::core::result::Result::Ok(#struct_ident {
                    #(#field_unwraps,)*
                })
            }
        }

        const __FIELDS: &[&str] = &[#(#field_names_array),*];

        _serde::Deserializer::deserialize_struct(
            __deserializer,
            #struct_name_str,
            __FIELDS,
            __Visitor { __alloc: __alloc }
        )
    }
}
