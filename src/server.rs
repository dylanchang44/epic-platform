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
    let review_context = state.review.clone();
    Router::new()
        .route(
            "/",
            get(|| async { axum::response::Redirect::temporary("/portfolio") }),
        )
        .route("/health", get(|| async { "ok\n" }))
        .route("/api/portfolio", get(get_portfolio))
        .route("/api/portfolio/reload", post(reload_portfolio))
        .route("/api/reviews", get(list_reviews).post(create_review))
        .route("/api/reviews/{id}", get(get_review))
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
                provide_context(review_context.clone());
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

type ReviewHttpError = (StatusCode, Json<crate::review::domain::ReviewApiError>);

fn review_error(error: crate::review::domain::ReviewError) -> ReviewHttpError {
    use crate::review::domain::ReviewError;
    let status = match error {
        ReviewError::NoPortfolio => StatusCode::CONFLICT,
        ReviewError::InvalidPortfolio => StatusCode::UNPROCESSABLE_ENTITY,
        ReviewError::NotFound => StatusCode::NOT_FOUND,
        ReviewError::InvalidId => StatusCode::BAD_REQUEST,
        ReviewError::Calculation => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::SERVICE_UNAVAILABLE,
    };
    let message = error.to_string();
    (
        status,
        Json(crate::review::domain::ReviewApiError { error, message }),
    )
}

async fn create_review(
    State(state): State<AppState>,
) -> Result<
    (
        StatusCode,
        [(header::HeaderName, String); 1],
        Json<crate::review::domain::SavedReview>,
    ),
    ReviewHttpError,
> {
    let review = state
        .review
        .create_portfolio_review(&state.portfolio, &state.research)
        .await
        .map_err(review_error)?;
    Ok((
        StatusCode::CREATED,
        [(header::LOCATION, format!("/api/reviews/{}", review.id))],
        Json(review),
    ))
}

async fn list_reviews(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::review::domain::ReviewHistoryEntry>>, ReviewHttpError> {
    state.review.history().await.map(Json).map_err(review_error)
}

async fn get_review(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<crate::review::domain::ReviewDetail>, ReviewHttpError> {
    let id = id
        .parse::<i64>()
        .map_err(|_| review_error(crate::review::domain::ReviewError::InvalidId))?;
    state
        .review
        .detail(id)
        .await
        .map(Json)
        .map_err(review_error)
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
