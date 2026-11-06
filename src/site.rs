//! The control plane's endpoints.
//!
//! Public, at `/api/site/{workspace}/control/…`:
//! * `POST /report`: a cell's readings, sent by its tooling with the cell's name and token
//!   (`x-control-cell`, `x-control-token`).
//! * `GET /status`: each cell's state by region, open incidents and coming maintenance. It names
//!   no customer.
//!
//! Signed in, at `/api/app/control/…`:
//! * `GET /mine`: the workspaces the caller is the contact for, with their health, their cell's,
//!   coming maintenance, incidents, recovery requests and cases.
//! * `GET /partner`: a partner member's portfolio, statements and setup templates.
//! * `POST /partner/onboard`: a workspace for a partner's new customer, made on its cell at once when
//!   its first administrator is named.
//! * `POST /case`: a support case for a workspace the caller is the contact for, or whose partner
//!   they are with.

use crate::fleet::{is_digest, refresh_cell, window};
use crate::{at, obj};
use chrono::{DateTime, Duration, NaiveDate, Utc};
use ferrum_domain::util::{elevated, id, rows, s, sys};
use ferrum_kernel::{Ctx, Doc, Error, Filter, Kernel, Result, Site, SiteRequest, SiteResponse, Tx};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;

pub struct ControlSite;

#[async_trait::async_trait]
impl Site for ControlSite {
    async fn handle(&self, k: &Kernel, req: SiteRequest) -> Result<SiteResponse> {
        let path = req.path.trim_matches('/').to_string();
        match (req.method.as_str(), path.as_str(), req.user.as_ref()) {
            ("POST", "report", _) => report(k, &req).await,
            ("GET", "status", _) => status(k, req.tenant).await,
            (_, _, None) => Err(Error::Unauthorized("sign in first".into())),
            ("GET", "mine", Some(u)) => mine(k, u).await,
            ("GET", "partner", Some(u)) => partner(k, u).await,
            ("POST", "partner/onboard", Some(u)) => onboard(k, u, &req.body).await,
            ("POST", "case", Some(u)) => case(k, u, &req.body).await,
            _ => Err(Error::NotFound(format!("control endpoint {} {}", req.method, req.path))),
        }
    }
}

fn reply(v: Value) -> SiteResponse {
    let mut r = SiteResponse::json(200, &v);
    r.headers.push(("cache-control".into(), "no-store".into()));
    r
}

/// Equal, taking as long whatever the bytes.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn body(raw: &[u8]) -> Result<Doc> {
    serde_json::from_slice::<Value>(raw).ok().and_then(|v| v.as_object().cloned()).ok_or_else(|| Error::invalid("the body is a JSON object"))
}

async fn report(k: &Kernel, req: &SiteRequest) -> Result<SiteResponse> {
    let header = |n: &str| req.headers.get(n).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty());
    let (name, token) =
        header("x-control-cell").zip(header("x-control-token")).ok_or_else(|| Error::Unauthorized("a cell reports with its name and its token".into()))?;
    k.rate_limit(&format!("control.report:{}:{name}", req.tenant), std::time::Duration::from_secs(3600), 120, "a cell reports at most twice a minute").await?;
    let ctx = Ctx::system(req.tenant, k.registry.schema(req.tenant).await?);
    let mut tx = k.db.tenant_tx(req.tenant).await?;
    let cell = k.find_tx(&ctx, &mut tx, "Cell", &[Filter::eq("cell_name", name)], None, 1).await?.first().and_then(|c| id(c, "id"));
    let held = match cell {
        Some(c) => k.secret(&mut tx, req.tenant, "Cell", c, "report_token").await?,
        None => None,
    };
    // An unknown cell and a wrong token are refused alike.
    let Some(cell) = cell.filter(|_| held.as_deref().is_some_and(|h| same(h.as_bytes(), token.as_bytes()))) else {
        return Err(Error::Forbidden("that is not this cell's token".into()));
    };
    let r = body(&req.body)?;
    let digest = s(&r, "digest").filter(|d| is_digest(d)).ok_or_else(|| Error::field("digest", "is sha256: and 64 lower-case hexadecimal digits"))?;
    let percent = |f: &str| -> Result<Value> {
        match r.get(f) {
            None | Some(Value::Null) => Ok(Value::Null),
            Some(v) => v.as_i64().filter(|p| (0..=100).contains(p)).map(Value::from).ok_or_else(|| Error::field(f, "is 0 to 100")),
        }
    };
    let moment = |f: &str| -> Result<Value> {
        match r.get(f) {
            None | Some(Value::Null) => Ok(Value::Null),
            Some(v) => v
                .as_str()
                .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                .map(|t| json!(t.with_timezone(&Utc).to_rfc3339()))
                .ok_or_else(|| Error::field(f, "is a moment in RFC 3339")),
        }
    };
    let now = Utc::now();
    let values = json!({
        "reported_version": s(&r, "version"), "reported_digest": digest, "reported_config": s(&r, "config"),
        "backup_at": moment("backup_at")?, "certificate_expires_at": moment("certificate_expires_at")?,
        "cpu_percent": percent("cpu")?, "memory_percent": percent("memory")?, "disk_percent": percent("disk")?, "reported_at": now.to_rfc3339(),
    });
    k.set_values_tx(&ctx, &mut tx, "Cell", cell, obj(values)).await?;
    let cell = refresh_cell(k, &ctx, &mut tx, cell, now).await?;
    tx.commit().await?;
    Ok(reply(json!({"health": s(&cell, "health"), "problems": s(&cell, "problems")})))
}

