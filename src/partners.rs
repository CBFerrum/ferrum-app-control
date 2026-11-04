//! Partners, setup templates, support cases and revenue statements. A certified or premier tier
//! needs a certification that has not lapsed, and lapses to registered when it does. A setup
//! template is versioned: changing what it installs is a new revision, and a workspace keeps the
//! revision it was set up from. A support case is due by its workspace's support plan and its
//! severity, and resolving it says how. A partner's revenue statement is issued once a month has
//! ended, once for that month, from what its customers' workspaces pay.

use crate::{at, obj, operator};
use async_trait::async_trait;
use chrono::{Datelike, Duration, Months, NaiveDate, Utc};
use ferrum_domain::util::{d, elevated, id, money, round, rows, s};
use ferrum_kernel::{Ctx, Doc, DocHook, Error, Filter, HookCtx, Kernel, Result, Tx};
use rust_decimal::Decimal;
use serde_json::{Value, json};
use std::collections::HashSet;

/// Hours to answer a case, by support plan and severity.
pub(crate) fn response_hours(plan: &str, severity: &str) -> i64 {
    match (plan, severity) {
        ("Premium", "Critical") => 1,
        ("Premium", "High") => 4,
        ("Premium", _) => 24,
        (_, "Critical") => 4,
        (_, "High") => 8,
        _ => 72,
    }
}

/// Whether a certified tier still stands on `today`.
fn certified(doc: &Doc, today: NaiveDate) -> bool {
    s(doc, "certified_until").and_then(|v| NaiveDate::parse_from_str(&v[..v.len().min(10)], "%Y-%m-%d").ok()).is_some_and(|until| until >= today)
}

/// Certified tiers whose certification has lapsed by `today`, back to registered.
pub(crate) async fn lapse(k: &Kernel, ctx: &Ctx, tx: &mut Tx, today: NaiveDate) -> Result<()> {
    for p in k.find_tx(ctx, tx, "Partner", &[], None, 10_000).await? {
        if s(&p, "tier").is_some_and(|t| t != "Registered") && !certified(&p, today) {
            k.set_values_tx(ctx, tx, "Partner", id(&p, "id").unwrap_or_default(), obj(json!({"tier": "Registered"}))).await?;
        }
    }
    Ok(())
}

pub struct PartnerHook;

#[async_trait]
impl DocHook for PartnerHook {
    fn name(&self) -> &'static str {
        "control.partner"
    }

    async fn validate(&self, _hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        if s(doc, "tier").is_some_and(|t| t != "Registered") && !certified(doc, Utc::now().date_naive()) {
            return Err(Error::field("certified_until", "a certified tier needs a certification that has not lapsed"));
        }
        let mut seen = HashSet::new();
        if !rows(doc, "members").iter().filter_map(|m| s(m, "email")).all(|e| seen.insert(e.to_lowercase())) {
            return Err(Error::field("members", "each member once"));
        }
        Ok(())
    }

    async fn action(&self, hc: &mut HookCtx<'_>, name: &str, doc: &Doc, params: &Value) -> Result<Option<Value>> {
        if name != "issue_statement" {
            return Ok(None);
        }
        if !operator(hc.ctx) {
            return Err(Error::Forbidden("a Fleet Operator issues statements".into()));
        }
        let partner = id(doc, "id").ok_or_else(|| Error::internal("a partner without an id"))?;
        let today = Utc::now().date_naive();
        let this_month = today.with_day(1).unwrap_or(today);
        let day = match params.get("month").and_then(Value::as_str) {
            Some(v) => NaiveDate::parse_from_str(&v[..v.len().min(10)], "%Y-%m-%d").map_err(|_| Error::field("month", "is a day of the month"))?,
            None => this_month - Duration::days(1),
        };
        let month = day.with_day(1).unwrap_or(day);
        if month.checked_add_months(Months::new(1)).is_none_or(|end| end > this_month) {
            return Err(Error::field("month", "a statement is for a month that has ended"));
        }
        let system = elevated(hc.ctx);
        let issued = hc
            .kernel
            .find_tx(&system, hc.tx, "RevenueStatement", &[Filter::eq("partner", partner.to_string()), Filter::eq("month", month.to_string())], None, 1)
            .await?;
        if !issued.is_empty() {
            return Err(Error::invalid(format!("{}'s statement for {} has been issued", s(doc, "partner_name").unwrap_or_default(), month.format("%B %Y"))));
        }
        let share = d(doc, "revenue_share");
        let (mut lines, mut total, mut owed) = (Vec::new(), Decimal::ZERO, Decimal::ZERO);
        for w in hc.kernel.find_tx(&system, hc.tx, "HostedWorkspace", &[Filter::eq("partner", partner.to_string())], Some("slug asc"), 10_000).await? {
            let amount = d(&w, "monthly_amount");
            if s(&w, "status") != Some("Active") || amount <= Decimal::ZERO {
                continue;
            }
            let part = round(amount * share / Decimal::ONE_HUNDRED, 2);
            lines.push(json!({"workspace": id(&w, "id").map(|i| i.to_string()), "amount": money(amount), "share": money(part)}));
            total += amount;
            owed += part;
        }
        let statement =
            json!({"partner": partner.to_string(), "month": month.to_string(), "lines": lines, "total": money(total), "partner_share": money(owed)});
        let made = hc.kernel.insert_tx(&system, hc.tx, "RevenueStatement", obj(statement)).await?;
        Ok(Some(json!({"statement": made.get("id"), "partner_share": money(owed)})))
    }
}

