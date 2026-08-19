mod api;
mod chat;
mod hadiths;
mod templates;

use topcoat::{
    Result,
    asset::{Asset, AssetBundle, RouterBuilderAssetExt, asset},
    font::{Font, RouterBuilderFontExt, fontsource::fontsource_font},
    router::{Router, layout, page},
    view::view,
};

use crate::application::AppServices;
use templates::{home::home_view, layout::page_shell};

const STYLES: Asset = asset!("assets/app.css");
const FAVICON: Asset = asset!("assets/favicon.svg");

/// Amiri covers both display roles: it is a classical naskh, so the headings
/// and the hadith Arabic come from one family. The Arabic subset is requested
/// explicitly — unlike weight and style, an omitted `subset` pulls in the
/// family default (Latin) alone, which would leave the narrations unstyled.
pub(crate) const AMIRI: Font = fontsource_font!(
    AMIRI,
    weight: [400, 700],
    style: Normal,
    subset: [Arabic, Latin, LatinExt],
    host: Asset,
);

/// Inter carries body and UI text. Only the three weights the stylesheet
/// actually asks for are built, rather than the nine the family ships.
pub(crate) const INTER: Font = fontsource_font!(
    INTER,
    weight: [400, 700, 800],
    style: Normal,
    subset: [Latin, LatinExt],
    host: Asset,
);
/// Referenced from the chat view so the bundler keeps it; an unused Asset
/// const is optimized out and vanishes from the bundle.
pub(crate) const CHAT_SCRIPT: Asset = asset!("assets/chat.js");

pub fn router(services: AppServices) -> std::io::Result<Router> {
    Ok(router_with_assets(services, AssetBundle::load()?))
}

fn router_with_assets(services: AppServices, assets: AssetBundle) -> Router {
    topcoat::router::module_router!()
        .app_context(services)
        .assets(assets)
        .font(AMIRI)
        .font(INTER)
        .build()
}

/// Builds the module router without registering an asset bundle.
///
/// `module_router!()` resolves module routes relative to its call site, so
/// this must live here rather than in the test module. Test-only: production
/// always serves a real bundle via [`router`].
#[cfg(test)]
fn router_without_assets(services: AppServices) -> Router {
    topcoat::router::module_router!()
        .app_context(services)
        .font(AMIRI)
        .font(INTER)
        .build()
}

#[layout]
async fn root_layout(slot: Result) -> Result {
    view! {
        page_shell(
            styles: STYLES,
            favicon: FAVICON,
            (slot?)
        )
    }
}

#[page]
async fn home() -> Result {
    view! { home_view() }
}

#[cfg(test)]
mod tests {
    use sqlx::postgres::PgPoolOptions;

    use super::*;

    #[tokio::test]
    async fn module_router_builds_without_route_conflicts() {
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://postgres:postgres@localhost/hadiths")
            .expect("test database URL should parse");

        // `.assets(...)` is an optional router-builder step (it only
        // registers asset routes and stashes AssetConfig as app context) —
        // this test only cares that module routes don't conflict, so it
        // skips asset registration instead of faking a bundle on disk.
        router_without_assets(AppServices::new(
            pool,
            crate::config::EmbeddingConfig::default(),
            crate::config::VectorConfig::default(),
            crate::config::ChatConfig::default(),
            crate::application::SessionConfig::default(),
        ));
    }
}
