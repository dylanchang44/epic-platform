use leptos::prelude::*;
use leptos_meta::{MetaTags, Stylesheet, Title, provide_meta_context};
use leptos_router::{components::*, path};

use crate::pages::{PortfolioPage, ResearchPage, ReviewPage};

/// The document wrapper runs on the server; App is shared with the browser.
pub fn shell(options: LeptosOptions) -> impl IntoView {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8"/>
                <meta name="viewport" content="width=device-width, initial-scale=1"/>
                <AutoReload options=options.clone()/>
                <HydrationScripts options/>
                <MetaTags/>
            </head>
            <body><App/></body>
        </html>
    }
}

#[component]
pub fn App() -> impl IntoView {
    provide_meta_context();
    view! {
        <Stylesheet id="leptos" href="/pkg/epic-platform.css"/>
        <Title text="EPIC Platform"/>
        <Router>
            <a class="skip-link" href="#main">"Skip to content"</a>
            <div class="workbench">
                <header class="topbar">
                    <a class="brand" href="/portfolio">
                        <span class="brand-mark" aria-hidden="true">"ep"</span>
                        <span>"EPIC Platform"</span>
                    </a>
                    <span class="local-badge">"Local workspace"</span>
                </header>
                <nav aria-label="Main navigation">
                    <A href="/portfolio">"Portfolio"</A>
                    <A href="/research">"Research"</A>
                    <A href="/review">"Review"</A>
                </nav>
                <main id="main">
                    <Routes fallback=NotFound>
                        <Route path=path!("/portfolio") view=PortfolioPage/>
                        <Route path=path!("/research") view=ResearchPage/>
                        <Route path=path!("/review") view=ReviewPage/>
                    </Routes>
                </main>
                <footer>"Stage 01"<span>"Sample workspace · no account data connected"</span></footer>
            </div>
        </Router>
    }
}

#[component]
fn NotFound() -> impl IntoView {
    #[cfg(feature = "ssr")]
    if let Some(response) = use_context::<leptos_axum::ResponseOptions>() {
        response.set_status(axum::http::StatusCode::NOT_FOUND);
    }
    view! {
        <Title text="Page not found | EPIC Platform"/>
        <h1>"Page not found"</h1>
        <p>"This page is not part of your workspace."</p>
        <A href="/portfolio">"Back to Portfolio"</A>
    }
}
