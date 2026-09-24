//! Native-only HTTP boundary for portfolio commands, snapshots and rendered pages.
use axum::{
    Json, Router,
    extract::{Path, State},
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
    let research_context = state.research.clone();
    Router::new()
        .route(
            "/",
            get(|| async { axum::response::Redirect::temporary("/portfolio") }),
        )
        .route("/health", get(|| async { "ok\n" }))
        .route("/api/portfolio", get(get_portfolio))
        .route("/api/portfolio/reload", post(reload_portfolio))
        .route("/api/research/holdings", get(get_research_holdings))
        .route(
            "/api/research/companies/{symbol}",
            get(get_research_company),
        )
        .route(
            "/api/research/companies/{symbol}/refresh",
            post(refresh_research_company),
        )
        .leptos_routes_with_context(
            &state,
            routes,
            move || {
                provide_context(context.clone());
                provide_context(research_context.clone());
            },
            {
                let options = options.clone();
                move || shell(options.clone())
            },
        )
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

async fn get_research_holdings(
    State(state): State<AppState>,
) -> Json<crate::research::domain::HoldingsResearch> {
    Json(state.research.holdings(&state.portfolio).await)
}

async fn get_research_company(
    State(state): State<AppState>,
    Path(symbol): Path<String>,
) -> (StatusCode, Json<crate::research::domain::CompanyView>) {
    let company = state.research.company(&symbol).await;
    (research_status(company.error.as_ref()), Json(company))
}

async fn refresh_research_company(
    State(state): State<AppState>,
    Path(symbol): Path<String>,
) -> (StatusCode, Json<crate::research::domain::RefreshResponse>) {
    let result = state.research.refresh(&symbol).await;
    let error = result
        .company
        .refresh_error
        .as_ref()
        .or(result.company.error.as_ref());
    (research_status(error), Json(result))
}

fn research_status(error: Option<&crate::research::domain::ResearchError>) -> StatusCode {
    use crate::research::domain::ResearchError;
    match error {
        None => StatusCode::OK,
        Some(ResearchError::InvalidSymbol) => StatusCode::BAD_REQUEST,
        Some(ResearchError::UnsupportedSymbol) => StatusCode::NOT_FOUND,
        Some(ResearchError::Busy) => StatusCode::CONFLICT,
        Some(ResearchError::Timeout) => StatusCode::GATEWAY_TIMEOUT,
        Some(
            ResearchError::SourceUnavailable
            | ResearchError::SourceHttp { .. }
            | ResearchError::MalformedSource,
        ) => StatusCode::BAD_GATEWAY,
        Some(_) => StatusCode::SERVICE_UNAVAILABLE,
    }
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
