//! proc macros for tein scheme interpreter
//!
//! provides `#[tein_fn]`, `#[tein_module]`, `#[tein_type]`, `#[tein_methods]`,
//! and `#[tein_const]` for ergonomic foreign function and module definition.
//! legacy `#[scheme_fn]` has been removed — use `#[tein_fn]` instead.

use std::collections::HashMap;

use proc_macro::TokenStream;
use proc_macro2::Span;
use quote::quote;
use syn::parse_macro_input;
use syn::{FnArg, ImplItem, Item, ItemFn, ItemImpl, ItemMod, ItemStruct, Pat, ReturnType, Type};

// ── public proc macros ────────────────────────────────────────────────────────

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

/// attribute macro that generates ForeignType impls, extern "C" wrappers,
/// VFS module content, and a `register_module_*` function from an annotated
/// rust mod block.
///
/// # usage
///
/// ```ignore
/// use tein::{tein_module, tein_fn, tein_type, tein_methods};
///
/// #[tein_module("mymod")]
/// mod mymod {
///     #[tein_fn]
///     fn add(a: i64, b: i64) -> i64 { a + b }
///
///     #[tein_type]
///     struct Counter { n: i64 }
///
///     #[tein_methods]
///     impl Counter {
///         fn get(&self) -> i64 { self.n }
///         fn increment(&mut self) -> i64 { self.n += 1; self.n }
///     }
/// }
///
/// // register with a context before importing:
/// mymod::register_module_mymod(&ctx)?;
/// // then in scheme: (import (tein mymod))
/// ```
///
/// generated scheme names follow these conventions:
/// - free fns: `{module}-{fn}` (e.g. `mymod-add`)
/// - type predicates: `{type}?` (e.g. `counter?`)
/// - type methods: `{type}-{method}` (e.g. `counter-get`)
/// - `_q` suffix → `?`, `_bang` suffix → `!`, `_` → `-`
///
/// override with `#[tein_fn(name = "scheme-name")]` or `#[tein_type(name = "scheme-name")]`.
#[proc_macro_attribute]
pub fn tein_module(attr: TokenStream, item: TokenStream) -> TokenStream {
    let mod_item = parse_macro_input!(item as ItemMod);
    let (module_name, ext) = match parse_module_attr(attr.into()) {
        Ok(v) => v,
        Err(err) => return err.to_compile_error().into(),
    };
    match parse_and_generate_module(module_name, ext, mod_item) {
        Ok(tokens) => tokens.into(),
        Err(err) => err.to_compile_error().into(),
    }
}

/// marks a struct as a foreign type within a `#[tein_module]`.
///
/// optional `name = "scheme-name"` overrides the auto-derived kebab-case name.
/// outside a `#[tein_module]`, this attribute is a passthrough with no effect.
#[proc_macro_attribute]
pub fn tein_type(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item // passthrough — actual processing done by #[tein_module]
}

/// marks an impl block's methods for scheme exposure within a `#[tein_module]`.
///
/// must appear on an impl for a struct annotated with `#[tein_type]` in the
/// same module. outside a `#[tein_module]`, this attribute is a passthrough.
#[proc_macro_attribute]
pub fn tein_methods(_attr: TokenStream, item: TokenStream) -> TokenStream {
    item // passthrough — actual processing done by #[tein_module]
}

// ── naming convention helpers ─────────────────────────────────────────────────

/// convert a rust identifier to a scheme name.
///
/// - `snake_case` → `kebab-case`
/// - trailing `_q` → `?`
/// - trailing `_bang` → `!`
///
/// examples: `is_match` → `is-match`, `object_q` → `object?`, `set_bang` → `set!`
fn rust_to_scheme_name(rust_name: &str) -> String {
    if let Some(stem) = rust_name.strip_suffix("_q") {
        format!("{}?", stem.replace('_', "-"))
    } else if let Some(stem) = rust_name.strip_suffix("_bang") {
        format!("{}!", stem.replace('_', "-"))
    } else {
        rust_name.replace('_', "-")
    }
}

/// convert a PascalCase type name to kebab-case.
///
/// examples: `JsonValue` → `json-value`, `HttpClient` → `http-client`
fn pascal_to_kebab(name: &str) -> String {
    let mut result = String::new();
    for (i, ch) in name.chars().enumerate() {
        if ch.is_uppercase() {
            if i > 0 {
                result.push('-');
            }
            result.push(ch.to_ascii_lowercase());
        } else {
            result.push(ch);
        }
    }
    result
}

/// convert SCREAMING_SNAKE_CASE to kebab-case.
///
/// examples: `UUID_NIL` → `uuid-nil`, `MAX_SIZE` → `max-size`
fn screaming_to_kebab(name: &str) -> String {
    name.to_ascii_lowercase().replace('_', "-")
}

/// extract a scheme literal representation from a const expression.
///
/// supported: string literals → `"..."`, integer → digits, float → digits,
/// bool → `#t`/`#f`. returns `Err` for unsupported expressions.
fn const_to_scheme_literal(expr: &syn::Expr) -> syn::Result<String> {
    match expr {
        syn::Expr::Lit(syn::ExprLit { lit, .. }) => match lit {
            syn::Lit::Str(s) => {
                // escape backslashes and double quotes for scheme string
                let escaped = s.value().replace('\\', "\\\\").replace('"', "\\\"");
                Ok(format!("\"{}\"", escaped))
            }
            syn::Lit::Int(i) => Ok(i.base10_digits().to_string()),
            syn::Lit::Float(f) => Ok(f.base10_digits().to_string()),
            syn::Lit::Bool(b) => Ok(if b.value { "#t" } else { "#f" }.to_string()),
            _ => Err(syn::Error::new_spanned(
                lit,
                "#[tein_const] supports string, integer, float, and bool literals",
            )),
        },
        // handle negative literals: `-42`, `-3.14`
        syn::Expr::Unary(syn::ExprUnary {
            op: syn::UnOp::Neg(_),
            expr: inner,
            ..
        }) => {
            let inner_lit = const_to_scheme_literal(inner)?;
            Ok(format!("-{}", inner_lit))
        }
        _ => Err(syn::Error::new_spanned(
            expr,
            "#[tein_const] requires a literal expression (string, integer, float, or bool)",
        )),
    }
}

/// extract `///` doc comments from an attribute list.
///
/// `///` comments are parsed by rustc into `#[doc = "..."]` attributes.
/// the leading space rustc adds is trimmed. returns empty vec if no docs.
fn extract_doc_comments(attrs: &[syn::Attribute]) -> Vec<String> {
    attrs
        .iter()
        .filter_map(|attr| {
            if !attr.path().is_ident("doc") {
                return None;
            }
            if let syn::Meta::NameValue(nv) = &attr.meta
                && let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(s),
                    ..
                }) = &nv.value
            {
                return Some(
                    s.value()
                        .strip_prefix(' ')
                        .unwrap_or(&s.value())
                        .to_string(),
                );
            }
            None
        })
        .collect()
}

// ── module data model ─────────────────────────────────────────────────────────

/// parsed representation of a `#[tein_module("name")]` block.
struct ModuleInfo {
    /// module name as it appears in scheme, e.g. `"json"`
    name: String,
    /// whether this is a cdylib extension module (`ext = true`)
    ext: bool,
    /// the original mod item (preserved in output, with tein attrs stripped)
    mod_item: ItemMod,
    /// free functions annotated with `#[tein_fn]`
    free_fns: Vec<FreeFnInfo>,
    /// constants annotated with `#[tein_const]`
    consts: Vec<ConstInfo>,
    /// types annotated with `#[tein_type]`
    types: Vec<TypeInfo>,
}

/// a `#[tein_fn]` free function inside a module
struct FreeFnInfo {
    /// the original function (tein attrs stripped)
    func: ItemFn,
    /// scheme name (derived from module+fn name, or overridden via `name = "..."`)
    scheme_name: String,
    /// `///` doc comments from the original item — used by docs alist codegen
    doc: Vec<String>,
}

/// a `#[tein_type]` struct together with its `#[tein_methods]` impl block
struct TypeInfo {
    /// the original struct (tein attrs stripped)
    struct_item: ItemStruct,
    /// scheme type name (derived from struct name, or overridden via `name = "..."`)
    scheme_type_name: String,
    /// methods from the `#[tein_methods]` impl block
    methods: Vec<MethodInfo>,
    /// `///` doc comments from the original item — used by docs alist codegen
    doc: Vec<String>,
}

/// a single method from a `#[tein_methods]` impl block
struct MethodInfo {
    /// the original method
    method: syn::ImplItemFn,
    /// scheme method name (e.g. `"is-match"` for `fn is_match`)
    scheme_name: String,
    /// whether the method takes `&mut self`
    is_mut: bool,
    /// `///` doc comments from the original item — used by docs alist codegen
    doc: Vec<String>,
}

