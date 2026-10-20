//! The statistics ranges, in local time (spec "Statistics"): *Today* from local
//! midnight, *7 days* and *30 days* as the last 7 or 30 local calendar days including
//! today. Each runs up to now.

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::Deserialize;

use crate::Clock;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum Range {
    #[serde(rename = "today")]
    Today,
    #[serde(rename = "7d")]
    SevenDays,
    #[serde(rename = "30d")]
    ThirtyDays,
}

impl Range {
    fn days(self) -> i64 {
        match self {
            Range::Today => 1,
            Range::SevenDays => 7,
            Range::ThirtyDays => 30,
        }
    }

    /// The range's first instant, in milliseconds since the epoch.
    pub fn start(self, clock: &dyn Clock) -> i64 {
        let now = clock.now();
        let today = now.with_timezone(&clock.local_offset(now)).date_naive();
        local_midnight(today - Duration::days(self.days() - 1), clock).timestamp_millis()
    }
}

/// The instant a local calendar day starts, in the clock's time zone, whose offset may
/// differ on that day from today's (a daylight-saving change in between).
pub fn local_midnight(day: NaiveDate, clock: &dyn Clock) -> DateTime<Utc> {
    let midnight = day.and_hms_opt(0, 0, 0).unwrap();
    let guess = (midnight - clock.local_offset(clock.now())).and_utc();
    (midnight - clock.local_offset(guess)).and_utc()
}
