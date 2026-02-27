extern crate proc_macro;

use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::{
    Attribute, Data, DeriveInput, Expr, Fields, ItemFn, LitStr, Type,
    parse::{Parse, ParseStream},
    parse_macro_input,
};

struct FieldArgs {
    example: Expr,
}

impl Parse for FieldArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let ident: syn::Ident = input.parse()?;
        if ident != "example" {
            return Err(syn::Error::new(ident.span(), "expected `example`"));
        }
        input.parse::<syn::Token![=]>()?;
        let expr: Expr = input.parse()?;
        Ok(FieldArgs { example: expr })
    }
}

struct AsyncApiArgs {
    pub summary: LitStr,
    pub description: LitStr,
    pub tags: Vec<LitStr>,
    pub responses: Vec<Type>,
}

impl Parse for AsyncApiArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let summary_ident: syn::Ident = input.parse()?;
        if summary_ident != "summary" {
            return Err(syn::Error::new(summary_ident.span(), "expected `summary`"));
        }
        input.parse::<syn::Token![=]>()?;
        let summary: LitStr = input.parse()?;
        input.parse::<syn::Token![,]>()?;
        let desc_ident: syn::Ident = input.parse()?;
        if desc_ident != "description" {
            return Err(syn::Error::new(desc_ident.span(), "expected `description`"));
        }
        input.parse::<syn::Token![=]>()?;
        let description: LitStr = input.parse()?;
        let mut tags = Vec::new();
        let mut responses = Vec::new();
        while input.parse::<syn::Token![,]>().is_ok() {
            let ident: syn::Ident = input.parse()?;
            input.parse::<syn::Token![=]>()?;
            if ident == "tags" {
                let content;
                syn::bracketed!(content in input);
                let arr = syn::punctuated::Punctuated::<LitStr, syn::Token![,]>::parse_terminated(&content)?;
                tags = arr.into_iter().collect();
            } else if ident == "response" {
                if input.peek(syn::token::Bracket) {
                    let content;
                    syn::bracketed!(content in input);
                    let arr = syn::punctuated::Punctuated::<Type, syn::Token![,]>::parse_terminated(&content)?;
                    responses = arr.into_iter().collect();
                } else {
                    responses.push(input.parse()?);
                }
            } else {
                return Err(syn::Error::new(ident.span(), "unexpected argument"));
            }
        }
        Ok(AsyncApiArgs { summary, description, tags, responses })
    }
}