/// a `#[tein_const]` constant inside a module
struct ConstInfo {
    /// scheme name (derived from const name, or overridden via `name = "..."`)
    scheme_name: String,
    /// scheme literal representation (e.g. `"\"hello\""`, `42`, `#t`)
    scheme_literal: String,
    /// `///` doc comments from the original item
    doc: Vec<String>,
}

// ── module parsing ────────────────────────────────────────────────────────────

/// parse the `#[tein_module(...)]` attribute arguments.
///
/// accepts either `"name"` or `"name", ext = true`.
fn parse_module_attr(tokens: proc_macro2::TokenStream) -> syn::Result<(String, bool)> {
    let parser = syn::punctuated::Punctuated::<syn::Expr, syn::Token![,]>::parse_terminated;
    let args = syn::parse::Parser::parse2(parser, tokens)?;
    let mut iter = args.iter();

    // first arg: string literal (module name)
    let name_expr = iter
        .next()
        .ok_or_else(|| syn::Error::new(Span::call_site(), "expected module name string"))?;
    let name = match name_expr {
        syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(s),
            ..
        }) => s.value(),
        _ => {
            return Err(syn::Error::new_spanned(
                name_expr,
                "expected string literal for module name",
            ));
        }
    };

    // optional second arg: ext = true
    let mut ext = false;
    if let Some(ext_expr) = iter.next() {
        match ext_expr {
            syn::Expr::Assign(assign) => {
                let key = assign.left.as_ref();
                if let syn::Expr::Path(p) = key
                    && p.path.is_ident("ext")
                {
                    if let syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Bool(b),
                        ..
                    }) = assign.right.as_ref()
                    {
                        ext = b.value();
                    } else {
                        return Err(syn::Error::new_spanned(
                            &assign.right,
                            "expected `true` or `false`",
                        ));
                    }
                } else {
                    return Err(syn::Error::new_spanned(key, "expected `ext`"));
                }
            }
            _ => return Err(syn::Error::new_spanned(ext_expr, "expected `ext = true`")),
        }
    }

    if iter.next().is_some() {
        return Err(syn::Error::new(
            Span::call_site(),
            "unexpected extra arguments",
        ));
    }

    Ok((name, ext))
}

fn parse_and_generate_module(
    module_name: String,
    ext: bool,
    mod_item: ItemMod,
) -> syn::Result<proc_macro2::TokenStream> {
    let info = parse_module_info(module_name, ext, mod_item)?;
    generate_module(info)
}

fn parse_module_info(module_name: String, ext: bool, mod_item: ItemMod) -> syn::Result<ModuleInfo> {
    let (brace, items) = mod_item.content.as_ref().ok_or_else(|| {
        syn::Error::new_spanned(&mod_item, "#[tein_module] requires an inline mod body")
    })?;

    let mut free_fns = Vec::new();
    let mut consts: Vec<ConstInfo> = Vec::new(); // populated below
    let mut types: Vec<TypeInfo> = Vec::new();
    let mut type_names: HashMap<String, usize> = HashMap::new();
    let mut clean_items: Vec<Item> = Vec::new();

    for item in items {
        match item {
            Item::Fn(func) if has_tein_attr(&func.attrs, "tein_fn") => {
                let override_name = extract_name_override(&func.attrs, "tein_fn")?;
                let rust_name = func.sig.ident.to_string();
                let scheme_name = override_name.unwrap_or_else(|| {
                    format!("{}-{}", module_name, rust_to_scheme_name(&rust_name))
                });
                let mut clean_func = func.clone();
                clean_func
                    .attrs
                    .retain(|a| !is_tein_attr_named(a, "tein_fn"));
                free_fns.push(FreeFnInfo {
                    func: clean_func,
                    scheme_name,
                    doc: extract_doc_comments(&func.attrs),
                });
                // don't push to clean_items — generate_scheme_fn emits the fn + wrapper
            }
            Item::Const(c) if has_tein_attr(&c.attrs, "tein_const") => {
                let override_name = extract_name_override(&c.attrs, "tein_const")?;
                let rust_name = c.ident.to_string();
                // constants get no module prefix — GREETING in module "foo" → "greeting",
                // not "foo-greeting". this differs from free fns. same in internal and ext mode.
                let scheme_name = override_name.unwrap_or_else(|| screaming_to_kebab(&rust_name));
                let scheme_literal = const_to_scheme_literal(&c.expr)?;
                consts.push(ConstInfo {
                    scheme_name,
                    scheme_literal,
                    doc: extract_doc_comments(&c.attrs),
                });
                let mut clean_const = c.clone();
                clean_const
                    .attrs
                    .retain(|a| !is_tein_attr_named(a, "tein_const"));
                clean_items.push(Item::Const(clean_const));
            }
            Item::Struct(s) if has_tein_attr(&s.attrs, "tein_type") => {
                let override_name = extract_name_override(&s.attrs, "tein_type")?;
                let rust_name = s.ident.to_string();
                let scheme_type_name = override_name.unwrap_or_else(|| pascal_to_kebab(&rust_name));
                let idx = types.len();
                type_names.insert(rust_name, idx);
                let mut clean_struct = s.clone();
                clean_struct
                    .attrs
                    .retain(|a| !is_tein_attr_named(a, "tein_type"));
                types.push(TypeInfo {
                    struct_item: clean_struct.clone(),
                    scheme_type_name,
                    methods: Vec::new(),
                    doc: extract_doc_comments(&s.attrs),
                });
                clean_items.push(Item::Struct(clean_struct));
            }
            Item::Impl(imp) if has_tein_attr(&imp.attrs, "tein_methods") => {
                let type_name = impl_self_type_name(imp)?;
                let type_idx = type_names.get(&type_name).copied().ok_or_else(|| {
                    syn::Error::new_spanned(
                        imp,
                        format!(
                            "#[tein_methods] on impl for '{}' but no #[tein_type] struct found \
                             in this module — add #[tein_type] to the struct definition",
                            type_name
                        ),
                    )
                })?;
                let scheme_type_name = types[type_idx].scheme_type_name.clone();

                for impl_item in &imp.items {
                    if let ImplItem::Fn(method) = impl_item {
                        let rust_name = method.sig.ident.to_string();
                        let scheme_name =
                            format!("{}-{}", scheme_type_name, rust_to_scheme_name(&rust_name));
                        let is_mut = method.sig.inputs.first().is_some_and(
                            |arg| matches!(arg, FnArg::Receiver(r) if r.mutability.is_some()),
                        );
                        types[type_idx].methods.push(MethodInfo {
                            method: method.clone(),
                            scheme_name,
                            is_mut,
                            doc: extract_doc_comments(&method.attrs),
                        });
                    }
                }
                let mut clean_impl = imp.clone();
                clean_impl
                    .attrs
                    .retain(|a| !is_tein_attr_named(a, "tein_methods"));
                clean_items.push(Item::Impl(clean_impl));
            }
            other => {
                clean_items.push(other.clone());
            }
        }
    }

    let mut clean_mod = mod_item.clone();
    clean_mod.content = Some((*brace, clean_items));
    clean_mod
        .attrs
        .retain(|a| !is_tein_attr_named(a, "tein_module"));

    Ok(ModuleInfo {
        name: module_name,
        ext,
        mod_item: clean_mod,
        free_fns,
        consts,
        types,
    })
}

// ── module codegen ────────────────────────────────────────────────────────────

fn generate_module(info: ModuleInfo) -> syn::Result<proc_macro2::TokenStream> {
    if info.ext {
        generate_module_ext(info)
    } else {
        generate_module_internal(info)
    }
}

fn generate_module_internal(info: ModuleInfo) -> syn::Result<proc_macro2::TokenStream> {
    let mod_name = &info.mod_item.ident;
    let mod_vis = &info.mod_item.vis;
    let mod_attrs = &info.mod_item.attrs;
    let (_, items) = info.mod_item.content.as_ref().unwrap();

    let foreign_impls: Vec<proc_macro2::TokenStream> = info
        .types
        .iter()
        .map(generate_foreign_type_impl)
        .collect::<syn::Result<_>>()?;

    let fn_wrappers: Vec<proc_macro2::TokenStream> = info
        .free_fns
        .iter()
        .map(|f| generate_scheme_fn(f.func.clone()))
        .collect::<syn::Result<_>>()?;

    let register_fn = generate_register_fn(&info);

    Ok(quote! {
        #(#mod_attrs)*
        #mod_vis mod #mod_name {
            // bring Value into scope so fn bodies inside #[tein_module] can use
            // `Value::String(_)` etc. without full path qualification.
            #[allow(unused_imports)]
            use tein::Value;
            #(#items)*
            #(#foreign_impls)*
            #(#fn_wrappers)*
            #register_fn
        }
    })
}

