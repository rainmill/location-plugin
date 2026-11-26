//! The Features panel on a place's page: each registered feature with what it
//! does, whether it is on there and why, and a control saying On, Off or
//! Follow parent. A change is stored at once and the panel is drawn again.

use std::collections::HashMap;
use std::sync::Arc;

use laterite_admin::axum::extract::Path;
use laterite_admin::axum::http::StatusCode;
use laterite_admin::axum::response::{Html, IntoResponse, Response};
use laterite_admin::axum::routing::{get, post};
use laterite_admin::axum::{Extension, Form, Router};
use laterite_admin::nav::Placement;
use laterite_admin::routes::{RouteCtx, Screen, ScreenReg};
use laterite_admin::AuthenticatedUser;
use laterite_admin::Shell;

use crate::admin::node::MANAGE;
use crate::features::{self, LocationFeature, LocationFeatureReg, Source};
use crate::store;

/// Where the panel's routes mount under the plugin's base.
pub(crate) const FEATURES_PATH: &str = "/features";

/// The panel's routes, reachable by the place's page alone.
pub(crate) fn screen() -> ScreenReg {
    ScreenReg::new(FEATURES_PATH, MANAGE, Arc::new(Features)).menu(Placement::hidden())
}

struct Features;

impl Screen for Features {
    fn mount(&self, ctx: &RouteCtx) -> Router {
        let (show_ctx, set_ctx) = (ctx.clone(), ctx.clone());
        Router::new()
            .route(
                "/{id}",
                get(
                    move |Path(id): Path<i64>, Extension(shell): Extension<Shell>| {
                        let ctx = show_ctx.clone();
                        async move { panel(&ctx, id, &shell).await }
                    },
                ),
            )
            .route(
                "/{id}/{key}",
                post(
                    move |Path((id, key)): Path<(i64, String)>,
                          Extension(shell): Extension<Shell>,
                          Extension(user): Extension<AuthenticatedUser>,
                          Form(data): Form<HashMap<String, String>>| {
                        let ctx = set_ctx.clone();
                        async move { change(&ctx, id, &key, &data, &user, &shell).await }
                    },
                ),
            )
    }
}

/// The registered features, in the order modules added them.
fn registered(ctx: &RouteCtx) -> Vec<Arc<dyn LocationFeature>> {
    ctx.contributions::<LocationFeatureReg>()
        .into_iter()
        .map(|reg| reg.0.clone())
        .collect()
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

async fn panel(ctx: &RouteCtx, id: i64, shell: &Shell) -> Response {
    let Ok(Some(place)) = store::find(ctx.db(), id).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let base = ctx.url(&id.to_string());
    let mut rows = String::new();
    for feature in registered(ctx) {
        if !feature.applies_to(&place.level) {
            continue;
        }
        let Ok(decision) = features::decide(ctx.db(), feature.as_ref(), id).await else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        let state = if decision.enabled {
            shell.t("On")
        } else {
            shell.t("Off")
        };
        let why = match decision.source {
            Source::Here => shell.t("set here"),
            Source::Inherited(from) => match store::find(ctx.db(), from).await {
                Ok(Some(ancestor)) => format!("{} {}", shell.t("from"), ancestor.name),
                _ => shell.t("from a parent"),
            },
            Source::Computed => shell.t("worked out"),
            Source::Default => shell.t("by default"),
        };
        let here = matches!(decision.source, Source::Here);
        let choice = |value: &str, label: String, selected: bool| {
            format!(
                r#"<option value="{value}"{}>{}</option>"#,
                if selected { " selected" } else { "" },
                escape(&label)
            )
        };
        let label = shell.tt(&feature.label());
        rows.push_str(&format!(
            r#"<tr><td><strong>{}</strong><span class="lat-checklist__note">{}</span></td><td><span class="lat-badge" style="margin-left:0{}">{}</span> <span style="color:var(--text-muted)">{}</span></td><td><select class="lat-input lat-select" name="setting" aria-label="{}" hx-post="{}/{}" hx-trigger="change" hx-target="closest .lat-features" hx-swap="outerHTML">{}{}{}</select></td></tr>"#,
            escape(&label),
            escape(&shell.tt(&feature.description())),
            if decision.enabled {
                ";color:var(--text-heading);font-weight:600"
            } else {
                ""
            },
            escape(&state),
            escape(&why),
            escape(&label),
            escape(&base),
            escape(feature.key()),
            choice("", shell.t("Follow parent"), !here),
            choice("on", shell.t("On"), here && decision.enabled),
            choice("off", shell.t("Off"), here && !decision.enabled),
        ));
    }
    if rows.is_empty() {
        return Html(format!(
            r#"<div class="lat-features"><p style="color:var(--text-muted);margin:0">{}</p></div>"#,
            escape(&shell.t("No feature is offered at this place."))
        ))
        .into_response();
    }
    Html(format!(
        r#"<div class="lat-features"><table class="lat-table"><thead><tr><th>{}</th><th>{}</th><th>{}</th></tr></thead><tbody>{rows}</tbody></table></div>"#,
        escape(&shell.t("Feature")),
        escape(&shell.t("Here")),
        escape(&shell.t("Setting")),
    ))
    .into_response()
}

async fn change(
    ctx: &RouteCtx,
    id: i64,
    key: &str,
    data: &HashMap<String, String>,
    user: &AuthenticatedUser,
    shell: &Shell,
) -> Response {
    let Some(feature) = registered(ctx).into_iter().find(|f| f.key() == key) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(Some(place)) = store::find(ctx.db(), id).await else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let to = match data.get("setting").map(String::as_str).unwrap_or("") {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    };
    if features::set(ctx.db(), feature.as_ref(), id, to)
        .await
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let detail = format!(
        "{}: {}",
        key,
        match to {
            Some(true) => "on",
            Some(false) => "off",
            None => "follows parent",
        }
    );
    ctx.audit(
        user,
        "rainmill.location.feature",
        Some("rainmill_location_node"),
        Some(&id.to_string()),
        Some(&place.name),
        Some(&detail),
    )
    .await;
    panel(ctx, id, shell).await
}
