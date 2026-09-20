//! Native-only HTTP boundary for portfolio commands, snapshots and rendered pages.
use axum::{
    Json, Router,
    extract::State,
    http::{StatusCode, header},
    routing::{get, post},
};
use leptos::prelude::*;
use leptos_axum::{LeptosRoutes, generate_route_list};

use crate::app::{App, shell};
use crate::{
    portfolio::{LoadStatus, PortfolioSnapshot},
    state::AppState,
};

pub fn router(state: AppState) -> Router {
    let options = state.leptos_options.clone();
    let routes = generate_route_list(App);
    let context = state.portfolio.clone();
    Router::new()
        .route(
            "/",
            get(|| async { axum::response::Redirect::temporary("/portfolio") }),
        )
        .route("/health", get(|| async { "ok\n" }))
        .route("/api/portfolio", get(get_portfolio))
        .route("/api/portfolio/reload", post(reload_portfolio))
        .leptos_routes_with_context(&state, routes, move || provide_context(context.clone()), {
            let options = options.clone();
            move || shell(options.clone())
        })
        .fallback(leptos_axum::file_and_error_handler::<AppState, _>(shell))
        .layer(axum::middleware::map_response(
            |mut response: axum::response::Response| async move {
                response.headers_mut().insert(
                    header::CACHE_CONTROL,
                    header::HeaderValue::from_static("no-store"),
                );
                response
            },
        ))
        .with_state(state)
}

async fn get_portfolio(State(state): State<AppState>) -> Json<PortfolioSnapshot> {
    Json(state.portfolio.snapshot().await)
}

async fn reload_portfolio(State(state): State<AppState>) -> (StatusCode, Json<PortfolioSnapshot>) {
    let snapshot = state.portfolio.reload().await;
    let status = if snapshot.status == LoadStatus::Failed {
        StatusCode::UNPROCESSABLE_ENTITY
    } else {
        StatusCode::OK
    };
    (status, Json(snapshot))
}
