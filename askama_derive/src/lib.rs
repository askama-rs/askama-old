#![cfg_attr(not(feature = "i18n"), forbid(unsafe_code))]
#![deny(elided_lifetimes_in_paths)]
#![deny(unreachable_pub)]

use std::borrow::Cow;
use std::fmt;

use proc_macro::TokenStream;
use proc_macro2::Span;

mod config;
mod generator;
mod heritage;
#[cfg(feature = "i18n")]
mod i18n;
mod input;
mod parser;

#[proc_macro_derive(Template, attributes(template, locale))]
pub fn derive_template(input: TokenStream) -> TokenStream {
    generator::derive_template(input)
}

#[proc_macro]
pub fn i18n_load(_input: TokenStream) -> TokenStream {
    #[cfg(feature = "i18n")]
    match i18n::load(_input) {
        Ok(ts) => ts,
        Err(err) => err.into_compile_error(),
    }

    #[cfg(not(feature = "i18n"))]
    CompileError::from(r#"Activate the "i18n" feature to use i18n_load!()."#).into_compile_error()
}

#[derive(Debug, Clone)]
struct CompileError {
    msg: Cow<'static, str>,
    span: Span,
}

impl CompileError {
    fn new<S: Into<Cow<'static, str>>>(s: S, span: Span) -> Self {
        Self {
            msg: s.into(),
            span,
        }
    }

    fn into_compile_error(self) -> TokenStream {
        syn::Error::new(self.span, self.msg)
            .to_compile_error()
            .into()
    }
}

impl std::error::Error for CompileError {}

impl fmt::Display for CompileError {
    #[inline]
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt.write_str(&self.msg)
    }
}

impl From<&'static str> for CompileError {
    #[inline]
    fn from(s: &'static str) -> Self {
        Self::new(s, Span::call_site())
    }
}

impl From<String> for CompileError {
    #[inline]
    fn from(s: String) -> Self {
        Self::new(s, Span::call_site())
    }
}

// This is used by the code generator to decide whether a named filter is part of
// Askama or should refer to a local `filters` module. It should contain all the
// filters shipped with Askama, even the optional ones (since optional inclusion
// in the const vector based on features seems impossible right now).
const BUILT_IN_FILTERS: &[&str] = &[
    "abs",
    "capitalize",
    "center",
    "e",
    "escape",
    "filesizeformat",
    "fmt",
    "format",
    "indent",
    "into_f64",
    "into_isize",
    "join",
    "linebreaks",
    "linebreaksbr",
    "paragraphbreaks",
    "lower",
    "lowercase",
    "safe",
    "trim",
    "truncate",
    "upper",
    "uppercase",
    "urlencode",
    "urlencode_strict",
    "wordcount",
    // optional features, reserve the names anyway:
    "json",
    "markdown",
    "yaml",
];