/// A cell's state, as customers are told it.
fn state(health: Option<&str>) -> &'static str {
    match health {
        Some("Healthy") => "Operational",
        Some("Degraded") => "Degraded",
        Some("Down") => "Outage",
        _ => "Unknown",
    }
}

/// Coming maintenance: each running rollout's waves not yet upgraded, with their cell's window.
async fn coming(k: &Kernel, ctx: &Ctx, tx: &mut Tx, now: DateTime<Utc>) -> Result<Vec<(Uuid, Value)>> {
    let mut out = Vec::new();
    for r in k.find_tx(ctx, tx, "Rollout", &[Filter::eq("status", "Running")], None, 200).await? {
        let rollout = k.get_tx(ctx, tx, "Rollout", id(&r, "id").unwrap_or_default(), false).await?;
        let release = k.get_tx(ctx, tx, "Release", id(&rollout, "release").unwrap_or_default(), false).await?;
        for w in rows(&rollout, "waves").iter().filter(|w| matches!(s(w, "state"), Some("Pending" | "Upgrading"))) {
            let Some(c) = id(w, "cell") else { continue };
            let cell = k.get_tx(ctx, tx, "Cell", c, false).await?;
            let (from, to) = window(&cell, now).unzip();
            out.push((
                c,
                json!({"cell": s(&cell, "cell_name"), "region": s(&cell, "region"), "release": s(&release, "release_version"),
                       "from": from.map(|t| t.to_rfc3339()), "to": to.map(|t| t.to_rfc3339())}),
            ));
        }
    }
    Ok(out)
}

/// Public incidents open now or resolved in the last three days, with the cells they touch.
async fn incidents(k: &Kernel, ctx: &Ctx, tx: &mut Tx, names: &HashMap<Uuid, String>, now: DateTime<Utc>) -> Result<Vec<(Vec<Uuid>, Value)>> {
    let mut out = Vec::new();
    for i in k.find_tx(ctx, tx, "ServiceIncident", &[Filter::eq("public", true)], Some("started_at desc"), 100).await? {
        if s(&i, "status") == Some("Resolved") && at(&i, "resolved_at").is_none_or(|t| now - t >= Duration::days(3)) {
            continue;
        }
        let i = k.get_tx(ctx, tx, "ServiceIncident", id(&i, "id").unwrap_or_default(), false).await?;
        let cells: Vec<Uuid> = rows(&i, "cells").iter().filter_map(|c| id(c, "cell")).collect();
        let mut updates: Vec<Value> =
            rows(&i, "updates").iter().map(|u| json!({"at": u.get("posted_at"), "status": s(u, "status"), "message": s(u, "message")})).collect();
        updates.reverse();
        let told = json!({"title": s(&i, "title"), "severity": s(&i, "severity"), "status": s(&i, "status"), "started_at": i.get("started_at"),
                          "resolved_at": i.get("resolved_at"), "cells": cells.iter().filter_map(|c| names.get(c)).collect::<Vec<_>>(), "updates": updates});
        out.push((cells, told));
    }
    Ok(out)
}

