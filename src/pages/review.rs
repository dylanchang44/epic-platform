use leptos::prelude::*;
use leptos_meta::Title;

#[component]
pub fn ReviewPage() -> impl IntoView {
    // A small, disposable interaction makes hydration observable without a backend workflow.
    let show_prompt = RwSignal::new(false);
    view! {
        <Title text="Review | EPIC Platform"/>
        <p class="eyebrow">"03 / YOUR PROCESS"</p>
        <h1>"Review"</h1>
        <p class="intro">"Make room to reflect on decisions, outcomes, and what to study next."</p>
        <div class="notice">"Sample review. Nothing entered or selected here is saved."</div>
        <section class="panel" aria-labelledby="review-title">
            <div class="panel-heading"><h2 id="review-title">"A moment to reflect"</h2><span class="tag">"Preview"</span></div>
            <p>"A future home for trading analytics and your investment review."</p>
            <button type="button" aria-expanded=move || show_prompt.get().to_string()
                aria-controls="review-prompt" on:click=move |_| show_prompt.update(|shown| *shown = !*shown)>
                {move || if show_prompt.get() { "Hide sample prompt" } else { "Show sample prompt" }}
            </button>
            <p id="review-prompt" class="prompt" hidden=move || !show_prompt.get()>
                "Which assumption behind a recent decision would you check again?"
            </p>
            <noscript>"Enable JavaScript to open the sample prompt."</noscript>
            <p class="panel-note">"This preview resets when you leave the page or reload."</p>
        </section>
    }
}
