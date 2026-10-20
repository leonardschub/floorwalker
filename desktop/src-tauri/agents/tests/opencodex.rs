//! The opencodex statistics driven through the registry's public seam: a
//! `~/.opencodex`-shaped directory built from the checked-in fixtures, a fixed clock in
//! a fixed local time zone, and assertions on what the statistics command returns
//! (spec "Statistics", "Testing Decisions").
//!
//! The fixtures (tests/fixtures/opencodex-2.84.0/, written by its `make.py`) follow the
//! shape opencodex 2.84.0 writes, with invented values. Their timeline is built around
//! now = 2026-10-17 10:00 UTC in UTC+02:00, so local midnight of the 17th is
//! 2026-10-16 22:00 UTC:
//!
//! | row     | at (UTC)              | surface        | served model      | notes              |
//! |---------|-----------------------|----------------|-------------------|--------------------|
//! | ocx-a1  | 10-16 21:59:59.999    | claude         | claude-opus-5-5   | just before Today  |
//! | ocx-b2  | 10-16 22:00:00.000    | claude         | claude-opus-5-5   | first of Today     |
//! | ocx-c3  | 10-17 08:00           | (Codex)        | gpt-6.1-sol       | 2 attempts         |
//! | ocx-d4  | 10-17 09:00           | grok           | grok-5            | 429                |
//! | ocx-e5  | 10-17 09:30           | claude-desktop | claude-sonnet-5-5 | 200, failure set   |
//! | ocx-f6  | 10-17 09:45           | claude         | claude-opus-5-5   | no `attempts`      |
//! | ocx-g7  | 10-10 22:00:00.000    | claude         | claude-opus-5-5   | first of 7 days    |
//! | ocx-h8  | 10-10 21:59:59.999    | (Codex)        | gpt-6.1-sol       | just before 7 days |
//! | ocx-i9  | 09-17 22:00:00.000    | (Codex)        | gpt-6.1-sol       | first of 30 days   |
//! | ocx-j10 | 09-17 21:59:59.999    | claude         | claude-opus-5-5   | just before 30 d.  |
//!
//! Plus a corrupt line and a line that is not a usage row, both skipped.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use agents::{
    Clock, LiveSession, Liveness, LivenessError, OpencodexAnswer, OpencodexStatistics, Range,
    Registry, Sources, SurfaceFilter,
};
use chrono::{DateTime, FixedOffset, Utc};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/opencodex-2.84.0")
}

/// A temporary home holding `.opencodex` with the fixture ledger and quota caches.
fn opencodex_home() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".opencodex");
    fs::create_dir_all(&dir).unwrap();
    for file in [
        "usage.jsonl",
        "codex-quota-cache.json",
        "provider-account-quota-cache.json",
    ] {
        fs::copy(fixtures().join(file), dir.join(file)).unwrap();
    }
    home
}

/// Now, in a local time zone of UTC+02:00, or one that changes at a given instant.
struct LocalClock {
    now: DateTime<Utc>,
    offset: FixedOffset,
    /// Before this instant the offset is `earlier` instead (a daylight-saving change).
    change: Option<(DateTime<Utc>, FixedOffset)>,
}

impl Clock for LocalClock {
    fn now(&self) -> DateTime<Utc> {
        self.now
    }

    fn local_offset(&self, at: DateTime<Utc>) -> FixedOffset {
        match self.change {
            Some((change, earlier)) if at < change => earlier,
            _ => self.offset,
        }
    }
}

fn plus_two() -> FixedOffset {
    FixedOffset::east_opt(2 * 3600).unwrap()
}

fn at(rfc3339: &str) -> LocalClock {
    LocalClock {
        now: rfc3339.parse().unwrap(),
        offset: plus_two(),
        change: None,
    }
}

const NOW: &str = "2026-10-17T10:00:00Z";

struct NobodyLive;

impl Liveness for NobodyLive {
    fn live_sessions(&self) -> Result<Vec<LiveSession>, LivenessError> {
        Ok(Vec::new())
    }
}

fn registry(home: &Path, clock: LocalClock) -> Registry {
    Registry::new(
        Sources::in_home(home),
        Box::new(clock),
        Box::new(NobodyLive),
    )
}

