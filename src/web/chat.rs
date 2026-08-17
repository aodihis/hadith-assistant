use topcoat::{Result, router::page, view::view};

use super::templates::chat::chat_view;

#[page]
async fn chat() -> Result {
    view! { chat_view() }
}
