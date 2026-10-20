//! The Of the month figures (spec "Statistics", "Of the month"): the model of the month
//! and the harness of the month, by total tokens over the current local calendar month,
//! across every surface opencodex proxies. A routed request counts as the model that
//! served it. A trimmed ledger is counted as it is.

use std::collections::HashMap;

use chrono::{Datelike, Months as MonthCount, NaiveDate};
use serde::Serialize;

use super::ledger::UsageRow;
use super::range::local_midnight;
use super::statistics::OpencodexUnavailable;
use crate::Clock;

/// One contender: a model (by version) or a harness.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonthEntry {
    /// The served model's id, or the harness's surface.
    pub id: String,
    /// The display name ("Opus 5.5", "Claude Code"), or the id when it has none.
    pub name: String,
    /// Total tokens this month.
    pub total: u64,
    /// Its share of the month's total, 0 to 1.
    pub share: f64,
}

/// One card: the winner and the next two runners-up, and last month's winner.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MonthCard {
    /// The winner first, then up to two runners-up. Empty when nothing was used this
    /// month.
    pub leaders: Vec<MonthEntry>,
    /// Last month's winner's display name, or `None` when the ledger has no last month.
    pub last_month: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfTheMonth {
    pub model: MonthCard,
    pub harness: MonthCard,
}

/// The Of the month command's answer: the figures, or `{ "error": "opencodex-unavailable" }`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum OfTheMonthAnswer {
    Ready(OfTheMonth),
    Unavailable { error: OpencodexUnavailable },
}

impl OfTheMonthAnswer {
    pub fn unavailable() -> Self {
        OfTheMonthAnswer::Unavailable {
            error: OpencodexUnavailable::OpencodexUnavailable,
        }
    }
}

/// How many contenders a card shows: the winner and two runners-up.
const SHOWN: usize = 3;

/// The current and the last local calendar month, as instants in milliseconds.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Months {
    /// Local midnight of last month's 1st.
    last: i64,
    /// Local midnight of this month's 1st.
    this: i64,
    /// Local midnight of next month's 1st.
    next: i64,
}

impl Months {
    /// The months around the clock's now, in its local time zone (each 1st's own
    /// offset, so a daylight-saving change in between is followed).
    pub(crate) fn around(clock: &dyn Clock) -> Self {
        let now = clock.now();
        let today = now.with_timezone(&clock.local_offset(now)).date_naive();
        let first = NaiveDate::from_ymd_opt(today.year(), today.month(), 1).unwrap();
        let midnight = |day: NaiveDate| local_midnight(day, clock).timestamp_millis();
        Months {
            last: midnight(first - MonthCount::new(1)),
            this: midnight(first),
            next: midnight(first + MonthCount::new(1)),
        }
    }
}

/// The figures for this month, with last month's winners.
pub(crate) fn of_the_month(rows: &[UsageRow], months: Months) -> OfTheMonth {
    let in_month = |from: i64, to: i64| -> Vec<&UsageRow> {
        rows.iter()
            .filter(|row| (from..to).contains(&row.timestamp) && total(row) > 0)
            .collect()
    };
    let (this_month, last_month) = (
        in_month(months.this, months.next),
        in_month(months.last, months.this),
    );
    let card = |contender: fn(&UsageRow) -> (String, String)| MonthCard {
        leaders: ranked(&this_month, contender)
            .into_iter()
            .take(SHOWN)
            .collect(),
        last_month: ranked(&last_month, contender)
            .into_iter()
            .next()
            .map(|winner| winner.name),
    };
    OfTheMonth {
        model: card(|row| {
            (
                row.served_model.clone(),
                crate::model::display_name(&row.served_model),
            )
        }),
        harness: card(|row| (row.surface.id().to_string(), row.surface.name().to_string())),
    }
}

/// A request's total tokens; none when opencodex has no counts for it.
fn total(row: &UsageRow) -> u64 {
    row.tokens.map_or(0, |tokens| tokens.total)
}

/// A contender's tally: its name, total tokens and last use (ms).
struct Tally {
    name: String,
    total: u64,
    last_used: i64,
}

/// The contenders `contender` (id, display name) finds in `rows`, ranked by total
/// tokens, a tie going to the one used most recently.
fn ranked(rows: &[&UsageRow], contender: fn(&UsageRow) -> (String, String)) -> Vec<MonthEntry> {
    let mut tallies: HashMap<String, Tally> = HashMap::new();
    let mut all = 0;
    for row in rows {
        all += total(row);
        let (id, name) = contender(row);
        let tally = tallies.entry(id).or_insert(Tally {
            name,
            total: 0,
            last_used: row.timestamp,
        });
        tally.total += total(row);
        tally.last_used = tally.last_used.max(row.timestamp);
    }
    let mut ranked: Vec<(String, Tally)> = tallies.into_iter().collect();
    ranked.sort_by(|(_, a), (_, b)| (b.total, b.last_used).cmp(&(a.total, a.last_used)));
    ranked
        .into_iter()
        .map(|(id, tally)| MonthEntry {
            id,
            name: tally.name,
            total: tally.total,
            share: tally.total as f64 / all as f64,
        })
        .collect()
}
