//! UI shared by the native server and the browser's WebAssembly build.
pub mod app;
pub mod jobs;
mod pages;
pub mod portfolio;
pub mod research;
pub mod review;
pub mod symbol;

#[cfg(feature = "ssr")]
pub mod config;
#[cfg(feature = "ssr")]
pub mod state;

#[cfg(feature = "ssr")]
pub mod server;

#[cfg(feature = "hydrate")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub fn hydrate() {
    console_error_panic_hook::set_once();
    leptos::mount::hydrate_body(app::App);
}
