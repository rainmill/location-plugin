//! The location store: the plugin's read and write path.
//!
//! The ancestor closure is maintained here, in Rust, rather than by a database
//! trigger, so the schema stays portable. Creating a node inserts its closure
//! rows (itself at depth 0, then each of its parent's ancestors at depth + 1) in
//! a single statement, after reading the parent's ancestors. Queries and inserts
//! go through the framework's portable helpers.

use laterite_core::strata::*;

use crate::schema::RainmillLocationAlias as Alias;
use crate::schema::RainmillLocationAncestor as Anc;
use crate::schema::RainmillLocationNode as Node;

/// A new place node to create.
#[derive(Debug, Clone)]
pub struct NewNode {
    pub parent_id: Option<i64>,
    /// Level label (`country`, `city`, ...); a label, not a fixed enum.
    pub level: String,
    pub canonical_name: String,
    pub slug: String,
    pub timezone: Option<String>,
    pub currency_code: Option<String>,
    pub locale: Option<String>,
    /// Portable centroid as `(latitude, longitude)`; the rich geometry lives in
    /// the geo plugin.
    pub centroid: Option<(f64, f64)>,
}

/// Creates a node and maintains its ancestor closure. Returns the new node id.
pub async fn create_node(db: &Db, new: NewNode) -> CoreResult<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    let (lat, lng) = match new.centroid {
        Some((lat, lng)) => (Some(lat), Some(lng)),
        None => (None, None),
    };
    let mut insert = Query::insert();
    insert
        .into_table(Node::Table)
        .columns([
            Node::ParentId,
            Node::Level,
            Node::CanonicalName,
            Node::Slug,
            Node::Timezone,
            Node::CurrencyCode,
            Node::Locale,
            Node::CentroidLat,
            Node::CentroidLng,
            Node::CreatedAt,
            Node::UpdatedAt,
        ])
        .values_panic([
            new.parent_id.into(),
            new.level.into(),
            new.canonical_name.into(),
            new.slug.into(),
            new.timezone.unwrap_or_else(|| "UTC".to_string()).into(),
            new.currency_code.into(),
            new.locale.into(),
            lat.into(),
            lng.into(),
            now.clone().into(),
            now.into(),
        ]);
    let id = insert_returning_id(db, insert, Node::Id).await?;

    // The closure: self at depth 0, then each parent ancestor at depth + 1. Depth
    // is a portable `integer` column, so its values bind as i32.
    let mut rows: Vec<(i64, i64, i32)> = vec![(id, id, 0)];
    if let Some(parent_id) = new.parent_id {
        for (ancestor_id, depth) in ancestors_of(db, parent_id).await? {
            rows.push((id, ancestor_id, (depth + 1) as i32));
        }
    }
    let mut closure = Query::insert();
    closure
        .into_table(Anc::Table)
        .columns([Anc::NodeId, Anc::AncestorId, Anc::Depth]);
    for (node_id, ancestor_id, depth) in rows {
        closure.values_panic([node_id.into(), ancestor_id.into(), depth.into()]);
    }
    let (sql, values) = build(db.backend, closure);
    bind_values(sqlx::query(&sql), values)
        .execute(&db.pool)
        .await?;
    Ok(id)
}

/// The `(ancestor_id, depth)` pairs for a node, nearest first.
async fn ancestors_of(db: &Db, node_id: i64) -> CoreResult<Vec<(i64, i64)>> {
    let mut select = Query::select();
    select
        .from(Anc::Table)
        .columns([Anc::AncestorId, Anc::Depth])
        .and_where(Expr::col(Anc::NodeId).eq(node_id))
        .order_by(Anc::Depth, Order::Asc);
    let (sql, values) = build(db.backend, select);
    let rows = bind_values(sqlx::query(&sql), values)
        .fetch_all(&db.pool)
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push((row.get_int("ancestor_id")?, row.get_int("depth")?));
    }
    Ok(out)
}

