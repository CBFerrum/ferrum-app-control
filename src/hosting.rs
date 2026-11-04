//! Hosted workspaces, incidents and recovery requests. A workspace left without a cell is placed in
//! its region's shared cell with the most room, and none is placed in a full or a down one. Its
//! health is read from its own address with the health-only support key its customer's
//! administrator gave (`/api/support/report`), and from nothing else: the control plane holds no
//! record of it. An incident's every change of status is said in an update, and updates stay as
//! they were posted. A recovery request is decided by an operator other than who asked for it.

use crate::fleet::{placed, refresh_cell};
use crate::{at, obj, operator};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use ferrum_domain::util::{elevated, id, rows, s};
use ferrum_kernel::{Ctx, Doc, DocHook, Error, HookCtx, Kernel, Result};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

/// A workspace's address: https, or plain http on this machine for development.
pub(crate) fn good_address(v: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(v) else { return false };
    let host = u.host_str().unwrap_or_default();
    let scheme = match u.scheme() {
        "https" => !host.is_empty(),
        "http" => matches!(host, "localhost" | "127.0.0.1"),
        _ => false,
    };
    scheme && matches!(u.path(), "" | "/") && u.query().is_none() && u.username().is_empty()
}

/// A workspace's slug: lower-case letters, digits and hyphens.
fn good_slug(v: &str) -> bool {
    (2..=63).contains(&v.len()) && v.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') && !v.starts_with('-')
}

pub struct WorkspaceHook;

#[async_trait]
impl DocHook for WorkspaceHook {
    fn name(&self) -> &'static str {
        "control.workspace"
    }

    fn fills(&self) -> &'static [&'static str] {
        &["cell", "template_revision"]
    }

    async fn before_validate(&self, hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        if let Some(slug) = s(doc, "slug").map(|v| v.trim().to_lowercase()) {
            doc.insert("slug".into(), json!(slug));
        }
        if id(doc, "cell").is_some() || s(doc, "status") == Some("Closed") {
            return Ok(());
        }
        let Some(region) = s(doc, "region").map(str::to_string) else { return Ok(()) };
        // The region's shared cell with the most room, and not a down one.
        let cell: Option<Uuid> = sqlx::query_scalar(
            "with room as (select c.id, c.cell_name, c.capacity - (select count(*) from e_hosted_workspace w where w.tenant_id = $1 and w.cell = c.id \
             and w.status <> 'Closed' and w.id is distinct from $3) as free from e_cell c where c.tenant_id = $1 and c.region = $2 and c.cell_kind = 'Shared' \
             and c.health is distinct from 'Down') select id from room where free > 0 order by free desc, cell_name limit 1",
        )
        .bind(hc.ctx.tenant)
        .bind(&region)
        .bind(id(doc, "id"))
        .fetch_optional(&mut **hc.tx)
        .await?;
        if let Some(c) = cell {
            doc.insert("cell".into(), json!(c.to_string()));
        }
        Ok(())
    }

    async fn validate(&self, hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        if !s(doc, "slug").is_some_and(good_slug) {
            return Err(Error::field("slug", "is lower-case letters, digits and hyphens"));
        }
        if !s(doc, "address").is_some_and(good_address) {
            return Err(Error::field("address", "is https://… where the workspace answers"));
        }
        let region = s(doc, "region").unwrap_or_default();
        let Some(cell_id) = id(doc, "cell") else {
            return Err(Error::field("cell", format!("no shared cell in {region} has room")));
        };
        let cell = hc.kernel.get_tx(&elevated(hc.ctx), hc.tx, "Cell", cell_id, false).await?;
        let name = s(&cell, "cell_name").unwrap_or_default();
        if s(&cell, "region") != Some(region) {
            return Err(Error::field("cell", format!("{name} is in {}, not {region}", s(&cell, "region").unwrap_or_default())));
        }
        if s(doc, "status") != Some("Closed") {
            let others = placed(hc.tx, hc.ctx.tenant, cell_id, id(doc, "id")).await?;
            if crate::int(&cell, "capacity").is_some_and(|c| others >= c) {
                return Err(Error::field("cell", format!("{name} is full")));
            }
        }
        Ok(())
    }

    async fn after_save(&self, hc: &mut HookCtx<'_>, doc: &Doc, old: Option<&Doc>) -> Result<()> {
        let system = elevated(hc.ctx);
        let me = id(doc, "id").unwrap_or_default();
        // The setup it was made from, as that setup stood.
        if id(doc, "template").is_some() && old.is_none_or(|o| id(o, "template") != id(doc, "template")) {
            let template = hc.kernel.get_tx(&system, hc.tx, "SetupTemplate", id(doc, "template").unwrap_or_default(), false).await?;
            let revision = template.get("revision").cloned().unwrap_or(json!(1));
            hc.kernel.set_values_tx(&system, hc.tx, "HostedWorkspace", me, obj(json!({"template_revision": revision}))).await?;
        }
        let mut cells: Vec<Uuid> = [id(doc, "cell"), old.and_then(|o| id(o, "cell"))].into_iter().flatten().collect();
        cells.dedup();
        for c in cells {
            refresh_cell(hc.kernel, &system, hc.tx, c, Utc::now()).await?;
        }
        Ok(())
    }
}

