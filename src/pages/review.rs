use crate::jobs::domain::*;
use crate::{portfolio::LoadStatus, review::domain::*};
use leptos::prelude::*;
use leptos_meta::Title;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
struct ReviewPageData {
    history: Vec<ReviewHistoryEntry>,
    latest: Option<ReviewDetail>,
    portfolio_available: bool,
    jobs: Vec<Job>,
    job_error: Option<String>,
}

#[cfg(all(feature = "hydrate", not(feature = "ssr")))]
async fn request<T: serde::de::DeserializeOwned>(path: &str, post: bool) -> Result<T, String> {
    request_with_key(path, post, None).await
}
#[cfg(all(feature = "hydrate", not(feature = "ssr")))]
async fn request_with_key<T: serde::de::DeserializeOwned>(
    path: &str,
    post: bool,
    key: Option<&str>,
) -> Result<T, String> {
    let mut request = if post {
        gloo_net::http::Request::post(path)
    } else {
        gloo_net::http::Request::get(path)
    };
    if let Some(key) = key {
        request = request.header("Idempotency-Key", key);
    }
    let response = request.send().await.map_err(|_| "Cannot reach EPIC. Saved jobs continue on the server; check recent jobs or repeat the submission with the same key.".to_string())?;
    if !response.ok() {
        return Err(response
            .json::<ErrorMessage>()
            .await
            .map(|e| e.message)
            .unwrap_or_else(|_| format!("Review request failed (HTTP {})", response.status())));
    }
    response
        .json()
        .await
        .map_err(|_| "EPIC returned an unexpected review response.".into())
}

#[cfg(all(feature = "hydrate", not(feature = "ssr")))]
#[derive(Deserialize)]
struct ErrorMessage {
    message: String,
}

async fn initial_page() -> Result<ReviewPageData, String> {
    #[cfg(feature = "ssr")]
    {
        let review = use_context::<std::sync::Arc<crate::review::service::ReviewService>>()
            .ok_or("Review state unavailable")?;
        let portfolio = use_context::<std::sync::Arc<crate::state::PortfolioState>>()
            .ok_or("Portfolio state unavailable")?;
        let portfolio_available = portfolio.snapshot().await.portfolio.is_some();
        let jobs = use_context::<std::sync::Arc<crate::jobs::service::JobService>>()
            .ok_or("Job state unavailable")?;
        let jobs = jobs.recent().await.map_err(|e| e.to_string());
        let history = review.history().await.map_err(|e| e.to_string())?;
        let latest = match history.first() {
            Some(entry) => Some(review.detail(entry.id).await.map_err(|e| e.to_string())?),
            None => None,
        };
        Ok(ReviewPageData {
            history,
            latest,
            portfolio_available,
            job_error: jobs.as_ref().err().cloned(),
            jobs: jobs.unwrap_or_default(),
        })
    }
    #[cfg(all(feature = "hydrate", not(feature = "ssr")))]
    {
        send_wrapper::SendWrapper::new(async {
            let history: Vec<ReviewHistoryEntry> = request("/api/reviews", false).await?;
            let latest = match history.first() {
                Some(entry) => Some(request(&format!("/api/reviews/{}", entry.id), false).await?),
                None => None,
            };
            // Existing reviews remain readable if current portfolio cannot be read.
            let portfolio_available =
                request::<crate::portfolio::PortfolioSnapshot>("/api/portfolio", false)
                    .await
                    .is_ok_and(|p| p.portfolio.is_some());
            let jobs = request::<Vec<Job>>("/api/jobs", false).await;
            Ok(ReviewPageData {
                history,
                latest,
                portfolio_available,
                job_error: jobs.as_ref().err().cloned(),
                jobs: jobs.unwrap_or_default(),
            })
        })
        .await
    }
    #[cfg(not(any(feature = "ssr", feature = "hydrate")))]
    {
        Err("Review requires a server or browser build".into())
    }
}

async fn create_review(key: String) -> Result<JobSubmission, String> {
    #[cfg(all(feature = "hydrate", not(feature = "ssr")))]
    {
        request_with_key("/api/reviews", true, Some(&key)).await
    }
    #[cfg(not(all(feature = "hydrate", not(feature = "ssr"))))]
    {
        let _ = key;
        Err("Enable JavaScript to create a review".into())
    }
}

async fn open_review(id: i64) -> Result<ReviewDetail, String> {
    #[cfg(all(feature = "hydrate", not(feature = "ssr")))]
    {
        request(&format!("/api/reviews/{id}"), false).await
    }
    #[cfg(not(all(feature = "hydrate", not(feature = "ssr"))))]
    {
        let _ = id;
        Err("Enable JavaScript to select a review".into())
    }
}

