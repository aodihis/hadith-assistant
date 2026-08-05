use topcoat::{
    Result,
    view::{component, view},
};

use crate::domain::Hadith;

#[component]
pub(crate) async fn hadith_list_view(
    hadiths: Vec<Hadith>,
    collection: String,
    book_number: String,
    hadith_number: String,
    grade: String,
    limit: i64,
) -> Result {
    view! {
        <main>
            <section class="page-heading">
                <p class="eyebrow">"Canonical records"</p>
                <h1>"Browse Hadiths"</h1>
                <p>
                    "Filter source records without changing or merging their canonical text."
                </p>
            </section>

            <form class="filters" action="/hadiths" method="get">
                <label>
                    "Collection"
                    <input
                        type="text"
                        name="collection"
                        value=(collection)
                        placeholder="e.g. bukhari"
                    >
                </label>
                <label>
                    "Book number"
                    <input
                        type="text"
                        name="book_number"
                        value=(book_number)
                        placeholder="e.g. 1"
                    >
                </label>
                <label>
                    "Hadith number"
                    <input
                        type="text"
                        name="hadith_number"
                        value=(hadith_number)
                        placeholder="e.g. 1"
                    >
                </label>
                <label>
                    "Grade"
                    <input
                        type="text"
                        name="grade"
                        value=(grade)
                        placeholder="e.g. Sahih"
                    >
                </label>
                <input type="hidden" name="limit" value=(limit)>
                <button class="button primary" type="submit">"Apply filters"</button>
                <a class="clear-link" href="/hadiths">"Clear"</a>
            </form>

            <div class="result-summary">
                <p>
                    <strong>(hadiths.len())</strong>
                    " records on this page"
                </p>
                <a href="/api/hadiths">"JSON endpoint"</a>
            </div>

            <section class="hadith-list" aria-label="Hadith records">
                if hadiths.is_empty() {
                    <div class="empty-state">
                        <h2>"No records found"</h2>
                        <p>"Try removing one or more filters."</p>
                    </div>
                } else {
                    for hadith in hadiths {
                        <article class="hadith-card">
                            <div class="hadith-meta">
                                <span class="collection">(hadith.collection)</span>
                                <span>
                                    "Book "
                                    (hadith.book_number)
                                    " · Hadith "
                                    (hadith.hadith_number)
                                </span>
                                <a href=(format!("/api/hadiths/{}", hadith.id))>
                                    "Record #"
                                    (hadith.id)
                                </a>
                            </div>
                            <p class="arabic" lang="ar" dir="rtl">
                                (hadith.arabic_text)
                            </p>
                            if let Some(english_text) = hadith.english_text {
                                <p class="translation">(english_text)</p>
                            }
                            <div class="grades">
                                <span>
                                    "Arabic grade: "
                                    (hadith.arabic_grade)
                                </span>
                                <span>
                                    "English grade: "
                                    (hadith.english_grade)
                                </span>
                            </div>
                        </article>
                    }
                }
            </section>
        </main>
    }
}
