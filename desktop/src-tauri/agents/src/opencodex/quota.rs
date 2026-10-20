//! Usage limits from opencodex's two quota caches (spec "Statistics"):
//!
//! - `codex-quota-cache.json`: `quotas` keyed by Codex account (`__main__` for the main
//!   one, otherwise an opaque hash), each with `shortPercent`/`weeklyPercent`/
//!   `monthlyPercent` and reset times in Unix seconds or milliseconds (opencodex stores
//!   both and tells them apart by size).
//! - `provider-account-quota-cache.json`: `rows` keyed by `provider\0accountId`, each
//!   with `fiveHourPercent`/`weeklyPercent`/`monthlyPercent`, reset times and
//!   `customWindows` (`label`, `percent`, `resetAt`).
//!
//! An account is labelled by its provider when the cache carries it (`codex` for the
//! Codex cache, the key's provider part for the other), otherwise "Account 1, 2, …",
//! numbered across both files. A window whose reset time has passed is left out: its
//! figure belongs to a window that has ended. A missing or unreadable cache gives no
//! limits; it never makes opencodex unavailable.

use std::path::Path;

use serde::Serialize;
use serde_json::{Map, Value};

/// Who a usage limit belongs to.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum AccountLabel {
    /// Labelled by its provider's id (`codex`, `anthropic`, …), which is data.
    Provider(String),
    /// "Account N".
    Account(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum WindowKind {
    FiveHour,
    /// A Codex short window that is not five hours long.
    Short,
    Weekly,
    Monthly,
    /// A provider's own window; its label is data.
    Custom,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitWindow {
    pub kind: WindowKind,
    /// The custom window's label, `None` for the others.
    pub label: Option<String>,
    /// How much of the window is used, 0 to 100.
    pub percent: f64,
    /// When the window resets, in milliseconds since the epoch.
    pub resets_at: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageLimit {
    pub account: AccountLabel,
    pub windows: Vec<LimitWindow>,
}

pub(crate) const CODEX_CACHE: &str = "codex-quota-cache.json";
pub(crate) const PROVIDER_CACHE: &str = "provider-account-quota-cache.json";

/// Below this a stored reset instant is in seconds, at or above it milliseconds.
const MILLISECOND_RESETS: f64 = 1e12;

/// Every account's current usage limits, as of `now` (ms).
pub(crate) fn read_limits(dir: &Path, now: i64) -> Vec<UsageLimit> {
    let mut limits = Vec::new();
    let mut unnamed = 0;
    let mut number = || {
        unnamed += 1;
        AccountLabel::Account(unnamed)
    };

    let codex_file = read(&dir.join(CODEX_CACHE));
    let codex = object(codex_file.as_ref(), "quotas");
    let main = codex.and_then(|quotas| quotas.get("__main__"));
    let others = codex
        .into_iter()
        .flatten()
        .filter(|(key, _)| key.as_str() != "__main__");
    for (account, quota) in main
        .map(|quota| (AccountLabel::Provider("codex".into()), quota))
        .into_iter()
        .chain(others.map(|(_, quota)| (number(), quota)))
    {
        let short_kind = match number_at(quota, "shortWindowSeconds") {
            None => WindowKind::FiveHour,
            Some(seconds) if seconds == 5.0 * 3600.0 => WindowKind::FiveHour,
            Some(_) => WindowKind::Short,
        };
        let windows = [
            (short_kind, "shortPercent", "shortResetAt"),
            (WindowKind::Weekly, "weeklyPercent", "weeklyResetAt"),
            (WindowKind::Monthly, "monthlyPercent", "monthlyResetAt"),
        ]
        .into_iter()
        .filter_map(|(kind, percent, reset)| window(quota, kind, None, percent, reset, now))
        .collect();
        push(&mut limits, account, windows);
    }

    let provider_file = read(&dir.join(PROVIDER_CACHE));
    let providers = object(provider_file.as_ref(), "rows");
    for (key, quota) in providers.into_iter().flatten() {
        let account = match key.split_once('\0') {
            Some((provider, _)) if !provider.is_empty() => {
                AccountLabel::Provider(provider.to_string())
            }
            _ => number(),
        };
        let mut windows: Vec<LimitWindow> = [
            (WindowKind::FiveHour, "fiveHourPercent", "fiveHourResetAt"),
            (WindowKind::Weekly, "weeklyPercent", "weeklyResetAt"),
            (WindowKind::Monthly, "monthlyPercent", "monthlyResetAt"),
        ]
        .into_iter()
        .filter_map(|(kind, percent, reset)| window(quota, kind, None, percent, reset, now))
        .collect();
        if let Some(custom) = quota.get("customWindows").and_then(Value::as_array) {
            windows.extend(custom.iter().filter_map(|custom| {
                let label = custom.get("label")?.as_str()?.to_string();
                window(
                    custom,
                    WindowKind::Custom,
                    Some(label),
                    "percent",
                    "resetAt",
                    now,
                )
            }));
        }
        push(&mut limits, account, windows);
    }
    limits
}

fn push(limits: &mut Vec<UsageLimit>, account: AccountLabel, windows: Vec<LimitWindow>) {
    if !windows.is_empty() {
        limits.push(UsageLimit { account, windows });
    }
}

fn read(path: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

fn object<'a>(file: Option<&'a Value>, key: &str) -> Option<&'a Map<String, Value>> {
    file?.get(key)?.as_object()
}

fn number_at(value: &Value, key: &str) -> Option<f64> {
    value.get(key)?.as_f64().filter(|n| n.is_finite())
}

fn window(
    quota: &Value,
    kind: WindowKind,
    label: Option<String>,
    percent: &str,
    reset: &str,
    now: i64,
) -> Option<LimitWindow> {
    let percent = number_at(quota, percent)?;
    let resets_at = number_at(quota, reset).map(|at| {
        if at < MILLISECOND_RESETS {
            (at * 1000.0) as i64
        } else {
            at as i64
        }
    });
    if resets_at.is_some_and(|at| at <= now) {
        return None;
    }
    Some(LimitWindow {
        kind,
        label,
        percent: percent.clamp(0.0, 100.0),
        resets_at,
    })
}