// ── ext-mode codegen ──────────────────────────────────────────────────────────

/// generate the ext-mode module — emits `tein_ext_init` instead of `register_module_*`.
fn generate_module_ext(info: ModuleInfo) -> syn::Result<proc_macro2::TokenStream> {
    let mod_name = &info.mod_item.ident;
    let mod_vis = &info.mod_item.vis;
    let mod_attrs = &info.mod_item.attrs;
    let (_, items) = info.mod_item.content.as_ref().unwrap();

    let fn_wrappers: Vec<proc_macro2::TokenStream> = info
        .free_fns
        .iter()
        .map(|f| generate_scheme_fn_ext(f.func.clone()))
        .collect::<syn::Result<_>>()?;

    let type_descs: Vec<proc_macro2::TokenStream> = info
        .types
        .iter()
        .map(generate_ext_type_desc)
        .collect::<syn::Result<_>>()?;

    let init_fn = generate_ext_init_fn(&info);

    // thread-local for the API pointer — set at init time and available to wrappers
    let api_tls = quote! {
        std::thread_local! {
            static __TEIN_API: std::cell::Cell<*const tein_ext::TeinExtApi> =
                const { std::cell::Cell::new(std::ptr::null()) };
        }
    };

    Ok(quote! {
        #(#mod_attrs)*
        // dead_code: const items appear unused to rustc (they're consumed by VFS codegen at
        //   macro expansion time, not at runtime). non_snake_case: method wrapper names like
        //   __tein_ext_method_Counter_get embed the PascalCase struct name by design.
        #[allow(dead_code, non_snake_case)]
        #mod_vis mod #mod_name {
            #(#items)*
            #api_tls
            #(#type_descs)*
            #(#fn_wrappers)*
            #init_fn
        }
    })
}

/// generate the `tein_ext_init` entry point for a cdylib extension.
///
/// registers VFS entries, foreign types, and free functions through the API vtable.
fn generate_ext_init_fn(info: &ModuleInfo) -> proc_macro2::TokenStream {
    let sld_content = generate_vfs_sld(info);
    let scm_content = generate_vfs_scm(info);
    let sld_path = format!("lib/tein/{}.sld", info.name);
    let scm_path = format!("lib/tein/{}.scm", info.name);
    let docs_sld_content = generate_vfs_docs_sld(info);
    let docs_scm_content = generate_vfs_docs_scm(info);
    let docs_sld_path = format!("lib/tein/{}/docs.sld", info.name);
    let docs_scm_path = format!("lib/tein/{}/docs.scm", info.name);

    let vfs_entries: Vec<proc_macro2::TokenStream> = [
        (&sld_path, &sld_content),
        (&scm_path, &scm_content),
        (&docs_sld_path, &docs_sld_content),
        (&docs_scm_path, &docs_scm_content),
    ]
    .iter()
    .map(|(path, content)| {
        quote! {
            {
                static PATH: &[u8] = #path.as_bytes();
                static CONTENT: &[u8] = #content.as_bytes();
                let rc = ((*api).register_vfs_module)(
                    ctx,
                    PATH.as_ptr() as *const ::std::ffi::c_char, PATH.len(),
                    CONTENT.as_ptr() as *const ::std::ffi::c_char, CONTENT.len(),
                );
                if rc != 0 { return tein_ext::TEIN_EXT_ERR_INIT; }
            }
        }
    })
    .collect();

    let type_registrations: Vec<proc_macro2::TokenStream> = info
        .types
        .iter()
        .map(|t| {
            let desc_ident = syn::Ident::new(
                &format!(
                    "__TEIN_TYPE_DESC_{}",
                    t.struct_item.ident.to_string().to_uppercase()
                ),
                t.struct_item.ident.span(),
            );
            quote! {
                {
                    let rc = ((*api).register_foreign_type)(ctx, &#desc_ident);
                    if rc != 0 { return tein_ext::TEIN_EXT_ERR_INIT; }
                }
            }
        })
        .collect();

    let fn_registrations: Vec<proc_macro2::TokenStream> = info
        .free_fns
        .iter()
        .map(|f| {
            let wrapper_ident = syn::Ident::new(
                &format!("__tein_{}", f.func.sig.ident),
                f.func.sig.ident.span(),
            );
            let scheme_name = &f.scheme_name;
            quote! {
                {
                    static NAME: &[u8] = #scheme_name.as_bytes();
                    let rc = ((*api).define_fn_variadic)(
                        ctx,
                        NAME.as_ptr() as *const ::std::ffi::c_char, NAME.len(),
                        #wrapper_ident,
                    );
                    if rc != 0 { return tein_ext::TEIN_EXT_ERR_INIT; }
                }
            }
        })
        .collect();

    quote! {
        /// Extension entry point — called by the tein host at load time.
        ///
        /// # Safety
        ///
        /// Must be called with a valid context pointer and API table.
        /// Only called by `Context::load_extension`.
        #[unsafe(no_mangle)]
        pub unsafe extern "C" fn tein_ext_init(
            ctx: *mut tein_ext::OpaqueCtx,
            api: *const tein_ext::TeinExtApi,
        ) -> i32 {
            unsafe {
                // version check
                if (*api).version < tein_ext::TEIN_EXT_API_VERSION {
                    return tein_ext::TEIN_EXT_ERR_VERSION;
                }

                // store API pointer for wrappers to use
                __TEIN_API.with(|cell| cell.set(api));

                // register VFS entries
                #(#vfs_entries)*

                // register foreign types
                #(#type_registrations)*

                // register free functions
                #(#fn_registrations)*

                tein_ext::TEIN_EXT_OK
            }
        }
    }
}

/// generate a static `TeinTypeDesc` (+ method array + method wrappers) for an ext-mode type.
fn generate_ext_type_desc(type_info: &TypeInfo) -> syn::Result<proc_macro2::TokenStream> {
    let struct_name = &type_info.struct_item.ident;
    let scheme_type_name = &type_info.scheme_type_name;
    let desc_ident = syn::Ident::new(
        &format!(
            "__TEIN_TYPE_DESC_{}",
            struct_name.to_string().to_uppercase()
        ),
        struct_name.span(),
    );
    let methods_ident = syn::Ident::new(
        &format!("__TEIN_METHODS_{}", struct_name.to_string().to_uppercase()),
        struct_name.span(),
    );

    let method_wrappers: Vec<proc_macro2::TokenStream> = type_info
        .methods
        .iter()
        .map(|m| generate_ext_method_wrapper(m, struct_name))
        .collect::<syn::Result<_>>()?;

    let method_descs: Vec<proc_macro2::TokenStream> = type_info
        .methods
        .iter()
        .map(|m| {
            let method_ident = &m.method.sig.ident;
            let wrapper_ident = syn::Ident::new(
                &format!("__tein_ext_method_{}_{}", struct_name, method_ident),
                method_ident.span(),
            );
            let scheme_name = &m.scheme_name;
            let is_mut = m.is_mut;
            quote! {
                tein_ext::TeinMethodDesc {
                    name: #scheme_name.as_ptr() as *const ::std::ffi::c_char,
                    name_len: #scheme_name.len(),
                    func: #wrapper_ident,
                    is_mut: #is_mut,
                }
            }
        })
        .collect();

    let method_count = method_descs.len();

    Ok(quote! {
        #(#method_wrappers)*

        static #methods_ident: [tein_ext::TeinMethodDesc; #method_count] = [
            #(#method_descs),*
        ];

        static #desc_ident: tein_ext::TeinTypeDesc = tein_ext::TeinTypeDesc {
            type_name: #scheme_type_name.as_ptr() as *const ::std::ffi::c_char,
            type_name_len: #scheme_type_name.len(),
            methods: #methods_ident.as_ptr(),
            method_count: #method_count,
        };
    })
}

