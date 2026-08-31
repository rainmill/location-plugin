# Rainmill Location

A first-party [Laterite](https://github.com/lateritecmf/laterite) plugin: a
portable, curated place tree.

Nodes form a flexible-depth tree (country down to locality) with a maintained
ancestor closure, name aliases, and resolution by id. It runs on any backend and
exposes each place as an attach point other modules reference by foreign key.
Geospatial features (polygon boundaries, point-in-polygon) live in the separate
[Rainmill Geo](https://github.com/lateritecmf/rainmill-geo) plugin.

## Development

Laterite is unpublished and consumed by path, so check it out beside this repo:

```
parent/
  laterite/
  rainmill-location/
```

Then `cargo build` and `cargo test`.
