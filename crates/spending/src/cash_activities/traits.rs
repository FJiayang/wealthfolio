use crate::activity_assignments::{ActivityTaxonomyAssignment, BulkCategoryAssignment};
use anyhow::Result;
use async_trait::async_trait;

use super::model::{CashActivity, CashActivitySearchRequest, CashActivitySearchResponse};
use super::service::CashActivityService;

/// Surface of `CashActivityService` consumed by agent tools.
/// Mirrors the inherent method signatures exactly; extend (don't change)
/// when write tools need more of the service.
#[async_trait]
pub trait CashActivityServiceTrait: Send + Sync {
    async fn search(&self, req: CashActivitySearchRequest) -> Result<CashActivitySearchResponse>;
    async fn get_by_activity_ids(&self, activity_ids: &[String]) -> Result<Vec<CashActivity>>;
    async fn bulk_assign_categories(
        &self,
        items: &[BulkCategoryAssignment],
    ) -> Result<Vec<ActivityTaxonomyAssignment>>;
    async fn list(&self, filter: super::model::CashActivityFilter) -> Result<Vec<CashActivity>>;

    async fn list_assignments(&self, activity_id: &str) -> Result<Vec<ActivityTaxonomyAssignment>>;

    async fn assign_category(
        &self,
        activity_id: &str,
        taxonomy_id: &str,
        category_id: &str,
    ) -> Result<ActivityTaxonomyAssignment>;

    async fn list_splits(
        &self,
        activity_id: &str,
    ) -> Result<Vec<crate::activity_splits::ActivitySplit>>;

    async fn replace_splits(
        &self,
        activity_id: &str,
        splits: Vec<crate::activity_splits::NewActivitySplit>,
    ) -> Result<Vec<crate::activity_splits::ActivitySplit>>;

    async fn set_event(
        &self,
        activity_id: &str,
        event_id: Option<String>,
    ) -> Result<wealthfolio_core::activities::Activity>;
}

#[async_trait]
impl CashActivityServiceTrait for CashActivityService {
    async fn search(&self, req: CashActivitySearchRequest) -> Result<CashActivitySearchResponse> {
        // `None`: the agent surface reads rows, not a headline figure, so it has
        // no base currency to convert into and skips FX entirely — which also
        // makes the timezone moot, since nothing is dated for a rate lookup.
        CashActivityService::search(self, req, None, "").await
    }

    async fn get_by_activity_ids(&self, activity_ids: &[String]) -> Result<Vec<CashActivity>> {
        CashActivityService::get_by_activity_ids(self, activity_ids).await
    }

    async fn bulk_assign_categories(
        &self,
        items: &[BulkCategoryAssignment],
    ) -> Result<Vec<ActivityTaxonomyAssignment>> {
        CashActivityService::bulk_assign_categories(self, items).await
    }
    async fn list(&self, filter: super::model::CashActivityFilter) -> Result<Vec<CashActivity>> {
        CashActivityService::list(self, filter).await
    }

    async fn list_assignments(&self, activity_id: &str) -> Result<Vec<ActivityTaxonomyAssignment>> {
        CashActivityService::list_assignments(self, activity_id).await
    }

    async fn assign_category(
        &self,
        activity_id: &str,
        taxonomy_id: &str,
        category_id: &str,
    ) -> Result<ActivityTaxonomyAssignment> {
        CashActivityService::assign_category(self, activity_id, taxonomy_id, category_id).await
    }

    async fn list_splits(
        &self,
        activity_id: &str,
    ) -> Result<Vec<crate::activity_splits::ActivitySplit>> {
        CashActivityService::list_splits(self, activity_id).await
    }

    async fn replace_splits(
        &self,
        activity_id: &str,
        splits: Vec<crate::activity_splits::NewActivitySplit>,
    ) -> Result<Vec<crate::activity_splits::ActivitySplit>> {
        CashActivityService::replace_splits(self, activity_id, splits).await
    }

    async fn set_event(
        &self,
        activity_id: &str,
        event_id: Option<String>,
    ) -> Result<wealthfolio_core::activities::Activity> {
        CashActivityService::set_event(self, activity_id, event_id).await
    }
}
