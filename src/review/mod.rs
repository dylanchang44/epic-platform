//! Saved portfolio reviews: copied inputs, deterministic facts, independent storage.
pub mod calculations;
pub mod domain;
#[cfg(feature = "ssr")]
pub mod repository;
#[cfg(feature = "ssr")]
pub mod service;
