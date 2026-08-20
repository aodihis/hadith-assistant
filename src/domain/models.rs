use serde::Serialize;
use sqlx::FromRow;

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Collection {
    pub id: i64,
    pub slug: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Hadith {
    pub id: i64,
    pub collection_id: i64,
    pub collection: String,
    /// The work's published title, for display and citation. `collection`
    /// stays the slug, which is what filters and URLs match on.
    pub collection_name: String,
    pub book_number: String,
    pub bab_id: f64,
    pub english_bab_number: Option<String>,
    pub arabic_bab_number: Option<String>,
    pub hadith_number: String,
    pub our_hadith_number: i32,
    pub arabic_urn: i64,
    pub arabic_bab_name: Option<String>,
    pub arabic_text: String,
    pub arabic_transliteration: Option<String>,
    pub arabic_grade: String,
    pub english_urn: i64,
    pub english_bab_name: Option<String>,
    pub english_text: Option<String>,
    pub english_grade: String,
    pub last_updated: Option<String>,
    pub xrefs: String,
}

#[derive(Debug, Clone, Default)]
pub struct HadithSearch {
    pub collection: Option<String>,
    pub book_number: Option<String>,
    pub hadith_number: Option<String>,
    pub grade: Option<String>,
    pub limit: i64,
    pub offset: i64,
}

#[derive(Debug, Clone)]
pub struct RetrievalQuery {
    pub query: String,
    pub collection: Option<String>,
    pub limit: i64,
}

/// The narrator a card attributes a narration to. Carried alongside the
/// narration rather than folded into it, so the canonical record stays the
/// single source for the text itself.
#[derive(Debug, Clone, Serialize)]
pub struct NarratorRef {
    pub name: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct RetrievedHadith {
    pub hadith_id: i64,
    pub collection: String,
    /// The work's published title, used to build the citation shown to the
    /// reader. `collection` stays the slug the vector store filters on.
    pub collection_name: String,
    pub book_number: String,
    pub hadith_number: String,
    pub arabic_text: String,
    pub english_text: Option<String>,
    /// Grades are authoritative source metadata, carried verbatim from the
    /// canonical record — never inferred, normalized, or re-characterized.
    pub arabic_grade: String,
    pub english_grade: String,
    pub narrator: Option<NarratorRef>,
    pub score: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RetrievalResult {
    pub query: String,
    pub results: Vec<RetrievedHadith>,
}

#[derive(Debug, Clone, Serialize, FromRow)]
pub struct Narrator {
    pub id: i64,
    pub hadith_id: i64,
    pub external_id: Option<i64>,
    pub role: String,
    pub name: String,
    pub position: i32,
}
