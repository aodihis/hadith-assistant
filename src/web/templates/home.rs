use topcoat::{
    Result,
    view::{component, view},
};

use super::layout::site_chrome;

#[component]
pub(crate) async fn home_view() -> Result {
    view! {
      site_chrome(
        <main>
            <section class="hero">
                <p class="eyebrow">"Source-grounded research"</p>
                <h1>"Explore Hadith with traceable references."</h1>
                <p class="lead">
                    "A single Rust application now serves the browser experience and the JSON API. "
                    "Search canonical records by collection, book, number, or grade."
                </p>
                <div class="actions">
                    <a class="button primary" href="/hadiths">
                        "Browse the collection"
                    </a>
                    <a class="button secondary" href="/api/collections">
                        "View the API"
                    </a>
                </div>
            </section>
            <section class="principles" aria-label="Application principles">
                <article>
                    <span>"01"</span>
                    <h2>"Traceable"</h2>
                    <p>
                        "Results retain collection, book, Hadith number, and stable record IDs."
                    </p>
                </article>
                <article>
                    <span>"02"</span>
                    <h2>"Canonical"</h2>
                    <p>
                        "PostgreSQL remains the source of truth; vector indexes never replace it."
                    </p>
                </article>
                <article>
                    <span>"03"</span>
                    <h2>"Full-stack Rust"</h2>
                    <p>
                        "Topcoat renders pages and serves typed API routes from one application."
                    </p>
                </article>
            </section>
        </main>
      )
    }
}
