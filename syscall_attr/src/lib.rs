use proc_macro2::TokenStream;
use quote::quote;
use std::iter::repeat;
use syn::{
    parse_macro_input, punctuated::Punctuated, spanned::Spanned, token::Paren, AttributeArgs,
    Error, Expr, FnArg, Ident, ItemFn, NestedMeta, Pat, PatIdent, PatType, Result, ReturnType,
    Stmt, Token, Type, TypeTuple,
};

#[proc_macro_attribute]
pub fn syscall(
    attrs: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let args = parse_macro_input!(attrs as AttributeArgs);
    let input = parse_macro_input!(item as ItemFn);
    expand(args, input)
        .unwrap_or_else(Error::into_compile_error)
        .into()
}

fn expand(_args: Vec<NestedMeta>, mut input: ItemFn) -> Result<proc_macro2::TokenStream> {
    let body = &input.block;
    let mut pre_block = Vec::new();
    let mut post_block = Vec::new();
    let mut real_args = None;
    let mut real_ret = None;

    for stmt in &body.stmts {
        if real_args.is_none() {
            match stmt {
                Stmt::Local(local) => {
                    // let x = real!(a1, a2, ...);
                    // binding real!() retrun value to x
                    if let Some((_, expr)) = &local.init {
                        real_args = is_real_macro(expr);
                        if real_args.is_some() {
                            if let Pat::Ident(pat_ident) = &local.pat {
                                real_ret = Some(pat_ident.clone());
                            }
                        }
                    }
                }
                Stmt::Expr(expr) => {
                    // real!(a1, a2, ...)
                    // this is the last expr, return value from real!() returned
                    real_args = is_real_macro(expr);
                }
                Stmt::Semi(expr, _) => {
                    // real!(a1, a2, ...);
                    // ignore the real!() return value
                    real_args = is_real_macro(expr)
                }
                Stmt::Item(_) => {
                    // ignore it
                }
            }

            if real_args.is_some() {
                continue;
            }

            pre_block.push(stmt.clone());
            continue;
        }

        post_block.push(stmt.clone());
    }

    let attrs = &input.attrs;
    let sig = &mut input.sig;
    let vis = &input.vis;
    let ident = &sig.ident;
    let ident_str = &sig.ident.to_string();

    // build pre function, signature is
    // fn pre_{origin_name}({origin_args_values} : {origin_args_types}) -> {origin_args_types}
    let mut sig_pre = sig.clone();
    sig_pre.ident = Ident::new(&format!("pre_{}", sig.ident), sig.span());
    let ident_pre = &sig_pre.ident;
    let mut args = sig_pre
        .inputs
        .iter()
        .filter_map(|a| {
            if let FnArg::Typed(t) = a {
                Some(t.ty.clone())
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let fn_variant = Ident::new(&format!("Func{}", args.len()), sig_pre.span());
    let dummy_args =
        repeat(Box::new(Type::Verbatim(quote!(u64)))).take(6usize.saturating_sub(args.len()));
    args.extend(dummy_args);
    let sig_ret = match &sig.output {
        ReturnType::Default => Box::new(Type::Verbatim(quote!(()))),
        ReturnType::Type(_, bt) => bt.clone(),
    };
    let (real_args, pre_func) = if real_args.is_none() {
        // if not call real() in function, syscall will not be sent to kernel.
        // we should not change origin function at this time.
        // in other words, pre_func == origin_func
        (
            quote!(),
            quote!(interceptor::syscall::Variant::<#sig_ret, #(#args),*>::Block(interceptor::syscall::BlockVariant::#fn_variant(#ident_pre))),
        )
    } else {
        sig_pre.output = ReturnType::Type(
            Token![->](sig_pre.span()),
            Box::new(Type::Tuple(TypeTuple {
                paren_token: Paren {
                    span: sig_pre.span(),
                },
                elems: {
                    sig.inputs
                        .iter()
                        .filter_map(|a| {
                            if let FnArg::Typed(pt) = a {
                                Some(*(pt.ty.clone()))
                            } else {
                                None
                            }
                        })
                        .collect()
                },
            })),
        );
        (
            quote!((#real_args)),
            quote!(interceptor::syscall::Variant::<#sig_ret, #(#args),*>::Passthrough(interceptor::syscall::PassthroughVariant::#fn_variant(#ident_pre))),
        )
    };

    // build post function, signature is
    // fn post_{origin_name}({real_ret_variable} : {origin_ret_type}) -> {origin_ret_type}
    let mut sig_post = sig.clone();
    sig_post.ident = Ident::new(&format!("post_{}", sig.ident), sig.span());
    let mut sig_post_args = Punctuated::new();
    let sig_post_arg = if let Some(rr) = &real_ret {
        rr.clone()
    } else {
        PatIdent {
            attrs: vec![],
            by_ref: None,
            mutability: None,
            ident: Ident::new("real_ret", sig_post_args.span()),
            subpat: None,
        }
    };
    let sig_post_arg_ident = &sig_post_arg.ident;
    let post_block = if post_block.is_empty() {
        quote!(#sig_post_arg_ident)
    } else {
        quote!(#(#post_block)*)
    };

    sig_post_args.push_value(FnArg::Typed(PatType {
        attrs: vec![],
        pat: Box::new(Pat::Ident(sig_post_arg)),
        colon_token: Token![:](sig_post_args.span()),
        ty: sig_ret.clone(),
    }));
    sig_post.inputs = sig_post_args;
    let ident_post = &sig_post.ident;

    Ok(quote!(
        #(#attrs)*
        #vis #sig_pre {
            {#(#pre_block)*}
            #real_args
        }

        #(#attrs)*
        #vis #sig_post {
            #post_block
        }

        #[allow(non_upper_case_globals)]
        #vis static #ident: interceptor::syscall::SysCall<#sig_ret, #(#args),*> = interceptor::syscall::SysCall {
            name: #ident_str,
            pre: #pre_func,
            post: #ident_post,
        };
    ))
}

fn is_real_macro(expr: &Expr) -> Option<TokenStream> {
    if let Expr::Macro(expr_macro) = expr {
        let mac = &expr_macro.mac;
        if let Some(ident) = mac.path.get_ident() {
            if *ident == "real" {
                return Some(mac.tokens.clone());
            }
        }
    }

    None
}
