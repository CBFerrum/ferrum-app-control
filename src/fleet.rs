//! Releases, cells and rollouts. A release is known by its digest, which never changes, and is
//! qualified by a Release Manager other than who recorded it. A cell's health is worked out from
//! what its tooling last reported: down when it has gone quiet or its certificate has expired,
//! degraded when it runs another release than it should, its configuration has drifted, its last
//! backup is old, its certificate is near its end or a resource is nearly full. A rollout takes a
//! qualified release to its cells one wave at a time: a wave starts in its cell's maintenance
//! window, soaks while the cell stays healthy, and fails, halting the rollout, when the cell is not
//! upgraded within the window or falls ill while soaking.

use crate::{at, int, obj, operator};
use async_trait::async_trait;
use chrono::{DateTime, Datelike, Duration, Utc};
use ferrum_domain::util::{elevated, id, rows, s};
use ferrum_kernel::{Ctx, Doc, DocHook, Error, Filter, HookCtx, Kernel, Result, Tx};
use serde_json::{Value, json};
use std::collections::HashSet;
use uuid::Uuid;

/// `sha256:` and 64 lower-case hexadecimal digits.
pub(crate) fn is_digest(v: &str) -> bool {
    v.strip_prefix("sha256:").is_some_and(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
}

const DAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

/// Where the cell's weekly maintenance window opens and closes, for the window open at `now` or
/// the next one.
pub(crate) fn window(cell: &Doc, now: DateTime<Utc>) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let day = s(cell, "maintenance_day").and_then(|d| DAYS.iter().position(|x| *x == d))?;
    let hour = u32::try_from(int(cell, "maintenance_hour").unwrap_or(2)).ok()?;
    let hours = Duration::hours(int(cell, "maintenance_hours").unwrap_or(2));
    (-1..8).find_map(|offset| {
        let d = now.date_naive() + Duration::days(offset);
        let start = d.and_hms_opt(hour, 0, 0)?.and_utc();
        (d.weekday().num_days_from_monday() as usize == day && start + hours > now).then_some((start, start + hours))
    })
}

/// Whether the cell's maintenance window is open at `now`.
pub(crate) fn in_window(cell: &Doc, now: DateTime<Utc>) -> bool {
    window(cell, now).is_some_and(|(start, _)| start <= now)
}

/// A cell's health from its last report as of `now`, and what is wrong; `wants` is the digest of
/// the release it should run.
pub(crate) fn cell_health(cell: &Doc, wants: Option<&str>, now: DateTime<Utc>) -> (&'static str, Vec<String>) {
    let Some(reported) = at(cell, "reported_at") else { return ("Unknown", vec!["it has not reported".into()]) };
    let (mut down, mut problems) = (Vec::new(), Vec::new());
    if now - reported > Duration::minutes(15) {
        down.push(format!("no report for {} minutes", (now - reported).num_minutes()));
    }
    match at(cell, "certificate_expires_at") {
        Some(c) if c <= now => down.push("its certificate has expired".into()),
        Some(c) if c - now < Duration::days(14) => problems.push(format!("its certificate expires on {}", c.date_naive())),
        _ => {}
    }
    if wants.is_some_and(|w| s(cell, "reported_digest") != Some(w)) {
        problems.push(format!("it runs {}, not the release it should", s(cell, "reported_version").unwrap_or("something unknown")));
    }
    if s(cell, "desired_config").is_some_and(|want| s(cell, "reported_config") != Some(want)) {
        problems.push("its configuration has drifted".into());
    }
    match at(cell, "backup_at") {
        None => problems.push("no backup reported".into()),
        Some(b) if now - b > Duration::hours(26) => problems.push(format!("its last backup was {} hours ago", (now - b).num_hours())),
        _ => {}
    }
    for (k, what) in [("cpu_percent", "processors"), ("memory_percent", "memory"), ("disk_percent", "disk")] {
        if let Some(p) = int(cell, k).filter(|p| *p >= 90) {
            problems.push(format!("{what} at {p}%"));
        }
    }
    if !down.is_empty() {
        down.extend(problems);
        return ("Down", down);
    }
    (if problems.is_empty() { "Healthy" } else { "Degraded" }, problems)
}

