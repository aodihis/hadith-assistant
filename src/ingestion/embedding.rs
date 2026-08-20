use crate::domain::Hadith;
use crate::error::AppError;
use crate::infrastructure::embedding::Embedder;
use crate::infrastructure::vector::{EmbeddingPoint, VectorStore};
use crate::text::to_plain_text;

const EMBEDDING_BATCH_SIZE: usize = 96;

pub async fn embed_hadiths(
    embedder: &(dyn Embedder + Send + Sync),
    vector_store: &(dyn VectorStore + Send + Sync),
    hadiths: &[Hadith],
) -> Result<usize, AppError> {
    let mut embedded_count = 0;

    for batch in hadiths.chunks(EMBEDDING_BATCH_SIZE) {
        let texts: Vec<String> = batch.iter().map(hadith_embedding_text).collect();
        let vectors = embedder.embed_batch(&texts).await?;

        if vectors.len() != batch.len() {
            return Err(AppError::Internal(format!(
                "embedding provider returned {} vectors for {} inputs",
                vectors.len(),
                batch.len()
            )));
        }

        let vector_size = vectors[0].len() as u64;
        vector_store.ensure_collection(vector_size).await?;

        let points = batch
            .iter()
            .zip(vectors)
            .map(|(hadith, vector)| EmbeddingPoint {
                hadith_id: hadith.id,
                vector,
                collection: hadith.collection.clone(),
            })
            .collect();

        vector_store.upsert(points).await?;
        embedded_count += batch.len();
    }

    Ok(embedded_count)
}

/// Builds the text a hadith is embedded from.
///
/// Source markup is stripped here rather than at import: the canonical record
/// keeps its original text, while the vector — derived data — is built from
/// clean prose. Embedding `<p>` tags spends tokens on markup and pushes every
/// record toward a shared, meaningless direction in the vector space.
fn hadith_embedding_text(hadith: &Hadith) -> String {
    embedding_text(&hadith.arabic_text, hadith.english_text.as_deref())
}

/// Composes the text a narration is represented by in the vector space.
///
/// Public because a search *for narrations like this one* has to build its
/// query the same way the index was built. Composed differently — markup left
/// in, or the translation dropped — the query lands somewhere else in the space
/// and the neighbours it finds are not the narration's neighbours.
pub fn embedding_text(arabic: &str, english: Option<&str>) -> String {
    let arabic = to_plain_text(arabic);

    let full = match english {
        Some(english_text) if !english_text.trim().is_empty() => {
            format!("{}\n{}", arabic, to_plain_text(english_text))
        }
        _ => arabic,
    };

    truncate_for_embedding(full)
}

/// Upper bound on the characters handed to the embedding provider.
///
/// The provider rejects any input over 8192 tokens, and it rejects the whole
/// request rather than the offending element — so one long narration used to
/// fail its entire batch of 96 and halt the run partway through a collection.
///
/// Arabic tokenizes at worst near one token per character, so this leaves
/// headroom under that ceiling. Around 90 of the corpus's records are long
/// enough to be affected; they are shortened for the vector only, and the
/// canonical text they are shown from is untouched.
const EMBEDDING_MAX_CHARS: usize = 6_000;