/// Reads every open workspace's health from its address with its support key, holding no
/// transaction while it asks.
pub(crate) async fn read_health(k: &Kernel, ctx: &Ctx, now: DateTime<Utc>) -> Result<()> {
    let mut tx = k.db.tenant_tx(ctx.tenant).await?;
    let mut asks = Vec::new();
    for w in k.find_tx(ctx, &mut tx, "HostedWorkspace", &[], None, 10_000).await? {
        let Some(me) = id(&w, "id").filter(|_| s(&w, "status") != Some("Closed")) else { continue };
        let key = k.secret(&mut tx, ctx.tenant, "HostedWorkspace", me, "health_key").await?;
        asks.push((me, s(&w, "address").unwrap_or_default().to_string(), key));
    }
    tx.commit().await?;
    for (me, address, key) in asks {
        let (health, problems, version) = match key {
            Some(key) => ask(k, &address, &key).await,
            None => ("Unknown", vec!["no support key to read its health with".into()], None),
        };
        let problems = if problems.is_empty() { Value::Null } else { json!(problems.join("; ")) };
        let values = json!({"health": health, "health_problems": problems, "running_version": version, "checked_at": now.to_rfc3339()});
        let mut tx = k.db.tenant_tx(ctx.tenant).await?;
        k.set_values_tx(ctx, &mut tx, "HostedWorkspace", me, obj(values)).await?;
        tx.commit().await?;
    }
    Ok(())
}

/// What the workspace at `address` says of itself to a health-only key.
async fn ask(k: &Kernel, address: &str, key: &str) -> (&'static str, Vec<String>, Option<String>) {
    let unreachable = |why: String| ("Unreachable", vec![why], None);
    let url = format!("{}/api/support/report", address.trim_end_matches('/'));
    let (client, url) = match k.vetted_client(&url, k.settings.webhooks_allow_private, Duration::from_secs(10), "Ferrum control plane").await {
        Ok(c) => c,
        Err(e) => return unreachable(e.to_string()),
    };
    let answer = match client.get(url).bearer_auth(key).send().await {
        Ok(a) => a,
        Err(e) => return unreachable(format!("it cannot be reached: {e}")),
    };
    match answer.status().as_u16() {
        200..=299 => {}
        401 | 403 => return unreachable("its support key was refused: the access has ended or was revoked".into()),
        code => return unreachable(format!("it answered {code}")),
    }
    let Ok(report) = answer.json::<Value>().await else { return unreachable("its report could not be read".into()) };
    let n = |p: &str| report.pointer(p).and_then(Value::as_i64).unwrap_or(0);
    let mut problems = Vec::new();
    if n("/events/dead") > 0 {
        problems.push(format!("{} events could not be delivered", n("/events/dead")));
    }
    if n("/jobs/failed") > 0 {
        problems.push(format!("{} jobs failed", n("/jobs/failed")));
    }
    if n("/events/pending") > 1000 {
        problems.push(format!("{} events are waiting", n("/events/pending")));
    }
    let version = report.get("version").and_then(Value::as_str).map(str::to_string);
    (if problems.is_empty() { "Healthy" } else { "Degraded" }, problems, version)
}

pub struct IncidentHook;