fn percent(value: Option<Decimal>) -> String {
    value
        .map(|v| format!("{v:.2}%"))
        .unwrap_or_else(|| "N/A (no positive denominator)".into())
}
fn points(value: Option<Decimal>) -> String {
    value
        .map(|v| format!("{v:+.2} percentage points"))
        .unwrap_or_else(|| "N/A".into())
}

#[component]
pub fn ReviewPage() -> impl IntoView {
    let initial = Resource::new_blocking(|| (), |_| initial_page());
    view! {
        <Title text="Review | EPIC Platform"/>
        <p class="eyebrow">"03 / YOUR PROCESS"</p><h1>"Review"</h1>
        <p class="intro">"Save a factual record of your portfolio and its research coverage."</p>
        <div class="notice">"Each saved review is an immutable historical record. Later portfolio reloads and research refreshes do not change it."</div>
        <noscript>"Enable JavaScript to create or select a review. The latest saved review is shown below."</noscript>
        <Suspense fallback=|| view! { <p>"Reading saved reviews…"</p> }>
            {move || initial.get().map(|result| match result {
                Ok(data) => view! { <ReviewWorkbench data/> }.into_any(),
                Err(error) => view! { <p role="alert">{error}</p> }.into_any(),
            })}
        </Suspense>
    }
}

#[component]
fn ReviewWorkbench(data: ReviewPageData) -> impl IntoView {
    let history = RwSignal::new(data.history);
    let selected = RwSignal::new(data.latest);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(None::<String>);
    let message = RwSignal::new(None::<String>);
    view! {
        {(!data.portfolio_available).then(|| view! { <p>"Load local Schwab data on "<a href="/portfolio">"Portfolio"</a>" before creating a review. Existing saved reviews remain available."</p> })}
        <ReviewJobs initial=data.jobs initial_error=data.job_error history selected/>
        <p role="status" aria-live="polite">{move || message.get()}</p>
        <p role="alert">{move || error.get()}</p>
        <p>"Latest saved review: "{move || history.get().first().map(|e| e.created_at.clone()).unwrap_or_else(|| "None yet".into())}</p>
        <section class="panel"><h2>"Review history"</h2>
            <p class="panel-note">"Newest saved first. Select a review to open its preserved inputs and facts."</p>
            <ul class="review-history">{move || history.get().into_iter().map(|entry| {
                let open = move |_| {
                    busy.set(true); error.set(None); message.set(None);
                    leptos::task::spawn_local(async move {
                        match open_review(entry.id).await {
                            Ok(detail) => selected.set(Some(detail)), Err(e) => error.set(Some(e)),
                        }
                        busy.set(false);
                    });
                };
                view! { <li><button type="button" data-review-id=entry.id on:click=open disabled=move || busy.get()>
                    {format!("#{} · {} · ${:.2} · {} positions", entry.id, entry.created_at, entry.summary.total_market_value, entry.summary.position_count)}
                </button></li> }
            }).collect_view()}</ul>
        </section>
        {move || selected.get().map(|detail| view! { <ReviewContent detail/> })}
    }.into_any()
}

async fn read_job(id: i64) -> Result<Job, String> {
    #[cfg(all(feature = "hydrate", not(feature = "ssr")))]
    {
        request(&format!("/api/jobs/{id}"), false).await
    }
    #[cfg(not(all(feature = "hydrate", not(feature = "ssr"))))]
    {
        let _ = id;
        Err("Enable JavaScript to check jobs".into())
    }
}
async fn retry_job(id: i64) -> Result<JobSubmission, String> {
    #[cfg(all(feature = "hydrate", not(feature = "ssr")))]
    {
        request(&format!("/api/jobs/{id}/retry"), true).await
    }
    #[cfg(not(all(feature = "hydrate", not(feature = "ssr"))))]
    {
        let _ = id;
        Err("Enable JavaScript to retry jobs".into())
    }
}
fn remember_job(jobs: RwSignal<Vec<Job>>, job: Job) {
    jobs.try_update(|rows| {
        rows.retain(|r| r.id != job.id);
        rows.push(job);
        rows.sort_by_key(|r| std::cmp::Reverse(r.id));
    });
}
async fn show_result(
    job: &Job,
    history: RwSignal<Vec<ReviewHistoryEntry>>,
    selected: RwSignal<Option<ReviewDetail>>,
) -> Result<(), String> {
    if let Some(JobResult::Review { id }) = job.result {
        let detail = open_review(id).await?;
        history.try_update(|rows| {
            rows.retain(|r| r.id != id);
            rows.push(ReviewHistoryEntry {
                id,
                created_at: detail.review.document.created_at.clone(),
                summary: detail.review.document.summary.clone(),
            });
            rows.sort_by_key(|r| std::cmp::Reverse(r.id));
        });
        selected.try_set(Some(detail));
    }
    Ok(())
}

