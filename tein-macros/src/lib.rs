//! proc macros for tein scheme interpreter
//!
//! provides `#[tein_fn]` and `#[tein_module]` for ergonomic foreign function
//! and module definition. `#[scheme_fn]` is a deprecated alias for `#[tein_fn]`.

use proc_macro::TokenStream;
use quote::quote;
use syn::{FnArg, ItemFn, Pat, ReturnType, Type, parse_macro_input};

/// attribute macro for defining scheme-callable foreign functions.
///
/// generates an `unsafe extern "C"` wrapper named `__tein_{fn_name}` that
/// handles argument extraction, type conversion, and panic safety.
///
/// works standalone (register via `ctx.define_fn_variadic`) or inside a
/// `#[tein_module]` block (the module macro handles registration).
///
/// # supported argument types
///
/// - `i64` — scheme integer
/// - `f64` — scheme float
/// - `String` — scheme string
/// - `bool` — scheme boolean
///
/// <!-- extensibility: add a branch in gen_arg_extraction() for new types -->
///
/// # supported return types
///
/// - `i64`, `f64`, `String`, `bool` — auto-converted to scheme
/// - `Result<T, E>` where T is a supported type — Err becomes scheme exception
/// - `()` — returns scheme void
///
/// <!-- extensibility: add a branch in gen_return_conversion() for new types -->
///
/// # examples
///
/// ```ignore
/// use tein::tein_fn;
///
/// #[tein_fn]
/// fn add(a: i64, b: i64) -> i64 { a + b }
///
/// // register manually:
/// ctx.define_fn_variadic("add", __tein_add)?;
/// ```
#[proc_macro_attribute]
pub fn tein_fn(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    match generate_scheme_fn(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// deprecated alias for `#[tein_fn]`.
///
/// use `#[tein_fn]` instead. this alias will be removed in a future release.
#[deprecated(note = "use #[tein_fn] instead")]
#[proc_macro_attribute]
pub fn scheme_fn(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let input = parse_macro_input!(item as ItemFn);
    match generate_scheme_fn(input) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// extracts the inner type from Result<T, E> if the return type is a Result
fn extract_result_inner(ty: &Type) -> Option<&Type> {
    if let Type::Path(type_path) = ty {
        let seg = type_path.path.segments.last()?;
        if seg.ident == "Result"
            && let syn::PathArguments::AngleBracketed(args) = &seg.arguments
            && let Some(syn::GenericArgument::Type(inner)) = args.args.first()
        {
            return Some(inner);
        }
    }
    None
}

/// returns the type name as a string for simple path types
fn type_name(ty: &Type) -> Option<String> {
    if let Type::Path(type_path) = ty {
        Some(type_path.path.segments.last()?.ident.to_string())
    } else {
        None
    }
}

/// generate the extraction code for a single argument from the scheme args list
fn gen_arg_extraction(arg_name: &syn::Ident, ty: &Type, index: usize) -> proc_macro2::TokenStream {
    let type_str = type_name(ty).unwrap_or_default();
    let err_msg = format!(
        "argument {} ({}): expected {}",
        index + 1,
        arg_name,
        type_str
    );

    match type_str.as_str() {
        "i64" => quote! {
            let #arg_name: i64 = {
                let raw = tein::raw::sexp_car(__tein_current_args);
                if tein::raw::sexp_integerp(raw) == 0 {
                    let msg = #err_msg;
                    let c_msg = ::std::ffi::CString::new(msg).unwrap();
                    return tein::raw::sexp_c_str(ctx, c_msg.as_ptr(), c_msg.as_bytes().len() as tein::raw::sexp_sint_t);
                }
                tein::raw::sexp_unbox_fixnum(raw) as i64
            };
            __tein_current_args = tein::raw::sexp_cdr(__tein_current_args);
        },
        "f64" => quote! {
            let #arg_name: f64 = {
                let raw = tein::raw::sexp_car(__tein_current_args);
                if tein::raw::sexp_flonump(raw) != 0 {
                    tein::raw::sexp_flonum_value(raw)
                } else if tein::raw::sexp_integerp(raw) != 0 {
                    // auto-coerce integers to float
                    tein::raw::sexp_unbox_fixnum(raw) as f64
                } else {
                    let msg = #err_msg;
                    let c_msg = ::std::ffi::CString::new(msg).unwrap();
                    return tein::raw::sexp_c_str(ctx, c_msg.as_ptr(), c_msg.as_bytes().len() as tein::raw::sexp_sint_t);
                }
            };
            __tein_current_args = tein::raw::sexp_cdr(__tein_current_args);
        },
        "String" => quote! {
            let #arg_name: String = {
                let raw = tein::raw::sexp_car(__tein_current_args);
                if tein::raw::sexp_stringp(raw) == 0 {
                    let msg = #err_msg;
                    let c_msg = ::std::ffi::CString::new(msg).unwrap();
                    return tein::raw::sexp_c_str(ctx, c_msg.as_ptr(), c_msg.as_bytes().len() as tein::raw::sexp_sint_t);
                }
                let ptr = tein::raw::sexp_string_data(raw);
                let len = tein::raw::sexp_string_size(raw) as usize;
                let bytes = ::std::slice::from_raw_parts(ptr as *const u8, len);
                match ::std::string::String::from_utf8(bytes.to_vec()) {
                    Ok(s) => s,
                    Err(_) => {
                        let msg = concat!("argument: invalid utf-8 in string");
                        let c_msg = ::std::ffi::CString::new(msg).unwrap();
                        return tein::raw::sexp_c_str(ctx, c_msg.as_ptr(), c_msg.as_bytes().len() as tein::raw::sexp_sint_t);
                    }
                }
            };
            __tein_current_args = tein::raw::sexp_cdr(__tein_current_args);
        },
        "bool" => quote! {
            let #arg_name: bool = {
                let raw = tein::raw::sexp_car(__tein_current_args);
                if tein::raw::sexp_booleanp(raw) == 0 {
                    let msg = #err_msg;
                    let c_msg = ::std::ffi::CString::new(msg).unwrap();
                    return tein::raw::sexp_c_str(ctx, c_msg.as_ptr(), c_msg.as_bytes().len() as tein::raw::sexp_sint_t);
                }
                raw == tein::raw::get_true()
            };
            __tein_current_args = tein::raw::sexp_cdr(__tein_current_args);
        },
        _ => {
            let err = format!("unsupported argument type: {}", type_str);
            quote! { compile_error!(#err); }
        }
    }
}

/// generate the conversion code for a return value to scheme sexp
fn gen_return_conversion(
    ty: &Type,
    result_expr: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    let type_str = type_name(ty).unwrap_or_default();

    match type_str.as_str() {
        "i64" => quote! {
            tein::raw::sexp_make_fixnum(#result_expr as tein::raw::sexp_sint_t)
        },
        "f64" => quote! {
            tein::raw::sexp_make_flonum(ctx, #result_expr)
        },
        "String" => quote! {
            {
                let __tein_s = #result_expr;
                let __tein_c = ::std::ffi::CString::new(__tein_s.as_str()).unwrap_or_default();
                tein::raw::sexp_c_str(ctx, __tein_c.as_ptr(), __tein_s.len() as tein::raw::sexp_sint_t)
            }
        },
        "bool" => quote! {
            tein::raw::sexp_make_boolean(#result_expr)
        },
        _ => quote! { tein::raw::get_void() },
    }
}

fn generate_scheme_fn(input: ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    let fn_name = &input.sig.ident;
    let wrapper_name = syn::Ident::new(&format!("__tein_{}", fn_name), fn_name.span());

    // parse arguments
    let mut arg_names = Vec::new();
    let mut arg_types = Vec::new();

    for arg in &input.sig.inputs {
        match arg {
            FnArg::Typed(pat_type) => {
                let name = match pat_type.pat.as_ref() {
                    Pat::Ident(pat_ident) => pat_ident.ident.clone(),
                    _ => {
                        return Err(syn::Error::new_spanned(
                            arg,
                            "expected simple argument name",
                        ));
                    }
                };
                arg_names.push(name);
                arg_types.push(pat_type.ty.as_ref().clone());
            }
            FnArg::Receiver(_) => {
                return Err(syn::Error::new_spanned(
                    arg,
                    "scheme_fn does not support self parameters",
                ));
            }
        }
    }

    // generate arg extraction
    let extractions: Vec<_> = arg_names
        .iter()
        .zip(arg_types.iter())
        .enumerate()
        .map(|(i, (name, ty))| gen_arg_extraction(name, ty, i))
        .collect();

    // generate the call
    let call_args = &arg_names;
    let call_expr = quote! { #fn_name(#(#call_args),*) };

    // generate return conversion based on return type
    let return_conversion = match &input.sig.output {
        ReturnType::Default => {
            // () return
            quote! {
                #call_expr;
                tein::raw::get_void()
            }
        }
        ReturnType::Type(_, ret_type) => {
            if let Some(inner_type) = extract_result_inner(ret_type) {
                // Result<T, E> return — unwrap or propagate error as exception string
                let success_conv = gen_return_conversion(inner_type, quote! { __tein_ok });
                quote! {
                    match #call_expr {
                        Ok(__tein_ok) => { #success_conv }
                        Err(__tein_err) => {
                            let msg = __tein_err.to_string();
                            let c_msg = ::std::ffi::CString::new(msg.as_str()).unwrap_or_default();
                            tein::raw::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as tein::raw::sexp_sint_t)
                        }
                    }
                }
            } else {
                let conv = gen_return_conversion(ret_type, quote! { __tein_result });
                quote! {
                    let __tein_result = #call_expr;
                    #conv
                }
            }
        }
    };

    let _num_args = arg_names.len();

    Ok(quote! {
        #input

        /// generated ffi wrapper for [`#fn_name`] — called by chibi-scheme
        #[allow(non_snake_case)]
        unsafe extern "C" fn #wrapper_name(
            ctx: tein::raw::sexp,
            _self: tein::raw::sexp,
            _n: tein::raw::sexp_sint_t,
            args: tein::raw::sexp,
        ) -> tein::raw::sexp {
            // catch panics at the ffi boundary (panics across ffi = UB)
            let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                unsafe {
                    let mut __tein_current_args = args;
                    #(#extractions)*
                    #return_conversion
                }
            }));
            match result {
                Ok(sexp) => sexp,
                Err(_) => unsafe {
                    // panic → return error string as scheme value
                    let msg = concat!("rust panic in ", stringify!(#fn_name));
                    let c_msg = ::std::ffi::CString::new(msg).unwrap();
                    tein::raw::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as tein::raw::sexp_sint_t)
                }
            }
        }

        /// registration info for [`#fn_name`]
        #[allow(non_upper_case_globals)]
        const _: () = {
            // marker to associate the wrapper with the original fn
            // use: ctx.define_fn_variadic("name", #wrapper_name)
        };
    })
}
