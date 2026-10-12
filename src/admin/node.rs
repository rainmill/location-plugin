//! The plugin's admin surface: a list and a create/edit form for curating place
//! nodes, plus the write and picker services they need.
//!
//! Contributed through the module's `register`, so the framework's generic
//! handlers render the screens from these descriptors rather than any hand-written
//! CRUD. A node write maintains the ancestor closure (see [`store::create_node`]),
//! which the generic form cannot do, so the form names a [`NodePersister`]. The
//! parent field is a reference picker over the node table itself.

use std::sync::Arc;

use laterite_admin::form::{FormConfig, FormField};
use laterite_admin::relation::RelationConfig;
use laterite_admin::ImportConfig;
use laterite_admin::list::{ListColumn, ListConfig, SortDir};
use laterite_admin::picker::{PickerSourceReg, TableSource};
use laterite_admin::{Permission, Resource};

/// The permission gating the location screens, offered in the role editor.
pub(crate) const MANAGE: &str = "rainmill.location.manage";

/// The node table, shared by the descriptors, the persister, and the picker.
pub(crate) const NODE_ENTITY: &str = "rainmill_location_node";
/// The registry key of the node write handler and of the node picker source (they
/// live in separate registries, so one dotted name serves both).
pub(crate) const NODE_KEY: &str = "rainmill.location.node";

/// The permission the plugin registers.
pub(crate) fn permission() -> Permission {
    Permission::new(MANAGE, "Manage locations", "Location")
        .roles([laterite_admin::ROLE_ADMIN, laterite_admin::ROLE_EDITOR])
}

/// The place-node resource: a list plus a create/edit form, gated by [`MANAGE`].
pub(crate) fn resource() -> Resource {
    Resource::new("/locations", "Locations", node_list_config())
        .form(node_form_config())
        .permission(MANAGE)
        .import(ImportConfig::new())
        // A node's children on its own edit page: the same list and form, scoped
        // by the parent column the persister already maintains the closure for.
        .relation(
            RelationConfig::has_many(
                "children",
                "Child location",
                NODE_ENTITY,
                "parent_id",
                node_list_config(),
                node_form_config(),
            )
            .title("Child locations"),
        )
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