/// How many open workspaces run on the cell, `but` aside.
pub(crate) async fn placed(tx: &mut Tx, tenant: Uuid, cell: Uuid, but: Option<Uuid>) -> Result<i64> {
    Ok(sqlx::query_scalar("select count(*) from e_hosted_workspace where tenant_id = $1 and cell = $2 and status <> 'Closed' and id is distinct from $3")
        .bind(tenant)
        .bind(cell)
        .bind(but)
        .fetch_one(&mut **tx)
        .await?)
}

/// Works the cell's health and count out again, writing them when they changed.
pub(crate) async fn refresh_cell(k: &Kernel, ctx: &Ctx, tx: &mut Tx, cell_id: Uuid, now: DateTime<Utc>) -> Result<Doc> {
    let cell = k.get_tx(ctx, tx, "Cell", cell_id, false).await?;
    let wants = match id(&cell, "desired_release") {
        Some(r) => k.get_tx(ctx, tx, "Release", r, false).await?.get("digest").and_then(Value::as_str).map(str::to_string),
        None => None,
    };
    let (health, problems) = cell_health(&cell, wants.as_deref(), now);
    let problems = if problems.is_empty() { Value::Null } else { json!(problems.join("; ")) };
    let count = placed(tx, ctx.tenant, cell_id, None).await?;
    if s(&cell, "health") != Some(health) || cell.get("problems").unwrap_or(&Value::Null) != &problems || int(&cell, "placed") != Some(count) {
        k.set_values_tx(ctx, tx, "Cell", cell_id, obj(json!({"health": health, "problems": problems, "placed": count}))).await?;
    }
    k.get_tx(ctx, tx, "Cell", cell_id, false).await
}

/// Every cell's health as of `now`: a cell that has gone quiet is down without saying so.
pub(crate) async fn refresh_cells(k: &Kernel, ctx: &Ctx, tx: &mut Tx, now: DateTime<Utc>) -> Result<()> {
    for cell in k.find_tx(ctx, tx, "Cell", &[], None, 10_000).await? {
        if let Some(c) = id(&cell, "id") {
            refresh_cell(k, ctx, tx, c, now).await?;
        }
    }
    Ok(())
}

pub struct ReleaseHook;

#[async_trait]
impl DocHook for ReleaseHook {
    fn name(&self) -> &'static str {
        "control.release"
    }

    fn fills(&self) -> &'static [&'static str] {
        &["recorded_by"]
    }

    async fn before_validate(&self, hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        if s(doc, "recorded_by").is_none() && !hc.ctx.email.is_empty() {
            doc.insert("recorded_by".into(), json!(hc.ctx.email.to_lowercase()));
        }
        Ok(())
    }

    async fn validate(&self, _hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        if !s(doc, "digest").is_some_and(is_digest) {
            return Err(Error::field("digest", "is sha256: and 64 lower-case hexadecimal digits"));
        }
        Ok(())
    }

    async fn validate_change(&self, _hc: &mut HookCtx<'_>, old: Option<&Doc>, doc: &Doc) -> Result<()> {
        if old.is_some_and(|o| s(o, "digest") != s(doc, "digest")) {
            return Err(Error::field("digest", "a release's digest never changes: record another release"));
        }
        Ok(())
    }

    async fn action(&self, hc: &mut HookCtx<'_>, name: &str, doc: &Doc, _params: &Value) -> Result<Option<Value>> {
        let release = id(doc, "id").ok_or_else(|| Error::internal("a release without an id"))?;
        let manager = hc.ctx.is_admin || hc.ctx.roles.iter().any(|r| r == "Release Manager");
        let system = elevated(hc.ctx);
        match name {
            "qualify" => {
                if !manager {
                    return Err(Error::Forbidden("a Release Manager qualifies a release".into()));
                }
                if s(doc, "recorded_by").is_some_and(|r| r.eq_ignore_ascii_case(&hc.ctx.email)) {
                    return Err(Error::Forbidden("a release is qualified by someone other than who recorded it".into()));
                }
                let values = json!({"status": "Qualified", "qualified_by": hc.ctx.email, "qualified_at": Utc::now().to_rfc3339()});
                hc.kernel.set_values_tx(&system, hc.tx, "Release", release, obj(values)).await?;
                Ok(Some(json!({"status": "Qualified"})))
            }
            "withdraw" => {
                if !manager {
                    return Err(Error::Forbidden("a Release Manager withdraws a release".into()));
                }
                hc.kernel.set_values_tx(&system, hc.tx, "Release", release, obj(json!({"status": "Withdrawn"}))).await?;
                Ok(Some(json!({"status": "Withdrawn"})))
            }
            _ => Ok(None),
        }
    }
}