fn statistics(registry: &Registry, range: Range, surface: SurfaceFilter) -> OpencodexStatistics {
    match registry.opencodex_statistics(range, surface) {
        OpencodexAnswer::Ready(statistics) => statistics,
        other => panic!("expected statistics, got {other:?}"),
    }
}

fn append(home: &Path, text: &str) {
    let mut ledger = fs::OpenOptions::new()
        .append(true)
        .open(home.join(".opencodex/usage.jsonl"))
        .unwrap();
    ledger.write_all(text.as_bytes()).unwrap();
}

#[test]
fn without_an_opencodex_directory_the_statistics_say_opencodex_is_unavailable() {
    let home = tempfile::tempdir().unwrap();
    let registry = registry(home.path(), at(NOW));

    let answer = registry.opencodex_statistics(Range::Today, SurfaceFilter::All);

    assert_eq!(
        serde_json::to_value(&answer).unwrap(),
        serde_json::json!({ "error": "opencodex-unavailable" }),
        "a missing ~/.opencodex is one error the view shows in place"
    );
}

#[test]
fn ranges_start_at_local_midnight_today_and_six_and_twenty_nine_days_before() {
    let home = opencodex_home();
    let registry = registry(home.path(), at(NOW));
    let requests = |range| {
        statistics(&registry, range, SurfaceFilter::All)
            .summary
            .requests
    };

    assert_eq!(
        requests(Range::Today),
        5,
        "ocx-b2 at local midnight to ocx-f6"
    );
    assert_eq!(
        requests(Range::SevenDays),
        7,
        "adds ocx-a1 and ocx-g7, not ocx-h8"
    );
    assert_eq!(
        requests(Range::ThirtyDays),
        9,
        "adds ocx-h8 and ocx-i9, not ocx-j10"
    );
}

#[test]
fn local_midnight_follows_the_offset_of_the_day_it_starts() {
    // A daylight-saving change between the 30-day start and today: before
    // 2026-09-25 the zone is UTC+03:00, so local midnight of 09-18 is 09-17 21:00 UTC
    // and ocx-j10 (09-17 21:59:59.999) is inside the range.
    let home = opencodex_home();
    let clock = LocalClock {
        change: Some((
            "2026-09-25T00:00:00Z".parse().unwrap(),
            FixedOffset::east_opt(3 * 3600).unwrap(),
        )),
        ..at(NOW)
    };
    let registry = registry(home.path(), clock);

    let summary = statistics(&registry, Range::ThirtyDays, SurfaceFilter::All).summary;

    assert_eq!(
        summary.requests, 10,
        "every fixture row from 09-17 21:00 UTC on"
    );
}

#[test]
fn the_surface_filter_maps_each_harness_to_its_rows() {
    let home = opencodex_home();
    let registry = registry(home.path(), at(NOW));
    let requests = |surface| {
        statistics(&registry, Range::Today, surface)
            .summary
            .requests
    };

    assert_eq!(
        requests(SurfaceFilter::ClaudeCode),
        2,
        "surface claude: ocx-b2, ocx-f6"
    );
    assert_eq!(requests(SurfaceFilter::Codex), 1, "no surface: ocx-c3");
    assert_eq!(
        requests(SurfaceFilter::ClaudeDesktop),
        1,
        "claude-desktop: ocx-e5"
    );
    assert_eq!(requests(SurfaceFilter::GrokBuild), 1, "grok: ocx-d4");
    assert_eq!(requests(SurfaceFilter::All), 5);
}

#[test]
fn the_summary_counts_errors_and_failovers() {
    let home = opencodex_home();
    let registry = registry(home.path(), at(NOW));

    let summary = statistics(&registry, Range::Today, SurfaceFilter::All).summary;

    assert_eq!(summary.errors, 2, "ocx-d4's 429 and ocx-e5's failure field");
    assert_eq!(summary.failovers, 1, "ocx-c3's two attempts");
}

