//! Native-only HTTP boundary. Future data access belongs behind this boundary.
use axum::{Router, routing::get};
use leptos::prelude::*;
use leptos_axum::{LeptosRoutes, generate_route_list};

use crate::app::{App, shell};

pub fn router(options: LeptosOptions) -> Router {
    let routes = generate_route_list(App);
    Router::new()
        .route(
            "/",
            get(|| async { axum::response::Redirect::temporary("/portfolio") }),
        )
        .route("/health", get(|| async { "ok\n" }))
        .leptos_routes(&options, routes, {
            let options = options.clone();
            move || shell(options.clone())
        })
        .fallback(leptos_axum::file_and_error_handler(shell))
        .with_state(options)
}