#[component]
fn ReviewJobs(
    initial: Vec<Job>,
    initial_error: Option<String>,
    history: RwSignal<Vec<ReviewHistoryEntry>>,
    selected: RwSignal<Option<ReviewDetail>>,
) -> impl IntoView {
    let jobs = RwSignal::new(initial);
    let busy = RwSignal::new(false);
    let error = RwSignal::new(initial_error);
    let message = RwSignal::new(None::<String>);
    let submission_key = RwSignal::new(None::<String>);
    #[cfg(all(feature = "hydrate", not(feature = "ssr")))]
    {
        let polling = RwSignal::new(false);
        Effect::new(move |_| {
            if polling.get() || !jobs.get().iter().any(|job| job.status.is_active()) {
                return;
            }
            polling.set(true);
            leptos::task::spawn_local(async move {
                loop {
                    let Some(rows) = jobs.try_get_untracked() else {
                        return;
                    };
                    let active: Vec<_> = rows
                        .into_iter()
                        .filter(|job| job.status.is_active())
                        .collect();
                    if active.is_empty() {
                        polling.try_set(false);
                        break;
                    }
                    for old in active {
                        match read_job(old.id).await {
                            Ok(job) => {
                                error.try_set(None);
                                if let Err(e) = show_result(&job, history, selected).await {
                                    error.try_set(Some(e));
                                }
                                remember_job(jobs, job);
                            }
                            Err(e) => {
                                error.try_set(Some(e));
                            }
                        }
                        if jobs.try_get_untracked().is_none() {
                            return;
                        }
                    }
                    gloo_timers::future::TimeoutFuture::new(2000).await;
                }
            });
        });
    }
    let create = move |_| {
        if busy.get_untracked() {
            return;
        }
        busy.set(true);
        error.set(None);
        let key = submission_key.get_untracked().unwrap_or_else(|| {
            #[cfg(all(feature = "hydrate", not(feature = "ssr")))]
            {
                uuid::Uuid::new_v4().to_string()
            }
            #[cfg(not(all(feature = "hydrate", not(feature = "ssr"))))]
            {
                String::new()
            }
        });
        submission_key.set(Some(key.clone()));
        leptos::task::spawn_local(async move {
            match create_review(key).await {
                Ok(submitted) => {
                    submission_key.try_set(None);
                    message.try_set(Some(format!(
                        "Job #{} accepted. You can close this page and return later.",
                        submitted.job_id
                    )));
                    match read_job(submitted.job_id).await {
                        Ok(job) => {
                            if let Err(e) = show_result(&job, history, selected).await {
                                error.try_set(Some(e));
                            }
                            remember_job(jobs, job);
                        }
                        Err(e) => {
                            error.try_set(Some(format!(
                                "Job #{} was accepted. Reload the page to recover status. {e}",
                                submitted.job_id
                            )));
                        }
                    }
                }
                Err(e) => {
                    error.try_set(Some(e));
                }
            }
            busy.try_set(false);
        });
    };
    view! {
        <button id="create-review" type="button" on:click=create disabled=move || busy.get()>
            {move || if busy.get() { "Submitting…" } else if submission_key.get().is_some() { "Retry submission with same key" } else { "Create portfolio review" }}
        </button>
        <p class="panel-note">"The review captures the portfolio and saved research when its job runs. A manual retry captures then-current inputs if no review was saved."</p>
        <p role="status" aria-live="polite">{move || message.get()}</p>
        <p role="alert">{move || error.get()}</p>
        <section class="panel"><h2>"Recent review jobs"</h2>
            <p>"Up to 30 recent jobs are restored when you reopen this page. Active jobs update every two seconds."</p>
            <Show when=move || jobs.get().is_empty()><p>"No jobs yet."</p></Show>
            <ul class="review-history">{move || jobs.get().into_iter().map(|job| {
                let id = job.id;
                let can_retry = job.status.can_retry();
                let result_id = job.result.as_ref().map(|JobResult::Review { id }| *id);
                let retry = move |_| {
                    busy.set(true); error.set(None);
                    leptos::task::spawn_local(async move {
                        match retry_job(id).await {
                            Ok(_) => match read_job(id).await {
                                Ok(job) => {
                                    if let Err(e) = show_result(&job, history, selected).await { error.try_set(Some(e)); }
                                    remember_job(jobs, job);
                                },
                                Err(e) => { error.try_set(Some(e)); }
                            },
                            Err(e) => { error.try_set(Some(e)); }
                        }
                        busy.try_set(false);
                    });
                };
                view! { <li data-job-id=id>
                    <p><strong>{format!("Job #{id} · {:?} · attempt {}", job.status, job.attempt_count)}</strong>" · "{job.step.as_str()}</p>
                    <p>{format!("Queued: {} · Started: {} · Completed: {}", job.queued_at, job.started_at.clone().unwrap_or_else(|| "—".into()), job.completed_at.clone().unwrap_or_else(|| "—".into()))}</p>
                    {job.error.map(|e| view! { <p role="alert">{e}</p> })}
                    {(!job.previous_failures.is_empty()).then(|| view! { <details><summary>"Previous failures / interruptions"</summary><ul>{job.previous_failures.into_iter().map(|f| view! { <li>{format!("Attempt {} · {:?} · {} · {}: {}", f.attempt, f.status, f.step.as_str(), f.completed_at, f.error)}</li> }).collect_view()}</ul></details> })}
                    {can_retry.then(|| view! { <button type="button" on:click=retry disabled=move || busy.get()>"Retry job"</button> })}
                    {result_id.map(|review_id| view! { <button type="button" disabled=move || busy.get() on:click=move |_| {
                        leptos::task::spawn_local(async move {
                            match open_review(review_id).await { Ok(detail) => { selected.try_set(Some(detail)); }, Err(e) => { error.try_set(Some(e)); } }
                        });
                    }>{format!("Open review #{review_id}")}</button> })}
                </li> }
            }).collect_view()}</ul>
        </section>
    }.into_any()
}