#[test]
fn each_served_model_gets_one_row_with_its_figures() {
    let home = opencodex_home();
    let registry = registry(home.path(), at(NOW));

    let models = statistics(&registry, Range::Today, SurfaceFilter::All).models;

    assert_eq!(
        serde_json::to_value(&models).unwrap(),
        serde_json::json!([
            {
                // ocx-b2 (served) and ocx-f6 (no servedModel: falls back to model).
                "model": "claude-opus-5-5", "name": "Opus 5.5", "provider": "anthropic-native", "requests": 2,
                "errorRate": 0.0, "medianFirstTokenMs": 500.0, "medianTokensPerSecond": 75.0,
                // (2000 − 1500) + (1000 − 900)
                "freshInput": 600, "output": 130, "cached": 2400, "cacheWrite": 0,
            },
            {
                "model": "claude-sonnet-5-5", "name": "Sonnet 5.5", "provider": "anthropic", "requests": 1,
                "errorRate": 1.0, "medianFirstTokenMs": 900.0, "medianTokensPerSecond": 20.0,
                "freshInput": 300, "output": 10, "cached": 0, "cacheWrite": 0,
            },
            {
                // Routed from the alias codex-auto: counted as what was served.
                "model": "gpt-6.1-sol", "name": "GPT-6.1 Sol", "provider": "openai", "requests": 1,
                "errorRate": 0.0, "medianFirstTokenMs": 1200.0, "medianTokensPerSecond": 100.0,
                "freshInput": 1000, "output": 200, "cached": 4000, "cacheWrite": 0,
            },
            {
                "model": "grok-5", "name": "grok-5", "provider": "xai", "requests": 1,
                "errorRate": 1.0, "medianFirstTokenMs": null, "medianTokensPerSecond": null,
                "freshInput": 0, "output": 0, "cached": 0, "cacheWrite": 0,
            },
        ]),
        "most requests first, then by id; no cost field"
    );
}

#[test]
fn a_last_line_without_a_newline_waits_until_it_is_committed() {
    let home = opencodex_home();
    let registry = registry(home.path(), at(NOW));
    let requests = || {
        statistics(&registry, Range::Today, SurfaceFilter::All)
            .summary
            .requests
    };
    assert_eq!(requests(), 5);

    let row = r#"{"requestId":"ocx-k11","timestamp":1792224000000,"provider":"openai","model":"gpt-6.1-sol","status":200,"durationMs":10}"#;
    append(home.path(), &row[..40]);
    assert_eq!(requests(), 5, "a partial last line is uncommitted");

    append(home.path(), &format!("{}\n", &row[40..]));
    assert_eq!(requests(), 6, "once committed, the row counts, once");

    append(
        home.path(),
        &format!("{}\n", row.replace("ocx-k11", "ocx-l12")),
    );
    assert_eq!(requests(), 7, "the ledger keeps growing");
}

#[test]
fn a_ledger_that_disappears_makes_opencodex_unavailable_again() {
    let home = opencodex_home();
    let registry = registry(home.path(), at(NOW));
    assert!(matches!(
        registry.opencodex_statistics(Range::Today, SurfaceFilter::All),
        OpencodexAnswer::Ready(_)
    ));

    fs::rename(
        home.path().join(".opencodex"),
        home.path().join("opencodex-aside"),
    )
    .unwrap();

    assert_eq!(
        registry.opencodex_statistics(Range::Today, SurfaceFilter::All),
        OpencodexAnswer::unavailable(),
        "~/.opencodex moved aside while the app runs"
    );
}

#[test]
fn usage_limits_are_labelled_by_provider_or_numbered() {
    let home = opencodex_home();
    let registry = registry(home.path(), at(NOW));

    let limits = statistics(&registry, Range::Today, SurfaceFilter::All).limits;

    // now = 1792231200 s. Accounts in cache order: Codex main first, then by key.
    assert_eq!(
        serde_json::to_value(&limits).unwrap(),
        serde_json::json!([
            { "account": { "provider": "codex" }, "windows": [
                { "kind": "five-hour", "label": null, "percent": 12.0, "resetsAt": 1792234800000_i64 },
                { "kind": "weekly", "label": null, "percent": 43.0, "resetsAt": 1792490400000_i64 },
            ] },
            { "account": { "account": 1 }, "windows": [
                { "kind": "weekly", "label": null, "percent": 80.0, "resetsAt": 1792317600000_i64 },
            ] },
            { "account": { "account": 2 }, "windows": [
                { "kind": "weekly", "label": null, "percent": 5.0, "resetsAt": 1792663200000_i64 },
            ] },
            // The monthly window reset an hour ago, so its 99 % is left out.
            { "account": { "provider": "anthropic" }, "windows": [
                { "kind": "five-hour", "label": null, "percent": 25.0, "resetsAt": 1792238400000_i64 },
                { "kind": "weekly", "label": null, "percent": 40.0, "resetsAt": 1792576800000_i64 },
                { "kind": "custom", "label": "Fable", "percent": 10.0, "resetsAt": 1792576800000_i64 },
            ] },
        ])
    );
}

