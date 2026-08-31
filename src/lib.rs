//! Rainmill Location: a portable, curated place tree.
//!
//! Nodes form a flexible-depth tree (country down to locality) with a maintained
//! ancestor closure, name aliases, and resolution by id. It runs on any backend
//! and exposes each place as an attach point that other modules reference by
//! foreign key. Geospatial features (polygon boundaries, point-in-polygon
//! resolution) live in the companion Rainmill Geo plugin.
//!
//! The migrations, query API, admin screens, and node-picker land incrementally.

use laterite_core::{Module, ModuleId};

/// The Rainmill Location plugin.
pub struct LocationModule;

impl Module for LocationModule {
    fn id(&self) -> ModuleId {
        ModuleId::new("rainmill.location")
    }
}
