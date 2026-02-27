extern crate proc_macro;

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse::Parse, parse::ParseStream, parse_macro_input, FnArg, GenericArgument, ItemFn, LitStr, PatType, PathArguments, Type};

struct AsyncApiArgs {
    summary: LitStr,
    description: LitStr,
    tags: Vec<LitStr>,
    responses: Vec<Type>,
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

fn is_ident(ty: &Type, name: &str) -> bool {
    if let Type::Path(p) = ty {
        if let Some(seg) = p.path.segments.last() {
            return seg.ident == name;
        }
    }
    false
}

fn generic_inner(ty: &Type) -> Option<Type> {
    if let Type::Path(p) = ty {
        if let Some(seg) = p.path.segments.last() {
            if let PathArguments::AngleBracketed(args) = &seg.arguments {
                if let Some(GenericArgument::Type(t)) = args.args.first() {
                    return Some(t.clone());
                }
            }
        }
    }
    None
}

#[proc_macro_attribute]
pub fn nt4(attr: TokenStream, item: TokenStream) -> TokenStream {
    let attr_s = attr.to_string();
    let mut kind = "pub".to_string();
    let mut pattern = "".to_string();
    for part in attr_s.split(',') {
        let part = part.trim();
        if part == "sub" || part == "pub" || part == "pubsub" {
            kind = part.to_string();
        } else if part.starts_with('"') && part.ends_with('"') {
            pattern = part.trim_matches('"').to_string();
        }
    }

    let path = pattern.split('.').filter(|s| !s.starts_with('{') && !s.ends_with('}')).collect::<Vec<_>>().join(".");

    let mut path_param_names = Vec::new();
    for part in pattern.split('.') {
        if let Some(stripped) = part.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
            path_param_names.push(stripped.to_string());
        }
    }

    let mut func = parse_macro_input!(item as ItemFn);
    let mut summary = None;
    let mut description = None;
    let mut tags_meta: Vec<LitStr> = Vec::new();
    let mut response_meta: Vec<Type> = Vec::new();
    func.attrs.retain(|attr| {
        if attr.path().is_ident("__asyncapi_meta") {
            if let Ok(args) = attr.parse_args::<AsyncApiArgs>() {
                summary = Some(args.summary);
                description = Some(args.description);
                tags_meta = args.tags;
                response_meta = args.responses;
            }
            false
        } else {
            true
        }
    });
    let vis = &func.vis;
    let sig = &func.sig;
    let block = &func.block;
    let name = &sig.ident;
    let asyncness = sig.asyncness.is_some();
    let struct_name = name.clone();

    let mut init_stmts = Vec::new();
    let mut call_args = Vec::new();
    let mut payload_ty: Option<Type> = None;
    let mut path_param_types: Vec<Type> = Vec::new();

