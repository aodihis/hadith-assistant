use topcoat::{
    Error, Result,
    context::{Cx, app_context},
    router::{page, query_params},
    router::error::{bad_request, internal_server_error, not_found},
    view::view,
};

use crate::application::AppServices;
use crate::domain::HadithSearch;
use crate::error::AppError;

#[topcoat::router::query_params(error = bad_request)]
struct BrowseQuery {
    collection: Option<String>,
    book_number: Option<String>,
    hadith_number: Option<String>,
    grade: Option<String>,
    limit: Option<i64>,
    offset: Option<i64>,
}

#[page]
async fn browse(cx: &Cx) -> Result {
    let query = query_params::<BrowseQuery>(cx)?;
    let collection = query.collection.clone().unwrap_or_default();
    let book_number = query.book_number.clone().unwrap_or_default();
    let hadith_number = query.hadith_number.clone().unwrap_or_default();
    let grade = query.grade.clone().unwrap_or_default();
    let limit = query.limit.unwrap_or(25);
    let offset = query.offset.unwrap_or_default();

    let services = app_context::<AppServices>(cx);
    let hadiths = services
        .hadiths
        .list(HadithSearch {
            collection: query.collection.clone(),
            book_number: query.book_number.clone(),
            hadith_number: query.hadith_number.clone(),
            grade: query.grade.clone(),
            limit,
            offset,
        })
        .await
        .map_err(page_error)?;

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

fn page_error(error: AppError) -> Error {
    match error {
        AppError::Validation(message) => bad_request(message).into(),
        AppError::NotFound(_) => not_found().into(),
        error => {
            tracing::error!(error = ?error, "page request failed");
            internal_server_error(error).into()
        }
    }
}
