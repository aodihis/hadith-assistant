use std::sync::Arc;

use sqlx::PgPool;

use crate::services::{CollectionService, HadithService, RetrievalService};

#[derive(Clone)]
pub struct AppState {
    pub collections: Arc<CollectionService>,
    pub hadiths: Arc<HadithService>,
    pub retrieval: Arc<RetrievalService>,
}

impl AppState {
    pub fn new(pool: PgPool) -> Self {
        Self {
            collections: Arc::new(CollectionService::new(pool.clone())),
            hadiths: Arc::new(HadithService::new(pool)),
            retrieval: Arc::new(RetrievalService::new()),
        }
    }
}
