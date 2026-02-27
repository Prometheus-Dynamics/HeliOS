use proc_macro::TokenStream;
use quote::{ToTokens, quote};
use syn::{Expr, ItemFn, Lit, Meta, Token, parse::Parser, parse_macro_input, punctuated::Punctuated, spanned::Spanned};

#[proc_macro_attribute]
pub fn tokio_test(attr: TokenStream, item: TokenStream) -> TokenStream {
    let parser = Punctuated::<Meta, Token![,]>::parse_terminated;
    let args = match parser.parse2(attr.into()) {
        Ok(value) => value,
        Err(err) => return err.to_compile_error().into(),
    };
    let mut timeout_override: Option<proc_macro2::TokenStream> = None;
    let mut tokio_args: Vec<proc_macro2::TokenStream> = Vec::new();

    for arg in args {
        if arg.path().is_ident("timeout") {
            match arg {
                Meta::NameValue(name_value) => {
                    if timeout_override.is_some() {
                        return syn::Error::new(name_value.path.span(), "duplicate timeout attribute").to_compile_error().into();
                    }

                    let tokens = match name_value.value {
                        Expr::Lit(expr_lit) => match expr_lit.lit {
                            Lit::Str(lit_str) => quote! { ::lib_test::duration_from_str(#lit_str) },
                            _ => quote! { #expr_lit },
                        },
                        other => quote! { #other },
                    };

                    timeout_override = Some(tokens);
                }
                _ => {
                    return syn::Error::new(arg.span(), "timeout expects a name-value pair").to_compile_error().into();
                }
            }
        } else {
            tokio_args.push(arg.to_token_stream());
        }
    }

    let input = parse_macro_input!(item as ItemFn);

    if input.sig.asyncness.is_none() {
        return syn::Error::new(input.sig.ident.span(), "tokio_test can only be applied to async functions").to_compile_error().into();
    }

    let ItemFn { attrs, vis, sig, block } = input;
    let fn_name = &sig.ident;
    let timeout_expr = timeout_override.unwrap_or_else(|| quote! { ::lib_test::default_timeout() });

    let tokio_attr = if tokio_args.is_empty() {
        quote! { #[tokio::test] }
    } else {
        quote! { #[tokio::test( #(#tokio_args),* )] }
    };

    let output = quote! {
        #tokio_attr
        #(#attrs)*
        #vis #sig {
            ::lib_test::__run_test_with_timeout(
                concat!(module_path!(), "::", stringify!(#fn_name)),
                #timeout_expr,
                async move #block
            ).await
        }
    };

    output.into()
}
