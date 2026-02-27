extern crate proc_macro;

use proc_macro::TokenStream;
use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{FnArg, ItemFn, LitStr, Pat, PatType, PathArguments, Type, TypePath, parse_macro_input, spanned::Spanned};

struct ArgSpec {
    rpc: TokenStream2,
    call: TokenStream2,
}

fn generic_type(segment: &syn::PathSegment, context: &str) -> syn::Result<Type> {
    match &segment.arguments {
        PathArguments::AngleBracketed(args) => match args.args.first() {
            Some(syn::GenericArgument::Type(ty)) => Ok(ty.clone()),
            Some(other) => Err(syn::Error::new(other.span(), format!("expected type parameter for {context}"))),
            None => Err(syn::Error::new(segment.span(), format!("missing type parameter for {context}"))),
        },
        _ => Err(syn::Error::new(segment.span(), format!("expected a generic type for {context}"))),
    }
}

fn build_arg_spec(arg: &PatType, counter: &mut usize) -> syn::Result<ArgSpec> {
    let pattern: &Pat = &arg.pat;
    match &*arg.ty {
        Type::Path(TypePath { path, .. }) => {
            if let Some(segment) = path.segments.last() {
                let ident_str = segment.ident.to_string();
                match ident_str.as_str() {
                    "Data" | "ReqData" | "AsyncApiData" => {
                        let ident = format_ident!("__data{}", *counter);
                        *counter += 1;
                        let ty = arg.ty.clone();
                        let call = quote! { (*#ident).clone() };
                        return Ok(ArgSpec { rpc: quote! { #ident: jsonrpc_v2::Data<#ty> }, call });
                    }
                    "Json" => {
                        let inner = generic_type(segment, "Json")?;
                        let ident = format_ident!("__json{}", *counter);
                        *counter += 1;
                        let call = quote! { actix_web::web::Json(#ident) };
                        return Ok(ArgSpec { rpc: quote! { jsonrpc_v2::Params(#ident): jsonrpc_v2::Params<#inner> }, call });
                    }
                    "Path" => {
                        let inner = generic_type(segment, "Path")?;
                        let ident = format_ident!("__path{}", *counter);
                        *counter += 1;
                        let call = quote! { actix_web::web::Path::from(#ident) };
                        return Ok(ArgSpec { rpc: quote! { jsonrpc_v2::Params(#ident): jsonrpc_v2::Params<#inner> }, call });
                    }
                    "Query" => {
                        let inner = generic_type(segment, "Query")?;
                        let ident = format_ident!("__query{}", *counter);
                        *counter += 1;
                        let call = quote! { actix_web::web::Query(#ident) };
                        return Ok(ArgSpec { rpc: quote! { jsonrpc_v2::Params(#ident): jsonrpc_v2::Params<#inner> }, call });
                    }
                    "AsyncApiPayload" => {
                        let inner = generic_type(segment, "AsyncApiPayload")?;
                        let mut base = path.clone();
                        if let Some(last) = base.segments.last_mut() {
                            last.arguments = PathArguments::None;
                        }
                        let ident = format_ident!("__payload{}", *counter);
                        *counter += 1;
                        let call = quote! { #base::new(#ident) };
                        return Ok(ArgSpec { rpc: quote! { jsonrpc_v2::Params(#ident): jsonrpc_v2::Params<#inner> }, call });
                    }
                    "AsyncApiPath" => {
                        let inner = generic_type(segment, "AsyncApiPath")?;
                        let mut base = path.clone();
                        if let Some(last) = base.segments.last_mut() {
                            last.arguments = PathArguments::None;
                        }
                        let ident = format_ident!("__apath{}", *counter);
                        *counter += 1;
                        let call = quote! { #base::new(#ident) };
                        return Ok(ArgSpec { rpc: quote! { jsonrpc_v2::Params(#ident): jsonrpc_v2::Params<#inner> }, call });
                    }
                    _ => {}
                }
            }
            Ok(ArgSpec { rpc: quote! { #arg }, call: quote! { #pattern } })
        }
        _ => Ok(ArgSpec { rpc: quote! { #arg }, call: quote! { #pattern } }),
    }
}

#[proc_macro_attribute]
pub fn rpc(attr: TokenStream, item: TokenStream) -> TokenStream {
    // Parse method string from attribute
    let method_lit = parse_macro_input!(attr as LitStr);

    let func = parse_macro_input!(item as ItemFn);
    let orig_name = func.sig.ident.clone();
    let rpc_name = format_ident!("{}_rpc", orig_name);

    // Build arguments for rpc wrapper and call arguments for original function
    let mut rpc_args = Vec::new();
    let mut call_args = Vec::new();
    let mut idx = 0usize;

    for input in &func.sig.inputs {
        match input {
            FnArg::Receiver(receiver) => {
                return syn::Error::new(receiver.span(), format!("`#[rpc]` does not support methods with a receiver on `{}`", orig_name)).to_compile_error().into();
            }
            FnArg::Typed(arg) => match build_arg_spec(arg, &mut idx) {
                Ok(spec) => {
                    rpc_args.push(spec.rpc);
                    call_args.push(spec.call);
                }
                Err(err) => return err.to_compile_error().into(),
            },
        }
    }

    // Determine return conversion
    let mut ret_inner = None;
    if let syn::ReturnType::Type(_, ty) = &func.sig.output
        && let Type::Path(tp) = ty.as_ref()
    {
        let segs: Vec<_> = tp.path.segments.iter().map(|s| s.ident.to_string()).collect();
        if segs.ends_with(&["ApiResult".into()])
            && let syn::PathArguments::AngleBracketed(args) = &tp.path.segments.last().unwrap().arguments
            && let Some(syn::GenericArgument::Type(Type::Path(tp2))) = args.args.first()
        {
            let segs2: Vec<_> = tp2.path.segments.iter().map(|s| s.ident.to_string()).collect();
            if segs2.ends_with(&["Json".into()])
                && let syn::PathArguments::AngleBracketed(inner_args) = &tp2.path.segments.last().unwrap().arguments
                && let Some(syn::GenericArgument::Type(final_ty)) = inner_args.args.first()
            {
                ret_inner = Some(quote! { #final_ty });
            }
        }
    }

    let call = if func.sig.asyncness.is_some() {
        quote! { #orig_name( #( #call_args ),* ).await }
    } else {
        quote! { #orig_name( #( #call_args ),* ) }
    };

    let rpc_ret = if let Some(inner) = ret_inner {
        quote! {
            pub async fn #rpc_name( #( #rpc_args ),* ) -> crate::api::endpoints::ApiResult<#inner> {
                let res = #call?;
                Ok(res.into_inner())
            }
        }
    } else {
        quote! {
            pub async fn #rpc_name( #( #rpc_args ),* ) -> crate::api::endpoints::ApiResult<impl core::fmt::Debug> {
                #call
            }
        }
    };

    let method_const = format_ident!("{}__METHOD", orig_name.to_string().to_uppercase());

    let expanded = quote! {
        #func
        pub const #method_const: &str = #method_lit;
        #rpc_ret
    };

    expanded.into()
}
