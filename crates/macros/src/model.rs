// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Serde-backed custom data model generation.

use proc_macro2::{Span, TokenStream};
use quote::{ToTokens, quote};
use syn::{
    Fields, Ident, ItemStruct, LitStr, Token, Type, parse::Parser, parse_quote, parse2,
    punctuated::Punctuated,
};

use crate::support::{
    CustomDataOption, FieldSpec, parse_field_options, type_for_macro, type_last_segment, type_path,
};

fn map_type_for_macro(ty: &Type) -> Option<(String, String, String)> {
    let segment = type_path(ty)?.segments.last()?;
    let outer = segment.ident.to_string();
    if outer != "HashMap" && outer != "IndexMap" {
        return None;
    }

    let syn::PathArguments::AngleBracketed(args) = &segment.arguments else {
        return None;
    };
    let mut types = args.args.iter().filter_map(|arg| match arg {
        syn::GenericArgument::Type(ty) => Some(ty),
        _ => None,
    });

    Some((
        outer,
        type_last_segment(types.next()?)?,
        type_last_segment(types.next()?)?,
    ))
}

fn is_typed_json_map_segment(segment: &str) -> bool {
    matches!(
        segment,
        "InstrumentId"
            | "AccountId"
            | "Currency"
            | "BarType"
            | "Price"
            | "Quantity"
            | "Money"
            | "String"
            | "f64"
            | "f32"
            | "bool"
            | "u64"
            | "i64"
            | "u32"
            | "i32"
    )
}

fn typed_json_map_kind(ty: &Type) -> Option<String> {
    let (outer, key, value) = map_type_for_macro(ty)?;
    (is_typed_json_map_segment(&key) && is_typed_json_map_segment(&value)).then_some(outer)
}

