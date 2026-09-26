use super::job_panel::JobPanel;
#[cfg(all(feature = "hydrate", not(feature = "ssr")))]
use super::job_panel::request;
use crate::{jobs::domain::*, watchlist::domain::*};
use leptos::prelude::*;
use leptos_meta::Title;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
struct PageData {
    configuration: WatchlistConfiguration,
    jobs: Vec<Job>,
    job_error: Option<String>,
    history: Vec<BriefingHistoryEntry>,
    latest: Option<SavedBriefing>,
}
async fn initial_page() -> Result<PageData, String> {
    #[cfg(feature = "ssr")]
    {
        let jobs = use_context::<std::sync::Arc<crate::jobs::service::JobService>>()
            .ok_or("Job state unavailable")?;
        let watchlist = jobs.watchlist();
        let repo = watchlist.repository().map_err(|e| e.to_string())?;
        let history = repo.history().await.map_err(|e| e.to_string())?;
        let latest = match history.first() {
            Some(entry) => Some(repo.get(entry.id).await.map_err(|e| e.to_string())?),
            None => None,
        };
        let recent = match jobs.repository() {
            Ok(repo) => repo.recent_kind(Some(JobKind::WatchlistBriefing)).await,
            Err(e) => Err(e),
        }
        .map_err(|e| e.to_string());
        Ok(PageData {
            configuration: watchlist.configuration(),
            history,
            latest,
            job_error: recent.as_ref().err().cloned(),
            jobs: recent.unwrap_or_default(),
        })
    }
    #[cfg(all(feature = "hydrate", not(feature = "ssr")))]
    {
        send_wrapper::SendWrapper::new(async {
            let configuration = request("/api/watchlist", false).await?;
            let history: Vec<BriefingHistoryEntry> =
                request("/api/watchlist/briefings", false).await?;
            let latest = match history.first() {
                Some(entry) => Some(open_briefing(entry.id).await?),
                None => None,
            };
            let recent = request::<Vec<Job>>("/api/jobs?kind=watchlist_briefing", false).await;
            Ok(PageData {
                configuration,
                history,
                latest,
                job_error: recent.as_ref().err().cloned(),
                jobs: recent.unwrap_or_default(),
            })
        })
        .await
    }
    #[cfg(not(any(feature = "ssr", feature = "hydrate")))]
    {
        Err("Watchlist requires a server or browser build".into())
    }
}
async fn open_briefing(id: i64) -> Result<SavedBriefing, String> {
    #[cfg(all(feature = "hydrate", not(feature = "ssr")))]
    {
        request(&format!("/api/watchlist/briefings/{id}"), false).await
    }
    #[cfg(not(all(feature = "hydrate", not(feature = "ssr"))))]
    {
        let _ = id;
        Err("Enable JavaScript to select a briefing".into())
    }
}
#[component]
pub fn WatchlistPage() -> impl IntoView {
    let initial = Resource::new_blocking(|| (), |_| initial_page());
    view! {
        <Title text="Watchlist | EPIC Platform"/>
        <p class="eyebrow">"04 / SAVED RESEARCH"</p><h1>"Watchlist"</h1>
        <p class="intro">"A factual briefing of saved company research, independent of your portfolio."</p>
        <div class="notice">"Briefings are immutable historical records. Creating one never fetches external research. Refresh company research separately through Research or its API."</div>
        <noscript>"Enable JavaScript to create or select a briefing. The latest saved result is shown below."</noscript>
        <Suspense fallback=|| view! { <p>"Reading saved briefings…"</p> }>
            {move || initial.get().map(|result| match result { Ok(data) => view! { <WatchlistWorkbench data/> }.into_any(),Err(e) => view! { <p role="alert">{e}</p> }.into_any() })}
        </Suspense>
    }
}
#[component]
fn WatchlistWorkbench(data: PageData) -> impl IntoView {
    let history = RwSignal::new(data.history);
    let selected = RwSignal::new(data.latest);
    let error = RwSignal::new(None::<String>);
    let open = Callback::new(move |id: i64| {
        error.set(None);
        leptos::task::spawn_local(async move {
            match open_briefing(id).await {
                Ok(saved) => {
                    history.try_update(|rows| {
                        rows.retain(|r| r.id != id);
                        rows.push(BriefingHistoryEntry {
                            id,
                            created_at: saved.document.created_at.clone(),
                            symbols: saved
                                .document
                                .entries
                                .iter()
                                .map(|e| e.symbol.clone())
                                .collect(),
                        });
                        rows.sort_by_key(|r| std::cmp::Reverse(r.id));
                    });
                    selected.try_set(Some(saved));
                }
                Err(e) => {
                    error.try_set(Some(e));
                }
            }
        });
    });
    let symbols = data
        .configuration
        .input
        .as_ref()
        .map(|input| {
            input
                .symbols()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    view! {
        <p>"Configured server watchlist: "{if symbols.is_empty() { "None".to_string() } else { symbols.clone() }}</p>
        {data.configuration.error.map(|e| view! { <p role="alert">{e.to_string()}</p> })}
        {symbols.is_empty().then(|| view! { <p>"Set WATCHLIST_SYMBOLS on the server and restart to configure a watchlist. Saved briefings and queued job inputs are unchanged."</p> })}
        <p class="panel-note">"Symbols use exact matching. An unrecognized symbol is retained as Never refreshed; this is not a claim that research coverage exists."</p>
        <JobPanel initial=data.jobs initial_error=data.job_error kind=JobKind::WatchlistBriefing on_result=Callback::new(move |result| { if let JobResult::WatchlistBriefing { id } = result { open.run(id); } })/>
        <p role="alert">{move || error.get()}</p>
        <section class="panel"><h2>"Briefing history"</h2>
            <ul class="review-history">{move || history.get().into_iter().map(|entry| view! {
                <li><button type="button" data-briefing-id=entry.id on:click=move |_| open.run(entry.id)>{format!("#{} · {} · {}",entry.id,entry.created_at,entry.symbols.iter().map(ToString::to_string).collect::<Vec<_>>().join(", "))}</button></li>
            }).collect_view()}</ul>
            <Show when=move || history.get().is_empty()><p>"No saved briefings yet."</p></Show>
        </section>
        {move || selected.get().map(|saved| view! { <BriefingContent saved/> })}
    }
}
#[component]
fn BriefingContent(saved: SavedBriefing) -> impl IntoView {
    view! {
        <section class="panel" id="selected-briefing" data-briefing-id=saved.id>
            <h2>{format!("Saved briefing #{}",saved.id)}</h2><p>"Created (UTC): "{saved.document.created_at}</p>
            {saved.document.entries.into_iter().map(|entry| view! {
                <article class="review-section"><h3>{format!("{} · {}",entry.symbol,entry.company_name.unwrap_or_else(|| "Company not in research registry".into()))}</h3>
                    {match entry.research {
                        None => view! { <p>"Never refreshed — no saved research snapshot was available when this briefing ran."</p> }.into_any(),
                        Some(saved) => { let s=saved.snapshot; view! {
                            <p>"Available · Research snapshot #"{saved.id}</p>
                            <dl class="briefing">
                                <div><dt>"Earnings period"</dt><dd>{s.period.label}" · "{s.period.ended_on}</dd></div>
                                <div><dt>"Revenue"</dt><dd>{format!("{} {}",s.currency,s.revenue)}</dd></div>
                                <div><dt>"Diluted EPS"</dt><dd>{s.diluted_eps.to_string()}</dd></div>
                                <div><dt>"Retrieved (UTC)"</dt><dd>{s.retrieved_at}</dd></div>
                                <div><dt>"Source updated (UTC)"</dt><dd>{s.source_updated_at}</dd></div>
                            </dl>
                            <ul>{s.sources.into_iter().map(|source| view! { <li><a href=source.url target="_blank" rel="noopener noreferrer">{source.label}</a></li> }).collect_view()}</ul>
                        }.into_any() }
                    }}
                </article>
            }).collect_view()}
        </section>
    }
}