async fn status(k: &Kernel, tenant: Uuid) -> Result<SiteResponse> {
    let ctx = Ctx::system(tenant, k.registry.schema(tenant).await?);
    let mut tx = k.db.tenant_tx(tenant).await?;
    let now = Utc::now();
    let cells = k.find_tx(&ctx, &mut tx, "Cell", &[], Some("cell_name asc"), 10_000).await?;
    let names: HashMap<Uuid, String> = cells.iter().filter_map(|c| Some((id(c, "id")?, s(c, "cell_name")?.to_string()))).collect();
    let mut regions: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    for c in &cells {
        regions.entry(s(c, "region").unwrap_or_default().to_string()).or_default().push(json!({"cell": s(c, "cell_name"), "state": state(s(c, "health"))}));
    }
    let incidents: Vec<Value> = incidents(k, &ctx, &mut tx, &names, now).await?.into_iter().map(|(_, v)| v).collect();
    let maintenance: Vec<Value> = coming(k, &ctx, &mut tx, now).await?.into_iter().map(|(_, v)| v).collect();
    tx.commit().await?;
    let regions: Vec<Value> = regions.into_iter().map(|(region, cells)| json!({"region": region, "cells": cells})).collect();
    let mut r = SiteResponse::json(200, &json!({"regions": regions, "incidents": incidents, "maintenance": maintenance}));
    r.headers.push(("cache-control".into(), "public, max-age=30".into()));
    Ok(r)
}

/// What a workspace's contact or partner sees of it.
async fn told(k: &Kernel, ctx: &Ctx, tx: &mut Tx, w: &Doc, now: DateTime<Utc>) -> Result<Value> {
    let me = id(w, "id").unwrap_or_default().to_string();
    let cell = match id(w, "cell") {
        Some(c) => Some(k.get_tx(ctx, tx, "Cell", c, false).await?),
        None => None,
    };
    let cell_id = id(w, "cell");
    let names: HashMap<Uuid, String> = cell.iter().filter_map(|c| Some((id(c, "id")?, s(c, "cell_name")?.to_string()))).collect();
    let incidents: Vec<Value> =
        incidents(k, ctx, tx, &names, now).await?.into_iter().filter(|(cells, _)| cell_id.is_some_and(|c| cells.contains(&c))).map(|(_, v)| v).collect();
    let maintenance: Vec<Value> = coming(k, ctx, tx, now).await?.into_iter().filter(|(c, _)| Some(*c) == cell_id).map(|(_, v)| v).collect();
    let recovery: Vec<Value> = k
        .find_tx(ctx, tx, "RecoveryRequest", &[Filter::eq("workspace", me.clone())], Some("created_at desc"), 20)
        .await?
        .iter()
        .map(|r| json!({"kind": s(r, "recovery_kind"), "status": s(r, "status"), "asked_at": r.get("created_at"), "note": s(r, "decision_note")}))
        .collect();
    let cases: Vec<Value> = k
        .find_tx(ctx, tx, "SupportCase", &[Filter::eq("workspace", me)], Some("due_at asc"), 50)
        .await?
        .iter()
        .map(|c| json!({"subject": s(c, "subject"), "severity": s(c, "severity"), "status": s(c, "status"), "due_at": c.get("due_at"), "resolution": s(c, "resolution")}))
        .collect();
    let cell = cell.map(|c| {
        let (from, to) = window(&c, now).unzip();
        json!({"cell": s(&c, "cell_name"), "region": s(&c, "region"), "state": state(s(&c, "health")), "last_backup": c.get("backup_at"),
               "window": {"from": from.map(|t| t.to_rfc3339()), "to": to.map(|t| t.to_rfc3339())}})
    });
    Ok(json!({
        "slug": s(w, "slug"), "customer": s(w, "customer_name"), "status": s(w, "status"), "health": s(w, "health"), "problems": s(w, "health_problems"),
        "version": s(w, "running_version"), "checked_at": w.get("checked_at"), "support_access_ends": s(w, "health_key_expires"),
        "plan": s(w, "plan"), "renews_on": s(w, "term_end"), "support_plan": s(w, "support_plan"),
        "cell": cell, "maintenance": maintenance, "incidents": incidents, "recovery": recovery, "cases": cases,
    }))
}

