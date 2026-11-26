# Changelog

Notable changes to `rainmill-location`. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/) as Cargo reads it: before 1.0 a minor
bump is breaking and a patch is additive.

## [Unreleased]

### Added

- Features by place: a module declares a `LocationFeature`, each place says On, Off or follows its parent, and `features::enabled` answers by the nearest place that says. `features::Open` is built in, and a place's page shows every feature with its switch.
- A Location features screen in Settings: each feature's switch for everywhere (`features::set_everywhere`) and the places that differ.
- `features::nearest_with_row` finds whose row applies for a feature that keeps settings of its own by place.
- Other names for a place: an Also known as section on each location, holding any number of names with a language and an official mark.
- `store::search` finds a place by its canonical name or any alias, best match first, and forgives a misspelling on PostgreSQL with `pg_trgm` (declared as an optional capability). `PlaceSource` is a picker source over it for an application's own forms; the plugin's picker and the command palette use it.
- `store::aliases` and `store::find`; `store::add_alias` keeps the alias a place already has.

### Changed

- A location's name is no longer editable from the list.
- Locations is listed in Settings under System, with a description. A menu move for `resource:/rainmill/location/locations` puts it back in the menu.
- Requires Laterite 0.12.2.

## [0.1.0] - 2026-11-10

### Added

- First release. A curated tree of places with alternative names and lookup, for the Laterite framework. Runs on PostgreSQL, MySQL or SQLite.
