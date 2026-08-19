use topcoat::{
    Result,
    view::{component, view},
};

use crate::web::CHAT_SCRIPT;

/// The Sanad chat shell.
///
/// Only the static chrome is server-rendered: the hero, its suggestion chips,
/// and the empty containers. Everything conversational — turns, citation cards,
/// the narration drawer, the saved list — is rendered by `chat.js` from the
/// events `/api/chat` streams back. No canonical hadith text is baked into this
/// page, so the browser and the JSON API read from the same retrieval path.
#[component]
pub(crate) async fn chat_view() -> Result {
    view! {
        <div class="chat-app" data-chat-state="empty">
            <header class="chat-header">
                <a class="chat-brand" href="/">
                    <span class="chat-mark" aria-hidden="true">"۞"</span>
                    "Sanad"
                </a>
                <button class="chat-saved-toggle" type="button" data-action="open-saved">
                    "❖ Saved "
                    <span data-bind="bookmark-count">"0"</span>
                </button>
            </header>

            <section class="chat-hero" data-region="hero">
                <p class="chat-eyebrow">"Sanad · Ask the Sunnah"</p>
                <h1>"Seek an answer, grounded in authentic hadith"</h1>
                <p class="chat-lead">
                    "Ask in your own words. Sanad searches the indexed collections and "
                    "returns the narrations that speak to your question — each with its "
                    "narrator, source, and grade."
                </p>

                <div class="chat-suggestions">
                    <button type="button" data-prompt="What do the narrations say about charity?">
                        "What do the narrations say about charity?"
                    </button>
                    <button type="button" data-prompt="How should I treat my parents?">
                        "How should I treat my parents?"
                    </button>
                    <button type="button" data-prompt="What is said about controlling anger?">
                        "What is said about controlling anger?"
                    </button>
                    <button type="button" data-prompt="The Prophet on good character">
                        "The Prophet on good character"
                    </button>
                </div>

                <p class="chat-disclaimer">
                    "A study companion for reflection — not a substitute for a qualified "
                    "scholar or a formal fatwa. Always verify and seek guidance in matters "
                    "of ruling."
                </p>
            </section>

            <div class="chat-transcript" data-region="transcript" aria-live="polite"></div>

            <form class="chat-composer" data-region="composer">
                // Input and button share one bordered field rather than
                // sitting side by side as two controls, so the composer reads
                // as a single place to type.
                <div class="chat-composer-field">
                    <input
                        id="chat-input"
                        type="text"
                        name="message"
                        autocomplete="off"
                        aria-label="Your question"
                        placeholder="Ask about a topic from the narrations…"
                    >
                    <button
                        class="chat-send"
                        type="submit"
                        data-bind="send"
                        aria-label="Send"
                    >
                        <span aria-hidden="true" data-bind="send-icon">"↑"</span>
                    </button>
                </div>
            </form>

            <div class="chat-backdrop" data-region="backdrop" hidden=(true)></div>

            <aside class="chat-drawer" data-region="drawer" hidden=(true) aria-label="Narration detail">
                <div class="chat-drawer-head">
                    <span>"Narration"</span>
                    <button type="button" data-action="close-drawer" aria-label="Close">"×"</button>
                </div>
                <div class="chat-drawer-body" data-region="drawer-body"></div>
            </aside>

            <section class="chat-saved" data-region="saved" hidden=(true) aria-label="Saved narrations">
                <div class="chat-drawer-head">
                    <span>"Your collection"</span>
                    <button type="button" data-action="close-saved" aria-label="Close">"×"</button>
                </div>
                <div class="chat-saved-body" data-region="saved-body"></div>
            </section>

            <div class="chat-toast" data-region="toast" hidden=(true)></div>
        </div>

        <script src=(CHAT_SCRIPT) defer=(true)></script>
    }
}
