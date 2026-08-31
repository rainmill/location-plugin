//! Shared `sea-query` identifiers for the location schema, used by both the
//! migrations and the query layer so the two never drift on a table or column
//! name. The enum name becomes the (vendor-prefixed) table name in snake_case:
//! `RainmillLocationNode` -> `rainmill_location_node`.

use sea_query::Iden;

/// A place node in the tree.
#[derive(Iden)]
pub(crate) enum RainmillLocationNode {
    Table,
    Id,
    ParentId,
    /// Level label (`country`, `state`, `city`, `zone`, `locality`, ...); a
    /// label, not a fixed enum, so depth stays flexible.
    Level,
    CanonicalName,
    Slug,
    Status,
    Timezone,
    CurrencyCode,
    Locale,
    CentroidLat,
    CentroidLng,
    CreatedAt,
    UpdatedAt,
}

/// The ancestor closure: one row per (node, ancestor) pair including self at
/// depth 0. Maintained by the plugin's write path, not a database trigger.
#[derive(Iden)]
pub(crate) enum RainmillLocationAncestor {
    Table,
    NodeId,
    AncestorId,
    Depth,
}

/// Name variants for a node, searchable by `normalized` (lowercased, unaccented
/// at write time).
#[derive(Iden)]
pub(crate) enum RainmillLocationAlias {
    Table,
    Id,
    NodeId,
    Alias,
    Normalized,
    Lang,
    Source,
    CreatedAt,
}
