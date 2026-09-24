use crate::{portfolio::LoadStatus, research::domain::*};
use leptos::prelude::*;
use leptos_meta::Title;

async fn current_research() -> Result<HoldingsResearch, String> {
    #[cfg(feature = "ssr")]
    {
        let portfolio = use_context::<std::sync::Arc<crate::state::PortfolioState>>()
            .ok_or("Portfolio state is unavailable")?;
        let research = use_context::<std::sync::Arc<crate::research::service::ResearchService>>()
            .ok_or("Research state is unavailable")?;
        Ok(research.holdings(&portfolio).await)
    }
    #[cfg(all(feature = "hydrate", not(feature = "ssr")))]
    {
        send_wrapper::SendWrapper::new(async {
            let response = gloo_net::http::Request::get("/api/research/holdings")
                .send()
                .await
                .map_err(|_| "Cannot reach EPIC. Check that the server is running.".to_string())?;
            if !response.ok() {
                return Err(format!("Research read failed (HTTP {})", response.status()));
            }
            response
                .json()
                .await
                .map_err(|_| "EPIC returned an unexpected research response.".into())
        })
        .await
    }
    #[cfg(not(any(feature = "ssr", feature = "hydrate")))]
    {
        Err("Research requires a server or browser build".into())
    }
}

async fn refresh_company(symbol: String) -> Result<RefreshResponse, String> {
    #[cfg(all(feature = "hydrate", not(feature = "ssr")))]
    {
        // symbol comes from the validated registry, never arbitrary browser input.
        let response =
            gloo_net::http::Request::post(&format!("/api/research/companies/{symbol}/refresh"))
                .send()
                .await
                .map_err(|_| {
                    "Cannot reach EPIC. Displayed research has not been refreshed.".to_string()
                })?;
        // Failed refreshes also carry a typed body and any older saved snapshot.
        response.json().await.map_err(|_| {
            format!(
                "Unexpected refresh response (HTTP {}). Displayed research has not been refreshed.",
                response.status()
            )
        })
    }
    #[cfg(not(all(feature = "hydrate", not(feature = "ssr"))))]
    {
        let _ = symbol;
        Err("Refresh is available after browser hydration".into())
    }
}

#[component]
pub fn ResearchPage() -> impl IntoView {
    let initial = Resource::new_blocking(|| (), |_| current_research());
    view! {
        <Title text="Research | EPIC Platform"/>
        <p class="eyebrow">"02 / YOUR UNDERSTANDING"</p>
        <h1>"Research"</h1>
        <p class="intro">"Saved company research for your holdings. Refresh one company when you need a new source check."</p>
        <div class="notice">"Source: Stock Analysis public quarterly results. Research is stored locally in EPIC. No automatic refresh."</div>
        <noscript>"Enable JavaScript for company refresh controls. Saved research is displayed below."</noscript>
        <Suspense fallback=|| view! { <p role="status">"Reading saved research…"</p> }>
            {move || initial.get().map(|result| match result {
                Ok(snapshot) => view! { <ResearchContent snapshot/> }.into_any(),
                Err(error) => view! { <p role="alert">{error}</p> }.into_any(),
            })}
        </Suspense>
    }
}

#[component]
fn ResearchContent(snapshot: HoldingsResearch) -> impl IntoView {
    let busy = RwSignal::new(None::<String>);
    let stale = snapshot.portfolio_available && snapshot.portfolio_status != LoadStatus::Loaded;
    view! {
        {snapshot.repository_error.map(|error| view! { <p role="alert">{error.to_string()}</p> })}
        {stale.then(|| view! { <p role="status">"Using the last successfully loaded portfolio. Visit Portfolio to check its current load status."</p> })}
        {if !snapshot.portfolio_available {
            view! { <section class="panel"><h2>"Load your portfolio first"</h2><p>"Open "<a href="/portfolio">"Portfolio"</a>" and load the local Schwab data to see holding-aware research."</p></section> }.into_any()
        } else {
            view! {
                <p class="panel-note">"Exact symbol matches against the Stage 3 company registry. Other stocks are unmatched; funds, options and cash are unsupported. Dates describe saved facts, not live market data."</p>
                <div class="research-cards">{snapshot.holdings.into_iter().map(|row| view! { <ResearchCard row busy/> }).collect_view()}</div>
            }.into_any()
        }}
    }
}