/// A package's or an app's name.
fn good_name(v: &str) -> bool {
    !v.is_empty() && v.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// The names a template lists, one per line.
pub(crate) fn names(doc: &Doc, field: &str) -> Vec<String> {
    s(doc, field).unwrap_or_default().lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()
}

pub struct TemplateHook;

#[async_trait]
impl DocHook for TemplateHook {
    fn name(&self) -> &'static str {
        "control.template"
    }

    async fn validate(&self, _hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        for field in ["packages", "apps"] {
            if let Some(bad) = names(doc, field).into_iter().find(|n| !good_name(n)) {
                return Err(Error::field(field, format!("'{bad}' is not a name")));
            }
        }
        Ok(())
    }

    async fn after_save(&self, hc: &mut HookCtx<'_>, doc: &Doc, old: Option<&Doc>) -> Result<()> {
        let Some(old) = old else { return Ok(()) };
        // What it installs changed: a new revision, so workspaces set up before keep theirs.
        if names(old, "packages") != names(doc, "packages") || names(old, "apps") != names(doc, "apps") || old.get("settings") != doc.get("settings") {
            let next = crate::int(old, "revision").unwrap_or(1) + 1;
            hc.kernel.set_values_tx(&elevated(hc.ctx), hc.tx, "SetupTemplate", id(doc, "id").unwrap_or_default(), obj(json!({"revision": next}))).await?;
        }
        Ok(())
    }
}

pub struct CaseHook;

#[async_trait]
impl DocHook for CaseHook {
    fn name(&self) -> &'static str {
        "control.case"
    }

    fn fills(&self) -> &'static [&'static str] {
        &["raised_by", "partner", "due_at", "resolved_at"]
    }

    async fn before_validate(&self, hc: &mut HookCtx<'_>, doc: &mut Doc) -> Result<()> {
        if s(doc, "raised_by").is_none() && !hc.ctx.email.is_empty() {
            doc.insert("raised_by".into(), json!(hc.ctx.email.to_lowercase()));
        }
        let now = Utc::now();
        if let Some(w) = id(doc, "workspace") {
            let workspace = hc.kernel.get_tx(&elevated(hc.ctx), hc.tx, "HostedWorkspace", w, false).await?;
            let hours = response_hours(s(&workspace, "support_plan").unwrap_or("Standard"), s(doc, "severity").unwrap_or("Low"));
            let from = at(doc, "created_at").unwrap_or(now);
            doc.insert("partner".into(), workspace.get("partner").cloned().unwrap_or(Value::Null));
            doc.insert("due_at".into(), json!((from + Duration::hours(hours)).to_rfc3339()));
        }
        let resolved = match s(doc, "status") {
            Some("Resolved") => doc.get("resolved_at").filter(|v| !v.is_null()).cloned().unwrap_or(json!(now.to_rfc3339())),
            _ => Value::Null,
        };
        doc.insert("resolved_at".into(), resolved);
        Ok(())
    }

    async fn validate(&self, _hc: &mut HookCtx<'_>, doc: &Doc) -> Result<()> {
        if s(doc, "status") == Some("Resolved") && s(doc, "resolution").is_none_or(|r| r.trim().is_empty()) {
            return Err(Error::field("resolution", "resolving a case says how"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_and_names() {
        assert_eq!((response_hours("Premium", "Critical"), response_hours("Standard", "Critical"), response_hours("Standard", "Low")), (1, 4, 72));
        assert!(good_name("crm") && good_name("field-service") && !good_name("Crm") && !good_name("a b"));
        let doc = json!({"packages": "crm\n\n helpdesk \n"}).as_object().cloned().unwrap();
        assert_eq!(names(&doc, "packages"), vec!["crm", "helpdesk"]);
    }
}
