//! Native-only HTTP boundary for portfolio commands, snapshots and rendered pages.
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
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
    let jobs_context = state.jobs.clone();
    Router::new()
        .route(
            "/",
            get(|| async { axum::response::Redirect::temporary("/portfolio") }),
        )
        .route("/health", get(|| async { "ok\n" }))
        .route("/ready", get(readiness))
        .route("/api/jobs", get(list_jobs))
        .route("/api/jobs/{id}", get(get_job))
        .route("/api/jobs/{id}/retry", post(retry_job))
        .route("/api/portfolio", get(get_portfolio))
        .route("/api/portfolio/reload", post(reload_portfolio))
        .route("/api/reviews", get(list_reviews).post(create_review))
        .route("/api/reviews/{id}", get(get_review))
        .route("/api/watchlist", get(watchlist_configuration))
        .route(
            "/api/watchlist/briefings",
            get(list_briefings).post(create_briefing),
        )
        .route("/api/watchlist/briefings/{id}", get(get_briefing))
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
                provide_context(jobs_context.clone());
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
    headers: HeaderMap,
) -> Result<
    (
        StatusCode,
        [(header::HeaderName, String); 1],
        Json<crate::jobs::domain::JobSubmission>,
    ),
    JobHttpError,
> {
    let key = headers
        .get("Idempotency-Key")
        .map(|v| v.to_str())
        .transpose()
        .map_err(|_| job_error(crate::jobs::domain::JobError::InvalidKey))?;
    let job = state
        .jobs
        .submit(&state.portfolio, key)
        .await
        .map_err(job_error)?;
    Ok((
        StatusCode::ACCEPTED,
        [(header::LOCATION, job.status_url.clone())],
        Json(job),
    ))
}

type JobHttpError = (StatusCode, Json<crate::jobs::domain::JobApiError>);
fn job_error(error: crate::jobs::domain::JobError) -> JobHttpError {
    use crate::jobs::domain::*;
    let status = match error {
        JobError::NotFound => StatusCode::NOT_FOUND,
        JobError::InvalidId | JobError::InvalidKey => StatusCode::BAD_REQUEST,
        JobError::InvalidTransition | JobError::NoPortfolio => StatusCode::CONFLICT,
        JobError::InvalidPortfolio => StatusCode::UNPROCESSABLE_ENTITY,
        JobError::Repository | JobError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        JobError::Watchlist { .. } => StatusCode::UNPROCESSABLE_ENTITY,
    };
    let message = error.to_string();
    (status, Json(JobApiError { error, message }))
}
fn job_id(id: String) -> Result<i64, JobHttpError> {
    id.parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .ok_or_else(|| job_error(crate::jobs::domain::JobError::InvalidId))
}
async fn list_jobs(
    State(state): State<AppState>,
    Query(query): Query<JobQuery>,
) -> Result<Json<Vec<crate::jobs::domain::Job>>, JobHttpError> {
    state
        .jobs
        .repository()
        .map_err(job_error)?
        .recent_kind(query.kind)
        .await
        .map(Json)
        .map_err(job_error)
}
#[derive(serde::Deserialize)]
struct JobQuery {
    kind: Option<crate::jobs::domain::JobKind>,
}

async fn watchlist_configuration(
    State(state): State<AppState>,
) -> Json<crate::watchlist::domain::WatchlistConfiguration> {
    Json(state.jobs.watchlist().configuration())
}
async fn create_briefing(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<
    (
        StatusCode,
        [(header::HeaderName, String); 1],
        Json<crate::jobs::domain::JobSubmission>,
    ),
    JobHttpError,
> {
    let key = headers
        .get("Idempotency-Key")
        .map(|v| v.to_str())
        .transpose()
        .map_err(|_| job_error(crate::jobs::domain::JobError::InvalidKey))?;
    let job = state.jobs.submit_briefing(key).await.map_err(job_error)?;
    Ok((
        StatusCode::ACCEPTED,
        [(header::LOCATION, job.status_url.clone())],
        Json(job),
    ))
}
type BriefingHttpError = (StatusCode, Json<serde_json::Value>);
fn briefing_error(error: crate::watchlist::domain::WatchlistError) -> BriefingHttpError {
    use crate::watchlist::domain::WatchlistError;
    let status = match error {
        WatchlistError::NotFound => StatusCode::NOT_FOUND,
        WatchlistError::InvalidId => StatusCode::BAD_REQUEST,
        WatchlistError::Empty | WatchlistError::InvalidConfiguration => {
            StatusCode::UNPROCESSABLE_ENTITY
        }
        _ => StatusCode::SERVICE_UNAVAILABLE,
    };
    (
        status,
        Json(serde_json::json!({"message":error.to_string(),"error":error})),
    )
}
async fn list_briefings(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::watchlist::domain::BriefingHistoryEntry>>, BriefingHttpError> {
    state
        .jobs
        .watchlist()
        .repository()
        .map_err(briefing_error)?
        .history()
        .await
        .map(Json)
        .map_err(briefing_error)
}
async fn get_briefing(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<crate::watchlist::domain::SavedBriefing>, BriefingHttpError> {
    let id = id
        .parse::<i64>()
        .map_err(|_| briefing_error(crate::watchlist::domain::WatchlistError::InvalidId))?;
    state
        .jobs
        .watchlist()
        .repository()
        .map_err(briefing_error)?
        .get(id)
        .await
        .map(Json)
        .map_err(briefing_error)
}
async fn get_job(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<crate::jobs::domain::Job>, JobHttpError> {
    state
        .jobs
        .get(job_id(id)?)
        .await
        .map(Json)
        .map_err(job_error)
}
async fn retry_job(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<
    (
        StatusCode,
        [(header::HeaderName, String); 1],
        Json<crate::jobs::domain::JobSubmission>,
    ),
    JobHttpError,
> {
    let job = state.jobs.retry(job_id(id)?).await.map_err(job_error)?;
    Ok((
        StatusCode::ACCEPTED,
        [(header::LOCATION, job.status_url.clone())],
        Json(job),
    ))
}
async fn readiness(
    State(state): State<AppState>,
) -> (StatusCode, Json<crate::jobs::domain::Readiness>) {
    let migrations_completed = state.review.initialization_error().is_none()
        && state.research.initialization_error().is_none();
    let job_repository_usable = state.jobs.usable().await;
    let worker_started = state.jobs.worker_started();
    let ready = migrations_completed && job_repository_usable && worker_started;
    (
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(crate::jobs::domain::Readiness {
            ready,
            migrations_completed,
            job_repository_usable,
            worker_started,
        }),
    )
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
