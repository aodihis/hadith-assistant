use topcoat::{
    Result,
    view::{component, view},
};

use super::layout::site_chrome;

/// The public landing page.
///
/// Deliberately says nothing about how the application is built. What matters
/// to someone arriving here is what the collection contains, where its text
/// came from, and what they can do with it — the stack is an implementation
/// detail they never have to care about.
#[component]
pub(crate) async fn home_view() -> Result {
    view! {
      site_chrome(
        <main>
            <section class="hero">
                <p class="eyebrow">"Sanad · Ask the Sunnah"</p>
                <h1>"Every answer, traced back to its narration."</h1>
                <p class="lead">
                    "Ask a question in your own words and read the narrations that "
                    "speak to it. Each one carries its collection, book, hadith number, "
                    "and grade, so you can follow it back to the source and judge it "
                    "for yourself. Nothing is paraphrased without the text it came from."
                </p>
                <div class="actions">
                    <a class="button primary" href="/chat">
                        "Have a chat"
                    </a>
                    <a class="button secondary" href="/hadiths">
                        "Browse the collections"
                    </a>
                </div>
            </section>

            <section class="principles" aria-label="What this is">
                <article>
                    <span>"01"</span>
                    <h2>"Traceable"</h2>
                    <p>
                        "Every narration keeps its collection, book, and hadith number, "
                        "so anything you find here can be looked up and verified "
                        "anywhere else."
                    </p>
                </article>
                <article>
                    <span>"02"</span>
                    <h2>"Graded"</h2>
                    <p>
                        "Gradings are shown beside the text rather than buried, "
                        "because the standing of a narration is part of reading it, "
                        "not a footnote to it."
                    </p>
                </article>
                <article>
                    <span>"03"</span>
                    <h2>"Grounded"</h2>
                    <p>
                        "Answers are built only from narrations that were actually "
                        "retrieved, and each is cited. When nothing fits the question, "
                        "it says so instead of inventing one."
                    </p>
                </article>
            </section>

            <section class="credit" aria-labelledby="credit-heading">
                <p class="eyebrow">"Sources"</p>
                <h2 id="credit-heading">
                    "44,896 narrations across 15 collections."
                </h2>
                <p>
                    "Ṣaḥīḥ al-Bukhārī, Ṣaḥīḥ Muslim, the four Sunan, Riyāḍ al-Ṣāliḥīn, "
                    "Mishkāt al-Maṣābīḥ, al-Adab al-Mufrad, al-Shamāʾil al-Muḥammadiyya, "
                    "and more. All in Arabic, with English translation."
                </p>
                <p class="credit-thanks">
                    "The hadith text, translations, and gradings all come from "
                    <a href="https://sunnah.com/" rel="noopener">"sunnah.com"</a>
                    ", whose work made this possible. Our thanks to them for making "
                    "these collections freely available and carefully referenced. "
                    "Please support their project. The scholarship here is theirs."
                </p>
            </section>
        </main>
      )
    }
}
