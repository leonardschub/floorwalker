//! The Of the month figures driven through the registry's public seam: a
//! `~/.opencodex`-shaped directory holding a ledger written by each test, a fixed clock
//! in a fixed local time zone, and assertions on what `Registry::of_the_month` returns
//! (spec "Statistics", "Of the month"). The rows follow opencodex 2.84.0's shape, with
//! invented values.

use std::fs;
use std::path::Path;

use agents::{
    Clock, LiveSession, Liveness, LivenessError, MonthCard, MonthEntry, OfTheMonth,
    OfTheMonthAnswer, Registry, Sources,
};
use chrono::{DateTime, FixedOffset, Utc};

/// Now, in a fixed local time zone.
struct LocalClock {
    now: DateTime<Utc>,
    offset: FixedOffset,
}

impl Clock for LocalClock {
    fn now(&self) -> DateTime<Utc> {
        self.now
    }

    fn local_offset(&self, _at: DateTime<Utc>) -> FixedOffset {
        self.offset
    }
}

struct NobodyLive;

impl Liveness for NobodyLive {
    fn live_sessions(&self) -> Result<Vec<LiveSession>, LivenessError> {
        Ok(Vec::new())
    }
}

/// UTC+02:00, so local midnight of the 1st is 22:00 UTC on the day before.
fn plus_two() -> FixedOffset {
    FixedOffset::east_opt(2 * 3600).unwrap()
}

fn ms(rfc3339: &str) -> i64 {
    rfc3339.parse::<DateTime<Utc>>().unwrap().timestamp_millis()
}

/// One ledger row: when (UTC), surface (`None` is Codex), served model, total tokens.
fn row(at: &str, surface: Option<&'static str>, served: &'static str, total: u64) -> String {
    let at = ms(at);
    let surface = surface.map_or(String::new(), |s| format!(r#","surface":"{s}""#));
    format!(
        r#"{{"requestId":"ocx-{at}-{total}","timestamp":{at},"provider":"anthropic-native","model":"{served}","requestedModel":"claude-haiku-5-5","servedModel":"{served}","status":200{surface},"usage":{{"inputTokens":{total},"outputTokens":0,"totalTokens":{total}}},"totalTokens":{total}}}"#
    )
}

/// A temporary home whose `.opencodex/usage.jsonl` holds `lines`.
fn home_with(lines: &[String]) -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".opencodex");
    fs::create_dir_all(&dir).unwrap();
    let mut ledger = lines.join("\n");
    ledger.push('\n');
    fs::write(dir.join("usage.jsonl"), ledger).unwrap();
    home
}

fn of_the_month(home: &Path, now: &str) -> OfTheMonth {
    let registry = Registry::new(
        Sources::in_home(home),
        Box::new(LocalClock {
            now: now.parse().unwrap(),
            offset: plus_two(),
        }),
        Box::new(NobodyLive),
    );
    match registry.of_the_month() {
        OfTheMonthAnswer::Ready(figures) => figures,
        other => panic!("expected figures, got {other:?}"),
    }
}

fn entry(id: &str, name: &str, total: u64, share: f64) -> MonthEntry {
    MonthEntry {
        id: id.to_string(),
        name: name.to_string(),
        total,
        share,
    }
}

#[test]
fn the_winner_is_the_model_with_the_most_total_tokens_this_month_with_its_share() {
    let home = home_with(&[
        row(
            "2026-10-05T10:00:00Z",
            Some("claude"),
            "claude-opus-5-5",
            300,
        ),
        row("2026-10-06T10:00:00Z", None, "gpt-6.1-sol", 100),
        row(
            "2026-10-07T10:00:00Z",
            Some("claude"),
            "claude-opus-5-5",
            300,
        ),
    ]);

    let month = of_the_month(home.path(), "2026-10-17T10:00:00Z");

    assert_eq!(
        month.model.leaders,
        vec![
            entry("claude-opus-5-5", "Opus 5.5", 600, 600.0 / 700.0),
            entry("gpt-6.1-sol", "GPT-6.1 Sol", 100, 100.0 / 700.0),
        ],
        "the winner first with its total and share, then the runner-up"
    );
}