#[component]
fn ResearchCard(row: HoldingResearch, busy: RwSignal<Option<String>>) -> impl IntoView {
    let company = RwSignal::new(row.research);
    let feedback = RwSignal::new(None::<String>);
    let request_error = RwSignal::new(None::<String>);
    let symbol = company.get_untracked().symbol;
    let busy_symbol = symbol.clone();
    let refresh = move |_| {
        let symbol = symbol.clone();
        busy.set(Some(symbol.clone()));
        feedback.set(None);
        request_error.set(None);
        leptos::task::spawn_local(async move {
            match refresh_company(symbol).await {
                Ok(mut response) => {
                    // A repository read error must not erase an already displayed snapshot.
                    if response.company.snapshot.is_none()
                        && company.get_untracked().snapshot.is_some()
                    {
                        response.company.snapshot = company.get_untracked().snapshot;
                    }
                    feedback.set(Some(match response.outcome {
                        RefreshOutcome::Saved => "New earnings snapshot saved.",
                        RefreshOutcome::Unchanged => "Source checked. This earnings period is already saved; its original facts and retrieval date are unchanged.",
                        RefreshOutcome::Failed => "Refresh failed. Any saved research is shown below.",
                    }.into()));
                    company.set(response.company);
                }
                Err(error) => request_error.set(Some(error)),
            }
            busy.set(None);
        });
    };
    let name = company
        .get_untracked()
        .company
        .map(|c| c.name)
        .unwrap_or(row.holding.description);
    view! {
        <section class="panel research-card" data-symbol=row.holding.held_symbol.clone()>
            <div class="panel-heading"><h2>{name}</h2><span class="tag">{row.holding.held_symbol.clone()}</span></div>
            <p>"Exported market value: "<strong>{format!("${:.2}", row.holding.market_value)}</strong></p>
            <Show when=move || company.get().company.is_some()>
                <button type="button" on:click=refresh.clone() disabled=move || busy.get().is_some()>
                    {let symbol = busy_symbol.clone(); move || if busy.get().as_ref() == Some(&symbol) { "Refreshing…" } else { "Refresh company" }}
                </button>
            </Show>
            <p role="status" aria-live="polite">{move || feedback.get()}</p>
            <p role="alert">{move || request_error.get()}</p>
            {move || view! { <CompanyContent company=company.get()/> }}
        </section>
    }.into_any()
}

#[component]
fn CompanyContent(company: CompanyView) -> impl IntoView {
    let status = match company.status {
        ResearchStatus::Available => "Research available",
        ResearchStatus::NeverRefreshed => "Never refreshed — no saved research",
        ResearchStatus::Unmatched => "Unmatched — no company in the Stage 3 registry",
        ResearchStatus::Unsupported => "Unsupported instrument — no company lookup attempted",
        ResearchStatus::RepositoryFailure => {
            "Research database unavailable — any displayed snapshot may be stale"
        }
    };
    view! {
        <p>{status}</p>
        {company.error.map(|error| view! { <p role="alert">{error.to_string()}</p> })}
        {company.refresh_error.map(|error| view! { <p role="alert">"Last refresh: "{error.to_string()}</p> })}
        {company.last_refresh_at.map(|date| view! { <p>"Last refresh attempt (UTC): "{date}</p> })}
        {company.snapshot.map(|snapshot| view! {
            <dl class="briefing">
                <div><dt>"Earnings period"</dt><dd>{snapshot.period.label}" · ended "{snapshot.period.ended_on}</dd></div>
                <div><dt>"Reported revenue (USD)"</dt><dd>{format!("${:.2}", snapshot.revenue)}</dd></div>
                <div><dt>"Diluted EPS (USD)"</dt><dd>{format!("${:.2}", snapshot.diluted_eps)}</dd></div>
                <div><dt>"Source updated (UTC)"</dt><dd>{snapshot.source_updated_at}</dd></div>
                <div><dt>"Snapshot retrieved (UTC)"</dt><dd>{snapshot.retrieved_at}</dd></div>
                <div><dt>"Sources"</dt><dd><ul>{snapshot.sources.into_iter().map(|source| view! {
                    <li><a href=source.url target="_blank" rel="noopener noreferrer">{source.label}</a></li>
                }).collect_view()}</ul></dd></div>
            </dl>
        })}
    }.into_any()
}
