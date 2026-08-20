use topcoat::{
    Result,
    view::{component, view},
};

use crate::domain::{Collection, Hadith};
use crate::text::to_plain_text;

use super::layout::site_chrome;

pub(crate) struct HadithPage {
    pub hadiths: Vec<Hadith>,
    pub total: i64,
    pub limit: i64,
    pub offset: i64,
    pub page_sizes: Vec<i64>,
    pub collections: Vec<Collection>,
    pub book_numbers: Vec<String>,
    pub grades: Vec<String>,
    pub selected_collection: String,
    pub selected_book_number: String,
    pub selected_grade: String,
    pub hadith_number: String,
}

impl HadithPage {
    /// Builds a link to another offset, carrying every active filter forward.
    /// Dropping the filters when paging is the classic version of this bug.
    fn page_link(&self, offset: i64) -> String {
        let mut query = vec![format!("limit={}", self.limit), format!("offset={offset}")];

        for (name, value) in [
            ("collection", &self.selected_collection),
            ("book_number", &self.selected_book_number),
            ("grade", &self.selected_grade),
            ("hadith_number", &self.hadith_number),
        ] {
            if !value.is_empty() {
                query.push(format!("{name}={}", urlencode(value)));
            }
        }

        format!("/hadiths?{}", query.join("&"))
    }

    fn first_shown(&self) -> i64 {
        if self.total == 0 { 0 } else { self.offset + 1 }
    }

    fn last_shown(&self) -> i64 {
        self.offset + self.hadiths.len() as i64
    }

    fn has_previous(&self) -> bool {
        self.offset > 0
    }

    fn has_next(&self) -> bool {
        self.last_shown() < self.total
    }
}

/// Percent-encodes a filter value for a query string.
///
/// Grades carry apostrophes and spaces, and collection slugs are user-supplied
/// through the URL, so paging links cannot simply interpolate them.
fn urlencode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            b' ' => "+".to_owned(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

#[component]
pub(crate) async fn hadith_list_view(page: HadithPage) -> Result {
    let previous_link = page.page_link((page.offset - page.limit).max(0));
    let next_link = page.page_link(page.offset + page.limit);
    let has_previous = page.has_previous();
    let has_next = page.has_next();
    let showing = format!(
        "Showing {}-{} of {}",
        page.first_shown(),
        page.last_shown(),
        page.total
    );
    let summary = showing.clone();

    view! {
      site_chrome(
        <main>
            <section class="page-heading">
                <p class="eyebrow">"The collections"</p>
                <h1>"Browse the narrations"</h1>
                <p>
                    "Filter by collection, book, number, or grade. Every narration is "
                    "shown as it was recorded, with the reference you need to look it "
                    "up at its source."
                </p>
            </section>

            <form class="filters" action="/hadiths" method="get">
                <label>
                    "Collection"
                    <select name="collection">
                        <option value="" selected=(page.selected_collection.is_empty())>
                            "All collections"
                        </option>
                        for collection in page.collections {
                            <option
                                value=(collection.slug.clone())
                                selected=(collection.slug == page.selected_collection)
                            >
                                (collection.name)
                            </option>
                        }
                    </select>
                </label>
                <label>
                    "Book"
                    <select name="book_number">
                        <option value="" selected=(page.selected_book_number.is_empty())>
                            "All books"
                        </option>
                        for book in page.book_numbers {
                            <option
                                value=(book.clone())
                                selected=(book == page.selected_book_number)
                            >
                                (book)
                            </option>
                        }
                    </select>
                </label>
                <label>
                    "Grade"
                    <select name="grade">
                        <option value="" selected=(page.selected_grade.is_empty())>
                            "All grades"
                        </option>
                        for grade in page.grades {
                            <option
                                value=(grade.clone())
                                selected=(grade == page.selected_grade)
                            >
                                (grade)
                            </option>
                        }
                    </select>
                </label>
                <label>
                    "Hadith number"
                    <input
                        type="text"
                        name="hadith_number"
                        value=(page.hadith_number)
                        placeholder="e.g. 1"
                    >
                </label>
                <label>
                    "Per page"
                    <select name="limit">
                        for size in page.page_sizes {
                            <option value=(size.to_string()) selected=(size == page.limit)>
                                (size.to_string())
                            </option>
                        }
                    </select>
                </label>
                <button class="button primary" type="submit">"Apply filters"</button>
                <a class="clear-link" href="/hadiths">"Clear"</a>
            </form>

            <div class="result-summary">
                <p>(summary)</p>
                <a href="/api/hadiths">"JSON endpoint"</a>
            </div>

            <section class="hadith-list" aria-label="Hadith records">
                if page.hadiths.is_empty() {
                    <div class="empty-state">
                        <h2>"No records found"</h2>
                        <p>"Try removing one or more filters."</p>
                    </div>
                } else {
                    for hadith in page.hadiths {
                        <article class="hadith-card">
                            <div class="hadith-meta">
                                <span class="collection">(hadith.collection_name)</span>
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
                                (to_plain_text(&hadith.arabic_text))
                            </p>
                            if let Some(english_text) = hadith.english_text {
                                // The stored record keeps its source markup;
                                // only the rendering is cleaned. `to_plain_text`
                                // returns one line per source paragraph, so each
                                // gets its own <p> — folded into a single one
                                // they would render as a wall of run-on prose.
                                <div class="translation">
                                    for paragraph in to_plain_text(&english_text).lines() {
                                        <p>(paragraph)</p>
                                    }
                                </div>
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

            <nav class="pagination" aria-label="Pagination">
                if has_previous {
                    <a class="button secondary" href=(previous_link)>"← Previous"</a>
                } else {
                    <span class="button secondary is-disabled">"← Previous"</span>
                }
                <span class="pagination-status">(showing)</span>
                if has_next {
                    <a class="button secondary" href=(next_link)>"Next →"</a>
                } else {
                    <span class="button secondary is-disabled">"Next →"</span>
                }
            </nav>
        </main>
      )
    }
}