#[test]
fn the_ledger_rows_carry_what_the_other_statistics_views_need() {
    let home = opencodex_home();
    let registry = registry(home.path(), at(NOW));

    let row = registry
        .with_ledger(|rows| {
            rows.iter()
                .find(|r| r.request_id.as_deref() == Some("ocx-c3"))
                .cloned()
        })
        .unwrap()
        .expect("ocx-c3 is read");

    assert_eq!(row.timestamp, 1792224000000);
    assert_eq!(row.surface, agents::Surface::Codex);
    assert_eq!(row.served_model, "gpt-6.1-sol");
    assert_eq!(row.provider, "openai");
    assert!(row.failed_over() && !row.is_error());
    let tokens = row.tokens.unwrap();
    assert_eq!(
        (
            tokens.fresh_input(),
            tokens.cache_read,
            tokens.cache_write,
            tokens.output,
            tokens.total
        ),
        (1000, 4000, 0, 200, 5200)
    );
}

#[test]
fn the_figures_name_the_harnesses_the_filter_offers() {
    let home = opencodex_home();
    let registry = registry(home.path(), at(NOW));

    let harnesses = statistics(&registry, Range::Today, SurfaceFilter::All).harnesses;

    assert_eq!(
        serde_json::to_value(&harnesses).unwrap(),
        serde_json::json!([
            { "id": "claude-code", "name": "Claude Code" },
            { "id": "codex", "name": "Codex" },
            { "id": "claude-desktop", "name": "Claude Desktop" },
            { "id": "grok-build", "name": "Grok Build" },
        ]),
        "the same names Of the month uses; each id is a surface filter"
    );
}

#[test]
fn rows_without_a_request_id_or_a_provider_still_count() {
    let home = opencodex_home();
    let registry = registry(home.path(), at(NOW));
    let rows = [
        r#"{"timestamp":1792224000000,"model":"grok-6","surface":"grok","status":200}"#,
        r#"{"requestId":"ocx-np","timestamp":1792224000000,"model":"grok-6","surface":"grok","status":500}"#,
    ];
    append(home.path(), &format!("{}\n", rows.join("\n")));

    let statistics = statistics(&registry, Range::Today, SurfaceFilter::GrokBuild);

    assert_eq!(statistics.summary.requests, 3, "ocx-d4 and both new rows");
    let grok = statistics
        .models
        .iter()
        .find(|m| m.model == "grok-6")
        .expect("a row for grok-6");
    assert_eq!(grok.requests, 2);
    assert_eq!(grok.provider, "unknown", "no provider reads as unknown");
}

#[test]
fn a_models_fresh_input_cache_read_cache_write_and_output_sum_to_its_total() {
    let home = opencodex_home();
    let registry = registry(home.path(), at(NOW));
    // opencodex's input includes both the cache read and the cache write.
    let row = r#"{"requestId":"ocx-w1","timestamp":1792224000000,"provider":"xai","model":"grok-6","surface":"grok","status":200,"usage":{"inputTokens":1000,"outputTokens":50,"cacheReadInputTokens":600,"cacheCreationInputTokens":100},"totalTokens":1050}"#;
    append(home.path(), &format!("{row}\n"));

    let models = statistics(&registry, Range::Today, SurfaceFilter::GrokBuild).models;

    let grok = models.iter().find(|m| m.model == "grok-6").unwrap();
    assert_eq!(
        (grok.fresh_input, grok.cached, grok.cache_write, grok.output),
        (300, 600, 100, 50),
        "fresh input is input less cache read and cache write, as in the Tokens view"
    );
}
