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

## Features by place

A module declares a feature once, and each place says On, Off or nothing about
it. A place that says nothing follows its nearest ancestor that does. Where
none does, the switch set for everywhere applies, and where there is none the
feature answers for itself: what it works out from data, or its default.

The Location features screen, in Settings, lists every feature with its switch
for everywhere and the places that differ. Every place's page shows the same
features with a control each.

```rust
use rainmill_location::features::{self, LocationFeature, LocationRegistry};

struct Guestlist;

impl LocationFeature for Guestlist {
    fn key(&self) -> &'static str { "acme.guestlist" }
    fn label(&self) -> Text { t!("Guestlist") }
    fn description(&self) -> Text { t!("Guests can ask for a place on the list.") }
}

registry.add_location_feature(Arc::new(Guestlist));

let on = features::enabled(db, &Guestlist, place_id).await?;
let places = features::enabled_nodes(db, &Guestlist).await?;
features::set_everywhere(db, &Guestlist, Some(false)).await?;
```

| Method | Default | Meaning |
| --- | --- | --- |
| `key`, `label`, `description` | required | The stored key, and the words beside the switch. |
| `default_enabled` | `false` | The answer where nothing else gives one. |
| `applies_to(level)` | every level | Whether a place at that level is offered the switch. |
| `computed(db, node_id)` | none | An answer worked out from data. A place that says On or Off always wins. |
| `changed(db, node_id, setting)` | nothing | Called after a place's own switch changed. |

`features::Open` is built in: whether the place itself is offered. It is on
unless a place, or one above it, is switched off.

### Settings of a feature by place

A feature with settings of its own for a place (a fee, a limit) keeps them in a
table of its own with a `node_id` column, and adds it to the place's page as a
section:

```rust
registry.add(ResourceHook::new("/rainmill/location/locations", |resource| {
    resource.relation(RelationConfig::has_many("fees", "Fee", "acme_guestlist_fee", "node_id", list, form))
}));

// The place whose row applies: the place itself, else its nearest ancestor with one.
let from = features::nearest_with_row(db, "acme_guestlist_fee", "node_id", place_id).await?;
```

## Status

In active development. The schema and module registration are in place; the query
API, admin screens, and picker widget are landing incrementally.

## License

Licensed under MIT OR Apache-2.0.