/// generate an `extern "C"` method wrapper matching `TeinMethodFn`.
fn generate_ext_method_wrapper(
    m: &MethodInfo,
    struct_name: &syn::Ident,
) -> syn::Result<proc_macro2::TokenStream> {
    let method_ident = &m.method.sig.ident;
    let wrapper_ident = syn::Ident::new(
        &format!("__tein_ext_method_{}_{}", struct_name, method_ident),
        method_ident.span(),
    );

    // collect non-self args
    let args: Vec<(&syn::Ident, &Type)> = m
        .method
        .sig
        .inputs
        .iter()
        .skip(1) // skip self
        .map(|arg| {
            if let FnArg::Typed(pt) = arg
                && let Pat::Ident(pi) = pt.pat.as_ref()
            {
                return Ok((&pi.ident, pt.ty.as_ref()));
            }
            Err(syn::Error::new_spanned(
                arg,
                "expected named argument in #[tein_methods] method",
            ))
        })
        .collect::<syn::Result<_>>()?;

    let extractions: Vec<proc_macro2::TokenStream> = args
        .iter()
        .enumerate()
        .map(|(i, (name, ty))| gen_arg_extraction_ext(name, ty, i, "api"))
        .collect::<syn::Result<_>>()?;

    let arg_names: Vec<&syn::Ident> = args.iter().map(|(n, _)| *n).collect();

    let cast = if m.is_mut {
        quote! { let __tein_this = &mut *(obj as *mut #struct_name); }
    } else {
        quote! { let __tein_this = &*(obj as *const #struct_name); }
    };

    let call_expr = quote! { #struct_name::#method_ident(__tein_this, #(#arg_names),*) };

    let return_conv = gen_return_conversion_ext(&m.method.sig.output, call_expr, "api")?;

    Ok(quote! {
        #[allow(non_snake_case)]
        unsafe extern "C" fn #wrapper_ident(
            obj: *mut ::std::ffi::c_void,
            ctx: *mut tein_ext::OpaqueCtx,
            api: *const tein_ext::TeinExtApi,
            _n: ::std::ffi::c_long,
            args: *mut tein_ext::OpaqueVal,
        ) -> *mut tein_ext::OpaqueVal {
            let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                unsafe {
                    #cast
                    let mut __tein_current_args = args;
                    #(#extractions)*
                    #return_conv
                }
            }));
            match result {
                Ok(val) => val,
                Err(_) => unsafe {
                    let msg = concat!("rust panic in method ", stringify!(#method_ident));
                    let c_msg = ::std::ffi::CString::new(msg).unwrap();
                    ((*api).make_error)(
                        ctx, c_msg.as_ptr(), msg.len() as ::std::ffi::c_long,
                    )
                }
            }
        }
    })
}

/// generate the extern "C" wrapper for a free function in ext mode.
///
/// same `SexpFn` ABI as internal mode, but uses the `__TEIN_API` thread-local
/// for all value operations instead of `tein::raw::*`.
fn generate_scheme_fn_ext(input: ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    let fn_name = &input.sig.ident;
    let wrapper_name = syn::Ident::new(&format!("__tein_{}", fn_name), fn_name.span());

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
                    "tein_fn does not support self parameters",
                ));
            }
        }
    }

    let extractions: Vec<proc_macro2::TokenStream> = arg_names
        .iter()
        .zip(arg_types.iter())
        .enumerate()
        .map(|(i, (name, ty))| gen_arg_extraction_ext(name, ty, i, "__tein_api"))
        .collect::<syn::Result<_>>()?;

    let call_args = &arg_names;
    let call_expr = quote! { #fn_name(#(#call_args),*) };

    let return_conversion = gen_return_conversion_ext_fn(&input.sig.output, call_expr)?;

    Ok(quote! {
        #input

        /// generated ffi wrapper for [`#fn_name`] — called via TeinExtApi vtable
        #[allow(non_snake_case)]
        unsafe extern "C" fn #wrapper_name(
            ctx: *mut tein_ext::OpaqueVal,
            _self: *mut tein_ext::OpaqueVal,
            _n: ::std::ffi::c_long,
            args: *mut tein_ext::OpaqueVal,
        ) -> *mut tein_ext::OpaqueVal {
            let result = ::std::panic::catch_unwind(::std::panic::AssertUnwindSafe(|| {
                unsafe {
                    let __tein_api = __TEIN_API.with(|cell| cell.get());
                    let mut __tein_current_args = args;
                    #(#extractions)*
                    #return_conversion
                }
            }));
            match result {
                Ok(val) => val,
                Err(_) => unsafe {
                    let __tein_api = __TEIN_API.with(|cell| cell.get());
                    let msg = concat!("rust panic in ", stringify!(#fn_name));
                    let c_msg = ::std::ffi::CString::new(msg).unwrap();
                    ((*__tein_api).make_error)(
                        ctx as *mut tein_ext::OpaqueCtx,
                        c_msg.as_ptr(),
                        msg.len() as ::std::ffi::c_long,
                    )
                }
            }
        }
    })
}

/// generate arg extraction code using the api vtable pointer named `api_ptr_name`.
///
/// <!-- extensibility: add a match arm here for new types. -->
fn gen_arg_extraction_ext(
    arg_name: &syn::Ident,
    ty: &Type,
    index: usize,
    api_ptr_name: &str,
) -> syn::Result<proc_macro2::TokenStream> {
    let type_str = type_name_str(ty).unwrap_or_default();
    let err_msg = format!(
        "argument {} ({}): expected {}",
        index + 1,
        arg_name,
        type_str
    );
    let api: syn::Expr = syn::parse_str(api_ptr_name).unwrap();

    let extraction = match type_str.as_str() {
        "i64" => quote! {
            let #arg_name: i64 = {
                let raw = ((*#api).sexp_car)(__tein_current_args);
                if ((*#api).sexp_integerp)(raw) == 0 {
                    let msg = #err_msg;
                    let c_msg = ::std::ffi::CString::new(msg).unwrap();
                    return ((*#api).sexp_c_str)(
                        ctx as *mut tein_ext::OpaqueCtx,
                        c_msg.as_ptr(), c_msg.as_bytes().len() as ::std::ffi::c_long,
                    );
                }
                ((*#api).sexp_unbox_fixnum)(raw) as i64
            };
            __tein_current_args = ((*#api).sexp_cdr)(__tein_current_args);
        },
        "f64" => quote! {
            let #arg_name: f64 = {
                let raw = ((*#api).sexp_car)(__tein_current_args);
                if ((*#api).sexp_flonump)(raw) != 0 {
                    ((*#api).sexp_flonum_value)(raw)
                } else if ((*#api).sexp_integerp)(raw) != 0 {
                    ((*#api).sexp_unbox_fixnum)(raw) as f64
                } else {
                    let msg = #err_msg;
                    let c_msg = ::std::ffi::CString::new(msg).unwrap();
                    return ((*#api).sexp_c_str)(
                        ctx as *mut tein_ext::OpaqueCtx,
                        c_msg.as_ptr(), c_msg.as_bytes().len() as ::std::ffi::c_long,
                    );
                }
            };
            __tein_current_args = ((*#api).sexp_cdr)(__tein_current_args);
        },
        "String" => quote! {
            let #arg_name: String = {
                let raw = ((*#api).sexp_car)(__tein_current_args);
                if ((*#api).sexp_stringp)(raw) == 0 {
                    let msg = #err_msg;
                    let c_msg = ::std::ffi::CString::new(msg).unwrap();
                    return ((*#api).sexp_c_str)(
                        ctx as *mut tein_ext::OpaqueCtx,
                        c_msg.as_ptr(), c_msg.as_bytes().len() as ::std::ffi::c_long,
                    );
                }
                let ptr = ((*#api).sexp_string_data)(raw);
                let len = ((*#api).sexp_string_size)(raw) as usize;
                let bytes = ::std::slice::from_raw_parts(ptr as *const u8, len);
                match ::std::string::String::from_utf8(bytes.to_vec()) {
                    Ok(s) => s,
                    Err(_) => {
                        let msg = "argument: invalid utf-8 in string";
                        let c_msg = ::std::ffi::CString::new(msg).unwrap();
                        return ((*#api).sexp_c_str)(
                            ctx as *mut tein_ext::OpaqueCtx,
                            c_msg.as_ptr(), msg.len() as ::std::ffi::c_long,
                        );
                    }
                }
            };
            __tein_current_args = ((*#api).sexp_cdr)(__tein_current_args);
        },
        "bool" => quote! {
            let #arg_name: bool = {
                let raw = ((*#api).sexp_car)(__tein_current_args);
                if ((*#api).sexp_booleanp)(raw) == 0 {
                    let msg = #err_msg;
                    let c_msg = ::std::ffi::CString::new(msg).unwrap();
                    return ((*#api).sexp_c_str)(
                        ctx as *mut tein_ext::OpaqueCtx,
                        c_msg.as_ptr(), c_msg.as_bytes().len() as ::std::ffi::c_long,
                    );
                }
                raw == ((*#api).get_true)()
            };
            __tein_current_args = ((*#api).sexp_cdr)(__tein_current_args);
        },
        _ => {
            return Err(syn::Error::new_spanned(
                ty,
                format!(
                    "unsupported argument type: '{}'. supported: i64, f64, String, bool",
                    type_str
                ),
            ));
        }
    };
    Ok(extraction)
}

/// generate return conversion for ext-mode free functions (SexpFn ABI, ctx is *mut OpaqueVal).
///
/// `ctx` in a free fn wrapper is `*mut OpaqueVal` so we cast to `*mut OpaqueCtx` for api calls.
///
/// <!-- extensibility: add a match arm here for new return types. -->
fn gen_return_conversion_ext_fn(
    output: &ReturnType,
    call_expr: proc_macro2::TokenStream,
) -> syn::Result<proc_macro2::TokenStream> {
    match output {
        ReturnType::Default => Ok(quote! {
            #call_expr;
            ((*__tein_api).get_void)()
        }),
        ReturnType::Type(_, ret_type) => {
            if let Some(inner) = extract_result_inner(ret_type) {
                let ok_conv = gen_return_conversion_ext_value_fn(inner, quote! { __tein_ok })?;
                Ok(quote! {
                    match #call_expr {
                        Ok(__tein_ok) => #ok_conv,
                        // Result::Err raises a scheme exception (error object).
                        Err(__tein_err) => {
                            let msg = __tein_err.to_string();
                            let c_msg = ::std::ffi::CString::new(msg.as_str()).unwrap_or_default();
                            ((*__tein_api).make_error)(
                                ctx as *mut tein_ext::OpaqueCtx,
                                c_msg.as_ptr(), msg.len() as ::std::ffi::c_long,
                            )
                        }
                    }
                })
            } else {
                let conv = gen_return_conversion_ext_value_fn(ret_type, quote! { __tein_result })?;
                Ok(quote! {
                    let __tein_result = #call_expr;
                    #conv
                })
            }
        }
    }
}

/// convert a rust value expression to `*mut OpaqueVal` for free fn wrappers.
///
/// in free fn wrappers, `ctx` is `*mut OpaqueVal` so needs casting to `*mut OpaqueCtx`.
///
/// <!-- extensibility: add a match arm for new return types. -->
fn gen_return_conversion_ext_value_fn(
    ty: &Type,
    expr: proc_macro2::TokenStream,
) -> syn::Result<proc_macro2::TokenStream> {
    let type_str = type_name_str(ty).unwrap_or_default();
    let conv = match type_str.as_str() {
        "i64" => quote! { ((*__tein_api).sexp_make_fixnum)(#expr as ::std::ffi::c_long) },
        "f64" => {
            quote! { ((*__tein_api).sexp_make_flonum)(ctx as *mut tein_ext::OpaqueCtx, #expr) }
        }
        "String" => quote! {
            {
                let __tein_s = #expr;
                let __tein_c = ::std::ffi::CString::new(__tein_s.as_str()).unwrap_or_default();
                ((*__tein_api).sexp_c_str)(
                    ctx as *mut tein_ext::OpaqueCtx,
                    __tein_c.as_ptr(), __tein_s.len() as ::std::ffi::c_long,
                )
            }
        },
        "bool" => quote! {
            ((*__tein_api).sexp_make_boolean)(if #expr { 1 } else { 0 })
        },
        "Value" => {
            // ext fns returning Value are not supported: ext crates don't link against chibi,
            // so `to_raw` is unavailable. ext fns should return concrete types instead.
            return Err(syn::Error::new_spanned(
                ty,
                "Value return type not yet supported in ext mode #[tein_fn]; return a concrete type (i64, f64, String, bool) instead",
            ));
        }
        _ => {
            return Err(syn::Error::new_spanned(
                ty,
                format!(
                    "unsupported ext fn return type: '{}'. supported: i64, f64, String, bool, ()",
                    type_str
                ),
            ));
        }
    };
    Ok(conv)
}

/// generate return conversion for ext-mode method wrappers (TeinMethodFn ABI, ctx is *mut OpaqueCtx).
fn gen_return_conversion_ext(
    output: &ReturnType,
    call_expr: proc_macro2::TokenStream,
    api_ptr_name: &str,
) -> syn::Result<proc_macro2::TokenStream> {
    let api: syn::Expr = syn::parse_str(api_ptr_name).unwrap();
    match output {
        ReturnType::Default => Ok(quote! {
            #call_expr;
            ((*#api).get_void)()
        }),
        ReturnType::Type(_, ret_type) => {
            if let Some(inner) = extract_result_inner(ret_type) {
                let ok_conv =
                    gen_return_conversion_ext_value(inner, quote! { __tein_ok }, api_ptr_name)?;
                Ok(quote! {
                    match #call_expr {
                        Ok(__tein_ok) => #ok_conv,
                        Err(__tein_err) => {
                            let msg = __tein_err.to_string();
                            let c_msg = ::std::ffi::CString::new(msg.as_str()).unwrap_or_default();
                            ((*#api).make_error)(
                                ctx, c_msg.as_ptr(), msg.len() as ::std::ffi::c_long,
                            )
                        }
                    }
                })
            } else {
                let conv = gen_return_conversion_ext_value(
                    ret_type,
                    quote! { __tein_result },
                    api_ptr_name,
                )?;
                Ok(quote! {
                    let __tein_result = #call_expr;
                    #conv
                })
            }
        }
    }
}

/// convert a rust value expression to `*mut OpaqueVal` using the api vtable.
///
/// <!-- extensibility: add a match arm for new return types. -->
fn gen_return_conversion_ext_value(
    ty: &Type,
    expr: proc_macro2::TokenStream,
    api_ptr_name: &str,
) -> syn::Result<proc_macro2::TokenStream> {
    let type_str = type_name_str(ty).unwrap_or_default();
    let api: syn::Expr = syn::parse_str(api_ptr_name).unwrap();
    let conv = match type_str.as_str() {
        "i64" => quote! { ((*#api).sexp_make_fixnum)(#expr as ::std::ffi::c_long) },
        "f64" => quote! { ((*#api).sexp_make_flonum)(ctx, #expr) },
        "String" => quote! {
            {
                let __tein_s = #expr;
                let __tein_c = ::std::ffi::CString::new(__tein_s.as_str()).unwrap_or_default();
                ((*#api).sexp_c_str)(ctx, __tein_c.as_ptr(), __tein_s.len() as ::std::ffi::c_long)
            }
        },
        "bool" => quote! {
            ((*#api).sexp_make_boolean)(if #expr { 1 } else { 0 })
        },
        _ => {
            return Err(syn::Error::new_spanned(
                ty,
                format!(
                    "unsupported ext method return type: '{}'. supported: i64, f64, String, bool, ()",
                    type_str
                ),
            ));
        }
    };
    Ok(conv)
}

// ── ForeignType codegen ───────────────────────────────────────────────────────

fn generate_foreign_type_impl(type_info: &TypeInfo) -> syn::Result<proc_macro2::TokenStream> {
    let struct_name = &type_info.struct_item.ident;
    let scheme_type_name = &type_info.scheme_type_name;

    let method_entries: Vec<proc_macro2::TokenStream> = type_info
        .methods
        .iter()
        .map(|m| generate_method_entry(m, struct_name, scheme_type_name))
        .collect::<syn::Result<_>>()?;

    Ok(quote! {
        impl tein::ForeignType for #struct_name {
            fn type_name() -> &'static str { #scheme_type_name }
            fn methods() -> &'static [(&'static str, tein::MethodFn)] {
                &[#(#method_entries),*]
            }
        }
    })
}

fn generate_method_entry(
    m: &MethodInfo,
    struct_name: &syn::Ident,
    scheme_type_name: &str,
) -> syn::Result<proc_macro2::TokenStream> {
    let method_ident = &m.method.sig.ident;
    let scheme_method_name = rust_to_scheme_name(&method_ident.to_string());

    // collect non-self args
    let args: Vec<(usize, &syn::Ident, &Type)> = m
        .method
        .sig
        .inputs
        .iter()
        .skip(1) // skip self
        .enumerate()
        .map(|(i, arg)| {
            if let FnArg::Typed(pt) = arg
                && let Pat::Ident(pi) = pt.pat.as_ref()
            {
                return Ok((i, &pi.ident, pt.ty.as_ref()));
            }
            Err(syn::Error::new_spanned(
                arg,
                "expected named argument in #[tein_methods] method",
            ))
        })
        .collect::<syn::Result<_>>()?;

    let extractions: Vec<proc_macro2::TokenStream> = args
        .iter()
        .map(|(i, name, ty)| {
            gen_method_arg_extraction(name, ty, *i, scheme_type_name, &scheme_method_name)
        })
        .collect::<syn::Result<_>>()?;

    let arg_names: Vec<&syn::Ident> = args.iter().map(|(_, n, _)| *n).collect();

    let cast = if m.is_mut {
        quote! { let __tein_this = __tein_obj.downcast_mut::<#struct_name>().unwrap(); }
    } else {
        quote! { let __tein_this = __tein_obj.downcast_ref::<#struct_name>().unwrap(); }
    };

    // build the call — we call the method as an inherent method via the ident
    let call = quote! { #struct_name::#method_ident(__tein_this, #(#arg_names),*) };

    let return_conv = gen_method_return_conversion(&m.method.sig.output, call)?;

    let scheme_name_lit = syn::LitStr::new(&scheme_method_name, method_ident.span());

    Ok(quote! {
        (#scheme_name_lit, |__tein_obj: &mut dyn ::std::any::Any,
                            _ctx: &tein::MethodContext,
                            __tein_args: &[tein::Value]| -> tein::Result<tein::Value> {
            #(#extractions)*
            #cast
            #return_conv
        } as tein::MethodFn)
    })
}

/// generate extraction code for a method argument from `&[tein::Value]`.
///
/// <!-- extensibility: add a match arm here for new types (e.g. Value, Vec<Value>).
///      each arm emits code extracting from `__tein_args[index]`. -->
fn gen_method_arg_extraction(
    arg_name: &syn::Ident,
    ty: &Type,
    index: usize,
    type_name: &str,
    method_name: &str,
) -> syn::Result<proc_macro2::TokenStream> {
    let type_str = type_name_str(ty).unwrap_or_default();
    let err_msg = format!(
        "{}-{}: argument {} ({}): expected {}",
        type_name,
        method_name,
        index + 1,
        arg_name,
        type_str
    );

    let extraction = match type_str.as_str() {
        "i64" => quote! {
            let #arg_name: i64 = __tein_args.get(#index)
                .and_then(|v| v.as_integer())
                .ok_or_else(|| tein::Error::TypeError(#err_msg.to_string()))?;
        },
        "f64" => quote! {
            let #arg_name: f64 = __tein_args.get(#index)
                .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
                .ok_or_else(|| tein::Error::TypeError(#err_msg.to_string()))?;
        },
        "String" => quote! {
            let #arg_name: String = __tein_args.get(#index)
                .and_then(|v| v.as_string())
                .ok_or_else(|| tein::Error::TypeError(#err_msg.to_string()))?
                .to_string();
        },
        "bool" => quote! {
            let #arg_name: bool = __tein_args.get(#index)
                .and_then(|v| v.as_bool())
                .ok_or_else(|| tein::Error::TypeError(#err_msg.to_string()))?;
        },
        "Value" => quote! {
            let #arg_name: tein::Value = __tein_args.get(#index)
                .cloned()
                .ok_or_else(|| tein::Error::TypeError(#err_msg.to_string()))?;
        },
        _ => {
            return Err(syn::Error::new_spanned(
                ty,
                format!(
                    "unsupported method argument type: '{}'. supported: i64, f64, String, bool, Value",
                    type_str
                ),
            ));
        }
    };
    Ok(extraction)
}

/// generate the return conversion for a `#[tein_methods]` method.
///
/// <!-- extensibility: add a match arm here for new return types. -->
fn gen_method_return_conversion(
    output: &ReturnType,
    call: proc_macro2::TokenStream,
) -> syn::Result<proc_macro2::TokenStream> {
    match output {
        ReturnType::Default => Ok(quote! {
            #call;
            Ok(tein::Value::Unspecified)
        }),
        ReturnType::Type(_, ret_type) => {
            if let Some(inner) = extract_result_inner(ret_type) {
                let ok_conv = method_value_conversion(inner, quote! { __tein_ok })?;
                Ok(quote! {
                    match #call {
                        Ok(__tein_ok) => Ok(#ok_conv),
                        Err(__tein_err) => Err(tein::Error::EvalError(__tein_err.to_string())),
                    }
                })
            } else {
                let conv = method_value_conversion(ret_type, quote! { __tein_result })?;
                Ok(quote! {
                    let __tein_result = #call;
                    Ok(#conv)
                })
            }
        }
    }
}

/// convert a rust value expression to `tein::Value` for method returns.
///
/// <!-- extensibility: add a match arm for new return types. -->
fn method_value_conversion(
    ty: &Type,
    expr: proc_macro2::TokenStream,
) -> syn::Result<proc_macro2::TokenStream> {
    let type_str = type_name_str(ty).unwrap_or_default();
    let conv = match type_str.as_str() {
        "i64" => quote! { tein::Value::Integer(#expr) },
        "f64" => quote! { tein::Value::Float(#expr) },
        "String" => quote! { tein::Value::String(#expr) },
        "bool" => quote! { tein::Value::Boolean(#expr) },
        "Value" => quote! { #expr },
        _ => {
            return Err(syn::Error::new_spanned(
                ty,
                format!(
                    "unsupported method return type: '{}'. supported: i64, f64, String, bool, Value, ()",
                    type_str
                ),
            ));
        }
    };
    Ok(conv)
}

// ── VFS + registration codegen ────────────────────────────────────────────────

fn generate_vfs_sld(info: &ModuleInfo) -> String {
    let mut exports = Vec::new();

    for f in &info.free_fns {
        exports.push(f.scheme_name.clone());
    }
    for c in &info.consts {
        exports.push(c.scheme_name.clone());
    }
    for t in &info.types {
        exports.push(format!("{}?", t.scheme_type_name));
        for m in &t.methods {
            exports.push(m.scheme_name.clone());
        }
    }

    let exports_str = exports.join("\n          ");
    format!(
        "(define-library (tein {name})\n  \
         (import (scheme base))\n  \
         (export {exports})\n  \
         (include \"{name}.scm\"))",
        name = info.name,
        exports = exports_str,
    )
}

fn generate_vfs_scm(info: &ModuleInfo) -> String {
    let mut lines = vec![format!(
        ";; (tein {}) — generated by #[tein_module]",
        info.name
    )];

    let has_any_docs = info.consts.iter().any(|c| !c.doc.is_empty());

    for c in &info.consts {
        if has_any_docs {
            lines.push(String::new()); // blank line separator
        }
        for doc_line in &c.doc {
            lines.push(format!(";; {}", doc_line));
        }
        lines.push(format!("(define {} {})", c.scheme_name, c.scheme_literal));
    }

    lines.join("\n") + "\n"
}

/// generate the `.sld` library definition for the docs sub-library.
///
/// emits `(define-library (tein {name} docs) ...)` with a single export:
/// `{name}-docs`, the documentation alist.
fn generate_vfs_docs_sld(info: &ModuleInfo) -> String {
    format!(
        "(define-library (tein {name} docs)\n  \
         (import (scheme base))\n  \
         (export {name}-docs)\n  \
         (include \"docs.scm\"))",
        name = info.name,
    )
}

/// generate the `.scm` implementation for the docs sub-library.
///
/// emits `{name}-docs` as an alist with `__module__` metadata,
/// then entries for all consts, free fns, type predicates, and methods.
/// entry ordering: `__module__`, consts, free fns, then per-type
/// (predicate + methods in declaration order).
/// multi-line doc comments are joined with a single space.
/// items without docs get `""`.
fn generate_vfs_docs_scm(info: &ModuleInfo) -> String {
    let mut entries = Vec::new();

    // __module__ metadata — always first
    entries.push(format!("(__module__ . \"tein {}\")", info.name));

    // constants
    for c in &info.consts {
        let doc = escape_scheme_string(&join_doc_lines(&c.doc));
        entries.push(format!("({} . \"{}\")", c.scheme_name, doc));
    }

    // free functions
    for f in &info.free_fns {
        let doc = escape_scheme_string(&join_doc_lines(&f.doc));
        entries.push(format!("({} . \"{}\")", f.scheme_name, doc));
    }

    // types: predicate + methods
    for t in &info.types {
        let type_doc = escape_scheme_string(&join_doc_lines(&t.doc));
        entries.push(format!("({}? . \"{}\")", t.scheme_type_name, type_doc));
        for m in &t.methods {
            let method_doc = escape_scheme_string(&join_doc_lines(&m.doc));
            entries.push(format!("({} . \"{}\")", m.scheme_name, method_doc));
        }
    }

    let entries_str = entries
        .iter()
        .map(|e| format!("    {}", e))
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        ";; generated by #[tein_module] — do not edit\n\
         (define {name}-docs\n  '({entries}))\n",
        name = info.name,
        entries = entries_str.trim_start(),
    )
}

/// join multi-line doc comments into a single line, separated by spaces.
fn join_doc_lines(doc: &[String]) -> String {
    doc.join(" ")
}

/// escape characters that are special in scheme string literals.
fn escape_scheme_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn generate_register_fn(info: &ModuleInfo) -> proc_macro2::TokenStream {
    let fn_ident = syn::Ident::new(
        &format!("register_module_{}", info.name.replace('-', "_")),
        Span::call_site(),
    );

    let sld_content = generate_vfs_sld(info);
    let scm_content = generate_vfs_scm(info);
    let sld_path = format!("lib/tein/{}.sld", info.name);
    let scm_path = format!("lib/tein/{}.scm", info.name);
    let docs_sld_content = generate_vfs_docs_sld(info);
    let docs_scm_content = generate_vfs_docs_scm(info);
    let docs_sld_path = format!("lib/tein/{}/docs.sld", info.name);
    let docs_scm_path = format!("lib/tein/{}/docs.scm", info.name);

    let type_registrations: Vec<proc_macro2::TokenStream> = info
        .types
        .iter()
        .map(|t| {
            let struct_name = &t.struct_item.ident;
            quote! { __tein_ctx.register_foreign_type::<#struct_name>()?; }
        })
        .collect();

    let fn_registrations: Vec<proc_macro2::TokenStream> = info
        .free_fns
        .iter()
        .map(|f| {
            let wrapper_ident = syn::Ident::new(
                &format!("__tein_{}", f.func.sig.ident),
                f.func.sig.ident.span(),
            );
            let scheme_name = &f.scheme_name;
            quote! { __tein_ctx.define_fn_variadic(#scheme_name, #wrapper_ident)?; }
        })
        .collect();

    quote! {
        /// Register this module's types, functions, and VFS entries with a context.
        ///
        /// Must be called before any scheme code does `(import (tein ...))`.
        /// VFS entries are cleared on `Context::drop()`.
        pub fn #fn_ident(__tein_ctx: &tein::Context) -> tein::Result<()> {
            // native fns must be defined in top-level BEFORE VFS modules are
            // registered. when chibi later loads the library sld (on first import),
            // it resolves exports by walking the env parent chain (localp=0). if
            // the native fn is already in top-level, chibi's rename-binding import
            // mechanism shares the top-level cell — no stubs needed.
            #(#type_registrations)*
            #(#fn_registrations)*
            __tein_ctx.register_vfs_module(#sld_path, #sld_content)?;
            __tein_ctx.register_vfs_module(#scm_path, #scm_content)?;
            __tein_ctx.register_vfs_module(#docs_sld_path, #docs_sld_content)?;
            __tein_ctx.register_vfs_module(#docs_scm_path, #docs_scm_content)?;
            Ok(())
        }
    }
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// check if an attribute list contains a tein attribute with the given name
fn has_tein_attr(attrs: &[syn::Attribute], name: &str) -> bool {
    attrs.iter().any(|a| is_tein_attr_named(a, name))
}

/// check if a single attribute matches the given tein attribute name
fn is_tein_attr_named(attr: &syn::Attribute, name: &str) -> bool {
    attr.path().segments.last().is_some_and(|s| s.ident == name)
}

/// extract a `name = "..."` string from an attribute's arguments, if present.
///
/// e.g. `#[tein_fn(name = "my-scheme-name")]` → `Some("my-scheme-name")`
fn extract_name_override(attrs: &[syn::Attribute], attr_name: &str) -> syn::Result<Option<String>> {
    for attr in attrs {
        if !is_tein_attr_named(attr, attr_name) {
            continue;
        }
        // check if the attribute has arguments at all
        if let syn::Meta::Path(_) = attr.meta {
            return Ok(None); // bare attribute with no args
        }
        // parse as name-value list
        let result = attr.parse_args::<syn::MetaNameValue>();
        match result {
            Ok(nv) if nv.path.is_ident("name") => {
                if let syn::Expr::Lit(syn::ExprLit {
                    lit: syn::Lit::Str(s),
                    ..
                }) = &nv.value
                {
                    return Ok(Some(s.value()));
                }
            }
            _ => {} // not a name=... form, treat as bare
        }
    }
    Ok(None)
}

/// extract the type name from an impl block's Self type
fn impl_self_type_name(imp: &ItemImpl) -> syn::Result<String> {
    if let Type::Path(tp) = imp.self_ty.as_ref()
        && let Some(seg) = tp.path.segments.last()
    {
        return Ok(seg.ident.to_string());
    }
    Err(syn::Error::new_spanned(
        &imp.self_ty,
        "#[tein_methods] impl must be for a simple named type",
    ))
}

/// returns the innermost type from `Result<T, E>`, if the type is Result
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
fn type_name_str(ty: &Type) -> Option<String> {
    if let Type::Path(type_path) = ty {
        Some(type_path.path.segments.last()?.ident.to_string())
    } else {
        None
    }
}

// ── extern "C" wrapper codegen ────────────────────────────────────────────────

/// generate the extraction code for a single argument from the scheme args list
fn gen_arg_extraction(arg_name: &syn::Ident, ty: &Type, index: usize) -> proc_macro2::TokenStream {
    let type_str = type_name_str(ty).unwrap_or_default();
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
        // accept any scheme value as a tein::Value — no type check needed,
        // from_raw converts whatever sexp chibi passes. on conversion failure
        // (shouldn't happen for valid sexps) returns a descriptive error string
        // so LLM callers get actionable diagnostics rather than silent Nil.
        "Value" => quote! {
            let #arg_name: tein::Value = {
                let raw = tein::raw::sexp_car(__tein_current_args);
                // safety: ctx and raw come from chibi-scheme internals
                match unsafe { tein::Value::from_raw(ctx, raw) } {
                    Ok(v) => v,
                    Err(e) => {
                        let msg = format!(
                            "failed to convert argument '{}': {}",
                            stringify!(#arg_name), e
                        );
                        let c_msg = ::std::ffi::CString::new(msg.clone()).unwrap();
                        return tein::raw::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as tein::raw::sexp_sint_t);
                    }
                }
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
    let type_str = type_name_str(ty).unwrap_or_default();

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
        "Value" => quote! {
            {
                let __tein_val: tein::Value = #result_expr;
                match unsafe { __tein_val.to_raw(ctx) } {
                    Ok(raw) => raw,
                    Err(e) => {
                        let msg = e.to_string();
                        let c_msg = ::std::ffi::CString::new(msg.as_str()).unwrap_or_default();
                        tein::raw::sexp_c_str(ctx, c_msg.as_ptr(), msg.len() as tein::raw::sexp_sint_t)
                    }
                }
            }
        },
        _ => {
            quote! { compile_error!(concat!("unsupported #[tein_fn] return type: '", #type_str, "'. supported: i64, f64, String, bool, Value, ()")); }
        }
    }
}

fn generate_scheme_fn(mut input: ItemFn) -> syn::Result<proc_macro2::TokenStream> {
    let fn_name = &input.sig.ident;
    let wrapper_name = syn::Ident::new(&format!("__tein_{}", fn_name), fn_name.span());

    // parse arguments; rewrite bare `Value` types to `tein::Value` so the emitted fn
    // compiles in any calling crate (the macro can't rely on `Value` being in scope)
    let mut arg_names = Vec::new();
    let mut arg_types = Vec::new();

    for arg in input.sig.inputs.iter_mut() {
        match arg {
            FnArg::Typed(pat_type) => {
                let name = match pat_type.pat.as_ref() {
                    Pat::Ident(pat_ident) => pat_ident.ident.clone(),
                    _ => {
                        return Err(syn::Error::new_spanned(
                            &*pat_type,
                            "expected simple argument name",
                        ));
                    }
                };
                // rewrite bare `Value` → `tein::Value` in the emitted fn signature
                if type_name_str(&pat_type.ty).as_deref() == Some("Value") {
                    *pat_type.ty = syn::parse_quote! { tein::Value };
                }
                arg_names.push(name);
                arg_types.push(pat_type.ty.as_ref().clone());
            }
            FnArg::Receiver(_) => {
                return Err(syn::Error::new_spanned(
                    &*arg,
                    "tein_fn does not support self parameters",
                ));
            }
        }
    }

    let extractions: Vec<_> = arg_names
        .iter()
        .zip(arg_types.iter())
        .enumerate()
        .map(|(i, (name, ty))| gen_arg_extraction(name, ty, i))
        .collect();

    let call_args = &arg_names;
    let call_expr = quote! { #fn_name(#(#call_args),*) };

    let return_conversion = match &input.sig.output {
        ReturnType::Default => {
            quote! {
                #call_expr;
                tein::raw::get_void()
            }
        }
        ReturnType::Type(_, ret_type) => {
            if let Some(inner_type) = extract_result_inner(ret_type) {
                let success_conv = gen_return_conversion(inner_type, quote! { __tein_ok });
                quote! {
                    match #call_expr {
                        Ok(__tein_ok) => { #success_conv }
                        // Result::Err raises a scheme exception (error object).
                        // catchable with (guard ...), inspectable via error-object-message.
                        Err(__tein_err) => {
                            let msg = __tein_err.to_string();
                            let c_msg = ::std::ffi::CString::new(msg.as_str()).unwrap_or_default();
                            tein::raw::make_error(ctx, c_msg.as_ptr(), msg.len() as tein::raw::sexp_sint_t)
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
                    // panic → raise scheme exception
                    let msg = concat!("rust panic in ", stringify!(#fn_name));
                    let c_msg = ::std::ffi::CString::new(msg).unwrap();
                    tein::raw::make_error(ctx, c_msg.as_ptr(), msg.len() as tein::raw::sexp_sint_t)
                }
            }
        }
    })
}

// ── unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rust_to_scheme_name() {
        assert_eq!(rust_to_scheme_name("parse"), "parse");
        assert_eq!(rust_to_scheme_name("is_match"), "is-match");
        assert_eq!(rust_to_scheme_name("object_q"), "object?");
        assert_eq!(rust_to_scheme_name("set_value_bang"), "set-value!");
        assert_eq!(rust_to_scheme_name("a_b_c"), "a-b-c");
    }

    #[test]
    fn test_pascal_to_kebab() {
        assert_eq!(pascal_to_kebab("JsonValue"), "json-value");
        assert_eq!(pascal_to_kebab("HttpClient"), "http-client");
        assert_eq!(pascal_to_kebab("Regex"), "regex");
        assert_eq!(pascal_to_kebab("URL"), "u-r-l"); // edge case, use name override
    }

    #[test]
    fn test_extract_doc_comments() {
        use syn::parse_quote;

        // single-line doc
        let attrs: Vec<syn::Attribute> = vec![parse_quote!(#[doc = " hello world"])];
        assert_eq!(extract_doc_comments(&attrs), vec!["hello world"]);

        // multi-line docs
        let attrs: Vec<syn::Attribute> = vec![
            parse_quote!(#[doc = " line one"]),
            parse_quote!(#[doc = " line two"]),
        ];
        assert_eq!(extract_doc_comments(&attrs), vec!["line one", "line two"]);

        // mixed with non-doc attrs
        let attrs: Vec<syn::Attribute> = vec![
            parse_quote!(#[allow(dead_code)]),
            parse_quote!(#[doc = " the doc"]),
            parse_quote!(#[cfg(test)]),
        ];
        assert_eq!(extract_doc_comments(&attrs), vec!["the doc"]);

        // empty — no docs
        let attrs: Vec<syn::Attribute> = vec![parse_quote!(#[allow(dead_code)])];
        assert!(extract_doc_comments(&attrs).is_empty());

        // empty doc comment (just `///`)
        let attrs: Vec<syn::Attribute> = vec![parse_quote!(#[doc = ""])];
        assert_eq!(extract_doc_comments(&attrs), vec![""]);
    }

    #[test]
    fn test_generate_vfs_scm_with_docs() {
        let info = ModuleInfo {
            name: "test".to_string(),
            ext: false,
            mod_item: syn::parse_quote! { mod test {} },
            free_fns: vec![],
            types: vec![],
            consts: vec![
                ConstInfo {
                    scheme_name: "greeting".to_string(),
                    scheme_literal: "\"hello\"".to_string(),
                    doc: vec!["a friendly greeting".to_string()],
                },
                ConstInfo {
                    scheme_name: "max-size".to_string(),
                    scheme_literal: "256".to_string(),
                    doc: vec![
                        "the maximum allowed size.".to_string(),
                        "must be a positive integer.".to_string(),
                    ],
                },
                ConstInfo {
                    scheme_name: "bare".to_string(),
                    scheme_literal: "#t".to_string(),
                    doc: vec![],
                },
            ],
        };

        let scm = generate_vfs_scm(&info);
        let expected = "\
;; (tein test) — generated by #[tein_module]

;; a friendly greeting
(define greeting \"hello\")

;; the maximum allowed size.
;; must be a positive integer.
(define max-size 256)

(define bare #t)
";
        assert_eq!(scm, expected);
    }

    #[test]
    fn test_generate_vfs_scm_no_docs() {
        let info = ModuleInfo {
            name: "plain".to_string(),
            ext: false,
            mod_item: syn::parse_quote! { mod plain {} },
            free_fns: vec![],
            types: vec![],
            consts: vec![
                ConstInfo {
                    scheme_name: "x".to_string(),
                    scheme_literal: "1".to_string(),
                    doc: vec![],
                },
                ConstInfo {
                    scheme_name: "y".to_string(),
                    scheme_literal: "2".to_string(),
                    doc: vec![],
                },
            ],
        };

        let scm = generate_vfs_scm(&info);
        // no docs → compact format, no blank lines between defines
        let expected = "\
;; (tein plain) — generated by #[tein_module]
(define x 1)
(define y 2)
";
        assert_eq!(scm, expected);
    }

    #[test]
    fn test_generate_vfs_docs_sld() {
        let info = ModuleInfo {
            name: "uuid".to_string(),
            ext: false,
            mod_item: syn::parse_quote! { mod uuid {} },
            free_fns: vec![],
            types: vec![],
            consts: vec![],
        };
        let sld = generate_vfs_docs_sld(&info);
        assert_eq!(
            sld,
            "(define-library (tein uuid docs)\n  \
             (import (scheme base))\n  \
             (export uuid-docs)\n  \
             (include \"docs.scm\"))"
        );
    }

    #[test]
    fn test_generate_vfs_docs_scm_full() {
        let info = ModuleInfo {
            name: "mymod".to_string(),
            ext: false,
            mod_item: syn::parse_quote! { mod mymod {} },
            free_fns: vec![FreeFnInfo {
                func: syn::parse_quote! { fn do_thing(x: i64) -> i64 { x } },
                scheme_name: "mymod-do-thing".to_string(),
                doc: vec!["do the thing".to_string()],
            }],
            consts: vec![ConstInfo {
                scheme_name: "max-size".to_string(),
                scheme_literal: "256".to_string(),
                doc: vec!["maximum allowed size".to_string()],
            }],
            types: vec![TypeInfo {
                struct_item: syn::parse_quote! { struct Widget { n: i64 } },
                scheme_type_name: "widget".to_string(),
                methods: vec![MethodInfo {
                    method: syn::parse_quote! { fn get(&self) -> i64 { self.n } },
                    scheme_name: "widget-get".to_string(),
                    is_mut: false,
                    doc: vec!["get the value".to_string()],
                }],
                doc: vec!["a widget".to_string()],
            }],
        };

        let scm = generate_vfs_docs_scm(&info);
        let expected = "\
;; generated by #[tein_module] — do not edit
(define mymod-docs
  '((__module__ . \"tein mymod\")
    (max-size . \"maximum allowed size\")
    (mymod-do-thing . \"do the thing\")
    (widget? . \"a widget\")
    (widget-get . \"get the value\")))
";
        assert_eq!(scm, expected);
    }

    #[test]
    fn test_generate_vfs_docs_scm_empty_docs() {
        let info = ModuleInfo {
            name: "bare".to_string(),
            ext: false,
            mod_item: syn::parse_quote! { mod bare {} },
            free_fns: vec![FreeFnInfo {
                func: syn::parse_quote! { fn noop() -> i64 { 0 } },
                scheme_name: "bare-noop".to_string(),
                doc: vec![],
            }],
            consts: vec![],
            types: vec![],
        };

        let scm = generate_vfs_docs_scm(&info);
        let expected = "\
;; generated by #[tein_module] — do not edit
(define bare-docs
  '((__module__ . \"tein bare\")
    (bare-noop . \"\")))
";
        assert_eq!(scm, expected);
    }

    #[test]
    fn test_generate_vfs_docs_scm_multiline_doc() {
        let info = ModuleInfo {
            name: "ml".to_string(),
            ext: false,
            mod_item: syn::parse_quote! { mod ml {} },
            free_fns: vec![],
            consts: vec![ConstInfo {
                scheme_name: "x".to_string(),
                scheme_literal: "1".to_string(),
                doc: vec!["first line.".to_string(), "second line.".to_string()],
            }],
            types: vec![],
        };

        let scm = generate_vfs_docs_scm(&info);
        // multi-line docs joined with space
        assert!(scm.contains("(x . \"first line. second line.\")"));
    }

    #[test]
    fn test_parse_module_attr_basic() {
        let tokens: proc_macro2::TokenStream = quote! { "testmod" };
        let (name, ext) = parse_module_attr(tokens).unwrap();
        assert_eq!(name, "testmod");
        assert!(!ext);
    }

    #[test]
    fn test_parse_module_attr_ext() {
        let tokens: proc_macro2::TokenStream = quote! { "testmod", ext = true };
        let (name, ext) = parse_module_attr(tokens).unwrap();
        assert_eq!(name, "testmod");
        assert!(ext);
    }
}
