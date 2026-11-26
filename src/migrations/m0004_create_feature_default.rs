//! A feature's switch for everywhere: one row for a feature an operator has
//! turned on or off as a whole. A place that says On or Off for itself, or
//! follows an ancestor that does, still goes its own way.

use laterite_core::strata::*;

use crate::schema::RainmillLocationFeatureDefault as FeatureDefault;

pub struct Migration;

#[async_trait(?Send)]
impl laterite_core::Migration for Migration {
    fn name(&self) -> &str {
        "0004_create_feature_default"
    }

    async fn up(&self, s: &mut Schema<'_>) -> CoreResult<()> {
        s.exec(
            Table::create()
                .table(FeatureDefault::Table)
                .if_not_exists()
                .col(key_col(FeatureDefault::Feature).not_null().primary_key())
                .col(bool_col(FeatureDefault::Enabled).not_null())
                .col(ColumnDef::new(FeatureDefault::UpdatedAt).text().not_null())
                .to_owned(),
        )
        .await
    }

    async fn down(&self, s: &mut Schema<'_>) -> CoreResult<()> {
        s.exec(Table::drop().table(FeatureDefault::Table).to_owned())
            .await
    }
}
