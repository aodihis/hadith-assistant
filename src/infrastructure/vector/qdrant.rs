use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use qdrant_client::qdrant::point_id::PointIdOptions;
use qdrant_client::qdrant::{
    Condition, CreateCollectionBuilder, Distance, Filter, GetPointsBuilder, PointId, PointStruct,
    QueryPointsBuilder, UpsertPointsBuilder, VectorParamsBuilder,
};
use qdrant_client::{Payload, Qdrant};

use crate::error::AppError;

use super::{EmbeddingPoint, VectorMatch, VectorStore};

#[derive(Clone)]
pub struct QdrantVectorStore {
    client: Arc<Qdrant>,
    collection_name: String,
}

impl QdrantVectorStore {
    pub fn new(url: &str, collection_name: String) -> Result<Self, AppError> {
        let client = Qdrant::from_url(url)
            .build()
            .map_err(|error| AppError::Internal(format!("qdrant client init failed: {error}")))?;

        Ok(Self {
            client: Arc::new(client),
            collection_name,
        })
    }
}

#[async_trait]
impl VectorStore for QdrantVectorStore {
    async fn ensure_collection(&self, vector_size: u64) -> Result<(), AppError> {
        let exists = self
            .client
            .collection_exists(self.collection_name.clone())
            .await
            .map_err(|error| {
                AppError::Internal(format!("qdrant collection_exists failed: {error}"))
            })?;

        if exists {
            return Ok(());
        }

        self.client
            .create_collection(
                CreateCollectionBuilder::new(self.collection_name.clone())
                    .vectors_config(VectorParamsBuilder::new(vector_size, Distance::Cosine)),
            )
            .await
            .map_err(|error| {
                AppError::Internal(format!("qdrant create_collection failed: {error}"))
            })?;

        Ok(())
    }

    async fn upsert(&self, points: Vec<EmbeddingPoint>) -> Result<(), AppError> {
        let points: Vec<PointStruct> = points.into_iter().map(to_point_struct).collect();

        self.client
            .upsert_points(UpsertPointsBuilder::new(
                self.collection_name.clone(),
                points,
            ))
            .await
            .map_err(|error| AppError::Internal(format!("qdrant upsert failed: {error}")))?;

        Ok(())
    }

    async fn existing_ids(&self, hadith_ids: &[i64]) -> Result<HashSet<i64>, AppError> {
        if hadith_ids.is_empty() {
            return Ok(HashSet::new());
        }

        // Points are written with the hadith id as a numeric point id (see
        // to_point_struct), so the lookup key must match that representation.
        let ids: Vec<PointId> = hadith_ids
            .iter()
            .map(|id| PointId::from(*id as u64))
            .collect();

        let response = self
            .client
            .get_points(
                GetPointsBuilder::new(self.collection_name.clone(), ids)
                    .with_payload(false)
                    .with_vectors(false),
            )
            .await
            .map_err(|error| AppError::Internal(format!("qdrant get_points failed: {error}")))?;

        Ok(response
            .result
            .into_iter()
            .filter_map(|point| point_id_to_hadith_id(point.id))
            .collect())
    }

    async fn search(
        &self,
        vector: Vec<f32>,
        collection_filter: Option<&str>,
        limit: i64,
    ) -> Result<Vec<VectorMatch>, AppError> {
        let mut query = QueryPointsBuilder::new(self.collection_name.clone())
            .query(vector)
            .limit(limit as u64)
            .with_payload(false);

        if let Some(collection) = collection_filter {
            query = query.filter(Filter::all([Condition::matches(
                "collection",
                collection.to_owned(),
            )]));
        }

        let response = self
            .client
            .query(query)
            .await
            .map_err(|error| AppError::Internal(format!("qdrant query failed: {error}")))?;

        Ok(response
            .result
            .into_iter()
            .filter_map(|scored_point| {
                point_id_to_hadith_id(scored_point.id).map(|hadith_id| VectorMatch {
                    hadith_id,
                    score: scored_point.score,
                })
            })
            .collect())
    }
}

fn to_point_struct(point: EmbeddingPoint) -> PointStruct {
    let payload: Payload = serde_json::json!({ "collection": point.collection })
        .try_into()
        .expect("payload literal is always valid JSON");

    PointStruct::new(point.hadith_id as u64, point.vector, payload)
}

fn point_id_to_hadith_id(id: Option<PointId>) -> Option<i64> {
    match id?.point_id_options? {
        PointIdOptions::Num(value) => Some(value as i64),
        PointIdOptions::Uuid(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::vector::EmbeddingPoint;

    #[test]
    fn to_point_struct_uses_hadith_id_as_the_point_id_and_carries_the_collection_payload() {
        let point = EmbeddingPoint {
            hadith_id: 42,
            vector: vec![0.1, 0.2],
            collection: "bukhari".to_owned(),
        };

        let point_struct = to_point_struct(point);

        assert_eq!(
            point_struct.id,
            Some(qdrant_client::qdrant::PointId::from(42u64))
        );
        assert!(point_struct.payload.contains_key("collection"));
    }

    #[test]
    fn point_id_to_hadith_id_reads_numeric_ids_and_ignores_uuids() {
        use qdrant_client::qdrant::PointId;
        use qdrant_client::qdrant::point_id::PointIdOptions;

        let numeric = PointId {
            point_id_options: Some(PointIdOptions::Num(7)),
        };
        let uuid = PointId {
            point_id_options: Some(PointIdOptions::Uuid("not-a-hadith-id".to_owned())),
        };

        assert_eq!(point_id_to_hadith_id(Some(numeric)), Some(7));
        assert_eq!(point_id_to_hadith_id(Some(uuid)), None);
        assert_eq!(point_id_to_hadith_id(None), None);
    }
}
