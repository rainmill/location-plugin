//! This plugin's admin surface.
//!
//! One module per resource, as `lat make:resource` lays it out, with the code
//! seams beside them.

pub(crate) mod node;
mod persist;

pub(crate) use node::{permission, picker_source, resource};
pub(crate) use persist::persister;

#[cfg(test)]
mod tests {
    use crate::admin::node::{MANAGE, NODE_ENTITY, NODE_KEY};
    use crate::admin::persist::NodePersister;
    use crate::store::{self, NewNode};
    use laterite_admin::persist::PersisterReg;
    use laterite_admin::persist::{Persister, SaveError};
    use laterite_admin::picker::PickerSourceReg;
    use laterite_admin::{Permission, Resource};
    use laterite_core::testing::connect_test;
    use laterite_core::{Actor, Db};
    use laterite_core::{Module, Registry};
    use laterite_core::{Record, SaveCx};

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