#[component]
fn ReviewContent(detail: ReviewDetail) -> impl IntoView {
    let review = detail.review;
    let document = review.document;
    let summary = document.summary;
    let covered: Vec<_> = document
        .positions
        .iter()
        .filter(|p| p.coverage == CoverageStatus::Available)
        .cloned()
        .collect();
    let missing: Vec<_> = document
        .positions
        .iter()
        .filter(|p| {
            matches!(
                p.coverage,
                CoverageStatus::Missing | CoverageStatus::RepositoryFailure
            )
        })
        .cloned()
        .collect();
    let unmatched: Vec<_> = document
        .positions
        .iter()
        .filter(|p| p.coverage == CoverageStatus::Unmatched)
        .cloned()
        .collect();
    let unsupported: Vec<_> = document
        .positions
        .iter()
        .filter(|p| p.coverage == CoverageStatus::Unsupported)
        .cloned()
        .collect();
    view! {
        <section class="panel review-section" id="selected-review" data-review-id=review.id>
            <h2>{format!("Saved review #{}", review.id)}</h2>
            <p>"Created (UTC): "{document.created_at}</p>
            <p>"Portfolio loaded (UTC): "{document.portfolio_loaded_at}" · Source file: "{document.portfolio_source_file}</p>
            { (document.portfolio_load_status != LoadStatus::Loaded).then(|| view! { <p class="notice">"This review used the last successful portfolio snapshot while its load status was not Loaded."</p> }) }
            <p class="panel-note">"Portfolio captured: "{document.portfolio_captured_at}" · Research read completed: "{document.research_captured_at}</p>
            {document.research_error.map(|e| view! { <p role="alert">"Research could not be read when this review was created: "{e.to_string()}" Coverage records that failure; it does not claim the database was empty."</p> })}
            <dl class="briefing">
                <div><dt>"Total exported holdings value"</dt><dd>{format!("${:.2}", summary.total_market_value)}</dd></div>
                <div><dt>"Position count"</dt><dd>{summary.position_count}</dd></div>
                <div><dt>"Largest absolute position"</dt><dd>{format!("{} · signed value ${:.2} · {} of gross value", summary.largest_position.symbol, summary.largest_position.market_value, percent(summary.largest_position.gross_weight_percent))}</dd></div>
                <div><dt>"Top-three concentration"</dt><dd>{percent(summary.top_three_concentration_percent)}</dd></div>
                <div><dt>"Research coverage by count"</dt><dd>{format!("{} / {} supported positions · {}", summary.covered_position_count, summary.supported_position_count, percent(summary.coverage_count_percent))}</dd></div>
                <div><dt>"Research coverage by value"</dt><dd>{format!("${:.2} / ${:.2} supported gross value · {}", summary.covered_gross_market_value, summary.supported_gross_market_value, percent(summary.coverage_value_percent))}</dd></div>
            </dl>
            <p class="panel-note">"Total and position weights use signed exported values, including cash and shorts. Weights are unavailable when the total is zero or negative. Concentration uses absolute values of all positions. Coverage uses only registry-supported equities and their absolute values; unmatched equities and unsupported instruments are excluded. These are portfolio facts, not investment recommendations."</p>
        </section>
        {detail.comparison_error.map(|e| view! { <p role="alert">"Comparison unavailable: "{e.to_string()}</p> })}
        {match detail.comparison {
            Some(comparison) => view! { <ComparisonContent comparison/> }.into_any(),
            None => view! { <p>"No previous-review comparison is available."</p> }.into_any(),
        }}
        <ReviewPositions title="Holdings with saved research" positions=covered/>
        <ReviewPositions title="Supported holdings without captured research" positions=missing/>
        <ReviewPositions title="Unmatched equities (outside supported coverage)" positions=unmatched/>
        <ReviewPositions title="Unsupported instruments (excluded from coverage)" positions=unsupported/>
    }.into_any()
}