pub struct CellHook;

#[async_trait]
impl DocHook for CellHook {
    fn name(&self) -> &'static str {
        "control.cell"
    }

    async fn validate(&self, hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        if s(doc, "api_address").is_some_and(|a| !crate::hosting::good_address(a)) {
            return Err(Error::field("api_address", "is https://… where the cell answers"));
        }
        if let Some(cell) = id(doc, "id") {
            let runs = placed(hc.tx, hc.ctx.tenant, cell, None).await?;
            if int(doc, "capacity").is_some_and(|c| c < runs) {
                return Err(Error::field("capacity", format!("it runs {runs} workspaces: its capacity is not less")));
            }
        }
        Ok(())
    }
}

pub struct RolloutHook;

#[async_trait]
impl DocHook for RolloutHook {
    fn name(&self) -> &'static str {
        "control.rollout"
    }

    fn fills(&self) -> &'static [&'static str] {
        &["started_by"]
    }

    async fn validate(&self, _hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        let mut seen = HashSet::new();
        if !rows(doc, "waves").iter().filter_map(|w| id(w, "cell")).all(|c| seen.insert(c)) {
            return Err(Error::field("waves", "a cell is in a rollout once"));
        }
        Ok(())
    }

    async fn action(&self, hc: &mut HookCtx<'_>, name: &str, doc: &Doc, params: &Value) -> Result<Option<Value>> {
        if !operator(hc.ctx) {
            return Err(Error::Forbidden("a Fleet Operator runs rollouts".into()));
        }
        let rollout = id(doc, "id").ok_or_else(|| Error::internal("a rollout without an id"))?;
        let system = elevated(hc.ctx);
        let doc = hc.kernel.get_tx(&system, hc.tx, "Rollout", rollout, false).await?;
        match name {
            "start" => {
                let release = hc.kernel.get_tx(&system, hc.tx, "Release", id(&doc, "release").unwrap_or_default(), false).await?;
                if s(&release, "status") != Some("Qualified") {
                    return Err(Error::invalid("only a qualified release is rolled out"));
                }
                let cells: HashSet<Uuid> = rows(&doc, "waves").iter().filter_map(|w| id(w, "cell")).collect();
                for other in hc.kernel.find_tx(&system, hc.tx, "Rollout", &[Filter::eq("status", "Running")], None, 200).await? {
                    let other = hc.kernel.get_tx(&system, hc.tx, "Rollout", id(&other, "id").unwrap_or_default(), false).await?;
                    if let Some(c) = rows(&other, "waves").iter().filter_map(|w| id(w, "cell")).find(|c| cells.contains(c)) {
                        let cell = hc.kernel.get_tx(&system, hc.tx, "Cell", c, false).await?;
                        return Err(Error::invalid(format!("{} is in another running rollout", s(&cell, "cell_name").unwrap_or_default())));
                    }
                }
                let values = json!({"status": "Running", "started_by": hc.ctx.email, "halted_reason": null});
                hc.kernel.set_values_tx(&system, hc.tx, "Rollout", rollout, obj(values)).await?;
                tell_customers(hc.kernel, &system, hc.tx, &release, &cells).await?;
                Ok(Some(json!({"status": "Running"})))
            }
            "halt" => {
                let reason = params.get("reason").and_then(Value::as_str).map(str::trim).filter(|r| !r.is_empty());
                let reason = reason.ok_or_else(|| Error::field("reason", "halting a rollout says why"))?;
                hc.kernel.set_values_tx(&system, hc.tx, "Rollout", rollout, obj(json!({"status": "Halted", "halted_reason": reason}))).await?;
                Ok(Some(json!({"status": "Halted"})))
            }
            "resume" => {
                let waves: Vec<Doc> = rows(&doc, "waves")
                    .into_iter()
                    .map(|mut w| {
                        if s(&w, "state") == Some("Failed") {
                            for k in ["started_at", "soaking_since", "note"] {
                                w.insert(k.into(), Value::Null);
                            }
                            w.insert("state".into(), json!("Pending"));
                        }
                        w
                    })
                    .collect();
                let patch = json!({"status": "Running", "halted_reason": null, "waves": waves});
                hc.kernel.update_tx(&system, hc.tx, "Rollout", rollout, obj(patch), None).await?;
                Ok(Some(json!({"status": "Running"})))
            }
            _ => Ok(None),
        }
    }
}

