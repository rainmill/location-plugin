//! The place-node write handler.
//!
//! A node's write is not one statement: creating one also writes its ancestor
//! closure, and a node whose closure is half-written is broken in a way no later
//! write repairs. The pipeline's transaction is carried in `SaveCx`, so these
//! extra statements commit or roll back with the row itself.

use std::collections::HashMap;

use std::sync::Arc;

use laterite_admin::persist::{MoveTo, Persister, PersisterReg, SaveError};
use laterite_core::strata::async_trait;
use laterite_core::validation::ErrorBag;
use laterite_core::{t, Record, SaveCx};

use crate::admin::node::NODE_KEY;
use crate::store::{self, NewNode, NodeEdit};

/// The node write handler, registered under [`NODE_KEY`].
pub(crate) fn persister() -> PersisterReg {
    PersisterReg::new(NODE_KEY, Arc::new(NodePersister))
}
/// The location node write handler. Create maintains the ancestor closure
/// atomically (see [`store::create_node`]); update edits a node's fields but
/// rejects re-parenting, which is "Move to" on the list (see
/// [`store::move_node`]), where the subtree's closure is rebuilt with it.
pub(crate) struct NodePersister;

#[async_trait]
impl Persister for NodePersister {
    async fn create(&self, cx: &mut SaveCx<'_>, rec: &Record) -> Result<i64, SaveError> {
        // The helpers below read the submission as text; the record's own typed
        // values arrive with the field-type save contract.
        let data = &rec.to_text_map();
        let new = NewNode {
            parent_id: opt_i64(data, "parent_id"),
            level: req(data, "level"),
            canonical_name: req(data, "canonical_name"),
            slug: req(data, "slug"),
            timezone: opt(data, "timezone"),
            currency_code: opt(data, "currency_code"),
            locale: opt(data, "locale"),
            centroid: None,
        };
        // Writes on the pipeline's transaction, so the node row and its closure
        // commit with the rest of the save or not at all.
        let backend = cx.backend();
        store::create_node_in(backend, cx.conn(), new)
            .await
            .map_err(|e| SaveError::Failed(e.to_string()))
    }

    async fn update(&self, cx: &mut SaveCx<'_>, id: &str, rec: &Record) -> Result<(), SaveError> {
        let data = &rec.to_text_map();
        let backend = cx.backend();
        let node_id: i64 = id
            .parse()
            .map_err(|_| SaveError::Failed("invalid node id".to_string()))?;
        // Re-parenting would move a whole subtree and rebuild its closure, which
        // this write does not do: a changed parent is a per-field rejection.
        match store::parent_of_in(backend, cx.conn(), node_id)
            .await
            .map_err(|e| SaveError::Failed(e.to_string()))?
        {
            None => return Err(SaveError::Failed("location not found".to_string())),
            Some(current_parent) => {
                if opt_i64(data, "parent_id") != current_parent {
                    let mut bag = ErrorBag::default();
                    bag.add(
                        "parent_id",
                        t!("Use Move to on the list to put a location under another parent."),
                    );
                    return Err(SaveError::Invalid(bag));
                }
            }
        }
        let edit = NodeEdit {
            level: req(data, "level"),
            canonical_name: req(data, "canonical_name"),
            slug: req(data, "slug"),
            timezone: opt(data, "timezone"),
            currency_code: opt(data, "currency_code"),
            locale: opt(data, "locale"),
        };
        store::update_node_in(backend, cx.conn(), node_id, edit)
            .await
            .map_err(|e| SaveError::Failed(e.to_string()))
    }

    async fn move_to(&self, cx: &mut SaveCx<'_>, mv: &MoveTo<'_>) -> Result<(), SaveError> {
        let node_id: i64 = mv
            .id
            .parse()
            .map_err(|_| SaveError::Failed("invalid node id".to_string()))?;
        let parent = match mv.parent {
            Some(parent) => Some(
                parent
                    .parse::<i64>()
                    .map_err(|_| SaveError::Failed("invalid parent id".to_string()))?,
            ),
            None => None,
        };
        let backend = cx.backend();
        store::move_node_in(backend, cx.conn(), node_id, parent)
            .await
            .map_err(|e| SaveError::Failed(e.to_string()))
    }
}

/// A trimmed, non-empty submission value, or None.
fn opt(data: &HashMap<String, String>, key: &str) -> Option<String> {
    data.get(key)
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// A required value: the form's rules have already ensured it is present.
fn req(data: &HashMap<String, String>, key: &str) -> String {
    opt(data, key).unwrap_or_default()
}

/// A submission value parsed as an id, or None when absent or unparsable.
fn opt_i64(data: &HashMap<String, String>, key: &str) -> Option<i64> {
    opt(data, key).and_then(|s| s.parse().ok())
}