/// The ancestor node ids of `node_id`, nearest first (includes the node itself
/// at depth 0): the breadcrumb path when reversed.
pub async fn ancestors(db: &Db, node_id: i64) -> CoreResult<Vec<i64>> {
    Ok(ancestors_of(db, node_id)
        .await?
        .into_iter()
        .map(|(id, _)| id)
        .collect())
}

/// Folds a name to its searchable form: accents stripped, lowercased, trimmed.
/// Uses `deunicode` rather than a hand-rolled accent table.
pub fn normalize(name: &str) -> String {
    deunicode::deunicode(name).to_lowercase().trim().to_string()
}

/// A URL-friendly slug for a name, via the `slug` crate.
pub fn slugify(name: &str) -> String {
    slug::slugify(name)
}

/// Adds a searchable alias for a node. Returns the alias id.
pub async fn add_alias(db: &Db, node_id: i64, alias: &str, lang: Option<&str>) -> CoreResult<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    let mut insert = Query::insert();
    insert
        .into_table(Alias::Table)
        .columns([
            Alias::NodeId,
            Alias::Alias,
            Alias::Normalized,
            Alias::Lang,
            Alias::Source,
            Alias::CreatedAt,
        ])
        .values_panic([
            node_id.into(),
            alias.into(),
            normalize(alias).into(),
            lang.map(String::from).into(),
            "curated".into(),
            now.into(),
        ]);
    Ok(insert_returning_id(db, insert, Alias::Id).await?)
}

/// Node ids whose alias matches `query` by exact or prefix on the normalized
/// form (accent- and case-insensitive).
pub async fn find_by_alias(db: &Db, query: &str) -> CoreResult<Vec<i64>> {
    let pattern = format!("{}%", normalize(query));
    let mut select = Query::select();
    select
        .distinct()
        .from(Alias::Table)
        .column(Alias::NodeId)
        .and_where(Expr::col(Alias::Normalized).like(pattern));
    let (sql, values) = build(db.backend, select);
    let rows = bind_values(sqlx::query(&sql), values)
        .fetch_all(&db.pool)
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(row.get_int("node_id")?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use laterite_core::testing::{connect_test, TestGuard};

    async fn db() -> (Db, TestGuard) {
        connect_test(&[crate::migrations::migrations()]).await
    }

    fn node(parent: Option<i64>, level: &str, name: &str) -> NewNode {
        NewNode {
            parent_id: parent,
            level: level.to_string(),
            canonical_name: name.to_string(),
            slug: slugify(name),
            timezone: None,
            currency_code: None,
            locale: None,
            centroid: None,
        }
    }

    #[tokio::test]
    async fn create_maintains_the_ancestor_closure() {
        let (db, _g) = db().await;
        let india = create_node(&db, node(None, "country", "India"))
            .await
            .unwrap();
        let karnataka = create_node(&db, node(Some(india), "state", "Karnataka"))
            .await
            .unwrap();
        let bengaluru = create_node(&db, node(Some(karnataka), "city", "Bengaluru"))
            .await
            .unwrap();
        // Nearest first: the node itself, then up the tree.
        assert_eq!(
            ancestors(&db, bengaluru).await.unwrap(),
            vec![bengaluru, karnataka, india]
        );
        assert_eq!(ancestors(&db, india).await.unwrap(), vec![india]);
    }

    #[tokio::test]
    async fn alias_search_folds_accents_and_case() {
        let (db, _g) = db().await;
        let id = create_node(&db, node(None, "city", "Bengaluru"))
            .await
            .unwrap();
        add_alias(&db, id, "Bengalūru", None).await.unwrap();
        add_alias(&db, id, "Bangalore", None).await.unwrap();
        // Accent stripped and case folded on both sides.
        assert_eq!(find_by_alias(&db, "bengaluru").await.unwrap(), vec![id]);
        assert_eq!(find_by_alias(&db, "BANGA").await.unwrap(), vec![id]);
    }
}