#[test]
fn the_harness_of_the_month_counts_every_surface_by_its_name() {
    let home = home_with(&[
        row(
            "2026-10-05T10:00:00Z",
            Some("claude"),
            "claude-opus-5-5",
            500,
        ),
        row("2026-10-06T10:00:00Z", None, "gpt-6.1-sol", 300),
        row("2026-10-07T10:00:00Z", Some("grok"), "grok-5", 150),
        row(
            "2026-10-08T10:00:00Z",
            Some("claude-desktop"),
            "deepseek-flash",
            50,
        ),
    ]);

    let month = of_the_month(home.path(), "2026-10-17T10:00:00Z");

    assert_eq!(
        month.harness.leaders,
        vec![
            entry("claude-code", "Claude Code", 500, 0.5),
            entry("codex", "Codex", 300, 0.3),
            entry("grok-build", "Grok Build", 150, 0.15),
        ],
        "the winner and two runners-up only; Claude Desktop is fourth"
    );
}

#[test]
fn the_month_starts_at_local_midnight_of_the_1st_even_on_the_1st_itself() {
    // Local midnight of 1 November in UTC+02:00 is 31 October 22:00 UTC; now is half
    // an hour into the 1st, still 31 October in UTC.
    let home = home_with(&[
        row(
            "2026-10-31T21:59:59.999Z",
            Some("claude"),
            "claude-opus-5-5",
            900,
        ),
        row("2026-10-31T22:00:00Z", None, "gpt-6.1-sol", 40),
        row(
            "2026-10-31T22:20:00Z",
            Some("claude"),
            "claude-sonnet-5-5",
            60,
        ),
    ]);

    let month = of_the_month(home.path(), "2026-10-31T22:30:00Z");

    assert_eq!(
        month.model.leaders,
        vec![
            entry("claude-sonnet-5-5", "Sonnet 5.5", 60, 0.6),
            entry("gpt-6.1-sol", "GPT-6.1 Sol", 40, 0.4),
        ],
        "the row a millisecond before local midnight is last month's"
    );
}

#[test]
fn the_month_runs_to_the_last_local_day() {
    // 31 October 23:30 local is 21:30 UTC; local midnight of 1 October is
    // 30 September 22:00 UTC.
    let home = home_with(&[
        row(
            "2026-09-30T21:59:59.999Z",
            Some("claude"),
            "claude-opus-5-5",
            900,
        ),
        row("2026-09-30T22:00:00Z", None, "gpt-6.1-sol", 10),
        row("2026-10-31T21:29:00Z", None, "gpt-6.1-sol", 10),
    ]);

    let month = of_the_month(home.path(), "2026-10-31T21:30:00Z");

    assert_eq!(
        month.model.leaders,
        vec![entry("gpt-6.1-sol", "GPT-6.1 Sol", 20, 1.0)],
        "the 1st from local midnight to the last day's last minute"
    );
}

#[test]
fn a_tie_goes_to_the_one_used_most_recently() {
    let home = home_with(&[
        row(
            "2026-10-05T10:00:00Z",
            Some("claude"),
            "claude-opus-5-5",
            50,
        ),
        row("2026-10-06T10:00:00Z", None, "gpt-6.1-sol", 100),
        row(
            "2026-10-07T10:00:00Z",
            Some("claude"),
            "claude-opus-5-5",
            50,
        ),
        row("2026-10-08T10:00:00Z", Some("grok"), "grok-5", 100),
    ]);

    let month = of_the_month(home.path(), "2026-10-17T10:00:00Z");

    let order: Vec<&str> = month.model.leaders.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(
        order,
        ["grok-5", "claude-opus-5-5", "gpt-6.1-sol"],
        "equal totals rank by their last use, latest first"
    );
    let order: Vec<&str> = month
        .harness
        .leaders
        .iter()
        .map(|e| e.id.as_str())
        .collect();
    assert_eq!(order, ["grok-build", "claude-code", "codex"]);
}

