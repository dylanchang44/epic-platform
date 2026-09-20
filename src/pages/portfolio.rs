use crate::portfolio::{LoadStatus, PortfolioSnapshot};
use leptos::prelude::*;
use leptos_meta::Title;
use rust_decimal::Decimal;

async fn current_portfolio() -> Result<PortfolioSnapshot, String> {
    #[cfg(feature = "ssr")]
    {
        let state = use_context::<std::sync::Arc<crate::state::PortfolioState>>()
            .ok_or("Portfolio state is unavailable")?;
        Ok(state.snapshot().await)
    }
    #[cfg(all(feature = "hydrate", not(feature = "ssr")))]
    {
        // Browser fetch futures stay on the browser thread; Resource requires Send.
        send_wrapper::SendWrapper::new(async {
            let response = gloo_net::http::Request::get("/api/portfolio")
                .send()
                .await
                .map_err(|e| e.to_string())?;
            if !response.ok() {
                return Err(format!(
                    "Cannot fetch portfolio (HTTP {})",
                    response.status()
                ));
            }
            response.json().await.map_err(|e| e.to_string())
        })
        .await
    }
    #[cfg(not(any(feature = "ssr", feature = "hydrate")))]
    {
        Err("Portfolio requires a server or browser build".into())
    }
}

async fn reload_portfolio() -> Result<PortfolioSnapshot, String> {
    #[cfg(all(feature = "hydrate", not(feature = "ssr")))]
    {
        let response = gloo_net::http::Request::post("/api/portfolio/reload")
            .send()
            .await
            .map_err(|e| e.to_string())?;
        if response.ok() {
            return current_portfolio().await;
        }
        if response.status() == 422 {
            return response.json().await.map_err(|e| e.to_string());
        }
        Err(format!(
            "Reload request failed (HTTP {})",
            response.status()
        ))
    }
    #[cfg(not(all(feature = "hydrate", not(feature = "ssr"))))]
    {
        Err("Reload is available after browser hydration".into())
    }
}

fn money(value: Option<Decimal>) -> String {
    value
        .map(|v| format!("${v:.2}"))
        .unwrap_or_else(|| "—".into())
}

#[component]
pub fn PortfolioPage() -> impl IntoView {
    // SSR serializes this result for hydration; later client navigation fetches GET.
    let initial = Resource::new_blocking(|| (), |_| current_portfolio());
    let updated = RwSignal::new(None::<PortfolioSnapshot>);
    let busy = RwSignal::new(false);
    let request_error = RwSignal::new(None::<String>);
    let reload = move |_| {
        busy.set(true);
        request_error.set(None);
        leptos::task::spawn_local(async move {
            match reload_portfolio().await {
                Ok(snapshot) => updated.set(Some(snapshot)),
                Err(error) => request_error.set(Some(error)),
            }
            busy.set(false);
        });
    };
    view! {
        <Title text="Portfolio | EPIC Platform"/>
        <p class="eyebrow">"01 / YOUR CAPITAL"</p>
        <h1>"Portfolio"</h1>
        <p class="intro">"Holdings from your local Schwab positions export."</p>
        <button type="button" on:click=reload disabled=move || busy.get()>
            {move || if busy.get() { "Reloading…" } else { "Reload local data" }}
        </button>
        <noscript>"Enable JavaScript to reload local data. The current server snapshot is displayed below."</noscript>
        <p role="alert">{move || request_error.get().map(|error| format!("{error}. Displayed holdings have not been refreshed."))}</p>
        <Suspense fallback=|| view! { <p>"Loading portfolio…"</p> }>
            {move || initial.get().map(|result| {
                match updated.get().map(Ok).unwrap_or(result) {
                    Ok(snapshot) => view! { <PortfolioContent snapshot/> }.into_any(),
                    Err(error) => view! { <p role="alert">{error}</p> }.into_any(),
                }
            })}
        </Suspense>
    }
}

#[component]
fn PortfolioContent(snapshot: PortfolioSnapshot) -> impl IntoView {
    let status = match snapshot.status {
        LoadStatus::NotLoaded => "Not loaded",
        LoadStatus::Loading => "Loading local data… Previous holdings may be shown.",
        LoadStatus::Loaded => "Local data loaded successfully",
        LoadStatus::Failed if snapshot.portfolio.is_some() => {
            "Reload failed — showing the last successful snapshot (stale)"
        }
        LoadStatus::Failed => "Portfolio unavailable — local data could not be loaded",
    };
    view! {
        <p>"Data directory: "<code>{snapshot.data_directory}</code></p>
        <div class="notice" role="status" aria-live="polite">{status}</div>
        {snapshot.error.map(|error| view! { <p role="alert">{error.message}</p> })}
        <p>"Last successful load (UTC): "{snapshot.last_successful_load.unwrap_or_else(|| "Never".into())}</p>
        {match snapshot.portfolio {
            None => view! { <section class="panel"><h2>"No holdings available"</h2><p>"Add a valid positions CSV to the configured directory, then press Reload local data."</p></section> }.into_any(),
            Some(portfolio) => view! {
                <section class="panel" aria-labelledby="positions-title">
                    <h2 id="positions-title">"Holdings"</h2>
                    <p>"Total exported holdings value: "<strong>{money(Some(portfolio.summary.total_market_value))}</strong>" · Positions: "{portfolio.summary.position_count}</p>
                    <p class="panel-note">"Signed USD market values from the export, including cash when present. This is not necessarily total account equity. Cash rows count as positions."</p>
                    <p>"Source: "{portfolio.source_file}</p>
                    <div class="table-scroll"><table>
                        <caption class="sr-only">"Loaded Schwab holdings"</caption>
                        <thead><tr><th>"Symbol"</th><th>"Description"</th><th>"Quantity"</th><th>"Price"</th><th>"Market value"</th><th>"Cost basis"</th><th>"Asset type"</th></tr></thead>
                        <tbody>{portfolio.positions.into_iter().map(|position| view! {
                            <tr><td class="mono">{position.symbol}</td><td>{position.description}</td>
                            <td>{position.quantity.map(|q| q.normalize().to_string()).unwrap_or_else(|| "—".into())}</td>
                            <td>{money(position.price)}</td><td>{money(Some(position.market_value))}</td>
                            <td>{money(position.cost_basis)}</td><td>{position.asset_type}</td></tr>
                        }).collect_view()}</tbody>
                    </table></div>
                </section>
            }.into_any(),
        }}
    }
}