async fn mine(k: &Kernel, user: &Ctx) -> Result<SiteResponse> {
    let ctx = sys(user);
    let mut tx = k.db.tenant_tx(user.tenant).await?;
    let now = Utc::now();
    let mut out = Vec::new();
    for w in k.find_tx(&ctx, &mut tx, "HostedWorkspace", &[Filter::eq("contact_email", user.email.to_lowercase())], Some("slug asc"), 100).await? {
        out.push(told(k, &ctx, &mut tx, &w, now).await?);
    }
    tx.commit().await?;
    Ok(reply(json!({"workspaces": out})))
}

/// The partner whose members include `email`.
async fn partner_of(k: &Kernel, ctx: &Ctx, tx: &mut Tx, email: &str) -> Result<Option<Doc>> {
    // ponytail: reads every partner's members; an index on member emails when partners number thousands.
    for p in k.find_tx(ctx, tx, "Partner", &[], None, 10_000).await? {
        let p = k.get_tx(ctx, tx, "Partner", id(&p, "id").unwrap_or_default(), false).await?;
        if rows(&p, "members").iter().any(|m| s(m, "email").is_some_and(|e| e.eq_ignore_ascii_case(email))) {
            return Ok(Some(p));
        }
    }
    Ok(None)
}

fn day(v: Option<&str>) -> Option<NaiveDate> {
    v.and_then(|v| NaiveDate::parse_from_str(&v[..v.len().min(10)], "%Y-%m-%d").ok())
}

async fn partner(k: &Kernel, user: &Ctx) -> Result<SiteResponse> {
    let ctx = sys(user);
    let mut tx = k.db.tenant_tx(user.tenant).await?;
    let p = partner_of(k, &ctx, &mut tx, &user.email).await?.ok_or_else(|| Error::Forbidden("this is for a partner's members".into()))?;
    let pid = id(&p, "id").unwrap_or_default().to_string();
    let today = Utc::now().date_naive();
    let mut workspaces = Vec::new();
    for w in k.find_tx(&ctx, &mut tx, "HostedWorkspace", &[Filter::eq("partner", pid.clone())], Some("slug asc"), 10_000).await? {
        let open: i64 = sqlx::query_scalar("select count(*) from e_support_case where tenant_id = $1 and workspace = $2 and status <> 'Resolved'")
            .bind(user.tenant)
            .bind(id(&w, "id"))
            .fetch_one(&mut *tx)
            .await?;
        let renews = day(s(&w, "term_end"));
        let access = day(s(&w, "health_key_expires"));
        workspaces.push(json!({
            "slug": s(&w, "slug"), "customer": s(&w, "customer_name"), "status": s(&w, "status"), "health": s(&w, "health"), "problems": s(&w, "health_problems"),
            "plan": s(&w, "plan"), "support_plan": s(&w, "support_plan"), "renews_on": renews, "renewing_soon": renews.is_some_and(|r| r - today <= Duration::days(60)),
            "support_access_ends": access, "support_access_ending": access.is_none_or(|a| a - today <= Duration::days(7)), "open_cases": open,
        }));
    }
    let statements: Vec<Value> = k
        .find_tx(&ctx, &mut tx, "RevenueStatement", &[Filter::eq("partner", pid.clone())], Some("month desc"), 36)
        .await?
        .iter()
        .map(|st| json!({"month": s(st, "month"), "total": st.get("total"), "partner_share": st.get("partner_share")}))
        .collect();
    let templates: Vec<Value> = k
        .find_tx(&ctx, &mut tx, "SetupTemplate", &[], Some("template_name asc"), 500)
        .await?
        .iter()
        .filter(|t| id(t, "partner").is_none_or(|o| o.to_string() == pid))
        .map(|t| json!({"name": s(t, "template_name"), "revision": t.get("revision"), "packages": crate::partners::names(t, "packages"), "apps": crate::partners::names(t, "apps")}))
        .collect();
    tx.commit().await?;
    Ok(reply(json!({
        "partner": {"name": s(&p, "partner_name"), "tier": s(&p, "tier"), "certified_until": s(&p, "certified_until")},
        "workspaces": workspaces, "statements": statements, "templates": templates,
    })))
}

