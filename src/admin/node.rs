//! The plugin's admin surface: the Locations screen as data in
//! `admin/locations.yaml`, rendered by the framework's generic handlers, plus
//! the two things the file names and Rust supplies: the node write handler
//! ([`crate::admin::persist`], which maintains the ancestor closure the
//! generic form cannot) and the node picker source.

use std::sync::Arc;

use laterite_admin::picker::{PickerSourceReg, TableSource};
use laterite_admin::{Permission, Resource};

/// The permission gating the location screens, offered in the role editor.
pub(crate) const MANAGE: &str = "rainmill.location.manage";

/// The node table, shared by the file, the persister and the picker.
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
        assert_eq!(resource.relations.len(), 1);
        assert_eq!(resource.relations[0].name, "children");
        assert_eq!(resource.relations[0].list.columns.len(), 5);
    }
}
