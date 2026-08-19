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
pub(crate) async fn page_shell(styles: Asset, favicon: Asset, child: View) -> Result {
    view! {
        <!DOCTYPE html>
        <html lang="en">
            <head>
                <meta charset="utf-8">
                <meta name="viewport" content="width=device-width, initial-scale=1">
                <meta
                    name="description"
                    content="Search and ask across 44,896 hadith from 15 collections, each answer cited to its narration"
                >
                <title>"Sanad · Ask the Sunnah"</title>
                topcoat::font::link(font: crate::web::AMIRI)
                topcoat::font::link(font: crate::web::INTER)
                <link rel="icon" type="image/svg+xml" href=(favicon)>
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
            <a class="brand" href="/">
                <span class="brand-mark" aria-hidden="true">"۞"</span>
                "Sanad"
            </a>
            <nav aria-label="Primary navigation">
                <a href="/">"Home"</a>
                <a href="/hadiths">"Browse"</a>
                <a href="/chat">"Ask"</a>
            </nav>
        </header>
        (child)
        <footer>
            "Hadith text, translations, and gradings courtesy of "
            <a href="https://sunnah.com/" rel="noopener">"sunnah.com"</a>
            ". Every result keeps its source reference so it can be verified there. "
            "A study companion for reflection, not a substitute for a qualified scholar."
        </footer>
    }
}