async fn onboard(k: &Kernel, user: &Ctx, raw: &[u8]) -> Result<SiteResponse> {
    let b = body(raw)?;
    let ctx = sys(user);
    let mut tx = k.db.tenant_tx(user.tenant).await?;
    let p = partner_of(k, &ctx, &mut tx, &user.email).await?.ok_or_else(|| Error::Forbidden("a partner's members onboard their customers".into()))?;
    let pid = id(&p, "id").unwrap_or_default();
    let mut doc = Doc::new();
    for f in ["slug", "customer_name", "address", "region", "contact_email", "admin_email", "plan", "support_plan", "monthly_amount", "term_end"] {
        if let Some(v) = b.get(f).filter(|v| !v.is_null()) {
            doc.insert(f.into(), v.clone());
        }
    }
    if let Some(name) = s(&b, "template") {
        let t = k.find_tx(&ctx, &mut tx, "SetupTemplate", &[Filter::eq("template_name", name)], None, 1).await?;
        let t = t
            .first()
            .filter(|t| id(t, "partner").is_none_or(|o| o == pid))
            .ok_or_else(|| Error::field("template", format!("{name} is not a setup you may use")))?;
        doc.insert("template".into(), json!(id(t, "id").map(|i| i.to_string())));
    }
    doc.insert("partner".into(), json!(pid.to_string()));
    doc.insert("status".into(), json!("Onboarding"));
    let made = k.insert_tx(&elevated(user), &mut tx, "HostedWorkspace", doc).await?;
    let cell = match id(&made, "cell") {
        Some(c) => k.get_tx(&ctx, &mut tx, "Cell", c, false).await?.get("cell_name").cloned(),
        None => None,
    };
    tx.commit().await?;
    // With its first administrator named, it is made on its cell at once; when the cell cannot, it
    // stays onboarding, for an operator to make it there.
    let (provisioned, provision_error) = if s(&made, "admin_email").is_some() {
        let mut tx = k.db.tenant_tx(user.tenant).await?;
        match crate::hosting::provision(k, &ctx, &mut tx, &made).await {
            Ok(_) => {
                tx.commit().await?;
                (true, None)
            }
            Err(e) => {
                tx.rollback().await?;
                (false, Some(e.to_string()))
            }
        }
    } else {
        (false, None)
    };
    Ok(reply(json!({"slug": s(&made, "slug"), "cell": cell, "provisioned": provisioned, "provision_error": provision_error})))
}

async fn case(k: &Kernel, user: &Ctx, raw: &[u8]) -> Result<SiteResponse> {
    let b = body(raw)?;
    let ctx = sys(user);
    let mut tx = k.db.tenant_tx(user.tenant).await?;
    let slug = s(&b, "workspace").ok_or_else(|| Error::field("workspace", "is required"))?;
    let w = k.find_tx(&ctx, &mut tx, "HostedWorkspace", &[Filter::eq("slug", slug)], None, 1).await?.into_iter().next();
    let contact = w.as_ref().is_some_and(|w| s(w, "contact_email").is_some_and(|c| c.eq_ignore_ascii_case(&user.email)));
    let theirs = match (w.as_ref().and_then(|w| id(w, "partner")), contact) {
        (_, true) => true,
        (Some(p), false) => partner_of(k, &ctx, &mut tx, &user.email).await?.is_some_and(|mine| id(&mine, "id") == Some(p)),
        (None, false) => false,
    };
    let Some(w) = w.filter(|_| theirs) else { return Err(Error::Forbidden("a case is raised for a workspace of yours".into())) };
    let doc = json!({"workspace": id(&w, "id").map(|i| i.to_string()), "subject": s(&b, "subject"), "details": s(&b, "details"), "severity": s(&b, "severity").unwrap_or("Low")});
    let made = k.insert_tx(&elevated(user), &mut tx, "SupportCase", obj(doc)).await?;
    tx.commit().await?;
    Ok(reply(json!({"due_at": made.get("due_at")})))
}
