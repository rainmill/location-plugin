//! Rainmill Location: a portable, curated place tree.
//!
//! Nodes form a flexible-depth tree (country down to locality) with a maintained
//! ancestor closure, name aliases, and resolution by id. It runs on any backend
//! and exposes each place as an attach point that other modules reference by
//! foreign key. Geospatial features (polygon boundaries, point-in-polygon
//! resolution) live in the companion Rainmill Geo plugin.
//!
//! The migrations, query API, admin screens, and node-picker land incrementally.

mod admin;
pub mod migrations;
mod schema;
pub mod store;

use laterite_core::{MigrationSet, Module, ModuleId, Registry};

/// This plugin's entry point. Every Laterite plugin exposes `module()`, so the
/// generated `plugins-manifest` collects it without naming the type.
pub fn module() -> Box<dyn Module> {
    Box::new(LocationModule)
}

/// The Rainmill Location plugin.
pub struct LocationModule;

impl Module for LocationModule {
    fn id(&self) -> ModuleId {
        ModuleId::new(migrations::MODULE_ID)
    }

    fn migrations(&self) -> MigrationSet {
        migrations::migrations()
    }

    fn register(&self, registry: &mut Registry) {
        use laterite_admin::AdminRegistry;
        registry.add_permission(admin::permission());
        registry.add_resource(admin::resource());
        registry.add_persister(admin::persister());
        registry.add_picker_source(admin::picker_source());
    }
}
