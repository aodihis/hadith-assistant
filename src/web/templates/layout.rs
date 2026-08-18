use topcoat::{
    Result,
    asset::Asset,
    view::{View, component, view},
};

/// The bare document every page shares: head, stylesheet, body.
///
/// Deliberately carries no header, nav, or footer. The root layout wraps every
/// page including `/chat`, which is a full-height surface with its own chrome —
/// so site furniture lives in `site_chrome`, which pages opt into, rather than
/// here where a page cannot opt out.
#[component]
pub(crate) async fn page_shell(styles: Asset, child: View) -> Result {
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
                <link rel="stylesheet" href=(styles)>
                topcoat::dev::script()
            </head>
            <body>
                (child)
            </body>
        </html>
    }
}

/// Site header, navigation, and footer.
///
/// Opted into per page, so a page that supplies its own chrome — the chat
/// surface — is not forced to render this above it.
#[component]
pub(crate) async fn site_chrome(child: View) -> Result {
    view! {
        <header class="site-header">
            <a class="brand" href="/">"Hadith Assistant"</a>
            <nav aria-label="Primary navigation">
                <a href="/">"Home"</a>
                <a href="/hadiths">"Browse Hadiths"</a>
                <a href="/chat">"Chat"</a>
                <a href="/api/health">"API health"</a>
            </nav>
        </header>
        (child)
        <footer>
            "Canonical text remains in PostgreSQL. Every result preserves its source reference."
        </footer>
    }
}
