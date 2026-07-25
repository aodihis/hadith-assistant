mod collections;
mod hadiths;
mod retrieval;

use std::sync::Arc;

pub use collections::CollectionService;
pub use hadiths::HadithService;
pub use retrieval::RetrievalService;
use sqlx::PgPool;

#[derive(Clone)]
pub struct AppServices {
    pub collections: Arc<CollectionService>,
    pub hadiths: Arc<HadithService>,
    pub retrieval: Arc<RetrievalService>,
}

impl AppServices {
    pub fn new(pool: PgPool) -> Self {
        Self {
            collections: Arc::new(CollectionService::new(pool.clone())),
            hadiths: Arc::new(HadithService::new(pool)),
            retrieval: Arc::new(RetrievalService::new()),
        }
    }
}
