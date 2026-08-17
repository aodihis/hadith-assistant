use topcoat::{
    Result,
    asset::Asset,
    view::{View, component, view},
};

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
            </body>
        </html>
    }
}
