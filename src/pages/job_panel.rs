use crate::jobs::domain::*;
use leptos::prelude::*;
#[cfg(all(feature = "hydrate", not(feature = "ssr")))]
use serde::Deserialize;

#[cfg(all(feature = "hydrate", not(feature = "ssr")))]
pub(super) async fn request<T: serde::de::DeserializeOwned>(
    path: &str,
    post: bool,
) -> Result<T, String> {
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
            .unwrap_or_else(|_| format!("EPIC request failed (HTTP {})", response.status())));
    }
    response
        .json()
        .await
        .map_err(|_| "EPIC returned an unexpected response.".into())
}

#[cfg(all(feature = "hydrate", not(feature = "ssr")))]
#[derive(Deserialize)]
struct ErrorMessage {
    message: String,
}

async fn submit_job(kind: JobKind, key: String) -> Result<JobSubmission, String> {
    #[cfg(all(feature = "hydrate", not(feature = "ssr")))]
    {
        request_with_key(
            match kind {
                JobKind::PortfolioReview => "/api/reviews",
                JobKind::WatchlistBriefing => "/api/watchlist/briefings",
            },
            true,
            Some(&key),
        )
        .await
    }
    #[cfg(not(all(feature = "hydrate", not(feature = "ssr"))))]
    {
        let _ = (kind, key);
        Err("Enable JavaScript to submit a job".into())
    }
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
fn show_result(job: &Job, on_result: Callback<JobResult>) {
    if let Some(result) = job.result.clone() {
        on_result.run(result);
    }
}

#[component]
pub(super) fn JobPanel(
    initial: Vec<Job>,
    initial_error: Option<String>,
    kind: JobKind,
    on_result: Callback<JobResult>,
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
                                show_result(&job, on_result);
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
            match submit_job(kind, key).await {
                Ok(submitted) => {
                    submission_key.try_set(None);
                    message.try_set(Some(format!(
                        "Job #{} accepted. You can close this page and return later.",
                        submitted.job_id
                    )));
                    match read_job(submitted.job_id).await {
                        Ok(job) => {
                            show_result(&job, on_result);
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
        <button id=if kind == JobKind::PortfolioReview { "create-review" } else { "create-briefing" } type="button" on:click=create disabled=move || busy.get()>
            {move || if busy.get() { "Submitting…" } else if submission_key.get().is_some() { "Retry submission with same key" } else if kind == JobKind::PortfolioReview { "Create portfolio review" } else { "Create briefing" }}
        </button>
        <p class="panel-note">{if kind == JobKind::PortfolioReview { "The review captures the portfolio and saved research when its job runs. A retry captures then-current inputs if no review was saved." } else { "The configured symbol list is saved at submission. Research is read when the job runs; retry keeps those symbols. No external refresh is performed." }}</p>
        <p role="status" aria-live="polite">{move || message.get()}</p>
        <p role="alert">{move || error.get()}</p>
        <section class="panel"><h2>{if kind == JobKind::PortfolioReview { "Recent review jobs" } else { "Recent briefing jobs" }}</h2>
            <p>"Up to 30 recent jobs are restored when you reopen this page. Active jobs update every two seconds."</p>
            <Show when=move || jobs.get().is_empty()><p>"No jobs yet."</p></Show>
            <ul class="review-history">{move || jobs.get().into_iter().map(|job| {
                let id = job.id;
                let can_retry = job.status.can_retry();
                let result = job.result.clone();
                let retry = move |_| {
                    busy.set(true); error.set(None);
                    leptos::task::spawn_local(async move {
                        match retry_job(id).await {
                            Ok(_) => match read_job(id).await {
                                Ok(job) => {
                                    show_result(&job, on_result);
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
                    {match job.input { JobInput::WatchlistBriefing { symbols } => Some(view! { <p>"Captured symbols: "{symbols.symbols().iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")}</p> }), JobInput::PortfolioReview => None }}
                    <p>{format!("Queued: {} · Started: {} · Completed: {}", job.queued_at, job.started_at.clone().unwrap_or_else(|| "—".into()), job.completed_at.clone().unwrap_or_else(|| "—".into()))}</p>
                    {job.error.map(|e| view! { <p role="alert">{e}</p> })}
                    {(!job.previous_failures.is_empty()).then(|| view! { <details><summary>"Previous failures / interruptions"</summary><ul>{job.previous_failures.into_iter().map(|f| view! { <li>{format!("Attempt {} · {:?} · {} · {}: {}", f.attempt, f.status, f.step.as_str(), f.completed_at, f.error)}</li> }).collect_view()}</ul></details> })}
                    {can_retry.then(|| view! { <button type="button" on:click=retry disabled=move || busy.get()>"Retry job"</button> })}
                    {result.map(|result| { let id=result.id(); view! { <button type="button" disabled=move || busy.get() on:click=move |_| on_result.run(result.clone())>{format!("Open saved result #{id}")}</button> } })}
                </li> }
            }).collect_view()}</ul>
        </section>
    }.into_any()
}
