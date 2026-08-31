//! Create the portable location tables: the place-node tree, its ancestor
//! closure, and name aliases. Portable SQL only (no PostGIS): auto-increment
//! integer ids, text timestamps, and `key_col`/`bool_col` polyfills so the schema
//! builds on PostgreSQL, MySQL, and SQLite alike. The closure is maintained by
//! the plugin's write path, not a database trigger.

use laterite_core::strata::*;

use crate::schema::RainmillLocationAlias as Alias;
use crate::schema::RainmillLocationAncestor as Anc;
use crate::schema::RainmillLocationNode as Node;

pub struct Migration;

#[async_trait(?Send)]
impl laterite_core::Migration for Migration {
    fn name(&self) -> &str {
        "0001_create_location"
    }

    async fn up(&self, s: &mut Schema<'_>) -> CoreResult<()> {
        s.exec(
            Table::create()
                .table(Node::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(Node::Id)
                        .big_integer()
                        .not_null()
                        .auto_increment()
                        .primary_key(),
                )
                .col(ColumnDef::new(Node::ParentId).big_integer())
                .col(key_col(Node::Level).not_null())
                .col(ColumnDef::new(Node::CanonicalName).text().not_null())
                .col(key_col(Node::Slug).not_null().unique_key())
                .col(key_col(Node::Status).not_null().default("active"))
                .col(key_col(Node::Timezone).not_null().default("UTC"))
                .col(key_col(Node::CurrencyCode))
                .col(key_col(Node::Locale))
                .col(ColumnDef::new(Node::CentroidLat).double())
                .col(ColumnDef::new(Node::CentroidLng).double())
                .col(ColumnDef::new(Node::CreatedAt).text().not_null())
                .col(ColumnDef::new(Node::UpdatedAt).text().not_null())
                .foreign_key(
                    ForeignKey::create()
                        .from(Node::Table, Node::ParentId)
                        .to(Node::Table, Node::Id)
                        .on_delete(ForeignKeyAction::Restrict),
                )
                .to_owned(),
        )
        .await?;

        s.exec(
            Table::create()
                .table(Anc::Table)
                .if_not_exists()
                .col(ColumnDef::new(Anc::NodeId).big_integer().not_null())
                .col(ColumnDef::new(Anc::AncestorId).big_integer().not_null())
                .col(ColumnDef::new(Anc::Depth).integer().not_null())
                .primary_key(Index::create().col(Anc::NodeId).col(Anc::AncestorId))
                .foreign_key(
                    ForeignKey::create()
                        .from(Anc::Table, Anc::NodeId)
                        .to(Node::Table, Node::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .foreign_key(
                    ForeignKey::create()
                        .from(Anc::Table, Anc::AncestorId)
                        .to(Node::Table, Node::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

        s.exec(
            Table::create()
                .table(Alias::Table)
                .if_not_exists()
                .col(
                    ColumnDef::new(Alias::Id)
                        .big_integer()
                        .not_null()
                        .auto_increment()
                        .primary_key(),
                )
                .col(ColumnDef::new(Alias::NodeId).big_integer().not_null())
                .col(ColumnDef::new(Alias::Alias).text().not_null())
                .col(key_col(Alias::Normalized).not_null())
                .col(key_col(Alias::Lang))
                .col(key_col(Alias::Source).not_null().default("curated"))
                .col(ColumnDef::new(Alias::CreatedAt).text().not_null())
                .foreign_key(
                    ForeignKey::create()
                        .from(Alias::Table, Alias::NodeId)
                        .to(Node::Table, Node::Id)
                        .on_delete(ForeignKeyAction::Cascade),
                )
                .to_owned(),
        )
        .await?;

        // A node's children, by parent.
        s.exec(
            Index::create()
                .name("rainmill_location_node_parent_idx")
                .table(Node::Table)
                .col(Node::ParentId)
                .to_owned(),
        )
        .await?;
        // One alias per (node, normalized), and search by normalized name.
        s.exec(
            Index::create()
                .name("rainmill_location_alias_node_normalized_idx")
                .table(Alias::Table)
                .col(Alias::NodeId)
                .col(Alias::Normalized)
                .unique()
                .to_owned(),
        )
        .await?;
        s.exec(
            Index::create()
                .name("rainmill_location_alias_normalized_idx")
                .table(Alias::Table)
                .col(Alias::Normalized)
                .to_owned(),
        )
        .await?;
        // Descendants, by ancestor (the closure PK covers the node side).
        s.exec(
            Index::create()
                .name("rainmill_location_ancestor_ancestor_idx")
                .table(Anc::Table)
                .col(Anc::AncestorId)
                .to_owned(),
        )
        .await
    }

    async fn down(&self, s: &mut Schema<'_>) -> CoreResult<()> {
        s.exec(Table::drop().table(Alias::Table).to_owned()).await?;
        s.exec(Table::drop().table(Anc::Table).to_owned()).await?;
        s.exec(Table::drop().table(Node::Table).to_owned()).await
    }
}
