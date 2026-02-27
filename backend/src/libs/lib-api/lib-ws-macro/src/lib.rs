extern crate proc_macro;

use proc_macro::TokenStream;
use quote::quote;
use syn::{FnArg, GenericArgument, Ident, ItemFn, LitStr, PatType, PathArguments, Type, ext::IdentExt, parse::Parse, parse::ParseStream, parse_macro_input, spanned::Spanned};

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

enum WsKind {
    None,
    Command(LitStr),
    Loop(LitStr),
}

struct WsArgs {
    kind: WsKind,
}

impl Parse for WsArgs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.is_empty() {
            return Ok(WsArgs { kind: WsKind::None });
        }
        if input.peek(LitStr) {
            let lit: LitStr = input.parse()?;
            return Ok(WsArgs { kind: WsKind::Command(lit) });
        }
        let ident: Ident = input.call(Ident::parse_any)?;
        if ident == "loop" {
            input.parse::<syn::Token![=]>()?;
            let lit: LitStr = input.parse()?;
            return Ok(WsArgs { kind: WsKind::Loop(lit) });
        }
        Err(syn::Error::new(ident.span(), "expected string literal or `loop = \"name\"`"))
    }
}

#[proc_macro_attribute]
pub fn ws(attr: TokenStream, item: TokenStream) -> TokenStream {
    fn strip_reference(ty: &Type) -> &Type {
        if let Type::Reference(r) = ty { &r.elem } else { ty }
    }

    fn is_ident(ty: &Type, name: &str) -> bool {
        if let Type::Path(p) = strip_reference(ty)
            && let Some(seg) = p.path.segments.last()
        {
            return seg.ident == name;
        }
        false
    }

    fn generic_inner(ty: &Type) -> Option<Type> {
        if let Type::Path(p) = strip_reference(ty)
            && let Some(seg) = p.path.segments.last()
            && let PathArguments::AngleBracketed(args) = &seg.arguments
            && let Some(GenericArgument::Type(t)) = args.args.first()
        {
            return Some(t.clone());
        }
        None
    }

    let args = parse_macro_input!(attr as WsArgs);
    let (path, is_loop) = match args.kind {
        WsKind::None => (None, false),
        WsKind::Command(p) => (Some(p.value()), false),
        WsKind::Loop(p) => (Some(p.value()), true),
    };
    if is_loop {
        let lit = match &path {
            Some(p) => LitStr::new(p, proc_macro2::Span::call_site()),
            None => LitStr::new("", proc_macro2::Span::call_site()),
        };
        return ws_loop_impl(lit, item);
    }
    let mut path_param_names = Vec::new();
    if let Some(ref p) = path {
        for part in p.split('.') {
            if let Some(stripped) = part.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                path_param_names.push(stripped.to_string());
            }
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
    let asyncness = &func.sig.asyncness;
    let inputs = &func.sig.inputs;
    let output = &func.sig.output;
    let block = &func.block;
    let name = &func.sig.ident;

    let struct_name = name.clone();

    let path_const = if let Some(ref p) = path {
        quote! { pub const PATH: &'static str = #p; }
    } else {
        quote! { pub const PATH: &'static str = ""; }
    };

    // build execute method
    let mut pre_stmts = Vec::new();
    let mut init_stmts = Vec::new();
    let mut parse_msg = false;
    let mut call_args = Vec::new();
    let mut payload_ty: Option<Type> = None;
    let mut path_param_types: Vec<Type> = Vec::new();

    for arg in inputs.iter() {
        if let FnArg::Typed(PatType { pat, ty, .. }) = arg {
            let ident = match &**pat {
                syn::Pat::Ident(p) => &p.ident,
                _ => continue,
            };
            if is_ident(ty, "AsyncApiData") {
                if let Some(inner) = generic_inner(ty) {
                    init_stmts.push(quote! { let #ident = ::lib_asyncapi::AsyncApiData::new(app.get_data::<#inner>().expect("missing data")); });
                    call_args.push(quote! { #ident });
                }
            } else if is_ident(ty, "AsyncApiPath") {
                let mut single_inner = None;
                if let Some(inner) = generic_inner(ty) {
                    match &inner {
                        syn::Type::Tuple(t) => {
                            for elem in &t.elems {
                                path_param_types.push(elem.clone());
                            }
                        }
                        syn::Type::Paren(p) => {
                            path_param_types.push((*p.elem).clone());
                            single_inner = Some((*p.elem).clone());
                        }
                        _ => {
                            path_param_types.push(inner.clone());
                            single_inner = Some(inner.clone());
                        }
                    }
                }
                if let Some(inner_ty) = single_inner {
                    pre_stmts.push(quote! {
                        let cmd_str = match val.get("cmd").and_then(|c| c.as_str()) {
                            Some(c) => c,
                            None => {
                                ::lib_ws::WsApp::handle_error(app.clone(), ctx, ::lib_ws::Error::Handler("missing cmd".into()));
                                return ::std::boxed::Box::pin(async {});
                            }
                        };
                        let tmp = match ::lib_asyncapi::AsyncApiPath::<(#inner_ty,)>::extract(Self::PATH, cmd_str) {
                            Some(p) => p,
                            None => {
                                ::lib_ws::WsApp::handle_error(app.clone(), ctx, ::lib_ws::Error::Handler("path mismatch".into()));
                                return ::std::boxed::Box::pin(async {});
                            }
                        };
                        let #ident = ::lib_asyncapi::AsyncApiPath::new(tmp.0 .0);
                    });
                } else {
                    pre_stmts.push(quote! {
                        let cmd_str = match val.get("cmd").and_then(|c| c.as_str()) {
                            Some(c) => c,
                            None => {
                                ::lib_ws::WsApp::handle_error(app.clone(), ctx, ::lib_ws::Error::Handler("missing cmd".into()));
                                return ::std::boxed::Box::pin(async {});
                            }
                        };
                        let #ident = match ::lib_asyncapi::AsyncApiPath::extract(Self::PATH, cmd_str) {
                            Some(p) => p,
                            None => {
                                ::lib_ws::WsApp::handle_error(app.clone(), ctx, ::lib_ws::Error::Handler("path mismatch".into()));
                                return ::std::boxed::Box::pin(async {});
                            }
                        };
                    });
                }
                call_args.push(quote! { #ident });
            } else if is_ident(ty, "AsyncApiPayload") {
                if let Some(inner) = generic_inner(ty) {
                    payload_ty = Some(inner.clone());
                    if path.is_some() {
                        pre_stmts.push(quote! {
                            let payload_val = match val.get("payload") {
                                Some(v) => v.clone(),
                                None => {
                                    ::lib_ws::WsApp::handle_error(app.clone(), ctx, ::lib_ws::Error::Handler("missing payload".into()));
                                    return ::std::boxed::Box::pin(async {});
                                }
                            };
                            let #ident = match ::serde_json::from_value::<#inner>(payload_val) {
                                Ok(v) => ::lib_asyncapi::AsyncApiPayload::new(v),
                                Err(e) => {
                                    ::lib_ws::WsApp::handle_error(app.clone(), ctx, e.into());
                                    return ::std::boxed::Box::pin(async {});
                                }
                            };
                        });
                    } else {
                        parse_msg = true;
                        pre_stmts.push(quote! {
                            let payload_val = match __msg_val.get("payload") {
                                Some(v) => v.clone(),
                                None => {
                                    ::lib_ws::WsApp::handle_error(app.clone(), ctx, ::lib_ws::Error::Handler("missing payload".into()));
                                    return ::std::boxed::Box::pin(async {});
                                }
                            };
                            let #ident = match ::serde_json::from_value::<#inner>(payload_val) {
                                Ok(v) => ::lib_asyncapi::AsyncApiPayload::new(v),
                                Err(e) => {
                                    ::lib_ws::WsApp::handle_error(app.clone(), ctx, e.into());
                                    return ::std::boxed::Box::pin(async {});
                                }
                            };
                        });
                    }
                    call_args.push(quote! { #ident });
                }
            } else if is_ident(ty, "WebsocketContext") || is_ident(ty, "WsContext") {
                call_args.push(quote! { ctx });
            } else if is_ident(ty, "Message") {
                call_args.push(quote! { &msg });
            } else if is_ident(ty, "Value") {
                call_args.push(quote! { val.clone() });
            } else {
                return syn::Error::new(ty.span(), "unsupported argument type").to_compile_error().into();
            }
        }
    }

    if path.is_none() && parse_msg {
        pre_stmts.insert(
            0,
            quote! {
                let __msg_val = match msg {
                    ::actix_web_actors::ws::Message::Text(t) => match ::serde_json::from_str::<serde_json::Value>(t) {
                        Ok(v) => v,
                        Err(e) => {
                            ::lib_ws::WsApp::handle_error(app.clone(), ctx, e.into());
                            return ::std::boxed::Box::pin(async {});
                        }
                    },
                    _ => {
                        ::lib_ws::WsApp::handle_error(app.clone(), ctx, ::lib_ws::Error::Handler("expected text message".into()));
                        return ::std::boxed::Box::pin(async {});
                    }
                };
            },
        );
    }

    let execute = if path.is_some() {
        quote! {
            pub fn execute(app: ::std::sync::Arc<::lib_ws::WsApp>, ctx: &mut ::lib_ws::WsContext, val: serde_json::Value) -> ::futures::future::LocalBoxFuture<'static, ()> {
                #(#pre_stmts)*
                let mut ctx_val = *ctx;
                Box::pin(async move {
                    #(#init_stmts)*
                    Self::call(#(#call_args),*).await;
                    let _ = ctx_val; // keep alive
                })
            }
        }
    } else {
        quote! {
            pub fn execute(app: ::std::sync::Arc<::lib_ws::WsApp>, ctx: &mut ::lib_ws::WsContext, msg: &::actix_web_actors::ws::Message) -> ::futures::future::LocalBoxFuture<'static, ()> {
                #(#pre_stmts)*
                let mut ctx_val = *ctx;
                let msg_val: &'static ::actix_web_actors::ws::Message = unsafe { &*(msg as *const _) };
                Box::pin(async move {
                    #(#init_stmts)*
                    Self::call(#(#call_args),*).await;
                    let _ = (ctx_val, msg_val);
                })
            }
        }
    };

    let trait_impl = if let Some(p) = path {
        quote! {
            impl ::lib_ws::CommandHandler for #struct_name {
                const PATH: &'static str = #p;
                fn execute(app: ::std::sync::Arc<::lib_ws::WsApp>, ctx: &mut ::actix_web_actors::ws::WebsocketContext<::lib_ws::app::WsSession>, val: serde_json::Value) -> ::futures::future::LocalBoxFuture<'static, ()> {
                    let mut wctx = ::lib_ws::WsContext::new(Self::PATH, ctx);
                    #struct_name::execute(app, &mut wctx, val)
                }
            }
        }
    } else {
        quote! {
            impl ::lib_ws::MessageHandler for #struct_name {
                fn execute(app: ::std::sync::Arc<::lib_ws::WsApp>, ctx: &mut ::actix_web_actors::ws::WebsocketContext<::lib_ws::app::WsSession>, msg: &::actix_web_actors::ws::Message) -> ::futures::future::LocalBoxFuture<'static, ()> {
                    let mut wctx = ::lib_ws::WsContext::new("", ctx);
                    #struct_name::execute(app, &mut wctx, msg)
                }
            }
        }
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
        let payload_type =
            payload_ty.as_ref().map(|ty| quote! { Some(::lib_asyncapi::TypeSchema{ name: <#ty as ::lib_asyncapi::SchemaProvider>::NAME, schema: <#ty as ::lib_asyncapi::SchemaProvider>::schema() }) });
        let response_vec = if response_meta.is_empty() {
            quote! { Vec::new() }
        } else {
            let types =
                response_meta.iter().map(|ty| quote! { ::lib_asyncapi::TypeSchema{ name: <#ty as ::lib_asyncapi::SchemaProvider>::NAME, schema: <#ty as ::lib_asyncapi::SchemaProvider>::schema() } });
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
                        path: Self::PATH,
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

    TokenStream::from(quote! {
        #[allow(non_camel_case_types)]
        #vis struct #struct_name;

        impl #struct_name {
            #path_const
            #asyncness fn call(#inputs) #output #block
            #execute
        }

        #trait_impl
        #doc_impl
    })
}

fn ws_loop_impl(path: LitStr, item: TokenStream) -> TokenStream {
    let mut func = parse_macro_input!(item as ItemFn);
    let async_token = match func.sig.asyncness.take() {
        Some(tok) => tok,
        None => {
            return syn::Error::new(func.sig.ident.span(), "ws loop functions must be async").to_compile_error().into();
        }
    };
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
    let asyncness = &async_token;
    let inputs = &func.sig.inputs;
    let block = &func.block;
    let name = &func.sig.ident;

    fn strip_reference(ty: &Type) -> &Type {
        if let Type::Reference(r) = ty { &r.elem } else { ty }
    }

    fn is_ident(ty: &Type, name: &str) -> bool {
        if let Type::Path(p) = strip_reference(ty)
            && let Some(seg) = p.path.segments.last()
        {
            return seg.ident == name;
        }
        false
    }

    fn generic_inner(ty: &Type) -> Option<Type> {
        if let Type::Path(p) = strip_reference(ty)
            && let Some(seg) = p.path.segments.last()
            && let PathArguments::AngleBracketed(args) = &seg.arguments
            && let Some(GenericArgument::Type(t)) = args.args.first()
        {
            return Some(t.clone());
        }
        None
    }

    let mut init_stmts = Vec::new();
    let mut arg_names = Vec::new();

    for arg in inputs {
        if let FnArg::Typed(pat) = arg {
            let ident = match &*pat.pat {
                syn::Pat::Ident(i) => &i.ident,
                _ => continue,
            };
            let ty = &*pat.ty;
            if is_ident(ty, "WebsocketContext") || is_ident(ty, "WsContext") {
                arg_names.push(quote! { &mut ctx_val });
            } else if is_ident(ty, "Message") {
                arg_names.push(quote! { msg_val });
            } else if is_ident(ty, "WsApp") || is_ident(ty, "Arc") {
                arg_names.push(quote! { app.clone() });
            } else if is_ident(ty, "AsyncApiData") {
                if let Some(inner) = generic_inner(ty) {
                    init_stmts.push(quote! { let #ident = ::lib_asyncapi::AsyncApiData(app.get_data::<#inner>().expect("missing data")); });
                    arg_names.push(quote! { #ident });
                } else {
                    return syn::Error::new(ty.span(), "AsyncApiData missing type").to_compile_error().into();
                }
            } else {
                return syn::Error::new(ty.span(), "unsupported argument type").to_compile_error().into();
            }
        }
    }

    let response_vec = if response_meta.is_empty() {
        quote! { Vec::new() }
    } else {
        let types = response_meta.iter().map(|ty| {
            quote! { ::lib_asyncapi::TypeSchema{ name: <#ty as ::lib_asyncapi::SchemaProvider>::NAME, schema: <#ty as ::lib_asyncapi::SchemaProvider>::schema() } }
        });
        quote! { vec![#(#types),*] }
    };

    let register_responses = response_meta.iter().map(|ty| {
        quote! { <#ty as ::lib_asyncapi::SchemaProvider>::register_schemas(map); }
    });

    let doc_impl = {
        let doc_body = if let (Some(summary), Some(description)) = (summary.as_ref(), description.as_ref()) {
            quote! {
                Some(::lib_asyncapi::WsDoc {
                    path: Self::PATH,
                    summary: #summary,
                    description: #description,
                    tags: vec![#(#tags_meta.to_string()),*],
                    payload: None,
                    responses: #response_vec,
                    params: Vec::new(),
                })
            }
        } else {
            quote! { None }
        };
        quote! {
            impl ::lib_asyncapi::DocumentedLoop for #name {
                fn doc() -> ::core::option::Option<::lib_asyncapi::WsDoc> {
                    #doc_body
                }
                fn register_schemas(map: &mut ::std::collections::BTreeMap<String, serde_json::Value>) {
                    #( #register_responses )*
                }
            }
        }
    };

    let execute_impl = quote! {
        pub fn execute(app: ::std::sync::Arc<::lib_ws::WsApp>, ctx: &mut ::lib_ws::WsContext, msg: &::actix_web_actors::ws::Message) -> ::futures::future::LocalBoxFuture<'static, ()> {
            let mut ctx_val = *ctx;
            let msg_val: &'static ::actix_web_actors::ws::Message = unsafe { &*(msg as *const _) };
            Box::pin(async move {
                #( #init_stmts )*
                #name::call(#(#arg_names),*).await;
                let _ = (ctx_val, msg_val);
            })
        }
    };

    TokenStream::from(quote! {
        #[allow(non_camel_case_types)]
        #vis struct #name;

        impl #name {
            #asyncness fn call(#inputs) #block

            pub const PATH: &'static str = #path;

            #execute_impl
        }

        impl ::lib_ws::MessageHandler for #name {
            fn execute(
                app: ::std::sync::Arc<::lib_ws::WsApp>,
                ctx: &mut ::actix_web_actors::ws::WebsocketContext<::lib_ws::app::WsSession>,
                msg: &::actix_web_actors::ws::Message,
            ) -> ::futures::future::LocalBoxFuture<'static, ()> {
                let mut wctx = ::lib_ws::WsContext::new(Self::PATH, ctx);
                #name::execute(app, &mut wctx, msg)
            }
        }

        #doc_impl
    })
}