#[async_trait]
impl DocHook for IncidentHook {
    fn name(&self) -> &'static str {
        "control.incident"
    }

    fn fills(&self) -> &'static [&'static str] {
        &["started_at", "resolved_at", "updates"]
    }

    async fn before_validate(&self, _hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        let now = json!(Utc::now().to_rfc3339());
        if at(doc, "started_at").is_none() {
            doc.insert("started_at".into(), now.clone());
        }
        if let Some(Value::Array(updates)) = doc.get_mut("updates") {
            for u in updates.iter_mut().filter_map(Value::as_object_mut) {
                if u.get("posted_at").is_none_or(Value::is_null) {
                    u.insert("posted_at".into(), now.clone());
                }
            }
        }
        let resolved = match s(doc, "status") {
            Some("Resolved") => doc.get("resolved_at").filter(|v| !v.is_null()).cloned().unwrap_or(now),
            _ => Value::Null,
        };
        doc.insert("resolved_at".into(), resolved);
        Ok(())
    }

    async fn validate(&self, _hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        let updates = rows(doc, "updates");
        if updates.last().and_then(|u| s(u, "status")) != s(doc, "status") {
            return Err(Error::field("updates", format!("the latest update says the incident is {}", s(doc, "status").unwrap_or_default())));
        }
        Ok(())
    }

    async fn validate_change(&self, _hc: &mut HookCtx<'_>, old: Option<&Doc>, doc: &Doc) -> Result<()> {
        let Some(old) = old else { return Ok(()) };
        let (was, now) = (rows(old, "updates"), rows(doc, "updates"));
        let kept = |u: &Doc| now.iter().any(|n| id(n, "id") == id(u, "id") && s(n, "message") == s(u, "message") && s(n, "status") == s(u, "status"));
        if !was.iter().all(kept) {
            return Err(Error::field("updates", "an update stays as it was posted"));
        }
        if s(old, "status") != s(doc, "status") && now.len() <= was.len() {
            return Err(Error::field("updates", "a change of status is said in a new update"));
        }
        Ok(())
    }
}

pub struct RecoveryHook;

#[async_trait]
impl DocHook for RecoveryHook {
    fn name(&self) -> &'static str {
        "control.recovery"
    }

    fn fills(&self) -> &'static [&'static str] {
        &["requested_by"]
    }

    async fn before_validate(&self, hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        if s(doc, "requested_by").is_none() && !hc.ctx.email.is_empty() {
            doc.insert("requested_by".into(), json!(hc.ctx.email.to_lowercase()));
        }
        Ok(())
    }

    async fn validate(&self, _hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        if s(doc, "recovery_kind") == Some("Restore") && at(doc, "restore_to").is_none_or(|t| t >= Utc::now()) {
            return Err(Error::field("restore_to", "a restore goes back to a moment that has passed"));
        }
        Ok(())
    }

    async fn action(&self, hc: &mut HookCtx<'_>, name: &str, doc: &Doc, params: &Value) -> Result<Option<Value>> {
        if !operator(hc.ctx) {
            return Err(Error::Forbidden("a Fleet Operator decides recovery requests".into()));
        }
        let request = id(doc, "id").ok_or_else(|| Error::internal("a request without an id"))?;
        let values = match name {
            "approve" => {
                if s(doc, "requested_by").is_some_and(|r| r.eq_ignore_ascii_case(&hc.ctx.email)) {
                    return Err(Error::Forbidden("a recovery is approved by someone other than who asked for it".into()));
                }
                json!({"status": "Approved", "decided_by": hc.ctx.email})
            }
            "refuse" => {
                let note = params.get("note").and_then(Value::as_str).map(str::trim).filter(|n| !n.is_empty());
                let note = note.ok_or_else(|| Error::field("note", "refusing a recovery says why"))?;
                json!({"status": "Refused", "decided_by": hc.ctx.email, "decision_note": note})
            }
            "complete" => json!({"status": "Done"}),
            _ => return Ok(None),
        };
        hc.kernel.set_values_tx(&elevated(hc.ctx), hc.tx, "RecoveryRequest", request, obj(values.clone())).await?;
        Ok(Some(json!({"status": values["status"]})))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_and_slugs() {
        assert!(good_address("https://acme.eu1.ferrum.cloud") && good_address("http://127.0.0.1:8080") && good_address("https://a.example/"));
        assert!(
            !good_address("http://acme.example")
                && !good_address("https://acme.example/api")
                && !good_address("ftp://x")
                && !good_address("https://u@acme.example")
        );
        assert!(good_slug("acme-eu") && !good_slug("Acme") && !good_slug("-acme") && !good_slug("a"));
    }
}
