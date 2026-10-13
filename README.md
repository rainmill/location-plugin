# Rainmill Location

A first-party [Laterite](https://github.com/lateritecmf/laterite) plugin: a
portable, curated **place tree** for any application that works with geographic
places, such as venues, stores, delivery areas, listings, or content regions.

Places form a flexible-depth tree (country down to neighbourhood) with a
maintained ancestor closure, name **aliases with search**, URL-friendly
**slugs**, and resolution by id. It runs on **any database** and exposes each
place as an **attach point** other modules reference by foreign key. Geospatial
features (polygon boundaries, point-in-polygon, nearest-place) come from the
companion [Rainmill Geo](https://github.com/rainmill/geo-plugin) plugin.

## Requirements

- Laterite (the module API).
- Database: **any** (PostgreSQL, MySQL, or SQLite).
- Optional: the `pg_trgm` and `unaccent` PostgreSQL extensions enable fuzzy alias
  search. Without them, alias search is exact and prefix only; the plugin still
  runs on every backend.

## Installation

Add the dependency:

```toml
[dependencies]
rainmill-location = "0.1"
```

Register the module when building the application:

```rust
laterite_admin::Bootstrap::new("config")
    .module(rainmill_location::LocationModule)
    .serve()
    .await
```

Registration runs the plugin's migrations (the `rainmill_location_*` tables) and
mounts its admin screens for curating places and aliases.

## Usage

The plugin manages a tree of place **nodes** and offers:

- **Tree curation**: create nodes at any depth with a level label
  (`country`, `city`, `zone`, ...), a canonical name, a slug, and a portable
  centroid (latitude/longitude). The admin lists places as a tree, and "Move
  to" puts a place and everything beneath it under another parent, with the
  closure rebuilt in the same transaction.
- **Aliases**: multiple names per place (`Bengaluru`, `Bangalore`) that resolve to
  one node; searchable by exact and prefix match, and fuzzy where the database
  supports it.
- **Ancestor and breadcrumb queries** over the closure table, and **slug/path
  lookup** (for URLs like `/india/karnataka/bengaluru`).

## Attaching a place to your model

Reference a place by foreign key to `rainmill_location_node.id`, and use the
`location_ref` field type in your form descriptor to get a node picker:

```rust
form.field(FieldType::location_ref("home_location_id", "Home city"));
```

Your module declares `requires("rainmill.location")` so its migrations and screens
load after this plugin's.

## Status

In active development. The schema and module registration are in place; the query
API, admin screens, and picker widget are landing incrementally.

## License

Licensed under MIT OR Apache-2.0.