/// Tells the contacts of every workspace on `cells` that the release is coming, and when.
async fn tell_customers(k: &Kernel, ctx: &Ctx, tx: &mut Tx, release: &Doc, cells: &HashSet<Uuid>) -> Result<()> {
    let version = s(release, "release_version").unwrap_or_default().to_string();
    let now = Utc::now();
    for cell_id in cells {
        let cell = k.get_tx(ctx, tx, "Cell", *cell_id, false).await?;
        let when = window(&cell, now).map(|(a, b)| format!("{} to {} UTC", a.format("%A %-d %B %H:%M"), b.format("%H:%M"))).unwrap_or_default();
        let on = k.find_tx(ctx, tx, "HostedWorkspace", &[Filter::eq("cell", cell_id.to_string())], None, 10_000).await?;
        let mut told = HashSet::new();
        for w in on.iter().filter(|w| s(w, "status") != Some("Closed")) {
            let Some(to) = s(w, "contact_email").map(str::to_lowercase).filter(|t| told.insert(t.clone())) else { continue };
            let html = format!(
                "<p>Ferrum {version} comes to {} in its next maintenance window, {when}. The workspace may be unavailable for a few minutes then.</p>",
                s(w, "slug").unwrap_or_default()
            );
            k.queue_mail(tx, ctx.tenant, &to, &format!("Planned maintenance: Ferrum {version}"), &html).await?;
        }
    }
    Ok(())
}

