use leptos::prelude::*;
use leptos_meta::Title;

#[component]
pub fn PortfolioPage() -> impl IntoView {
    view! {
        <Title text="Portfolio | EPIC Platform"/>
        <p class="eyebrow">"01 / YOUR CAPITAL"</p>
        <h1>"Portfolio"</h1>
        <p class="intro">"A place to understand what you own and how it changes."</p>
        <div class="notice">"Demo data only. These positions are fictional."</div>
        <section class="panel" aria-labelledby="positions-title">
            <div class="panel-heading"><h2 id="positions-title">"Example positions"</h2><span class="tag">"Placeholder"</span></div>
            <div class="table-scroll">
                <table>
                    <caption class="sr-only">"Fictional positions for the Stage 1 preview"</caption>
                    <thead><tr><th scope="col">"Holding"</th><th scope="col">"Symbol"</th><th scope="col">"Quantity"</th></tr></thead>
                    <tbody>
                        <tr><td>"Example Technology"</td><td class="mono">"DEMO-A"</td><td>"12"</td></tr>
                        <tr><td>"Example Index Fund"</td><td class="mono">"DEMO-B"</td><td>"25"</td></tr>
                    </tbody>
                </table>
            </div>
            <p class="panel-note">"Your imported positions and account snapshots will live here."</p>
        </section>
    }
}
