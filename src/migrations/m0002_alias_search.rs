//! Mark official names among the aliases, and switch on fuzzy name matching
//! where the database offers it.
//!
//! On PostgreSQL the `pg_trgm` extension lets a search forgive a misspelling.
//! Creating an extension needs a privilege the application's role may lack, so
//! the attempt is wrapped: a refusal leaves a notice and the migration goes on,
//! and search then matches by exact name, prefix and substring alone.

use laterite_core::strata::*;

use crate::schema::RainmillLocationAlias as Alias;

pub struct Migration;

#[async_trait(?Send)]
impl laterite_core::Migration for Migration {
    fn name(&self) -> &str {
        "0002_alias_search"
    }

    async fn up(&self, s: &mut Schema<'_>) -> CoreResult<()> {
        s.exec(
            Table::alter()
                .table(Alias::Table)
                .add_column(bool_col(Alias::IsOfficial).not_null().default(0))
                .to_owned(),
        )
        .await?;
        if s.backend() == DbBackend::Postgres {
            s.raw(
                "DO $$ BEGIN CREATE EXTENSION IF NOT EXISTS pg_trgm; \
                 EXCEPTION WHEN OTHERS THEN \
                 RAISE NOTICE 'pg_trgm is not available: %', SQLERRM; END $$",
            )
            .await?;
        }
        Ok(())
    }

    async fn down(&self, s: &mut Schema<'_>) -> CoreResult<()> {
        s.exec(
            Table::alter()
                .table(Alias::Table)
                .drop_column(Alias::IsOfficial)
                .to_owned(),
        )
        .await
    }
}
