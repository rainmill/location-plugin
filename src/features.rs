//! Features switched on and off by place.
//!
//! A module declares a feature once, in code: a key, a label and a line that
//! says what it does. Each place then says On, Off or nothing about it. A
//! place that says nothing follows its nearest ancestor that does, so a
//! feature turned on for a state is on in its cities until one of them turns
//! it off. Where no place on the way up says anything, the switch an
//! operator set for everywhere applies; and where there is none, the
//! feature's own answer: what it works out from data, or else its default.
//!
//! ```ignore
//! struct Guestlist;
//!
//! impl LocationFeature for Guestlist {
//!     fn key(&self) -> &'static str { "acme.guestlist" }
//!     fn label(&self) -> Text { t!("Guestlist") }
//!     fn description(&self) -> Text { t!("Guests can ask for a place on the list.") }
//! }
//!
//! registry.add_location_feature(Arc::new(Guestlist));
//! let on = features::enabled(db, &Guestlist, place_id).await?;
//! ```
//!
//! A feature with settings of its own for a place (a fee, a limit) keeps them
//! in a table of its own with a `node_id` column, and shows it on the place's
//! page as a section through a `ResourceHook`. [`nearest_with_row`] answers
//! which place's row applies, by the same nearest-ancestor rule.

use std::sync::Arc;

use laterite_core::sea_query::Alias as Name;
use laterite_core::sqlx;
use laterite_core::strata::*;
use laterite_core::{t, Registry, Text};

use crate::schema::RainmillLocationAncestor as Anc;
use crate::schema::RainmillLocationFeature as Feature;
use crate::schema::RainmillLocationFeatureDefault as FeatureDefault;
use crate::schema::RainmillLocationNode as Node;

/// A feature that places switch on and off.
#[async_trait]
pub trait LocationFeature: Send + Sync + 'static {
    /// The feature's dotted key, `vendor.name`. Stored with each switch, so
    /// it never changes once places have used it.
    fn key(&self) -> &'static str;

    /// The name shown beside the switch.
    fn label(&self) -> Text;

    /// A line saying what switching it on does.
    fn description(&self) -> Text;

    /// The answer where no place on the way up says anything and
    /// [`computed`](Self::computed) has none. Default: off.
    fn default_enabled(&self) -> bool {
        false
    }

    /// Whether a place at `level` (`country`, `city`, ...) is offered the
    /// switch. Default: every level.
    fn applies_to(&self, _level: &str) -> bool {
        true
    }

    /// The answer worked out from data for `node_id`, for a feature that
    /// switches itself on (enough events this month, enough members). A place
    /// that says On or Off, or follows an ancestor that does, is never asked.
    /// Default: no answer.
    async fn computed(&self, _db: &Db, _node_id: i64) -> CoreResult<Option<bool>> {
        Ok(None)
    }

    /// Called after `node_id`'s own switch changed to `setting` (`None`: it
    /// follows its parent again).
    async fn changed(&self, _db: &Db, _node_id: i64, _setting: Option<bool>) {}
}

/// A registered feature.
pub struct LocationFeatureReg(pub Arc<dyn LocationFeature>);

/// Registers location features.
pub trait LocationRegistry {
    /// Adds a feature, offered as a switch on every place it applies to.
    fn add_location_feature(&mut self, feature: Arc<dyn LocationFeature>);
}

impl LocationRegistry for Registry {
    fn add_location_feature(&mut self, feature: Arc<dyn LocationFeature>) {
        self.add(LocationFeatureReg(feature));
    }
}

/// The place itself: whether it is offered to people at all. On unless a
/// place, or one above it, is switched off.
pub struct Open;

#[async_trait]
impl LocationFeature for Open {
    fn key(&self) -> &'static str {
        "rainmill.location.open"
    }
    fn label(&self) -> Text {
        t!("Open")
    }
    fn description(&self) -> Text {
        t!("The place is offered to people. Switch it off to hold a place back, with everything beneath it.")
    }
    fn default_enabled(&self) -> bool {
        true
    }
}

