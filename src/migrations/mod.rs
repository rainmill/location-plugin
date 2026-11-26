//! This plugin's migrations, in apply order.

laterite_core::migration_set! {
    module_id: "rainmill.location",
    m0001_create_location,
    m0002_alias_search,
    m0003_create_feature,
    m0004_create_feature_default,
}

#[cfg(test)]
mod tests {
    use laterite_core::testing::connect_test;

    #[tokio::test]
    async fn migration_applies_on_the_test_backend() {
        // connect_test applies the migration set and panics on failure; reaching
        // the end means the portable schema built on the configured backend
        // (SQLite in-memory by default), proving it is backend-portable.
        let (_db, _guard) = connect_test(&[super::migrations()]).await;
    }
}