/// Cuts `text` to [`EMBEDDING_MAX_CHARS`], preferring a word boundary.
///
/// Slicing has to land on a character boundary or it panics on the multi-byte
/// Arabic this corpus is mostly made of, so the cut is found by character
/// rather than by byte offset.
fn truncate_for_embedding(text: String) -> String {
    let Some((cut, _)) = text.char_indices().nth(EMBEDDING_MAX_CHARS) else {
        return text;
    };

    let head = &text[..cut];
    match head.rfind(char::is_whitespace) {
        // Ignore a boundary so early that it would throw away most of the
        // budget; a mid-word cut costs less than half the text.
        Some(space) if space > cut / 2 => head[..space].to_owned(),
        _ => head.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;

    use super::*;
    use crate::infrastructure::vector::{EmbeddingPoint, VectorMatch, VectorStore};

    struct FakeEmbedder;

    #[async_trait]
    impl Embedder for FakeEmbedder {
        async fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, AppError> {
            Ok(texts.iter().map(|_| vec![0.1_f32]).collect())
        }
    }

    #[derive(Default)]
    struct RecordingVectorStore {
        upsert_calls: Mutex<Vec<usize>>,
    }

    #[async_trait]
    impl VectorStore for RecordingVectorStore {
        async fn ensure_collection(&self, _vector_size: u64) -> Result<(), AppError> {
            Ok(())
        }

        async fn upsert(&self, points: Vec<EmbeddingPoint>) -> Result<(), AppError> {
            self.upsert_calls.lock().unwrap().push(points.len());
            Ok(())
        }

        async fn existing_ids(
            &self,
            _hadith_ids: &[i64],
        ) -> Result<std::collections::HashSet<i64>, AppError> {
            Ok(std::collections::HashSet::new())
        }

        async fn search(
            &self,
            _vector: Vec<f32>,
            _collection_filter: Option<&str>,
            _limit: i64,
        ) -> Result<Vec<VectorMatch>, AppError> {
            Ok(Vec::new())
        }
    }

    fn hadith(id: i64) -> Hadith {
        Hadith {
            id,
            collection_id: 1,
            collection: "bukhari".to_owned(),
            collection_name: "Sahih al-Bukhari".to_owned(),
            book_number: "1".to_owned(),
            bab_id: 1.0,
            english_bab_number: None,
            arabic_bab_number: None,
            hadith_number: id.to_string(),
            our_hadith_number: id as i32,
            arabic_urn: id,
            arabic_bab_name: None,
            arabic_text: "نص".to_owned(),
            arabic_transliteration: None,
            arabic_grade: "Sahih".to_owned(),
            english_urn: id,
            english_bab_name: None,
            english_text: Some("text".to_owned()),
            english_grade: "Sahih".to_owned(),
            last_updated: None,
            xrefs: String::new(),
        }
    }

    #[tokio::test]
    async fn embed_hadiths_batches_in_groups_of_the_configured_size() {
        let hadiths: Vec<Hadith> = (1..=150).map(hadith).collect();
        let vector_store = RecordingVectorStore::default();

        let embedded = embed_hadiths(&FakeEmbedder, &vector_store, &hadiths)
            .await
            .expect("embedding fakes should not fail");

        assert_eq!(embedded, 150);
        assert_eq!(*vector_store.upsert_calls.lock().unwrap(), vec![96, 54]);
    }

    #[tokio::test]
    async fn embed_hadiths_returns_zero_for_an_empty_slice() {
        let vector_store = RecordingVectorStore::default();

        let embedded = embed_hadiths(&FakeEmbedder, &vector_store, &[])
            .await
            .expect("empty input should succeed trivially");

        assert_eq!(embedded, 0);
        assert!(vector_store.upsert_calls.lock().unwrap().is_empty());
    }

    /// A search for narrations like a given one builds its query with this,
    /// so it has to produce exactly what the index was built from — markup
    /// stripped, translation included.
    #[test]
    fn the_shared_composition_matches_what_the_index_holds() {
        let hadith = hadith(1);

        assert_eq!(
            embedding_text(&hadith.arabic_text, hadith.english_text.as_deref()),
            hadith_embedding_text(&hadith)
        );
    }

    #[test]
    fn composition_strips_markup_and_keeps_the_translation() {
        let composed = embedding_text("<p>نص</p>", Some("<p>Actions are by intentions.</p>"));

        assert!(!composed.contains('<'), "markup survived: {composed}");
        assert!(composed.contains("نص"), "{composed}");
        assert!(
            composed.contains("Actions are by intentions."),
            "{composed}"
        );
    }

    #[test]
    fn composition_falls_back_to_arabic_when_there_is_no_translation() {
        assert_eq!(embedding_text("نص", None), "نص");
        assert_eq!(embedding_text("نص", Some("   ")), "نص");
    }

    #[test]
    fn text_within_the_budget_is_left_alone() {
        let text = "Narrated Abu Hurayra: a short narration.".to_owned();

        assert_eq!(truncate_for_embedding(text.clone()), text);
    }

    #[test]
    fn overlong_text_is_cut_to_the_budget() {
        let text = "word ".repeat(4_000);

        let truncated = truncate_for_embedding(text);

        assert!(truncated.chars().count() <= EMBEDDING_MAX_CHARS);
        // Cut at a space, so the vector is not built from half a word.
        assert!(truncated.ends_with("word"));
    }

    /// A byte-offset slice would panic here: Arabic is multi-byte throughout,
    /// so the budget almost never lands on a character boundary.
    #[test]
    fn overlong_arabic_is_cut_without_panicking() {
        let text = "بسم الله الرحمن الرحيم ".repeat(1_000);

        let truncated = truncate_for_embedding(text);

        assert!(truncated.chars().count() <= EMBEDDING_MAX_CHARS);
        assert!(!truncated.is_empty());
    }

    /// Unbroken text has no whitespace to back off to; it still has to be cut
    /// rather than sent whole.
    #[test]
    fn text_with_no_whitespace_is_still_cut() {
        let text = "ا".repeat(20_000);

        let truncated = truncate_for_embedding(text);

        assert_eq!(truncated.chars().count(), EMBEDDING_MAX_CHARS);
    }
}
