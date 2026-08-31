//! The plugin's admin surface: a list screen for curating place nodes.
//!
//! Contributed through the module's `register`, so the framework's generic
//! handlers render it from this descriptor rather than any hand-written CRUD.
//!
//! List-only for now. A node write maintains the ancestor closure (see
//! `store::create_node`), which the generic form cannot yet call, so `form` is
//! `None` and the screen is read-only until a closure-aware write hook lands.

use laterite_admin::list::{ListColumn, ListConfig, SortDir};
use laterite_admin::{Permission, Resource};

/// The permission gating the location screens, offered in the role editor.
pub(crate) const MANAGE: &str = "rainmill.location.manage";

/// The permission the plugin registers.
pub(crate) fn permission() -> Permission {
    Permission {
        code: MANAGE.to_string(),
        label: "Manage locations".to_string(),
        group: "Location".to_string(),
    }
}

/// The place-node list resource, gated by [`MANAGE`].
pub(crate) fn resource() -> Resource {
    Resource {
        base_path: "/locations".to_string(),
        nav_label: "Locations".to_string(),
        list: node_list_config(),
        // No create/edit form yet: a node write must maintain the ancestor
        // closure, which the generic form cannot do until the write hook exists.
        form: None,
        permission: Some(MANAGE.to_string()),
    }
}

/// The list view over the place-node table: name, level, slug, status, created.
fn node_list_config() -> ListConfig {
    ListConfig {
        entity: "rainmill_location_node".to_string(),
        title: "Locations".to_string(),
        columns: vec![
            ListColumn::new("canonical_name", "Name"),
            ListColumn::new("level", "Level"),
            ListColumn::new("slug", "Slug"),
            ListColumn::new("status", "Status"),
            ListColumn::new("created_at", "Created").datetime(),
        ],
        order_by: "canonical_name".to_string(),
        order_dir: SortDir::Asc,
        per_page: 25,
        id_field: "id".to_string(),
        edit_base: None,
        creatable: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use laterite_core::{Module, Registry};

    #[test]
    fn register_contributes_the_list_resource_and_permission() {
        let module = crate::LocationModule;
        let mut registry = Registry::new();
        // The framework stamps the owner before each module's register(); mirror
        // that here so add() has an active owner to attribute contributions to.
        registry.set_owner(module.id());
        module.register(&mut registry);

        let resources = registry.take::<Resource>();
        assert_eq!(resources.len(), 1);
        assert_eq!(resources[0].base_path, "/locations");
        assert_eq!(resources[0].list.entity, "rainmill_location_node");
        // List-only until the closure-aware write hook lands.
        assert!(resources[0].form.is_none());
        assert!(!resources[0].list.creatable);

        let permissions = registry.take::<Permission>();
        assert_eq!(permissions.len(), 1);
        assert_eq!(permissions[0].code, MANAGE);
    }
}