fn py_param_ty(ty: &Type, serde: bool) -> Option<TokenStream> {
    if serde {
        return Some(quote! { pyo3::Py<pyo3::PyAny> });
    }

    let (outer, inner) = type_for_macro(ty)?;
    if outer == "UnixNanos" {
        return Some(quote! { u64 });
    }

    if outer == inner && outer == "Params" {
        return Some(quote! { pyo3::Py<pyo3::types::PyDict> });
    }
    Some(quote! { #ty })
}

fn py_field_init(ident: &Ident, ty: &Type, serde: bool) -> Option<TokenStream> {
    if serde {
        if let Some(map_kind) = typed_json_map_kind(ty) {
            let conversion = if map_kind == "IndexMap" {
                quote! { indexmap_from_pyobject_pyo3 }
            } else {
                quote! { hashmap_from_pyobject_pyo3 }
            };
            return Some(quote! {
                pyo3::Python::attach(|py| -> pyo3::PyResult<#ty> {
                    let value = #ident.bind(py);
                    nautilus_core::python::serialization::#conversion::<_, _>(py, value)
                        .map_err(|e| nautilus_core::python::to_pyvalue_err(format!("failed to deserialize JSON field '{}': {e}", stringify!(#ident))))
                })?
            });
        }

        return Some(quote! {
            pyo3::Python::attach(|py| -> pyo3::PyResult<#ty> {
                let value = #ident.bind(py);
                nautilus_core::python::serialization::from_pyobject_pyo3::<#ty>(py, value)
                    .map_err(|e| nautilus_core::python::to_pyvalue_err(format!("failed to deserialize JSON field '{}': {e}", stringify!(#ident))))
            })?
        });
    }

    let (outer, inner) = type_for_macro(ty)?;
    if outer == "UnixNanos" {
        return Some(quote! { #ident.into() });
    }

    if outer == inner && outer == "Params" {
        return Some(quote! {
            pyo3::Python::attach(|py| nautilus_core::from_pydict(py, &#ident))?.unwrap_or_default()
        });
    }
    Some(quote! { #ident })
}

fn py_getter_ret_ty(ty: &Type, serde: bool) -> Option<TokenStream> {
    if serde {
        return Some(quote! { pyo3::PyResult<pyo3::Py<pyo3::PyAny>> });
    }

    let (outer, inner) = type_for_macro(ty)?;
    if outer == "UnixNanos" {
        return Some(quote! { u64 });
    }

    if outer == inner && outer == "Params" {
        return Some(quote! { pyo3::PyResult<pyo3::Py<pyo3::types::PyDict>> });
    }
    Some(quote! { #ty })
}

fn py_getter_body(ident: &Ident, ty: &Type, serde: bool) -> Option<TokenStream> {
    if serde {
        if let Some(map_kind) = typed_json_map_kind(ty) {
            let conversion = if map_kind == "IndexMap" {
                quote! { indexmap_to_pydict_pyo3 }
            } else {
                quote! { hashmap_to_pydict_pyo3 }
            };
            return Some(quote! {
                pyo3::Python::attach(|py| {
                    nautilus_core::python::serialization::#conversion(py, &self.#ident)
                        .map_err(|e| nautilus_core::python::to_pyvalue_err(format!("failed to serialize JSON field '{}': {e}", stringify!(#ident))))
                })
            });
        }

        return Some(quote! {
            pyo3::Python::attach(|py| {
                nautilus_core::python::serialization::to_pyobject_pyo3(py, &self.#ident)
                    .map_err(|e| nautilus_core::python::to_pyvalue_err(format!("failed to serialize JSON field '{}': {e}", stringify!(#ident))))
            })
        });
    }

    let (outer, inner) = type_for_macro(ty)?;
    if outer == "UnixNanos" {
        return Some(quote! { self.#ident.as_u64() });
    }

    if outer == inner && outer == "Params" {
        return Some(quote! { pyo3::Python::attach(|py| self.#ident.to_pydict(py)) });
    }

    if outer == "Vec" || outer == "String" {
        return Some(quote! { self.#ident.clone() });
    }
    Some(quote! { self.#ident })
}

#[derive(Debug, Default)]
struct CustomDataOptions {
    pyo3: bool,
    no_display: bool,
    stub_module: Option<LitStr>,
}

fn parse_options(attr: &TokenStream) -> Result<CustomDataOptions, syn::Error> {
    if attr.is_empty() {
        return Ok(CustomDataOptions::default());
    }

    let mut options = CustomDataOptions::default();

    for option in
        Punctuated::<CustomDataOption, Token![,]>::parse_terminated.parse2(attr.clone())?
    {
        let name = option.ident.to_string();
        match (name.as_str(), option.value) {
            ("pyo3", None) => options.pyo3 = true,
            ("no_display", None) => options.no_display = true,
            ("stub_module", Some(module)) => options.stub_module = Some(module),
            ("pyo3" | "no_display", Some(_)) => {
                return Err(syn::Error::new_spanned(
                    option.ident,
                    "option does not accept a value",
                ));
            }
            ("stub_module", None) => {
                return Err(syn::Error::new_spanned(
                    option.ident,
                    "`stub_module` requires a string value",
                ));
            }
            _ => {
                return Err(syn::Error::new_spanned(
                    option.ident,
                    "expected `pyo3`, `no_display`, or `stub_module`; unknown option",
                ));
            }
        }
    }

    if options.stub_module.is_some() && !options.pyo3 {
        return Err(syn::Error::new_spanned(
            attr,
            "`stub_module` requires `pyo3`",
        ));
    }
    Ok(options)
}

fn attr_has_ident(attr: &syn::Attribute, ident: &str) -> bool {
    attr.path().get_ident().is_some_and(|value| value == ident)
}

fn push_derive_path(paths: &mut Vec<syn::Path>, path: syn::Path) {
    let key = path.to_token_stream().to_string();

    if !paths
        .iter()
        .any(|existing| existing.to_token_stream().to_string() == key)
    {
        paths.push(path);
    }
}

fn push_required_derive_path(paths: &mut Vec<syn::Path>, path: syn::Path) {
    let name = path
        .segments
        .last()
        .map(|segment| segment.ident.to_string());
    if !paths.iter().any(|existing| {
        existing
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
            == name
    }) {
        push_derive_path(paths, path);
    }
}

fn derived_attr(attrs: &[syn::Attribute]) -> Result<TokenStream, syn::Error> {
    let mut paths = Vec::new();

    for attr in attrs.iter().filter(|attr| attr_has_ident(attr, "derive")) {
        for path in attr.parse_args_with(Punctuated::<syn::Path, Token![,]>::parse_terminated)? {
            push_derive_path(&mut paths, path);
        }
    }

    push_required_derive_path(&mut paths, parse_quote!(Debug));
    push_required_derive_path(&mut paths, parse_quote!(Clone));
    push_required_derive_path(&mut paths, parse_quote!(serde::Serialize));
    push_required_derive_path(&mut paths, parse_quote!(serde::Deserialize));
    push_required_derive_path(&mut paths, parse_quote!(PartialEq));
    Ok(quote! { #[derive(#(#paths),*)] })
}

struct ExpansionContext<'a> {
    name: &'a Ident,
    name_str: &'a str,
    generics: &'a syn::Generics,
    vis: &'a syn::Visibility,
    fields: &'a [FieldSpec],
    options: &'a CustomDataOptions,
}

fn gen_new_fn(ctx: &ExpansionContext<'_>) -> TokenStream {
    let name = ctx.name;
    let generics = ctx.generics;
    let vis = ctx.vis;
    let params = ctx.fields.iter().map(|field| {
        let ident = &field.ident;
        let ty = &field.ty;
        quote! { #ident: #ty }
    });
    let fields = ctx.fields.iter().map(|field| &field.ident);
    quote! {
        impl #generics #name #generics {
            #[allow(dead_code)]
            #[allow(clippy::too_many_arguments)]
            /// Constructor from all fields.
            #vis fn new(#(#params),*) -> Self {
                Self { #(#fields),* }
            }
        }
    }
}

fn gen_repr_impl(ctx: &ExpansionContext<'_>) -> TokenStream {
    if ctx.options.no_display {
        return quote! {};
    }

    let name = ctx.name;
    let generics = ctx.generics;
    let format_parts = ctx.fields.iter().map(|field| {
        let name = field.ident.to_string();
        if name == "ts_event" || name == "ts_init" {
            format!("{name}={{}}")
        } else {
            format!("{name}={{:?}}")
        }
    });
    let format = LitStr::new(
        &format!(
            "{}({})",
            ctx.name_str,
            format_parts.collect::<Vec<_>>().join(", ")
        ),
        Span::call_site(),
    );
    let args = ctx.fields.iter().map(|field| {
        let ident = &field.ident;
        if ident == "ts_event" || ident == "ts_init" {
            quote! { nautilus_core::datetime::unix_nanos_to_iso8601(self.#ident) }
        } else {
            quote! { self.#ident }
        }
    });
    quote! {
        impl #generics #name #generics {
            /// Returns a string representation with ISO 8601 timestamps.
            pub fn repr(&self) -> String {
                format!(#format, #(#args),*)
            }
        }

        impl #generics std::fmt::Display for #name #generics {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                std::write!(f, "{}", self.repr())
            }
        }
    }
}

fn gen_model_impls(ctx: &ExpansionContext<'_>) -> TokenStream {
    let name = ctx.name;
    let name_str = ctx.name_str;
    let generics = ctx.generics;
    let to_pyobject = ctx.options.pyo3.then(|| {
        quote! {
            #[cfg(feature = "python")]
            fn to_pyobject(&self, py: pyo3::Python<'_>) -> pyo3::PyResult<pyo3::Py<pyo3::PyAny>> {
                nautilus_model::data::custom::clone_pyclass_to_pyobject(self, py)
            }
        }
    });
    quote! {
        impl #generics nautilus_model::data::HasTsInit for #name #generics {
            fn ts_init(&self) -> nautilus_core::UnixNanos {
                self.ts_init
            }
        }

        impl #generics nautilus_model::data::CustomDataTrait for #name #generics {
            fn type_name(&self) -> &'static str {
                #name_str
            }

            fn type_name_static() -> &'static str {
                #name_str
            }

            fn as_any(&self) -> &dyn std::any::Any {
                self
            }

            fn ts_event(&self) -> nautilus_core::UnixNanos {
                self.ts_event
            }

            fn to_json(&self) -> anyhow::Result<String> {
                serde_json::to_string(self).map_err(Into::into)
            }

            fn clone_arc(&self) -> std::sync::Arc<dyn nautilus_model::data::CustomDataTrait> {
                std::sync::Arc::new(std::clone::Clone::clone(self))
            }

            fn eq_arc(&self, other: &dyn nautilus_model::data::CustomDataTrait) -> bool {
                other.as_any().downcast_ref::<Self>().is_some_and(|other| self == other)
            }

            fn from_json(value: serde_json::Value) -> anyhow::Result<std::sync::Arc<dyn nautilus_model::data::CustomDataTrait>> {
                Ok(std::sync::Arc::new(serde_json::from_value::<Self>(value)?))
            }

            #to_pyobject
        }

        impl #generics std::convert::From<#name #generics> for nautilus_model::data::Data {
            fn from(value: #name #generics) -> Self {
                nautilus_model::data::Data::Custom(
                    nautilus_model::data::CustomData::from_arc(std::sync::Arc::new(value)),
                )
            }
        }

        impl #generics std::convert::TryFrom<nautilus_model::data::Data> for #name #generics {
            type Error = anyhow::Error;

            fn try_from(value: nautilus_model::data::Data) -> std::result::Result<Self, Self::Error> {
                match value {
                    nautilus_model::data::Data::Custom(custom) => custom
                        .data
                        .as_any()
                        .downcast_ref::<Self>()
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("Expected {}", #name_str)),
                    _ => anyhow::bail!("Expected Custom data variant"),
                }
            }
        }
    }
}

fn gen_pymethods_impl(ctx: &ExpansionContext<'_>) -> TokenStream {
    if !ctx.options.pyo3 {
        return quote! {};
    }

    let name = ctx.name;
    let generics = ctx.generics;
    let params = ctx.fields.iter().map(|field| {
        let ident = &field.ident;
        let ty = py_param_ty(&field.ty, field.options.serde).expect("validated PyO3 field");
        quote! { #ident: #ty }
    });
    let bindings = ctx.fields.iter().map(|field| {
        let ident = &field.ident;
        let init =
            py_field_init(ident, &field.ty, field.options.serde).expect("validated PyO3 field");
        quote! { let #ident = #init; }
    });
    let args = ctx.fields.iter().map(|field| &field.ident);
    let signature_args = ctx.fields.iter().map(|field| &field.ident);
    let getters = ctx.fields.iter().map(|field| {
        let ident = &field.ident;
        let return_ty =
            py_getter_ret_ty(&field.ty, field.options.serde).expect("validated PyO3 field");
        let body =
            py_getter_body(ident, &field.ty, field.options.serde).expect("validated PyO3 field");
        quote! {
            #[getter]
            fn #ident(&self) -> #return_ty {
                #body
            }
        }
    });
    let repr_methods = (!ctx.options.no_display).then(|| {
        quote! {
            fn __repr__(&self) -> pyo3::PyResult<String> {
                Ok(std::string::ToString::to_string(self))
            }

            fn __str__(&self) -> pyo3::PyResult<String> {
                Ok(std::string::ToString::to_string(self))
            }
        }
    });
    let stub_attr = ctx.options.stub_module.is_some().then(|| {
        quote! { #[cfg_attr(feature = "python", pyo3_stub_gen::derive::gen_stub_pymethods)] }
    });
    quote! {
        #[cfg(feature = "python")]
        use pyo3::prelude::*;

        #[cfg(feature = "python")]
        #stub_attr
        #[pyo3::pymethods]
        #[expect(
            clippy::needless_pass_by_value,
            reason = "PyO3 constructors take owned Python argument values"
        )]
        impl #generics #name #generics {
            #[allow(clippy::too_many_arguments)]
            #[new]
            #[pyo3(signature = (#(#signature_args),*))]
            fn py_new(#(#params),*) -> pyo3::PyResult<Self> {
                #(#bindings)*
                Ok(Self::new(#(#args),*))
            }

            #(#getters)*
            #repr_methods

            fn to_json(&self) -> pyo3::PyResult<String> {
                <#name as nautilus_model::data::CustomDataTrait>::to_json_py(self)
                    .map_err(nautilus_core::python::to_pyvalue_err)
            }

            #[classmethod]
            fn from_json(
                _cls: &pyo3::Bound<'_, pyo3::types::PyType>,
                py: pyo3::Python<'_>,
                data: &pyo3::Bound<'_, pyo3::PyAny>,
            ) -> pyo3::PyResult<pyo3::Py<pyo3::PyAny>> {
                let inner: #name = nautilus_core::python::serialization::from_pyobject_pyo3(py, data)?;
                Ok(pyo3::Py::new(py, inner)?.into_any())
            }
        }
    }
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "proc-macro entry points consume owned token streams"
)]
#[expect(
    clippy::too_many_lines,
    reason = "macro expansion orchestration is clearer as one ordered assembly function"
)]
pub(crate) fn expand_custom_data(attr: TokenStream, item: TokenStream) -> TokenStream {
    let options = match parse_options(&attr) {
        Ok(options) => options,
        Err(e) => return e.to_compile_error(),
    };
    let input = match parse2::<ItemStruct>(item) {
        Ok(input) => input,
        Err(e) => return e.to_compile_error(),
    };
    let fields = match &input.fields {
        Fields::Named(fields) => &fields.named,
        _ => {
            return syn::Error::new_spanned(
                input,
                "#[custom_data] requires a struct with named fields",
            )
            .to_compile_error();
        }
    };
    let field_specs = match fields
        .iter()
        .map(|field| {
            Ok(FieldSpec {
                ident: field.ident.clone().expect("named field"),
                ty: field.ty.clone(),
                options: parse_field_options(field)?,
            })
        })
        .collect::<Result<Vec<_>, syn::Error>>()
    {
        Ok(fields) => fields,
        Err(e) => return e.to_compile_error(),
    };

    if !field_specs.iter().any(|field| field.ident == "ts_event")
        || !field_specs.iter().any(|field| field.ident == "ts_init")
    {
        return syn::Error::new_spanned(
            input,
            "#[custom_data] requires fields ts_event and ts_init (e.g. nautilus_core::UnixNanos)",
        )
        .to_compile_error();
    }

    if options.pyo3 {
        for field in &field_specs {
            if py_param_ty(&field.ty, field.options.serde).is_none() {
                return syn::Error::new_spanned(
                    &field.ty,
                    format!(
                        "#[custom_data(pyo3)] cannot convert field '{}'; use #[custom_data_field(serde)]",
                        field.ident,
                    ),
                )
                .to_compile_error();
            }
        }
    }

    let name = &input.ident;
    let name_str = name.to_string();
    let vis = &input.vis;
    let generics = &input.generics;
    let ctx = ExpansionContext {
        name,
        name_str: &name_str,
        generics,
        vis,
        fields: &field_specs,
        options: &options,
    };
    let derives = match derived_attr(&input.attrs) {
        Ok(derives) => derives,
        Err(e) => return e.to_compile_error(),
    };
    let attrs = input
        .attrs
        .iter()
        .filter(|attr| !attr_has_ident(attr, "custom_data") && !attr_has_ident(attr, "derive"));
    let fields = fields.iter().cloned().map(|mut field| {
        field
            .attrs
            .retain(|attr| !attr_has_ident(attr, "custom_data_field"));
        field
    });
    let pyclass_attr = options.pyo3.then(|| {
        quote! { #[cfg_attr(feature = "python", pyo3::pyclass(from_py_object))] }
    });
    let stub_pyclass_attr = options.stub_module.as_ref().map(|module| {
        quote! {
            #[cfg_attr(feature = "python", pyo3_stub_gen::derive::gen_stub_pyclass(module = #module))]
        }
    });
    let new_fn = gen_new_fn(&ctx);
    let repr_impl = gen_repr_impl(&ctx);
    let model_impls = gen_model_impls(&ctx);
    let pymethods = gen_pymethods_impl(&ctx);

    quote! {
        #derives
        #(#attrs)*
        #stub_pyclass_attr
        #pyclass_attr
        #vis struct #name #generics {
            #(#fields),*
        }

        #new_fn
        #repr_impl
        #model_impls
        #pymethods
    }
}

#[cfg(test)]
mod tests {
    use quote::quote;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn expansion_is_model_only() {
        let expanded = expand_custom_data(
            quote! {},
            quote! {
                pub struct TestData {
                    pub value: f64,
                    pub ts_event: nautilus_core::UnixNanos,
                    pub ts_init: nautilus_core::UnixNanos,
                }
            },
        )
        .to_string();

        assert!(expanded.contains("CustomDataTrait"));
        assert!(expanded.contains("serde :: Serialize"));
        assert!(!expanded.contains("arrow ::"));
        assert!(!expanded.contains("nautilus_serialization"));
        assert!(!expanded.contains("record_batch"));
    }

    #[rstest]
    fn pyo3_expansion_is_model_only() {
        let expanded = expand_custom_data(
            quote! { pyo3, stub_module = "nautilus_trader.test" },
            quote! {
                pub struct TestData {
                    pub value: f64,
                    pub ts_event: nautilus_core::UnixNanos,
                    pub ts_init: nautilus_core::UnixNanos,
                }
            },
        )
        .to_string();

        assert!(expanded.contains("pyo3 :: pymethods"));
        assert!(expanded.contains("gen_stub_pymethods"));
        assert!(!expanded.contains("encode_record_batch_py"));
        assert!(!expanded.contains("decode_record_batch_py"));
    }

    #[rstest]
    fn no_arrow_option_is_rejected() {
        let error = parse_options(&quote! { no_arrow }).unwrap_err();

        assert_eq!(
            error.to_string(),
            "expected `pyo3`, `no_display`, or `stub_module`; unknown option",
        );
    }
}
