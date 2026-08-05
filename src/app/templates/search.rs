use topcoat::{
    Result,
    view::{component, view},
};

use crate::domain::{Collection, RetrievedHadith};

pub(crate) struct SearchOutcome {
    pub submitted: bool,
    pub validation_error: Option<String>,
    pub service_error: bool,
    pub no_results: bool,
    pub results: Vec<RetrievedHadith>,
}

#[component]
pub(crate) async fn search_view(
    collections: Vec<Collection>,
    q: String,
    selected_collection: String,
    outcome: SearchOutcome,
) -> Result {
    let SearchOutcome {
        submitted,
        validation_error,
        service_error,
        no_results,
        results,
    } = outcome;

    view! {
        <main>
            <section class="page-heading">
                <p class="eyebrow">"Semantic search"</p>
                <h1>"Search Hadiths"</h1>
                <p>
                    "Ask a question or describe a topic. Results are matched by meaning, "
                    "then resolved back to their canonical source records."
                </p>
            </section>

            <form class="filters" action="/search" method="get">
                <label>
                    "Query"
                    <input
                        type="text"
                        name="q"
                        value=(q)
                        placeholder="e.g. the reward of intentions"
                    >
                </label>
                <label>
                    "Collection"
                    <select name="collection">
                        <option value="" selected=(selected_collection.is_empty())>
                            "All collections"
                        </option>
                        for collection in collections {
                            <option
                                value=(collection.slug.clone())
                                selected=(collection.slug == selected_collection)
                            >
                                (collection.name)
                            </option>
                        }
                    </select>
                </label>
                <button class="button primary" type="submit">"Search"</button>
                <a class="clear-link" href="/search">"Clear"</a>
            </form>

            if let Some(message) = validation_error {
                <p class="form-error">(message)</p>
            }

            if service_error {
                <div class="empty-state">
                    <h2>"Search is temporarily unavailable"</h2>
                    <p>"Please try again shortly."</p>
                </div>
            } else if !submitted {
                <div class="empty-state">
                    <h2>"Enter a question or topic to search Hadiths."</h2>
                </div>
            } else if no_results {
                <div class="empty-state">
                    <h2>"No matching Hadiths"</h2>
                    <p>"Try a different phrasing."</p>
                </div>
            } else {
                <section class="hadith-list" aria-label="Search results">
                    for hadith in results {
                        <article class="hadith-card">
                            <div class="hadith-meta">
                                <span class="collection">(hadith.collection)</span>
                                <span>
                                    "Book "
                                    (hadith.book_number)
                                    " · Hadith "
                                    (hadith.hadith_number)
                                </span>
                                <a href=(format!("/api/hadiths/{}", hadith.hadith_id))>
                                    "Record #"
                                    (hadith.hadith_id)
                                </a>
                            </div>
                            <p class="arabic" lang="ar" dir="rtl">
                                (hadith.arabic_text)
                            </p>
                            if let Some(english_text) = hadith.english_text {
                                <p class="translation">(english_text)</p>
                            }
                        </article>
                    }
                </section>
            }
        </main>
    }
}
