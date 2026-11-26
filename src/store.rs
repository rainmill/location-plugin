//! The location store: the plugin's read and write path.
//!
//! The ancestor closure is maintained here, in Rust, rather than by a database
//! trigger, so the schema stays portable. Creating a node inserts its closure
//! rows (itself at depth 0, then each of its parent's ancestors at depth + 1) in
//! a single statement, after reading the parent's ancestors. Queries and inserts
//! go through the framework's portable helpers.

use laterite_core::sea_query::{Func, LikeExpr};
use laterite_core::sqlx;
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

/// Creates a node and maintains its ancestor closure, atomically. Returns the new
/// node id. The node row and its closure rows land together or not at all.
pub async fn create_node(db: &Db, new: NewNode) -> CoreResult<i64> {
    let mut tx = db.pool.begin().await?;
    let id = create_node_in(db.backend, &mut tx, new).await?;
    tx.commit().await?;
    Ok(id)
}

/// Creates a node and its closure rows on one connection, so `create_node` runs
/// them in a single transaction. Reads the parent's ancestors on the same
/// connection so the in-flight parentage is seen.
pub async fn create_node_in(
    backend: DbBackend,
    conn: &mut sqlx::AnyConnection,
    new: NewNode,
) -> CoreResult<i64> {
    let now = chrono::Utc::now().to_rfc3339();
    let (lat, lng) = match new.centroid {
        Some((lat, lng)) => (Some(lat), Some(lng)),
        None => (None, None),
    };
    let insert = {
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
        insert.to_owned()
    };
    let id = insert_returning_id_on(backend, &mut *conn, insert, Node::Id).await?;

    // The closure: self at depth 0, then each parent ancestor at depth + 1. Depth
    // is a portable `integer` column, so its values bind as i32.
    let mut rows: Vec<(i64, i64, i32)> = vec![(id, id, 0)];
    if let Some(parent_id) = new.parent_id {
        for (ancestor_id, depth) in ancestors_of_in(backend, &mut *conn, parent_id).await? {
            rows.push((id, ancestor_id, (depth + 1) as i32));
        }
    }
    let (sql, values) = {
        let mut closure = Query::insert();
        closure
            .into_table(Anc::Table)
            .columns([Anc::NodeId, Anc::AncestorId, Anc::Depth]);
        for (node_id, ancestor_id, depth) in rows {
            closure.values_panic([node_id.into(), ancestor_id.into(), depth.into()]);
        }
        build(backend, closure)
    };
    bind_values(sqlx::query(&sql), values)
        .execute(&mut *conn)
        .await?;
    Ok(id)
}

/// The `(ancestor_id, depth)` pairs for a node, nearest first.
async fn ancestors_of(db: &Db, node_id: i64) -> CoreResult<Vec<(i64, i64)>> {
    let mut conn = db.pool.acquire().await?;
    ancestors_of_in(db.backend, &mut conn, node_id).await
}

/// [`ancestors_of`] on an existing connection, so a create in progress reads the
/// closure rows it has just written.
async fn ancestors_of_in(
    backend: DbBackend,
    conn: &mut sqlx::AnyConnection,
    node_id: i64,
) -> CoreResult<Vec<(i64, i64)>> {
    let (sql, values) = {
        let mut select = Query::select();
        select
            .from(Anc::Table)
            .columns([Anc::AncestorId, Anc::Depth])
            .and_where(Expr::col(Anc::NodeId).eq(node_id))
            .order_by(Anc::Depth, Order::Asc);
        build(backend, select)
    };
    let rows = bind_values(sqlx::query(&sql), values)
        .fetch_all(&mut *conn)
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

/// Adds a searchable alias for a node and returns its id. A node that already
/// has the name, in any spelling of case or accents, keeps the one it has.
pub async fn add_alias(db: &Db, node_id: i64, alias: &str, lang: Option<&str>) -> CoreResult<i64> {
    let normalized = normalize(alias);
    let (sql, values) = {
        let mut select = Query::select();
        select
            .from(Alias::Table)
            .column(Alias::Id)
            .and_where(Expr::col(Alias::NodeId).eq(node_id))
            .and_where(Expr::col(Alias::Normalized).eq(normalized.clone()))
            .limit(1);
        build(db.backend, select)
    };
    if let Some(row) = bind_values(sqlx::query(&sql), values)
        .fetch_optional(&db.pool)
        .await?
    {
        return Ok(row.get_int("id")?);
    }
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
            alias.trim().into(),
            normalized.into(),
            lang.map(String::from).into(),
            "curated".into(),
            now.into(),
        ]);
    Ok(insert_returning_id(db, insert, Alias::Id).await?)
}

