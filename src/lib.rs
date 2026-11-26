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
pub mod features;
pub mod migrations;
mod schema;
pub mod store;

use laterite_core::{Capability, MigrationSet, Module, ModuleId, Registry};

pub use admin::PlaceSource;

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

    /// Name search forgives a misspelling where `pg_trgm` is installed, and
    /// matches by exact name, prefix and substring where it is not.
    fn optional_db_capabilities(&self) -> &'static [Capability] {
        const CAPS: &[Capability] = &[Capability::new("pg_trgm")];
        CAPS
    }

    fn register(&self, registry: &mut Registry) {
        use features::LocationRegistry;
        use laterite_admin::AdminRegistry;
        registry.add_permission(admin::permission());
        registry.add_resource(admin::resource());
        registry.add_persister(admin::persister());
        registry.add_picker_source(admin::picker_source());
        registry.add(admin::alias_listener());
        registry.add_screen(admin::features_screen());
        registry.add_location_feature(std::sync::Arc::new(features::Open));
    }
}