#[proc_macro_attribute]
pub fn asyncapi(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as AsyncApiArgs);
    let mut func = parse_macro_input!(item as ItemFn);
    let summary = args.summary;
    let description = args.description;
    let tags = args.tags;
    let responses = args.responses;
    let meta: Attribute = if responses.is_empty() {
        syn::parse_quote! { #[__asyncapi_meta(summary = #summary, description = #description, tags = [#(#tags),*])] }
    } else {
        syn::parse_quote! { #[__asyncapi_meta(summary = #summary, description = #description, tags = [#(#tags),*], response = [#(#responses),*])] }
    };
    func.attrs.push(meta);
    quote!(#func).into()
}

#[proc_macro_derive(AsyncApiSchema, attributes(asyncapi))]
pub fn derive_async_api_schema(item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as DeriveInput);
    let name = &input.ident;
    let generics = input.generics.clone();
    let (impl_generics, ty_generics, where_clause) = generics.split_for_impl();

    let fields = match input.data {
        Data::Struct(ref s) => match &s.fields {
            Fields::Named(named) => named.named.iter().cloned().collect::<Vec<_>>(),
            _ => {
                return syn::Error::new_spanned(s.fields.clone(), "AsyncApiSchema requires named fields").to_compile_error().into();
            }
        },
        _ => {
            return syn::Error::new_spanned(input, "AsyncApiSchema can only be derived for structs").to_compile_error().into();
        }
    };

    let inserts = fields.iter().map(|f| {
        let name_str = f.ident.as_ref().unwrap().to_string();
        let ty = &f.ty;
        let mut example = None;
        for attr in &f.attrs {
            if attr.path().is_ident("asyncapi")
                && let Ok(args) = attr.parse_args::<FieldArgs>()
            {
                let ex = args.example;
                example = Some(quote! { Some(serde_json::json!(#ex)) });
            }
        }
        let get_example = if let Some(ex) = example {
            quote! { #ex }
        } else {
            quote! { <#ty as ::lib_asyncapi::SchemaProvider>::example() }
        };
        quote! {
            if let ::serde_json::Value::Object(ref mut obj) = val {
                if let Some(::serde_json::Value::Object(props)) = obj.get_mut("properties") {
                    if let Some(::serde_json::Value::Object(field)) = props.get_mut(#name_str) {
                        if let Some(ex_val) = #get_example {
                            field.insert("example".to_string(), ex_val);
                        }
                    }
                }
            }
        }
    });

    let example_inits = fields.iter().map(|f| {
        let name_str = f.ident.as_ref().unwrap().to_string();
        let ty = &f.ty;
        let mut example = None;
        for attr in &f.attrs {
            if attr.path().is_ident("asyncapi")
                && let Ok(args) = attr.parse_args::<FieldArgs>()
            {
                let ex = args.example;
                example = Some(quote! { Some(serde_json::json!(#ex)) });
            }
        }
        let get_example = if let Some(ex) = example {
            quote! { #ex }
        } else {
            quote! { <#ty as ::lib_asyncapi::SchemaProvider>::example() }
        };
        quote! {
            if let Some(val) = #get_example {
                obj.insert(#name_str.to_string(), val);
            }
        }
    });
    let field_types: Vec<_> = fields.iter().map(|f| &f.ty).collect();
    let helper_ident = format_ident!("__{}_Json", name);

    let helper_fields = fields.iter().map(|f| {
        let mut f2 = f.clone();
        f2.attrs.retain(|a| !a.path().is_ident("asyncapi"));
        f2
    });

    TokenStream::from(quote! {
        #[allow(non_camel_case_types, dead_code)]
        #[derive(schemars::JsonSchema)]
        struct #helper_ident #generics { #( #helper_fields ),* }

        impl #impl_generics schemars::JsonSchema for #name #ty_generics #where_clause {
            fn schema_name() -> ::std::borrow::Cow<'static, str> {
                ::std::borrow::Cow::Borrowed(stringify!(#name))
            }
            fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
                <#helper_ident #ty_generics as schemars::JsonSchema>::json_schema(generator)
            }
        }

        impl #impl_generics ::lib_asyncapi::SchemaProvider for #name #ty_generics #where_clause {
            const NAME: &'static str = stringify!(#name);
            fn schema() -> serde_json::Value {
                let mut val = serde_json::to_value(&schemars::schema_for!(#name #ty_generics)).unwrap();
                fn fix_refs(v: &mut serde_json::Value) {
                    match v {
                        serde_json::Value::Object(obj) => {
                            if let Some(serde_json::Value::String(r)) = obj.get_mut("$ref") {
                                if let Some(name) = r.strip_prefix("#/definitions/") {
                                    *r = format!("#/components/schemas/{}", name);
                                } else if let Some(name) = r.strip_prefix("#/$defs/") {
                                    // Draft 2020-12 places internal definitions under $defs.
                                    // We hoist those to components/schemas and fix refs accordingly.
                                    *r = format!("#/components/schemas/{}", name);
                                }
                            }
                            for val in obj.values_mut() {
                                fix_refs(val);
                            }
                        }
                        serde_json::Value::Array(arr) => {
                            for val in arr {
                                fix_refs(val);
                            }
                        }
                        _ => {}
                    }
                }

                fn add_no_extra(v: &mut serde_json::Value) {
                    match v {
                        serde_json::Value::Object(obj) => {
                            if obj.contains_key("properties") && !obj.contains_key("additionalProperties") {
                                obj.insert("additionalProperties".to_string(), serde_json::Value::Bool(false));
                            }
                            for val in obj.values_mut() {
                                add_no_extra(val);
                            }
                        }
                        serde_json::Value::Array(arr) => {
                            for val in arr {
                                add_no_extra(val);
                            }
                        }
                        _ => {}
                    }
                }

                fn prune_defs(v: &mut serde_json::Value) {
                    match v {
                        serde_json::Value::Object(obj) => {
                            // Remove any leftover $defs blocks to avoid unresolved local refs.
                            obj.remove("$defs");
                            for val in obj.values_mut() {
                                prune_defs(val);
                            }
                        }
                        serde_json::Value::Array(arr) => {
                            for val in arr {
                                prune_defs(val);
                            }
                        }
                        _ => {}
                    }
                }

                fix_refs(&mut val);
                add_no_extra(&mut val);
                prune_defs(&mut val);
                #( #inserts )*
                val
            }
            fn example() -> ::core::option::Option<serde_json::Value> {
                let mut obj = ::serde_json::Map::new();
                #( #example_inits )*
                if obj.is_empty() { None } else { Some(::serde_json::Value::Object(obj)) }
            }
            fn register_schemas(map: &mut ::std::collections::BTreeMap<String, serde_json::Value>) {
                if map.contains_key(Self::NAME) {
                    return;
                }
                map.insert(Self::NAME.to_string(), Self::schema());
                #( <#field_types as ::lib_asyncapi::SchemaProvider>::register_schemas(map); )*
            }
        }
    })
}
