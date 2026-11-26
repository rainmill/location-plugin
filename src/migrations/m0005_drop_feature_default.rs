//! Drop the feature switch for everywhere. A feature's master switch belongs
//! with whatever owns feature flags; this plugin keeps only what a place says.

use laterite_core::strata::*;

use crate::schema::RainmillLocationFeatureDefault as FeatureDefault;

pub struct Migration;

#[async_trait(?Send)]
impl laterite_core::Migration for Migration {
    fn name(&self) -> &str {
        "0005_drop_feature_default"
    }

    async fn up(&self, s: &mut Schema<'_>) -> CoreResult<()> {
        s.exec(
            Table::drop()
                .table(FeatureDefault::Table)
                .if_exists()
                .to_owned(),
        )
        .await
    }
}
