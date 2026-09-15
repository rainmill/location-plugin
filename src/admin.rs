//! The plugin's admin surface: a list and a create/edit form for curating place
//! nodes, plus the write and picker services they need.
//!
//! Contributed through the module's `register`, so the framework's generic
//! handlers render the screens from these descriptors rather than any hand-written
//! CRUD. A node write maintains the ancestor closure (see [`store::create_node`]),
//! which the generic form cannot do, so the form names a [`NodePersister`]. The
//! parent field is a reference picker over the node table itself.

use std::collections::HashMap;
use std::sync::Arc;

use laterite_admin::form::{FormConfig, FormField};
use laterite_admin::list::{ListColumn, ListConfig, SortDir};
use laterite_admin::persist::{Persister, PersisterReg, SaveError};
use laterite_admin::picker::{PickerSourceReg, TableSource};
use laterite_admin::{Permission, Resource};
use laterite_core::strata::async_trait;
use laterite_core::validation::ErrorBag;
use laterite_core::{t, Record, SaveCx};

use crate::store::{self, NewNode, NodeEdit};

/// The permission gating the location screens, offered in the role editor.
pub(crate) const MANAGE: &str = "rainmill.location.manage";

/// The node table, shared by the descriptors, the persister, and the picker.
const NODE_ENTITY: &str = "rainmill_location_node";
/// The registry key of the node write handler and of the node picker source (they
/// live in separate registries, so one dotted name serves both).
const NODE_KEY: &str = "rainmill.location.node";

/// The permission the plugin registers.
pub(crate) fn permission() -> Permission {
    Permission {
        code: MANAGE.to_string(),
        label: "Manage locations".into(),
        group: "Location".into(),
    }
}

/// The place-node resource: a list plus a create/edit form, gated by [`MANAGE`].
pub(crate) fn resource() -> Resource {
    Resource::new("/locations", "Locations", node_list_config())
        .form(node_form_config())
        .permission(MANAGE)
}

/// The node write handler, registered under [`NODE_KEY`].
pub(crate) fn persister() -> PersisterReg {
    PersisterReg::new(NODE_KEY, Arc::new(NodePersister))
}

/// The node picker source: the built-in single-table source over the node table,
/// matching on name and hinting each node's level. Gated on [`MANAGE`], the same
/// permission as the screens that use it.
pub(crate) fn picker_source() -> PickerSourceReg {
    PickerSourceReg::new(
        NODE_KEY,
        Arc::new(TableSource::new(NODE_ENTITY, "id", "canonical_name").with_hint("level")),
    )
    .require(MANAGE)
}

/// The list view over the place-node table: name, level, slug, status, created.
fn node_list_config() -> ListConfig {
    // No filters yet: `level` and `status` are the natural ones, but their value
    // sets are not declared anywhere here yet. And no `.deletable()`: nodes form
    // a hierarchy, so removing one has to say what happens to its children first.
    ListConfig::new(
        NODE_ENTITY,
        "Locations",
        vec![
            ListColumn::new("canonical_name", "Name"),
            ListColumn::new("level", "Level"),
            ListColumn::new("slug", "Slug"),
            ListColumn::new("status", "Status"),
            ListColumn::new("created_at", "Created").datetime(),
        ],
    )
    .order("canonical_name", SortDir::Asc)
    .edit_base("/locations")
    .creatable()
}

/// The create/edit form. The parent field is a reference picker over the node
/// table (a node's parent is another node); the [`NodePersister`] rejects a
/// changed parent on edit, so re-parenting is not offered as an ordinary edit.
fn node_form_config() -> FormConfig {
    // No `.timestamps()`: the node persister stamps its own inside the closure
    // write.
    FormConfig::new(
        NODE_ENTITY,
        "Location",
        "/locations",
        "id",
        vec![
            FormField::text("canonical_name", "Name").required(),
            FormField::text("slug", "Slug").required().unique(),
            FormField::text("level", "Level").required(),
            FormField::reference("parent_id", "Parent", NODE_KEY),
            FormField::text("timezone", "Timezone"),
            FormField::text("currency_code", "Currency"),
            FormField::text("locale", "Locale"),
        ],
    )
    .persist(NODE_KEY)
}

