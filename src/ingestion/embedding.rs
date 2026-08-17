use crate::domain::Hadith;
use crate::error::AppError;
use crate::infrastructure::embedding::Embedder;
use crate::infrastructure::vector::{EmbeddingPoint, VectorStore};

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

fn hadith_embedding_text(hadith: &Hadith) -> String {
    match &hadith.english_text {
        Some(english_text) if !english_text.trim().is_empty() => {
            format!("{}\n{}", hadith.arabic_text, english_text)
        }
        _ => hadith.arabic_text.clone(),
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
}
