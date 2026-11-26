//! Where a feature is switched on or off: one row for each place that says so
//! itself. A place with no row for a feature follows its nearest ancestor that
//! has one, so only the exceptions are stored.

use laterite_core::strata::*;

use crate::schema::RainmillLocationFeature as Feature;
use crate::schema::RainmillLocationNode as Node;

pub struct Migration;

#[async_trait(?Send)]
impl laterite_core::Migration for Migration {
    fn name(&self) -> &str {
        "0003_create_feature"
    }

    async fn up(&self, s: &mut Schema<'_>) -> CoreResult<()> {
        s.exec(
            Table::create()
                .table(Feature::Table)
                .if_not_exists()
                .col(ColumnDef::new(Feature::NodeId).big_integer().not_null())
                .col(key_col(Feature::Feature).not_null())
                .col(bool_col(Feature::Enabled).not_null())
                .col(ColumnDef::new(Feature::UpdatedAt).text().not_null())
                .primary_key(
                    Index::create()
                        .col(Feature::NodeId)
                        .col(Feature::Feature),
                )
                .foreign_key(
                    ForeignKey::create()
                        .from(Feature::Table, Feature::NodeId)
                        .to(Node::Table, Node::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;
        s.exec(
            Index::create()
                .if_not_exists()
                .name("rainmill_location_feature_feature_idx")
                .table(Feature::Table)
                .col(Feature::Feature)
                .to_owned(),
        )
        .await
    }

    async fn down(&self, s: &mut Schema<'_>) -> CoreResult<()> {
        s.exec(Table::drop().table(Feature::Table).to_owned()).await
    }
}