#[component]
fn ComparisonContent(comparison: ReviewComparison) -> impl IntoView {
    view! {
        <section class="panel review-section" id="review-comparison"><h2>{format!("Compared with review #{}", comparison.previous_id)}</h2>
            <dl class="briefing">
                <div><dt>"Exported value change"</dt><dd>{format!("${:+.2}", comparison.total_value_change)}</dd></div>
                <div><dt>"Position count change"</dt><dd>{format!("{:+}", comparison.position_count_change)}</dd></div>
                <div><dt>"Largest position"</dt><dd>{comparison.previous_largest_symbol}" → "{comparison.current_largest_symbol}</dd></div>
                <div><dt>"Concentration change"</dt><dd>{points(comparison.concentration_change_points)}</dd></div>
                <div><dt>"Count coverage change"</dt><dd>{points(comparison.coverage_count_change_points)}</dd></div>
                <div><dt>"Value coverage change"</dt><dd>{points(comparison.coverage_value_change_points)}</dd></div>
                <div><dt>"Added symbols"</dt><dd>{if comparison.added_symbols.is_empty() { "None".into() } else { comparison.added_symbols.join(", ") }}</dd></div>
                <div><dt>"Removed symbols"</dt><dd>{if comparison.removed_symbols.is_empty() { "None".into() } else { comparison.removed_symbols.join(", ") }}</dd></div>
            </dl>
            <p class="panel-note">"Differences between saved records. Value changes are not investment returns and are not adjusted for deposits, withdrawals or trades."</p>
        </section>
    }.into_any()
}

#[component]
fn ReviewPositions(title: &'static str, positions: Vec<ReviewPosition>) -> impl IntoView {
    view! {
        <section class="panel review-section"><h2>{title}" ("{positions.len()}")"</h2>
            {positions.is_empty().then(|| view! { <p>"None in this review."</p> })}
            {positions.into_iter().map(|position| view! { <div class="review-position">
                <h3>{position.symbol}" · "{position.description}</h3>
                <p>{if position.asset_type.is_empty() { "Instrument type not supplied".into() } else { position.asset_type }}
                    " · Quantity: "{position.quantity.map(|q| q.normalize().to_string()).unwrap_or_else(|| "Not supplied".into())}
                    " · Value: "{format!("${:.2}", position.market_value)}" · Weight: "{percent(position.weight_percent)}</p>
                {(position.coverage == CoverageStatus::RepositoryFailure).then(|| view! { <p>"Research repository was unavailable during capture."</p> })}
                {position.research.map(|research| view! {
                    <p>{research.snapshot.company.name}" · "{research.snapshot.period.label}" · Period ended "{research.snapshot.period.ended_on}</p>
                    <p>"Snapshot retrieved: "{research.snapshot.retrieved_at}" · Source updated: "{research.snapshot.source_updated_at}</p>
                    <p>"Age at creation: "{research.retrieval_age_seconds}" seconds since retrieval; "{research.source_age_seconds}" seconds since source update."</p>
                    <p class="panel-note">"Age is frozen at review creation; no stale/fresh threshold is applied. Negative ages indicate a future timestamp or clock discrepancy."</p>
                    <ul>{research.snapshot.sources.into_iter().map(|source| view! {
                        <li><a href=source.url target="_blank" rel="noopener noreferrer">{source.label}</a></li>
                    }).collect_view()}</ul>
                })}
            </div> }).collect_view()}
        </section>
    }.into_any()
}
