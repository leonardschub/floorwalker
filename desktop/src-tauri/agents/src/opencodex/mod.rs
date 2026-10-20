//! What the registry reads from opencodex, under the default `~/.opencodex`: the usage
//! ledger (`usage.jsonl`, tailed) and the two quota caches. Read-only, and nothing else
//! there is ever opened: never `admin-api-token`, `auth.json`, `runtime-port.json`,
//! `claude-intercept/ca.key` or `proxy-token`, `config.json`, `*.salt` or
//! `responses-state*`, and never `/api/*` or `ocx usage --json` (spec "Sources").
//!
//! [`Ledger`] keeps the parsed rows ([`UsageRow`]) for every statistics view: the
//! opencodex view here (ticket 15), and Of the month and Tokens (tickets 16 and 17),
//! which reach the same rows through `Registry::with_ledger`.

mod ledger;
mod ledger_watch;
mod month;
mod quota;
mod range;
mod statistics;

pub use ledger::{Ledger, Surface, Tokens, UsageRow};
pub use ledger_watch::{LedgerTiming, LedgerWatch};
pub use month::{MonthCard, MonthEntry, OfTheMonth, OfTheMonthAnswer};
pub use quota::{AccountLabel, LimitWindow, UsageLimit, WindowKind};
pub use range::{local_midnight, Range};
pub use statistics::{
    Harness, ModelFigures, OpencodexAnswer, OpencodexStatistics, OpencodexUnavailable, Summary,
    SurfaceFilter,
};

pub(crate) use month::{of_the_month, Months};
pub(crate) use quota::read_limits;
pub(crate) use statistics::statistics;

/// The ledger's file name under `~/.opencodex`.
pub(crate) const LEDGER: &str = "usage.jsonl";