/// The location node write handler. Create maintains the ancestor closure
/// atomically (see [`store::create_node`]); update edits a node's fields but
/// rejects re-parenting, which would require rebuilding the subtree's closure.
struct NodePersister;

#[async_trait]
impl Persister for NodePersister {
    async fn create(&self, cx: &mut SaveCx<'_>, rec: &Record) -> Result<i64, SaveError> {
        // The helpers below read the submission as text; the record's own typed
        // values arrive with the field-type save contract.
        let data = &rec.to_text_map();
        let new = NewNode {
            parent_id: opt_i64(data, "parent_id"),
            level: req(data, "level"),
            canonical_name: req(data, "canonical_name"),
            slug: req(data, "slug"),
            timezone: opt(data, "timezone"),
            currency_code: opt(data, "currency_code"),
            locale: opt(data, "locale"),
            centroid: None,
        };
        // Writes on the pipeline's transaction, so the node row and its closure
        // commit with the rest of the save or not at all.
        let backend = cx.backend();
        store::create_node_in(backend, cx.conn(), new)
            .await
            .map_err(|e| SaveError::Failed(e.to_string()))
    }

    async fn update(&self, cx: &mut SaveCx<'_>, id: &str, rec: &Record) -> Result<(), SaveError> {
        let data = &rec.to_text_map();
        let backend = cx.backend();
        let node_id: i64 = id
            .parse()
            .map_err(|_| SaveError::Failed("invalid node id".to_string()))?;
        // Re-parenting would move a whole subtree and rebuild its closure, which
        // this write does not do: a changed parent is a per-field rejection.
        match store::parent_of_in(backend, cx.conn(), node_id)
            .await
            .map_err(|e| SaveError::Failed(e.to_string()))?
        {
            None => return Err(SaveError::Failed("location not found".to_string())),
            Some(current_parent) => {
                if opt_i64(data, "parent_id") != current_parent {
                    let mut bag = ErrorBag::default();
                    bag.add(
                        "parent_id",
                        t!("Re-parenting a location is not supported here."),
                    );
                    return Err(SaveError::Invalid(bag));
                }
            }
        }
        let edit = NodeEdit {
            level: req(data, "level"),
            canonical_name: req(data, "canonical_name"),
            slug: req(data, "slug"),
            timezone: opt(data, "timezone"),
            currency_code: opt(data, "currency_code"),
            locale: opt(data, "locale"),
        };
        store::update_node_in(backend, cx.conn(), node_id, edit)
            .await
            .map_err(|e| SaveError::Failed(e.to_string()))
    }
}

