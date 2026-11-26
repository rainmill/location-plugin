//! The plugin's admin surface: the Locations screen as data in
//! `admin/locations.yaml`, rendered by the framework's generic handlers, plus
//! the two things the file names and Rust supplies: the node write handler
//! ([`crate::admin::persist`], which maintains the ancestor closure the
//! generic form cannot) and the node picker source.

use std::sync::Arc;

use laterite_admin::picker::{PickerError, PickerNode, PickerSource, PickerSourceReg};
use laterite_admin::{Permission, Resource};
use laterite_core::strata::async_trait;
use laterite_core::validation::ErrorBag;
use laterite_core::{t, Db, ModelListener, ModelListenerReg, Op, Record, SaveCx};

use crate::store::{self, Found};

/// The permission gating the location screens, offered in the role editor.
pub(crate) const MANAGE: &str = "rainmill.location.manage";

/// The node table, shared by the file, the persister and the picker.
#[cfg(test)]
pub(crate) const NODE_ENTITY: &str = "rainmill_location_node";

/// The registry key of the node write handler and of the node picker source (they
/// live in separate registries, so one dotted name serves both). The file names
/// it as `persist`, as the tree's `source` and as the parent field's `source`.
pub(crate) const NODE_KEY: &str = "rainmill.location.node";

/// The permission the plugin registers.
pub(crate) fn permission() -> Permission {
    Permission::new(MANAGE, "Manage locations", "Location")
        .roles([laterite_admin::ROLE_ADMIN, laterite_admin::ROLE_EDITOR])
}

/// The place-node resource, from `admin/locations.yaml`.
pub(crate) fn resource() -> Resource {
    laterite_admin::resource!("admin/locations.yaml")
}

/// The alias table, which the Also known as section edits.
pub(crate) const ALIAS_ENTITY: &str = "rainmill_location_alias";

/// A picker source over the place tree that finds a place by its canonical
/// name or by any alias: typing `Bangalore` offers Bengaluru. Each candidate
/// shows its level, and the alias that matched when one did.
///
/// An application registers it under a name of its own to let a form of its
/// own pick a place:
///
/// ```ignore
/// registry.add_picker_source(PickerSourceReg::new("acme.places", Arc::new(PlaceSource)));
/// ```
pub struct PlaceSource;

fn picker_node(found: Found) -> PickerNode {
    let hint = match found.matched {
        Some(alias) => format!("{}, also {alias}", found.level),
        None => found.level,
    };
    PickerNode {
        id: found.id.to_string(),
        label: found.name,
        hint: Some(hint),
    }
}

#[async_trait]
impl PickerSource for PlaceSource {
    async fn search(&self, db: &Db, q: &str, limit: u32) -> Result<Vec<PickerNode>, PickerError> {
        Ok(store::search(db, q, limit)
            .await
            .map_err(|e| PickerError(e.to_string()))?
            .into_iter()
            .map(picker_node)
            .collect())
    }

    async fn resolve(&self, db: &Db, id: &str) -> Result<Option<PickerNode>, PickerError> {
        let Ok(id) = id.parse::<i64>() else {
            return Ok(None);
        };
        Ok(store::find(db, id)
            .await
            .map_err(|e| PickerError(e.to_string()))?
            .map(picker_node))
    }
}

/// The node picker source, searching names and aliases. Gated on [`MANAGE`],
/// the same permission as the screens that use it.
pub(crate) fn picker_source() -> PickerSourceReg {
    PickerSourceReg::new(NODE_KEY, Arc::new(PlaceSource)).require(MANAGE)
}

/// Keeps an alias searchable: its folded form is worked out from the name on
/// every save, and a new one is dated.
struct AliasListener;

#[async_trait]
impl ModelListener for AliasListener {
    async fn before_save(
        &self,
        _cx: &mut SaveCx<'_>,
        rec: &mut Record,
        op: Op,
    ) -> Result<(), ErrorBag> {
        let alias = rec.text("alias").map(str::trim).unwrap_or("").to_string();
        if alias.is_empty() {
            let mut bag = ErrorBag::default();
            bag.add("alias", t!("Write the name."));
            return Err(bag);
        }
        rec.set("normalized", store::normalize(&alias));
        rec.set("alias", alias);
        if matches!(op, Op::Create) {
            rec.set("created_at", chrono::Utc::now().to_rfc3339());
            rec.set("source", "curated");
        }
        Ok(())
    }
}

/// The listener that keeps aliases written from the admin searchable.
pub(crate) fn alias_listener() -> ModelListenerReg {
    ModelListenerReg::for_entity(ALIAS_ENTITY, Arc::new(AliasListener))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_names_the_keys_rust_registers() {
        let resource = resource();
        assert_eq!(resource.base_path, "/locations");
        assert_eq!(resource.permission.as_deref(), Some(MANAGE));
        assert_eq!(resource.list.entity, NODE_ENTITY);
        let form = resource.form.as_ref().expect("a form");
        assert_eq!(form.persist.as_deref(), Some(NODE_KEY));
        assert_eq!(
            resource
                .list
                .tree
                .as_ref()
                .and_then(|t| t.source.as_deref()),
            Some(NODE_KEY)
        );
        assert_eq!(resource.palette.as_deref(), Some(NODE_KEY));
        assert!(resource.import.is_some());
        assert_eq!(resource.relations.len(), 2);
        let aliases = &resource.relations[0];
        assert_eq!(aliases.name, "aliases");
        assert_eq!(resource.relations[1].name, "children");
        assert_eq!(resource.relations[1].list.columns.len(), 5);
        // A name is changed on the location's own page, never in the list.
        assert!(resource.list.columns.iter().all(|c| !c.editable));
    }
}