#[test]
fn each_card_names_last_months_winner() {
    let home = home_with(&[
        // August is not last month and is not counted.
        row("2026-08-20T10:00:00Z", Some("grok"), "grok-5", 9_000),
        // Local September (30 September 22:00 UTC is already October).
        row("2026-09-02T10:00:00Z", None, "gpt-6.1-sol", 300),
        row(
            "2026-09-15T10:00:00Z",
            Some("claude"),
            "claude-opus-5-5",
            200,
        ),
        row(
            "2026-09-30T21:00:00Z",
            Some("claude"),
            "claude-opus-5-5",
            200,
        ),
        row("2026-09-30T22:00:00Z", Some("grok"), "grok-5", 5_000),
        // October.
        row("2026-10-05T10:00:00Z", None, "gpt-6.1-sol", 10),
    ]);

    let month = of_the_month(home.path(), "2026-10-17T10:00:00Z");

    assert_eq!(
        month.model.last_month.as_deref(),
        Some("Opus 5.5"),
        "last month is the previous local calendar month only"
    );
    assert_eq!(month.harness.last_month.as_deref(), Some("Claude Code"));
}

#[test]
fn a_ledger_without_last_month_names_no_last_winner() {
    let home = home_with(&[
        row("2026-08-20T10:00:00Z", Some("grok"), "grok-5", 9_000),
        row("2026-10-05T10:00:00Z", None, "gpt-6.1-sol", 10),
    ]);

    let month = of_the_month(home.path(), "2026-10-17T10:00:00Z");

    assert_eq!(month.model.last_month, None);
    assert_eq!(month.harness.last_month, None);
}

#[test]
fn an_empty_month_has_no_leaders_but_still_last_months_winner() {
    let home = home_with(&[row("2026-09-20T10:00:00Z", None, "gpt-6.1-sol", 10)]);

    let month = of_the_month(home.path(), "2026-10-17T10:00:00Z");

    assert_eq!(
        month.model,
        MonthCard {
            leaders: Vec::new(),
            last_month: Some("GPT-6.1 Sol".to_string()),
        },
        "the view reads \"No usage yet this month\""
    );
    assert!(month.harness.leaders.is_empty());
}

#[test]
fn requests_without_tokens_are_no_usage() {
    let home = home_with(&[
        row("2026-10-05T10:00:00Z", Some("claude"), "unknown", 0),
        r#"{"requestId":"ocx-x","timestamp":1791194400000,"provider":"unknown","model":"unknown","status":502,"surface":"claude"}"#
            .to_string(),
    ]);

    let month = of_the_month(home.path(), "2026-10-17T10:00:00Z");

    assert!(
        month.model.leaders.is_empty() && month.harness.leaders.is_empty(),
        "a failed request with no tokens is not a contender: {month:?}"
    );
}

#[test]
fn a_routed_request_counts_as_the_model_that_served_it() {
    // Claude Code asked for the placeholder alias; opencodex served GPT-6.1 Sol.
    let routed = r#"{"requestId":"ocx-r","timestamp":1791194400000,"provider":"openai","model":"claude-opus-5-5","requestedModel":"claude-opus-5-5","servedModel":"gpt-6.1-sol","status":200,"surface":"claude","usage":{"inputTokens":70,"outputTokens":0,"totalTokens":70},"totalTokens":70}"#;
    let home = home_with(&[
        routed.to_string(),
        row(
            "2026-10-06T10:00:00Z",
            Some("claude"),
            "claude-opus-5-5",
            30,
        ),
        // Versions are counted apart.
        row(
            "2026-10-07T10:00:00Z",
            Some("claude"),
            "claude-opus-4-8",
            20,
        ),
    ]);

    let month = of_the_month(home.path(), "2026-10-17T10:00:00Z");

    assert_eq!(
        month.model.leaders,
        vec![
            entry("gpt-6.1-sol", "GPT-6.1 Sol", 70, 70.0 / 120.0),
            entry("claude-opus-5-5", "Opus 5.5", 30, 30.0 / 120.0),
            entry("claude-opus-4-8", "Opus 4.8", 20, 20.0 / 120.0),
        ]
    );
}

#[test]
fn without_a_ledger_opencodex_is_unavailable() {
    let home = tempfile::tempdir().unwrap();
    let registry = Registry::new(
        Sources::in_home(home.path()),
        Box::new(LocalClock {
            now: "2026-10-17T10:00:00Z".parse().unwrap(),
            offset: plus_two(),
        }),
        Box::new(NobodyLive),
    );

    let answer = registry.of_the_month();

    assert_eq!(answer, OfTheMonthAnswer::unavailable());
    assert_eq!(
        serde_json::to_value(&answer).unwrap(),
        serde_json::json!({ "error": "opencodex-unavailable" })
    );
}