/// Where an answer came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// The place says so itself.
    Here,
    /// The nearest ancestor that says anything, by id.
    Inherited(i64),
    /// The switch an operator set for everywhere.
    Everywhere,
    /// The feature worked it out from data.
    Computed,
    /// Nothing said anything: the feature's default.
    Default,
}

/// Whether a feature is on at a place, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Decision {
    pub enabled: bool,
    pub source: Source,
}

/// What `node_id` itself says about the feature `key`: on, off, or nothing.
pub async fn setting(db: &Db, key: &str, node_id: i64) -> CoreResult<Option<bool>> {
    let (sql, values) = {
        let mut select = Query::select();
        select
            .from(Feature::Table)
            .column(Feature::Enabled)
            .and_where(Expr::col(Feature::NodeId).eq(node_id))
            .and_where(Expr::col(Feature::Feature).eq(key))
            .limit(1);
        build(db.backend, select)
    };
    let row = bind_values(sqlx::query(&sql), values)
        .fetch_optional(&db.pool)
        .await?;
    Ok(match row {
        Some(row) => Some(row.get_int("enabled")? != 0),
        None => None,
    })
}

/// What an operator set for the feature `key` everywhere: on, off, or nothing.
pub async fn everywhere(db: &Db, key: &str) -> CoreResult<Option<bool>> {
    let (sql, values) = {
        let mut select = Query::select();
        select
            .from(FeatureDefault::Table)
            .column(FeatureDefault::Enabled)
            .and_where(Expr::col(FeatureDefault::Feature).eq(key))
            .limit(1);
        build(db.backend, select)
    };
    let row = bind_values(sqlx::query(&sql), values)
        .fetch_optional(&db.pool)
        .await?;
    Ok(match row {
        Some(row) => Some(row.get_int("enabled")? != 0),
        None => None,
    })
}