/// The other names a node is known by, in alphabetical order.
pub async fn aliases(db: &Db, node_id: i64) -> CoreResult<Vec<String>> {
    let (sql, values) = {
        let mut select = Query::select();
        select
            .from(Alias::Table)
            .column(Alias::Alias)
            .and_where(Expr::col(Alias::NodeId).eq(node_id))
            .order_by(Alias::Alias, Order::Asc);
        build(db.backend, select)
    };
    let rows = bind_values(sqlx::query(&sql), values)
        .fetch_all(&db.pool)
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push(row.get_text("alias")?);
    }
    Ok(out)
}

/// A place a search found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub id: i64,
    /// The canonical name, which is what to show.
    pub name: String,
    pub level: String,
    /// The alias the query matched, when it was not the canonical name.
    pub matched: Option<String>,
}

/// How well a name answers a query, best first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    Exact,
    Prefix,
    Contains,
    Fuzzy,
}

fn rank(normalized_name: &str, query: &str) -> Rank {
    if normalized_name == query {
        Rank::Exact
    } else if normalized_name.starts_with(query) {
        Rank::Prefix
    } else if normalized_name.contains(query) {
        Rank::Contains
    } else {
        Rank::Fuzzy
    }
}

/// Whether the connected database can match names by similarity.
async fn has_trigrams(db: &Db) -> bool {
    if db.backend != DbBackend::Postgres {
        return false;
    }
    sqlx::query("SELECT 1 FROM pg_extension WHERE extname = 'pg_trgm'")
        .fetch_optional(&db.pool)
        .await
        .ok()
        .flatten()
        .is_some()
}

/// Places whose canonical name or any alias answers `query`, best match
/// first: an exact name, then a name starting with it, then one containing it.
/// Case and accents are ignored. On PostgreSQL with `pg_trgm`, a near
/// misspelling is found too, after every closer match. An empty query lists
/// places by name.
pub async fn search(db: &Db, query: &str, limit: u32) -> CoreResult<Vec<Found>> {
    let q = normalize(query);
    let cap = u64::from(limit.max(1));
    // (rank, found), one entry a node, the best match kept.
    let mut hits: Vec<(Rank, Found)> = Vec::new();
    let mut keep = |rank: Rank, found: Found| match hits.iter_mut().find(|(_, f)| f.id == found.id)
    {
        Some(hit) if rank < hit.0 => *hit = (rank, found),
        Some(_) => {}
        None => hits.push((rank, found)),
    };

    // Canonical names.
    let (sql, values) = {
        let mut select = Query::select();
        select
            .from(Node::Table)
            .columns([Node::Id, Node::CanonicalName, Node::Level])
            .order_by(Node::CanonicalName, Order::Asc);
        if q.is_empty() {
            select.limit(cap);
        } else {
            select.and_where(
                Expr::expr(Func::lower(Expr::col(Node::CanonicalName)))
                    .like(LikeExpr::new(format!("%{}%", like_escaped(&q))).escape('\\')),
            );
        }
        build(db.backend, select)
    };
    let rows = bind_values(sqlx::query(&sql), values)
        .fetch_all(&db.pool)
        .await?;
    for row in rows {
        let name = row.get_text("canonical_name")?;
        keep(
            rank(&normalize(&name), &q),
            Found {
                id: row.get_int("id")?,
                name,
                level: row.get_text("level")?,
                matched: None,
            },
        );
    }
    if q.is_empty() {
        return Ok(hits.into_iter().map(|(_, found)| found).collect());
    }

    // Aliases.
    let (sql, values) = {
        let mut select = Query::select();
        select
            .from(Alias::Table)
            .inner_join(
                Node::Table,
                Expr::col((Node::Table, Node::Id)).equals((Alias::Table, Alias::NodeId)),
            )
            .column((Node::Table, Node::Id))
            .column((Node::Table, Node::CanonicalName))
            .column((Node::Table, Node::Level))
            .column((Alias::Table, Alias::Alias))
            .column((Alias::Table, Alias::Normalized))
            .and_where(
                Expr::col((Alias::Table, Alias::Normalized))
                    .like(LikeExpr::new(format!("%{}%", like_escaped(&q))).escape('\\')),
            );
        build(db.backend, select)
    };
    let rows = bind_values(sqlx::query(&sql), values)
        .fetch_all(&db.pool)
        .await?;
    for row in rows {
        keep(
            rank(&row.get_text("normalized")?, &q),
            Found {
                id: row.get_int("id")?,
                name: row.get_text("canonical_name")?,
                level: row.get_text("level")?,
                matched: Some(row.get_text("alias")?),
            },
        );
    }

    // Near misspellings, where the database can measure similarity.
    if has_trigrams(db).await {
        let rows = sqlx::query(
            "SELECT id, canonical_name, level, alias FROM ( \
             SELECT n.id, n.canonical_name, n.level, CAST(NULL AS text) AS alias, \
                    similarity(lower(n.canonical_name), $1) AS score \
             FROM rainmill_location_node n WHERE lower(n.canonical_name) % $1 \
             UNION ALL \
             SELECT n.id, n.canonical_name, n.level, a.alias, \
                    similarity(a.normalized, $1) AS score \
             FROM rainmill_location_alias a \
             JOIN rainmill_location_node n ON n.id = a.node_id \
             WHERE a.normalized % $1 \
             ) near ORDER BY score DESC LIMIT 50",
        )
        .bind(q.clone())
        .fetch_all(&db.pool)
        .await?;
        for row in rows {
            keep(
                Rank::Fuzzy,
                Found {
                    id: row.get_int("id")?,
                    name: row.get_text("canonical_name")?,
                    level: row.get_text("level")?,
                    matched: row.get_text_opt("alias")?,
                },
            );
        }
    }

    // A stable sort keeps the similarity order among the near matches.
    hits.sort_by(|a, b| {
        a.0.cmp(&b.0).then_with(|| match a.0 {
            Rank::Fuzzy => std::cmp::Ordering::Equal,
            _ => a.1.name.cmp(&b.1.name),
        })
    });
    hits.truncate(limit.max(1) as usize);
    Ok(hits.into_iter().map(|(_, found)| found).collect())
}