/// Moves each running rollout on by what its current wave's cell shows at `now`.
pub(crate) async fn advance(k: &Kernel, ctx: &Ctx, tx: &mut Tx, now: DateTime<Utc>) -> Result<()> {
    for found in k.find_tx(ctx, tx, "Rollout", &[Filter::eq("status", "Running")], None, 200).await? {
        let rollout = id(&found, "id").unwrap_or_default();
        let doc = k.get_tx(ctx, tx, "Rollout", rollout, false).await?;
        let release_id = id(&doc, "release").unwrap_or_default();
        let release = k.get_tx(ctx, tx, "Release", release_id, false).await?;
        let soak = Duration::hours(int(&doc, "soak_hours").unwrap_or(24));
        let before = rows(&doc, "waves");
        let mut waves = before.clone();
        let mut halted: Option<String> = None;
        if s(&release, "status") == Some("Withdrawn") {
            halted = Some("its release was withdrawn".into());
        } else if let Some(wave) = waves.iter_mut().find(|w| s(w, "state") != Some("Done")) {
            let cell_id = id(wave, "cell").unwrap_or_default();
            let cell = k.get_tx(ctx, tx, "Cell", cell_id, false).await?;
            let name = s(&cell, "cell_name").unwrap_or_default().to_string();
            let health = s(&cell, "health");
            let state = s(wave, "state").unwrap_or("Pending").to_string();
            let since = at(wave, "soaking_since");
            match state.as_str() {
                "Pending" if in_window(&cell, now) => {
                    k.set_values_tx(ctx, tx, "Cell", cell_id, obj(json!({"desired_release": release_id}))).await?;
                    refresh_cell(k, ctx, tx, cell_id, now).await?;
                    wave.insert("state".into(), json!("Upgrading"));
                    wave.insert("started_at".into(), json!(now.to_rfc3339()));
                }
                // Healthy means it runs the release it should: anything else is a drift.
                "Upgrading" if health == Some("Healthy") => {
                    wave.insert("state".into(), json!("Soaking"));
                    wave.insert("soaking_since".into(), json!(now.to_rfc3339()));
                }
                "Upgrading" if !in_window(&cell, now) => {
                    wave.insert("state".into(), json!("Failed"));
                    wave.insert("note".into(), json!("not upgraded within its window"));
                    halted = Some(format!("{name} was not upgraded within its maintenance window"));
                }
                "Soaking" if matches!(health, Some("Degraded" | "Down")) => {
                    let why = s(&cell, "problems").unwrap_or("it fell ill").to_string();
                    wave.insert("state".into(), json!("Failed"));
                    wave.insert("note".into(), json!(why));
                    halted = Some(format!("{name} while soaking: {why}"));
                }
                "Soaking" if since.is_some_and(|t| now - t >= soak) => {
                    wave.insert("state".into(), json!("Done"));
                    wave.insert("done_at".into(), json!(now.to_rfc3339()));
                }
                _ => {}
            }
        }
        let status = if halted.is_some() {
            "Halted"
        } else if waves.iter().all(|w| s(w, "state") == Some("Done")) {
            "Done"
        } else {
            "Running"
        };
        if waves != before || status != "Running" {
            k.update_tx(ctx, tx, "Rollout", rollout, obj(json!({"waves": waves, "status": status, "halted_reason": halted})), None).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(v: Value) -> Doc {
        v.as_object().cloned().unwrap()
    }

    #[test]
    fn windows_and_digests() {
        let sunday_2am = |h: u32, m: u32| chrono::NaiveDate::from_ymd_opt(2026, 11, 8).unwrap().and_hms_opt(h, m, 0).unwrap().and_utc();
        let cell = doc(json!({"maintenance_day": "Sunday", "maintenance_hour": 2, "maintenance_hours": 2}));
        assert!(
            !in_window(&cell, sunday_2am(1, 59))
                && in_window(&cell, sunday_2am(2, 0))
                && in_window(&cell, sunday_2am(3, 59))
                && !in_window(&cell, sunday_2am(4, 0))
        );
        assert_eq!(window(&cell, sunday_2am(5, 0)).map(|(a, _)| a), Some(sunday_2am(2, 0) + Duration::days(7)), "next week's");
        let late = doc(json!({"maintenance_day": "Saturday", "maintenance_hour": 23, "maintenance_hours": 3}));
        assert!(in_window(&late, sunday_2am(1, 30)), "a window past midnight");
        assert!(is_digest(&format!("sha256:{}", "a1".repeat(32))));
        assert!(!is_digest(&format!("sha256:{}", "A1".repeat(32))) && !is_digest("sha256:abc") && !is_digest(&"a1".repeat(32)));
    }
}
