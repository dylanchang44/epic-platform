//! Internal research boundary: shared domain, native-only storage/source/service.
pub mod domain;
#[cfg(feature = "ssr")]
pub mod repository;
#[cfg(feature = "ssr")]
pub mod service;
#[cfg(feature = "ssr")]
pub mod source;