/// One place by id, as a search would return it.
pub async fn find(db: &Db, id: i64) -> CoreResult<Option<Found>> {
    let (sql, values) = {
        let mut select = Query::select();
        select
            .from(Node::Table)
            .columns([Node::Id, Node::CanonicalName, Node::Level])
            .and_where(Expr::col(Node::Id).eq(id))
            .limit(1);
        build(db.backend, select)
    };
    let row = bind_values(sqlx::query(&sql), values)
        .fetch_optional(&db.pool)
        .await?;
    row.map(|row| {
        Ok(Found {
            id: row.get_int("id")?,
            name: row.get_text("canonical_name")?,
            level: row.get_text("level")?,
            matched: None,
        })
    })
    .transpose()
}

/// `text` with the characters a `LIKE` pattern reads as wildcards escaped.
fn like_escaped(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
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

/// The current parent of a node: `Some(parent)` (itself `Some` or `None`) when the
/// node exists, or `None` when it does not. The admin write uses this to detect an
/// attempted re-parenting.
pub async fn parent_of(db: &Db, id: i64) -> CoreResult<Option<Option<i64>>> {
    let mut conn = db.pool.acquire().await?;
    parent_of_in(db.backend, &mut conn, id).await
}

/// [`parent_of`] on an explicit connection, so a write running in a transaction
/// sees its own in-flight state.
pub async fn parent_of_in(
    backend: DbBackend,
    conn: &mut sqlx::AnyConnection,
    id: i64,
) -> CoreResult<Option<Option<i64>>> {
    let (sql, values) = {
        let mut select = Query::select();
        select
            .from(Node::Table)
            .column(Node::ParentId)
            .and_where(Expr::col(Node::Id).eq(id))
            .limit(1);
        build(backend, select)
    };
    let row = bind_values(sqlx::query(&sql), values)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(row.map(|r| r.get_int_opt("parent_id").ok().flatten()))
}

/// The editable fields of a node: everything except its parentage and closure.
#[derive(Debug, Clone)]
pub struct NodeEdit {
    pub level: String,
    pub canonical_name: String,
    pub slug: String,
    pub timezone: Option<String>,
    pub currency_code: Option<String>,
    pub locale: Option<String>,
}

/// Updates a node's editable fields in place, bumping `updated_at`. Parentage and
/// the ancestor closure are untouched (re-parenting is rejected by the caller), so
/// this is a single portable statement.
pub async fn update_node(db: &Db, id: i64, edit: NodeEdit) -> CoreResult<()> {
    let mut conn = db.pool.acquire().await?;
    update_node_in(db.backend, &mut conn, id, edit).await
}

/// [`update_node`] on an explicit connection, so the admin write joins the
/// pipeline's transaction rather than opening its own.
pub async fn update_node_in(
    backend: DbBackend,
    conn: &mut sqlx::AnyConnection,
    id: i64,
    edit: NodeEdit,
) -> CoreResult<()> {
    let now = chrono::Utc::now().to_rfc3339();
    let (sql, values) = {
        let mut update = Query::update();
        update
            .table(Node::Table)
            .value(Node::Level, edit.level)
            .value(Node::CanonicalName, edit.canonical_name)
            .value(Node::Slug, edit.slug)
            .value(
                Node::Timezone,
                edit.timezone.unwrap_or_else(|| "UTC".to_string()),
            )
            .value(Node::CurrencyCode, edit.currency_code)
            .value(Node::Locale, edit.locale)
            .value(Node::UpdatedAt, now)
            .and_where(Expr::col(Node::Id).eq(id));
        build(backend, update)
    };
    bind_values(sqlx::query(&sql), values)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

/// Puts a node under another parent (none for the top level), rebuilding the
/// closure of its whole subtree. Refuses a parent inside that subtree, which
/// would close a loop.
pub async fn move_node(db: &Db, id: i64, parent: Option<i64>) -> CoreResult<()> {
    let mut tx = db.pool.begin().await?;
    move_node_in(db.backend, &mut tx, id, parent).await?;
    tx.commit().await?;
    Ok(())
}

/// [`move_node`] on an explicit connection, so the admin write joins the
/// pipeline's transaction.
pub async fn move_node_in(
    backend: DbBackend,
    conn: &mut sqlx::AnyConnection,
    id: i64,
    parent: Option<i64>,
) -> CoreResult<()> {
    // The subtree: every node with `id` among its ancestors, itself included,
    // with how far beneath `id` each sits.
    let (sql, values) = {
        let mut select = Query::select();
        select
            .from(Anc::Table)
            .columns([Anc::NodeId, Anc::Depth])
            .and_where(Expr::col(Anc::AncestorId).eq(id));
        build(backend, select)
    };
    let rows = bind_values(sqlx::query(&sql), values)
        .fetch_all(&mut *conn)
        .await?;
    let mut subtree = Vec::with_capacity(rows.len());
    for row in rows {
        subtree.push((row.get_int("node_id")?, row.get_int("depth")?));
    }
    if subtree.is_empty() {
        return Err(laterite_core::CoreError::NotFound("location".to_string()));
    }
    let members: Vec<i64> = subtree.iter().map(|(node, _)| *node).collect();
    if parent.is_some_and(|p| members.contains(&p)) {
        return Err(laterite_core::CoreError::Validation(
            "a location cannot move under itself".to_string(),
        ));
    }
    // Cut the subtree loose: its links to every ancestor outside it.
    let (sql, values) = {
        let mut delete = Query::delete();
        delete
            .from_table(Anc::Table)
            .and_where(Expr::col(Anc::NodeId).is_in(members.clone()))
            .and_where(Expr::col(Anc::AncestorId).is_not_in(members.clone()));
        build(backend, delete)
    };
    bind_values(sqlx::query(&sql), values)
        .execute(&mut *conn)
        .await?;
    // Tie it to the new ancestry: each member to each ancestor of the parent
    // (the parent itself at depth 0), one deeper than the member sits plus
    // one more than the ancestor sits above the parent.
    if let Some(parent) = parent {
        let above = ancestors_of_in(backend, &mut *conn, parent).await?;
        let mut links: Vec<(i64, i64, i32)> = Vec::new();
        for (node, depth) in &subtree {
            for (ancestor, height) in &above {
                links.push((*node, *ancestor, (depth + height + 1) as i32));
            }
        }
        if !links.is_empty() {
            let (sql, values) = {
                let mut insert = Query::insert();
                insert
                    .into_table(Anc::Table)
                    .columns([Anc::NodeId, Anc::AncestorId, Anc::Depth]);
                for (node, ancestor, depth) in links {
                    insert.values_panic([node.into(), ancestor.into(), depth.into()]);
                }
                build(backend, insert)
            };
            bind_values(sqlx::query(&sql), values)
                .execute(&mut *conn)
                .await?;
        }
    }
    let now = chrono::Utc::now().to_rfc3339();
    let (sql, values) = {
        let mut update = Query::update();
        update
            .table(Node::Table)
            .value(Node::ParentId, parent)
            .value(Node::UpdatedAt, now)
            .and_where(Expr::col(Node::Id).eq(id));
        build(backend, update)
    };
    bind_values(sqlx::query(&sql), values)
        .execute(&mut *conn)
        .await?;
    Ok(())
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
    async fn search_finds_a_place_by_any_of_its_names() {
        let (db, _g) = db().await;
        let bengaluru = create_node(&db, node(None, "city", "Bengaluru"))
            .await
            .unwrap();
        let kolkata = create_node(&db, node(None, "city", "Kolkata"))
            .await
            .unwrap();
        create_node(&db, node(None, "city", "Mumbai"))
            .await
            .unwrap();
        add_alias(&db, bengaluru, "Bangalore", None).await.unwrap();
        add_alias(&db, kolkata, "Calcutta", None).await.unwrap();
        // The same name again, in another case, is the alias it already has.
        let again = add_alias(&db, kolkata, "CALCUTTA", None).await.unwrap();
        assert_eq!(aliases(&db, kolkata).await.unwrap(), ["Calcutta"]);
        assert_eq!(
            add_alias(&db, kolkata, "calcutta", None).await.unwrap(),
            again
        );

        let found = search(&db, "banga", 10).await.unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, bengaluru);
        assert_eq!(found[0].name, "Bengaluru");
        assert_eq!(found[0].matched.as_deref(), Some("Bangalore"));

        // The canonical name answers too, with no alias named.
        let found = search(&db, "KOLKATA", 10).await.unwrap();
        assert_eq!(found[0].id, kolkata);
        assert_eq!(found[0].matched, None);

        // An exact name outranks one that merely contains the query.
        let found = search(&db, "calcutta", 10).await.unwrap();
        assert_eq!(found[0].id, kolkata);

        // No query lists places by name; a wildcard is read as a character.
        assert_eq!(search(&db, "", 10).await.unwrap().len(), 3);
        assert!(search(&db, "%", 10).await.unwrap().is_empty());
        assert_eq!(find(&db, kolkata).await.unwrap().unwrap().name, "Kolkata");
    }

    #[tokio::test]
    async fn search_forgives_a_misspelling_where_the_database_can() {
        let (db, _g) = db().await;
        if !has_trigrams(&db).await {
            return;
        }
        let bengaluru = create_node(&db, node(None, "city", "Bengaluru"))
            .await
            .unwrap();
        add_alias(&db, bengaluru, "Bangalore", None).await.unwrap();
        let found = search(&db, "bangalor", 10).await.unwrap();
        assert_eq!(found[0].id, bengaluru);
        let found = search(&db, "bengluru", 10).await.unwrap();
        assert_eq!(found[0].id, bengaluru);
    }

    #[tokio::test]
    async fn move_rebuilds_the_closure_of_the_subtree() {
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
        let nepal = create_node(&db, node(None, "country", "Nepal"))
            .await
            .unwrap();

        // Karnataka, with Bengaluru beneath it, under Nepal.
        move_node(&db, karnataka, Some(nepal)).await.unwrap();
        assert_eq!(
            ancestors(&db, bengaluru).await.unwrap(),
            vec![bengaluru, karnataka, nepal]
        );
        assert_eq!(
            ancestors(&db, karnataka).await.unwrap(),
            vec![karnataka, nepal]
        );
        assert_eq!(parent_of(&db, karnataka).await.unwrap(), Some(Some(nepal)));

        // To the top level, and never under its own descendant.
        move_node(&db, karnataka, None).await.unwrap();
        assert_eq!(
            ancestors(&db, bengaluru).await.unwrap(),
            vec![bengaluru, karnataka]
        );
        assert!(move_node(&db, karnataka, Some(bengaluru)).await.is_err());
        assert_eq!(
            ancestors(&db, bengaluru).await.unwrap(),
            vec![bengaluru, karnataka]
        );
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

    #[tokio::test]
    async fn update_edits_fields_and_parent_of_reports_parentage() {
        let (db, _g) = db().await;
        let country = create_node(&db, node(None, "country", "India"))
            .await
            .unwrap();
        let city = create_node(&db, node(Some(country), "city", "Bengaluru"))
            .await
            .unwrap();

        // parent_of reports parentage: a root has none, a child has its parent, a
        // missing node is None.
        assert_eq!(parent_of(&db, country).await.unwrap(), Some(None));
        assert_eq!(parent_of(&db, city).await.unwrap(), Some(Some(country)));
        assert_eq!(parent_of(&db, 99999).await.unwrap(), None);

        // update_node changes editable fields, leaving parentage and closure intact.
        update_node(
            &db,
            city,
            NodeEdit {
                level: "city".to_string(),
                canonical_name: "Bengaluru (Bangalore)".to_string(),
                slug: "bengaluru".to_string(),
                timezone: Some("Asia/Kolkata".to_string()),
                currency_code: Some("INR".to_string()),
                locale: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(parent_of(&db, city).await.unwrap(), Some(Some(country)));
        assert_eq!(ancestors(&db, city).await.unwrap(), vec![city, country]);
    }
}