/// A trimmed, non-empty submission value, or None.
fn opt(data: &HashMap<String, String>, key: &str) -> Option<String> {
    data.get(key)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// A required value: the form's rules have already ensured it is present.
fn req(data: &HashMap<String, String>, key: &str) -> String {
    opt(data, key).unwrap_or_default()
}

/// A submission value parsed as an id, or None when absent or unparsable.
fn opt_i64(data: &HashMap<String, String>, key: &str) -> Option<i64> {
    opt(data, key).and_then(|s| s.parse().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use laterite_core::testing::connect_test;
    use laterite_core::{Actor, Db};
    use laterite_core::{Module, Registry};

    #[test]
    fn register_contributes_the_resource_permission_persister_and_picker() {
        let module = crate::LocationModule;
        let mut registry = Registry::new();
        // The framework stamps the owner before each module's register(); mirror
        // that here so add() has an active owner to attribute contributions to.
        registry.set_owner(module.id());
        module.register(&mut registry);

        let resources = registry.take::<Resource>();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].base_path, "/locations");
        assert_eq!(resources[0].list.entity, NODE_ENTITY);
        // The screen now creates and edits through the node persister.
        assert!(resources[0].form.is_some());
        assert!(resources[0].list.creatable);

        let permissions = registry.take::<Permission>();
        assert_eq!(permissions.len(), 1);
        assert_eq!(permissions[0].code, MANAGE);

        let persisters = registry.take::<PersisterReg>();
        assert_eq!(persisters.len(), 1);
        assert_eq!(persisters[0].name, NODE_KEY);

        let sources = registry.take::<PickerSourceReg>();
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name, NODE_KEY);
    }

    fn new_node(parent: Option<i64>, level: &str, name: &str, slug: &str) -> NewNode {
        NewNode {
            parent_id: parent,
            level: level.to_string(),
            canonical_name: name.to_string(),
            slug: slug.to_string(),
            timezone: None,
            currency_code: None,
            locale: None,
            centroid: None,
        }
    }

    fn record(pairs: &[(&str, &str)]) -> Record {
        let mut rec = Record::new("rainmill_location_nodes");
        for (key, value) in pairs {
            rec.set(*key, *value);
        }
        rec
    }

    /// Runs a create the way the pipeline does: on one transaction, committed.
    async fn create(db: &Db, rec: &Record) -> Result<i64, SaveError> {
        let mut tx = db.pool.begin().await.unwrap();
        let out = {
            let mut cx = SaveCx::new(db.backend, &mut tx, Actor::system("test"));
            NodePersister.create(&mut cx, rec).await
        };
        tx.commit().await.unwrap();
        out
    }

    /// The update counterpart. Commits so a following read sees the write; a
    /// rejected update leaves nothing behind either way.
    async fn update(db: &Db, id: &str, rec: &Record) -> Result<(), SaveError> {
        let mut tx = db.pool.begin().await.unwrap();
        let out = {
            let mut cx = SaveCx::new(db.backend, &mut tx, Actor::system("test"));
            NodePersister.update(&mut cx, id, rec).await
        };
        tx.commit().await.unwrap();
        out
    }

    #[tokio::test]
    async fn node_persister_rejects_re_parenting_but_allows_a_plain_edit() {
        let (db, _g) = connect_test(&[crate::migrations::migrations()]).await;
        let india = store::create_node(&db, new_node(None, "country", "India", "india"))
            .await
            .unwrap();
        let usa = store::create_node(&db, new_node(None, "country", "USA", "usa"))
            .await
            .unwrap();
        let city = store::create_node(&db, new_node(Some(india), "city", "Bengaluru", "bengaluru"))
            .await
            .unwrap();

        // Moving the city under a different parent is rejected with a field error.
        let moved = record(&[
            ("canonical_name", "Bengaluru"),
            ("slug", "bengaluru"),
            ("level", "city"),
            ("parent_id", &usa.to_string()),
        ]);
        let err = update(&db, &city.to_string(), &moved).await.unwrap_err();
        assert!(matches!(err, SaveError::Invalid(_)));

        // Editing fields while keeping the same parent succeeds.
        let renamed = record(&[
            ("canonical_name", "Bengaluru (Bangalore)"),
            ("slug", "bengaluru"),
            ("level", "city"),
            ("parent_id", &india.to_string()),
        ]);
        let Ok(()) = update(&db, &city.to_string(), &renamed).await else {
            panic!("editing without re-parenting should succeed");
        };
        // Parentage and closure survive the edit.
        assert_eq!(
            store::ancestors(&db, city).await.unwrap(),
            vec![city, india]
        );
    }

    #[tokio::test]
    async fn node_persister_creates_a_root_and_a_child() {
        let (db, _g) = connect_test(&[crate::migrations::migrations()]).await;
        let Ok(root) = create(
            &db,
            &record(&[
                ("canonical_name", "India"),
                ("slug", "india"),
                ("level", "country"),
            ]),
        )
        .await
        else {
            panic!("creating a root should succeed");
        };
        let Ok(child) = create(
            &db,
            &record(&[
                ("canonical_name", "Karnataka"),
                ("slug", "karnataka"),
                ("level", "state"),
                ("parent_id", &root.to_string()),
            ]),
        )
        .await
        else {
            panic!("creating a child should succeed");
        };
        // The closure was maintained through the persister's create path.
        assert_eq!(
            store::ancestors(&db, child).await.unwrap(),
            vec![child, root]
        );
    }
}
