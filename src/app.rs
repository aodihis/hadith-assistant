use topcoat::{
    Result,
    asset::{Asset, AssetBundle, RouterBuilderAssetExt, asset},
    router::{Router, Slot, layout, page},
    view::view,
};

use crate::application::AppServices;

const STYLES: Asset = asset!("assets/app.css");

pub fn router(services: AppServices) -> std::io::Result<Router> {
    Ok(router_with_assets(services, AssetBundle::load()?))
}

fn router_with_assets(services: AppServices, assets: AssetBundle) -> Router {
    topcoat::router::module_router!()
        .app_context(services)
        .assets(assets)
        .build()
}

#[layout]
async fn root_layout(slot: Slot<'_>) -> Result {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <meta
                    name="description"
                    content="A source-grounded Hadith research assistant"
                >
                <title>"Hadith Assistant"</title>
                <link rel="stylesheet" href=(STYLES)>
                topcoat::dev::script()
            </head>
            <body>
                <header class="site-header">
                    <a class="brand" href="/">"Hadith Assistant"</a>
                    <nav aria-label="Primary navigation">
                        <a href="/">"Home"</a>
                        <a href="/hadiths">"Browse Hadiths"</a>
                        <a href="/api/health">"API health"</a>
                    </nav>
                </header>
                (slot.await?)
                <footer>
                    "Canonical text remains in PostgreSQL. Every result preserves its source reference."
                </footer>
            </body>
        </html>
    }
}

#[page]
async fn home() -> Result {
    view! {
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
    }
}

#[cfg(test)]
mod tests {
    use sqlx::postgres::PgPoolOptions;

    use super::*;

    #[tokio::test]
    async fn module_router_builds_without_route_conflicts() {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://postgres:postgres@localhost/hadiths")
            .expect("test database URL should parse");

        router_with_assets(AppServices::new(pool), AssetBundle::empty());
    }
}
