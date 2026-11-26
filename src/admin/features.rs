//! Features by place, from both ends. The Location features screen lists
//! every registered feature with its switch for everywhere and the places
//! that say otherwise. The Features panel on a place's page lists the same
//! features with whether each is on there and why, and a control saying On,
//! Off or Follow parent. A change is stored at once and drawn again.

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
use laterite_core::t;

use crate::admin::node::MANAGE;
use crate::features::{self, LocationFeature, LocationFeatureReg, Source};
use crate::store;

/// Where the panel's routes mount under the plugin's base.
pub(crate) const FEATURES_PATH: &str = "/features";

/// The Location features screen, listed in Settings beside Locations, and
/// the routes a place's panel calls.
pub(crate) fn screen() -> ScreenReg {
    ScreenReg::new(FEATURES_PATH, MANAGE, Arc::new(Features))
        .in_menu(t!("Location features"))
        .menu(
            Placement::new()
                .icon("flag")
                .in_settings("System")
                .description(t!("What is switched on where, feature by feature.")),
        )
}

struct Features;

impl Screen for Features {
    fn mount(&self, ctx: &RouteCtx) -> Router {
        let (show_ctx, set_ctx) = (ctx.clone(), ctx.clone());
        let (list_ctx, everywhere_ctx) = (ctx.clone(), ctx.clone());
        Router::new()
            .route(
                "/",
                get(move |Extension(shell): Extension<Shell>| {
                    let ctx = list_ctx.clone();
                    async move {
                        match overview(&ctx, &shell).await {
                            Some(body) => shell.page(
                                &t!("Location features"),
                                laterite_admin::html::Markup::raw_unchecked(body),
                            ),
                            None => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
                        }
                    }
                }),
            )
            .route(
                "/everywhere/{key}",
                post(
                    move |Path(key): Path<String>,
                          Extension(shell): Extension<Shell>,
                          Extension(user): Extension<AuthenticatedUser>,
                          Form(data): Form<HashMap<String, String>>| {
                        let ctx = everywhere_ctx.clone();
                        async move { change_everywhere(&ctx, &key, &data, &user, &shell).await }
                    },
                ),
            )
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
            Source::Everywhere => shell.t("set for everywhere"),
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

/// The Location features screen's body: each feature, its switch for
/// everywhere, and the places that say On or Off themselves.
async fn overview(ctx: &RouteCtx, shell: &Shell) -> Option<String> {
    let places = format!(
        "{}/rainmill/location/locations",
        ctx.admin_path().trim_end_matches('/')
    );
    let mut rows = String::new();
    for feature in registered(ctx) {
        let set = features::everywhere(ctx.db(), feature.key()).await.ok()?;
        let own = if feature.default_enabled() {
            shell.t("Left to the feature (on)")
        } else {
            shell.t("Left to the feature (off)")
        };
        let choice = |value: &str, label: String, selected: bool| {
            format!(
                r#"<option value="{value}"{}>{}</option>"#,
                if selected { " selected" } else { "" },
                escape(&label)
            )
        };
        let mut differing = String::new();
        for (id, name, on) in features::said(ctx.db(), feature.key()).await.ok()? {
            differing.push_str(&format!(
                r#"<a class="lat-badge" style="margin:0 6px 4px 0;text-decoration:none" href="{}/{id}/edit">{}: {}</a>"#,
                escape(&places),
                escape(&name),
                escape(&if on { shell.t("On") } else { shell.t("Off") }),
            ));
        }
        if differing.is_empty() {
            differing = format!(
                r#"<span style="color:var(--text-muted)">{}</span>"#,
                escape(&shell.t("No place says otherwise"))
            );
        }
        let label = shell.tt(&feature.label());
        rows.push_str(&format!(
            r#"<tr><td><strong>{}</strong><span class="lat-checklist__note">{}</span></td><td><select class="lat-input lat-select" name="setting" aria-label="{}" hx-post="{}" hx-trigger="change" hx-target="closest .lat-features" hx-swap="outerHTML">{}{}{}</select></td><td>{differing}</td></tr>"#,
            escape(&label),
            escape(&shell.tt(&feature.description())),
            escape(&label),
            escape(&ctx.url(&format!("everywhere/{}", feature.key()))),
            choice("", own, set.is_none()),
            choice("on", shell.t("On"), set == Some(true)),
            choice("off", shell.t("Off"), set == Some(false)),
        ));
    }
    Some(format!(
        r#"<div class="lat-features"><div class="lat-card"><table class="lat-table"><thead><tr><th>{}</th><th>{}</th><th>{}</th></tr></thead><tbody>{rows}</tbody></table><p class="lat-field__help" style="margin:12px 0 0">{}</p></div></div>"#,
        escape(&shell.t("Feature")),
        escape(&shell.t("Everywhere")),
        escape(&shell.t("Places that differ")),
        escape(&shell.t(
            "A place that says On or Off goes its own way, with everything beneath it. Change that on the place's own page."
        )),
    ))
}

async fn change_everywhere(
    ctx: &RouteCtx,
    key: &str,
    data: &HashMap<String, String>,
    user: &AuthenticatedUser,
    shell: &Shell,
) -> Response {
    let Some(feature) = registered(ctx).into_iter().find(|f| f.key() == key) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let to = match data.get("setting").map(String::as_str).unwrap_or("") {
        "on" => Some(true),
        "off" => Some(false),
        _ => None,
    };
    if features::set_everywhere(ctx.db(), feature.as_ref(), to)
        .await
        .is_err()
    {
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    }
    let detail = format!(
        "{}: {}",
        key,
        match to {
            Some(true) => "on everywhere",
            Some(false) => "off everywhere",
            None => "left to the feature",
        }
    );
    ctx.audit(
        user,
        "rainmill.location.feature",
        None,
        None,
        None,
        Some(&detail),
    )
    .await;
    match overview(ctx, shell).await {
        Some(body) => Html(body).into_response(),
        None => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}
