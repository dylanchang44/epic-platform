use leptos::prelude::*;
use leptos_meta::Title;

#[component]
pub fn ResearchPage() -> impl IntoView {
    view! {
        <Title text="Research | EPIC Platform"/>
        <p class="eyebrow">"02 / YOUR UNDERSTANDING"</p>
        <h1>"Research"</h1>
        <p class="intro">"Keep company results, open questions, and their sources together."</p>
        <div class="notice">"Sample briefing. No earnings or market data has been retrieved."</div>
        <section class="panel" aria-labelledby="company-title">
            <div class="panel-heading"><h2 id="company-title">"Example Technology"</h2><span class="tag">"DEMO-A"</span></div>
            <dl class="briefing">
                <div><dt>"Earnings snapshot"</dt><dd>"Awaiting a verified reporting period"</dd></div>
                <div><dt>"Research question"</dt><dd>"What changed in revenue, margins, and guidance?"</dd></div>
                <div><dt>"Source links"</dt><dd>"No sources connected"</dd></div>
            </dl>
            <p class="panel-note">"Dated snapshots and links to original disclosures will live here."</p>
        </section>
    }
}