/// Switches `feature` on or off everywhere, or with `None` leaves it to the
/// feature's own answer. A place that says On or Off still goes its own way.
pub async fn set_everywhere(
    db: &Db,
    feature: &dyn LocationFeature,
    to: Option<bool>,
) -> CoreResult<()> {
    let mut tx = db.pool.begin().await?;
    let (sql, values) = {
        let mut delete = Query::delete();
        delete
            .from_table(FeatureDefault::Table)
            .and_where(Expr::col(FeatureDefault::Feature).eq(feature.key()));
        build(db.backend, delete)
    };
    bind_values(sqlx::query(&sql), values)
        .execute(&mut *tx)
        .await?;
    if let Some(on) = to {
        let (sql, values) = {
            let mut insert = Query::insert();
            insert
                .into_table(FeatureDefault::Table)
                .columns([
                    FeatureDefault::Feature,
                    FeatureDefault::Enabled,
                    FeatureDefault::UpdatedAt,
                ])
                .values_panic([
                    feature.key().into(),
                    i32::from(on).into(),
                    chrono::Utc::now().to_rfc3339().into(),
                ]);
            build(db.backend, insert)
        };
        bind_values(sqlx::query(&sql), values)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// The places that say On or Off for the feature `key` themselves, by name.
pub async fn said(db: &Db, key: &str) -> CoreResult<Vec<(i64, String, bool)>> {
    let (sql, values) = {
        let mut select = Query::select();
        select
            .from(Feature::Table)
            .inner_join(
                Node::Table,
                Expr::col((Node::Table, Node::Id)).equals((Feature::Table, Feature::NodeId)),
            )
            .column((Node::Table, Node::Id))
            .column((Node::Table, Node::CanonicalName))
            .column((Feature::Table, Feature::Enabled))
            .and_where(Expr::col((Feature::Table, Feature::Feature)).eq(key))
            .order_by((Node::Table, Node::CanonicalName), Order::Asc);
        build(db.backend, select)
    };
    let rows = bind_values(sqlx::query(&sql), values)
        .fetch_all(&db.pool)
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        out.push((
            row.get_int("id")?,
            row.get_text("canonical_name")?,
            row.get_int("enabled")? != 0,
        ));
    }
    Ok(out)
}

/// Whether `feature` is on at `node_id`, and where the answer came from: the
/// nearest place on the way up that says On or Off, else the switch set for
/// everywhere, else what the feature works out, else its default.
pub async fn decide(db: &Db, feature: &dyn LocationFeature, node_id: i64) -> CoreResult<Decision> {
    let (sql, values) = {
        let mut select = Query::select();
        select
            .from(Feature::Table)
            .inner_join(
                Anc::Table,
                Expr::col((Anc::Table, Anc::AncestorId)).equals((Feature::Table, Feature::NodeId)),
            )
            .column((Feature::Table, Feature::NodeId))
            .column((Feature::Table, Feature::Enabled))
            .and_where(Expr::col((Anc::Table, Anc::NodeId)).eq(node_id))
            .and_where(Expr::col((Feature::Table, Feature::Feature)).eq(feature.key()))
            .order_by((Anc::Table, Anc::Depth), Order::Asc)
            .limit(1);
        build(db.backend, select)
    };
    let row = bind_values(sqlx::query(&sql), values)
        .fetch_optional(&db.pool)
        .await?;
    if let Some(row) = row {
        let at = row.get_int("node_id")?;
        return Ok(Decision {
            enabled: row.get_int("enabled")? != 0,
            source: if at == node_id {
                Source::Here
            } else {
                Source::Inherited(at)
            },
        });
    }
    if let Some(enabled) = everywhere(db, feature.key()).await? {
        return Ok(Decision {
            enabled,
            source: Source::Everywhere,
        });
    }
    if let Some(enabled) = feature.computed(db, node_id).await? {
        return Ok(Decision {
            enabled,
            source: Source::Computed,
        });
    }
    Ok(Decision {
        enabled: feature.default_enabled(),
        source: Source::Default,
    })
}

/// Whether `feature` is on at `node_id`.
pub async fn enabled(db: &Db, feature: &dyn LocationFeature, node_id: i64) -> CoreResult<bool> {
    Ok(decide(db, feature, node_id).await?.enabled)
}

/// Switches `feature` on or off at `node_id`, or with `None` lets the place
/// follow its parent again. Tells the feature once the change is stored.
pub async fn set(
    db: &Db,
    feature: &dyn LocationFeature,
    node_id: i64,
    to: Option<bool>,
) -> CoreResult<()> {
    let mut tx = db.pool.begin().await?;
    let (sql, values) = {
        let mut delete = Query::delete();
        delete
            .from_table(Feature::Table)
            .and_where(Expr::col(Feature::NodeId).eq(node_id))
            .and_where(Expr::col(Feature::Feature).eq(feature.key()));
        build(db.backend, delete)
    };
    bind_values(sqlx::query(&sql), values)
        .execute(&mut *tx)
        .await?;
    if let Some(on) = to {
        let (sql, values) = {
            let mut insert = Query::insert();
            insert
                .into_table(Feature::Table)
                .columns([
                    Feature::NodeId,
                    Feature::Feature,
                    Feature::Enabled,
                    Feature::UpdatedAt,
                ])
                .values_panic([
                    node_id.into(),
                    feature.key().into(),
                    i32::from(on).into(),
                    chrono::Utc::now().to_rfc3339().into(),
                ]);
            build(db.backend, insert)
        };
        bind_values(sqlx::query(&sql), values)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    feature.changed(db, node_id, to).await;
    Ok(())
}

/// Every place where `feature` is on by what places say, the switch for
/// everywhere and its default,
/// without asking what it would work out from data.
pub async fn enabled_nodes(db: &Db, feature: &dyn LocationFeature) -> CoreResult<Vec<i64>> {
    // Each place with its parent, and each place's own switch.
    let (sql, values) = {
        let mut select = Query::select();
        select
            .from(Node::Table)
            .columns([Node::Id, Node::ParentId])
            .order_by(Node::Id, Order::Asc);
        build(db.backend, select)
    };
    let rows = bind_values(sqlx::query(&sql), values)
        .fetch_all(&db.pool)
        .await?;
    let mut parents = std::collections::HashMap::new();
    let mut ids = Vec::with_capacity(rows.len());
    for row in rows {
        let id = row.get_int("id")?;
        parents.insert(id, row.get_int_opt("parent_id")?);
        ids.push(id);
    }
    let (sql, values) = {
        let mut select = Query::select();
        select
            .from(Feature::Table)
            .columns([Feature::NodeId, Feature::Enabled])
            .and_where(Expr::col(Feature::Feature).eq(feature.key()));
        build(db.backend, select)
    };
    let rows = bind_values(sqlx::query(&sql), values)
        .fetch_all(&db.pool)
        .await?;
    let mut said = std::collections::HashMap::new();
    for row in rows {
        said.insert(row.get_int("node_id")?, row.get_int("enabled")? != 0);
    }
    let fallback = match everywhere(db, feature.key()).await? {
        Some(on) => on,
        None => feature.default_enabled(),
    };
    Ok(ids
        .into_iter()
        .filter(|&id| {
            let mut at = Some(id);
            // The tree has no loops, and its depth bounds the walk.
            for _ in 0..64 {
                let Some(node) = at else { break };
                if let Some(&on) = said.get(&node) {
                    return on;
                }
                at = parents.get(&node).copied().flatten();
            }
            fallback
        })
        .collect())
}

/// The nearest place, `node_id` first and then each ancestor, that has a row
/// in `table` under `column`: whose settings apply at `node_id`, for a
/// feature that keeps settings of its own by place.
pub async fn nearest_with_row(
    db: &Db,
    table: &str,
    column: &str,
    node_id: i64,
) -> CoreResult<Option<i64>> {
    let (sql, values) = {
        let mut select = Query::select();
        select
            .from(Anc::Table)
            .inner_join(
                Name::new(table),
                Expr::col((Name::new(table), Name::new(column)))
                    .equals((Anc::Table, Anc::AncestorId)),
            )
            .column((Anc::Table, Anc::AncestorId))
            .and_where(Expr::col((Anc::Table, Anc::NodeId)).eq(node_id))
            .order_by((Anc::Table, Anc::Depth), Order::Asc)
            .limit(1);
        build(db.backend, select)
    };
    let row = bind_values(sqlx::query(&sql), values)
        .fetch_optional(&db.pool)
        .await?;
    Ok(match row {
        Some(row) => Some(row.get_int("ancestor_id")?),
        None => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{create_node, slugify, NewNode};
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

    struct Guestlist;

    #[async_trait]
    impl LocationFeature for Guestlist {
        fn key(&self) -> &'static str {
            "acme.guestlist"
        }
        fn label(&self) -> Text {
            t!("Guestlist")
        }
        fn description(&self) -> Text {
            t!("Guests can ask for a place on the list.")
        }
    }

    /// On wherever the place's id is even, when nothing says otherwise.
    struct Even;

    #[async_trait]
    impl LocationFeature for Even {
        fn key(&self) -> &'static str {
            "acme.even"
        }
        fn label(&self) -> Text {
            t!("Even")
        }
        fn description(&self) -> Text {
            t!("On by arithmetic.")
        }
        async fn computed(&self, _db: &Db, node_id: i64) -> CoreResult<Option<bool>> {
            Ok(Some(node_id % 2 == 0))
        }
    }

    #[tokio::test]
    async fn a_place_follows_its_nearest_ancestor_that_says() {
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
        let mysuru = create_node(&db, node(Some(karnataka), "city", "Mysuru"))
            .await
            .unwrap();

        // Nothing said anywhere: the feature's default, and Open's is on.
        let decision = decide(&db, &Guestlist, bengaluru).await.unwrap();
        assert_eq!(
            (decision.enabled, decision.source),
            (false, Source::Default)
        );
        assert!(enabled(&db, &Open, bengaluru).await.unwrap());

        // On for the state: on in its cities, and off above it.
        set(&db, &Guestlist, karnataka, Some(true)).await.unwrap();
        let decision = decide(&db, &Guestlist, bengaluru).await.unwrap();
        assert_eq!(
            (decision.enabled, decision.source),
            (true, Source::Inherited(karnataka))
        );
        assert!(!enabled(&db, &Guestlist, india).await.unwrap());

        // A city says otherwise for itself alone.
        set(&db, &Guestlist, mysuru, Some(false)).await.unwrap();
        let decision = decide(&db, &Guestlist, mysuru).await.unwrap();
        assert_eq!((decision.enabled, decision.source), (false, Source::Here));
        assert!(enabled(&db, &Guestlist, bengaluru).await.unwrap());
        assert_eq!(
            enabled_nodes(&db, &Guestlist).await.unwrap(),
            vec![karnataka, bengaluru]
        );
        assert_eq!(
            setting(&db, "acme.guestlist", mysuru).await.unwrap(),
            Some(false)
        );

        // Following the parent again.
        set(&db, &Guestlist, mysuru, None).await.unwrap();
        assert!(enabled(&db, &Guestlist, mysuru).await.unwrap());
        assert_eq!(setting(&db, "acme.guestlist", mysuru).await.unwrap(), None);

        // Off everywhere reaches the places nothing is said for, and no further.
        set_everywhere(&db, &Open, Some(false)).await.unwrap();
        let decision = decide(&db, &Open, india).await.unwrap();
        assert_eq!(
            (decision.enabled, decision.source),
            (false, Source::Everywhere)
        );
        set_everywhere(&db, &Guestlist, Some(false)).await.unwrap();
        assert!(
            enabled(&db, &Guestlist, bengaluru).await.unwrap(),
            "the state says On"
        );
        assert_eq!(
            said(&db, "acme.guestlist").await.unwrap(),
            vec![(karnataka, "Karnataka".to_string(), true)]
        );
        set_everywhere(&db, &Open, None).await.unwrap();
        assert!(enabled(&db, &Open, india).await.unwrap());
    }

    #[tokio::test]
    async fn what_a_feature_works_out_yields_to_what_a_place_says() {
        let (db, _g) = db().await;
        let first = create_node(&db, node(None, "country", "First"))
            .await
            .unwrap();
        let second = create_node(&db, node(Some(first), "city", "Second"))
            .await
            .unwrap();
        let decision = decide(&db, &Even, second).await.unwrap();
        assert_eq!(decision.source, Source::Computed);
        assert_eq!(decision.enabled, second % 2 == 0);

        set(&db, &Even, first, Some(second % 2 != 0)).await.unwrap();
        let decision = decide(&db, &Even, second).await.unwrap();
        assert_eq!(decision.source, Source::Inherited(first));
        assert_eq!(decision.enabled, second % 2 != 0);
    }

    #[tokio::test]
    async fn settings_of_a_feature_apply_from_the_nearest_place_that_has_them() {
        let (db, _g) = db().await;
        let india = create_node(&db, node(None, "country", "India"))
            .await
            .unwrap();
        let goa = create_node(&db, node(Some(india), "state", "Goa"))
            .await
            .unwrap();
        // The alias table stands in for a feature's own table keyed by place.
        crate::store::add_alias(&db, india, "Bharat", None)
            .await
            .unwrap();
        assert_eq!(
            nearest_with_row(&db, "rainmill_location_alias", "node_id", goa)
                .await
                .unwrap(),
            Some(india)
        );
        crate::store::add_alias(&db, goa, "Gomantak", None)
            .await
            .unwrap();
        assert_eq!(
            nearest_with_row(&db, "rainmill_location_alias", "node_id", goa)
                .await
                .unwrap(),
            Some(goa)
        );
    }
}