    for arg in &sig.inputs {
        if let FnArg::Typed(PatType { pat, ty, .. }) = arg {
            let ident = match &**pat {
                syn::Pat::Ident(i) => &i.ident,
                _ => continue,
            };
            if is_ident(ty, "Nt4App") || is_ident(ty, "Arc") {
                call_args.push(quote! { app.clone() });
            } else if is_ident(ty, "AsyncApiData") {
                if let Some(inner) = generic_inner(ty) {
                    init_stmts.push(quote! { let #ident = ::lib_asyncapi::AsyncApiData::new(app.get_data::<#inner>().expect("missing data")); });
                    call_args.push(quote! { #ident });
                }
            } else if is_ident(ty, "AsyncApiPayload") {
                if let Some(inner) = generic_inner(ty) {
                    payload_ty = Some(inner.clone());
                    init_stmts.push(quote! { let #ident = ::lib_asyncapi::AsyncApiPayload::<#inner>::new(serde_json::from_value(val.clone()).expect("payload")); });
                    call_args.push(quote! { #ident });
                }
            } else if is_ident(ty, "AsyncApiPath") {
                if let Some(inner) = generic_inner(ty) {
                    path_param_types.push(inner.clone());
                    init_stmts.push(quote! { let #ident = ::lib_asyncapi::AsyncApiPath::<#inner>::extract(Self::PATTERN, &path).expect("path"); });
                    call_args.push(quote! { #ident });
                }
            } else if is_ident(ty, "String") {
                call_args.push(quote! { path.clone() });
            } else if is_ident(ty, "Value") {
                call_args.push(quote! { val.clone() });
            } else {
                call_args.push(quote! { #ident });
            }
        }
    }

    let call = if asyncness {
        quote! { Self::call(#(#call_args),*).await }
    } else {
        quote! { Self::call(#(#call_args),*) }
    };

    let inputs = sig.inputs.clone();
    let output = &sig.output;
    let async_token = sig.asyncness;
    let call_fn = quote! { #async_token fn call(#inputs) #output #block };
    let ret_ty: syn::Type = match &sig.output {
        syn::ReturnType::Type(_, ty) => *ty.clone(),
        _ => syn::parse_quote! { () },
    };

    let execute_fn = match kind.as_str() {
        "sub" => quote! {
            pub fn execute(app: ::std::sync::Arc<::lib_nt4::Nt4App>, path: String, val: serde_json::Value) -> ::futures::future::BoxFuture<'static, ()> {
                #( #init_stmts )*
                Box::pin(async move { #call; })
            }
        },
        "pubsub" => quote! {
            pub fn execute(app: ::std::sync::Arc<::lib_nt4::Nt4App>, path: String, val: serde_json::Value) -> ::futures::future::BoxFuture<'static, #ret_ty> {
                #( #init_stmts )*
                Box::pin(async move { #call })
            }
        },
        _ => quote! {
            pub fn execute(app: ::std::sync::Arc<::lib_nt4::Nt4App>) -> ::futures::future::BoxFuture<'static, #ret_ty> {
                #( #init_stmts )*
                Box::pin(async move { #call })
            }
        },
    };

    if path_param_names.len() != path_param_types.len() {
        return syn::Error::new(proc_macro2::Span::call_site(), "path parameter count mismatch").to_compile_error().into();
    }

    let param_pairs: Vec<_> = path_param_names
        .iter()
        .zip(path_param_types.iter())
        .map(|(n, ty)| {
            quote! {(#n.to_string(), ::lib_asyncapi::TypeSchema{ name: <#ty as ::lib_asyncapi::SchemaProvider>::NAME, schema: <#ty as ::lib_asyncapi::SchemaProvider>::schema() })}
        })
        .collect();

    let doc_impl = if let (Some(summary), Some(description)) = (summary.as_ref(), description.as_ref()) {
        let payload_type = payload_ty.as_ref().map(|ty| {
            quote! { Some(::lib_asyncapi::TypeSchema{ name: <#ty as ::lib_asyncapi::SchemaProvider>::NAME, schema: <#ty as ::lib_asyncapi::SchemaProvider>::schema() }) }
        });
        let response_vec = if response_meta.is_empty() {
            quote! { Vec::new() }
        } else {
            let types = response_meta.iter().map(|ty| {
                quote! { ::lib_asyncapi::TypeSchema{ name: <#ty as ::lib_asyncapi::SchemaProvider>::NAME, schema: <#ty as ::lib_asyncapi::SchemaProvider>::schema() } }
            });
            quote! { vec![#(#types),*] }
        };
        let payload_field = payload_type.unwrap_or_else(|| quote! { None });
        let register_payload = payload_ty.as_ref().map(|ty| quote! { <#ty as ::lib_asyncapi::SchemaProvider>::register_schemas(map); });
        let register_responses = response_meta.iter().map(|ty| quote! { <#ty as ::lib_asyncapi::SchemaProvider>::register_schemas(map); });
        let register_params = path_param_types.iter().map(|ty| quote! { <#ty as ::lib_asyncapi::SchemaProvider>::register_schemas(map); });
        quote! {
            impl ::lib_asyncapi::DocumentedCommand for #struct_name {
                fn doc() -> ::core::option::Option<::lib_asyncapi::WsDoc> {
                    Some(::lib_asyncapi::WsDoc {
                        path: Self::PATTERN,
                        summary: #summary,
                        description: #description,
                        tags: vec![#(#tags_meta.to_string()),*],
                        payload: #payload_field,
                        responses: #response_vec,
                        params: vec![#(#param_pairs),*],
                    })
                }
                fn register_schemas(map: &mut ::std::collections::BTreeMap<String, serde_json::Value>) {
                    #( #register_params )*
                    #( #register_responses )*
                    #register_payload
                }
            }
        }
    } else {
        quote! {}
    };

    let trait_impl = match kind.as_str() {
        "sub" => quote! {
            impl ::lib_nt4::SubHandler for #struct_name {
                const PATH: &'static str = #path;
                const PATTERN: &'static str = #pattern;
                fn execute(app: ::std::sync::Arc<::lib_nt4::Nt4App>, path: String, val: serde_json::Value) -> ::futures::future::BoxFuture<'static, ()> {
                    #struct_name::execute(app, path, val)
                }
            }
        },
        "pubsub" => quote! {
            impl ::lib_nt4::PubSubHandler for #struct_name {
                type Output = #ret_ty;
                const PATH: &'static str = #path;
                const PATTERN: &'static str = #pattern;
                fn execute(app: ::std::sync::Arc<::lib_nt4::Nt4App>, path: String, val: serde_json::Value) -> ::futures::future::BoxFuture<'static, Self::Output> {
                    #struct_name::execute(app, path, val)
                }
            }
        },
        _ => quote! {
            impl ::lib_nt4::PubHandler for #struct_name {
                type Output = #ret_ty;
                const PATH: &'static str = #path;
                const PATTERN: &'static str = #pattern;
                fn execute(app: ::std::sync::Arc<::lib_nt4::Nt4App>) -> ::futures::future::BoxFuture<'static, Self::Output> {
                    #struct_name::execute(app)
                }
            }
        },
    };

    TokenStream::from(quote! {
        #[allow(non_camel_case_types)]
        #vis struct #struct_name;
        impl #struct_name {
            pub const PATH: &'static str = #path;
            pub const PATTERN: &'static str = #pattern;
            #call_fn
            #execute_fn
        }
        #trait_impl
        #doc_impl
    })
}
