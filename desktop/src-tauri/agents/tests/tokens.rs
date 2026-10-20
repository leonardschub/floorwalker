//! The Tokens view (ticket 17), driven through the registry's public seam: a temporary
//! home holding a `~/.claude`-shaped tree of transcripts and a `~/.opencodex` ledger,
//! a fixed clock in a fixed local time zone, and assertions on what the statistics
//! command returns (spec "Statistics", "Tokens view").
//!
//! Every number is opencodex's; the transcripts only say which agent made each request,
//! by `requestId`. The homes are written by the tests themselves, so each test shows
//! the rows and lines it counts. Now is 2026-10-17 10:00 UTC in UTC+02:00, so Today
//! starts at 2026-10-16 22:00 UTC.

use std::fs;
use std::io::Write;
use std::path::Path;

use agents::{
    AgentKind, BucketSize, Clock, LiveSession, Liveness, LivenessError, OpencodexAnswer, Range,
    Registry, Sources, SurfaceFilter, TokenStatistics, TokensAnswer,
};
use chrono::{DateTime, FixedOffset, Utc};

const NOW: &str = "2026-10-17T10:00:00Z";

const ORCHARD: &str = "-home-dev-Projects-Orchard";
const HARBOR: &str = "-home-dev-Projects-Harbor";
const S1: &str = "11111111-1111-4111-8111-111111111111";
const S2: &str = "22222222-2222-4222-8222-222222222222";
const SCOUT: &str = "a000000000000000a";
const DIGGER: &str = "b000000000000000b";

struct LocalClock;

impl Clock for LocalClock {
    fn now(&self) -> DateTime<Utc> {
        NOW.parse().unwrap()
    }

    fn local_offset(&self, _at: DateTime<Utc>) -> FixedOffset {
        FixedOffset::east_opt(2 * 3600).unwrap()
    }
}

struct NobodyLive;

impl Liveness for NobodyLive {
    fn live_sessions(&self) -> Result<Vec<LiveSession>, LivenessError> {
        Ok(Vec::new())
    }
}

/// A home being written by a test.
struct Home(tempfile::TempDir);

impl Home {
    fn new() -> Self {
        Home(tempfile::tempdir().unwrap())
    }

    fn path(&self) -> &Path {
        self.0.path()
    }

    fn append(&self, relative: &str, text: &str) {
        let path = self.path().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        file.write_all(text.as_bytes()).unwrap();
    }

    /// An assistant line of `request` in a transcript (`agent`: none for the root).
    fn line(&self, project: &str, session: &str, agent: Option<&str>, request: &str, at: &str) {
        let cwd = format!("/home/dev/Projects/{}", project.rsplit('-').next().unwrap());
        let line = serde_json::json!({
            "type": "assistant",
            "timestamp": at,
            "cwd": cwd,
            "sessionId": session,
            "isSidechain": agent.is_some(),
            "requestId": request,
            "message": { "model": "claude-opus-5-5", "content": [{ "type": "text", "text": "[scrubbed]" }] },
        });
        let file = match agent {
            None => format!(".claude/projects/{project}/{session}.jsonl"),
            Some(agent) => {
                format!(".claude/projects/{project}/{session}/subagents/agent-{agent}.jsonl")
            }
        };
        self.append(&file, &format!("{line}\n"));
    }

    /// A subagent's meta: its type and who spawned it (none for the root).
    fn meta(&self, project: &str, session: &str, agent: &str, kind: &str, parent: Option<&str>) {
        let mut meta = serde_json::json!({ "agentType": kind, "finished": true });
        if let Some(parent) = parent {
            meta["parentAgentId"] = parent.into();
        }
        let path = self.path().join(format!(
            ".claude/projects/{project}/{session}/subagents/agent-{agent}.meta.json"
        ));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, meta.to_string()).unwrap();
    }

    /// A ledger row: `surface` none for Codex; tokens as (input incl. cache read and
    /// write, cache read, cache write, output), total = input + output.
    fn row(&self, request: &str, at: &str, surface: Option<&str>, tokens: (u64, u64, u64, u64)) {
        let (input, read, write, output) = tokens;
        let at: DateTime<Utc> = at.parse().unwrap();
        let mut row = serde_json::json!({
            "requestId": request,
            "timestamp": at.timestamp_millis(),
            "provider": "anthropic-native",
            "model": "claude-opus-5-5",
            "status": 200,
            "usage": {
                "inputTokens": input,
                "outputTokens": output,
                "cachedInputTokens": read,
                "cacheReadInputTokens": read,
                "cacheCreationInputTokens": write,
            },
            "totalTokens": input + output,
        });
        if let Some(surface) = surface {
            row["surface"] = surface.into();
        }
        self.append(".opencodex/usage.jsonl", &format!("{row}\n"));
    }

    fn registry(&self) -> Registry {
        let registry = Registry::new(
            Sources::in_home(self.path()),
            Box::new(LocalClock),
            Box::new(NobodyLive),
        );
        registry.build();
        registry
    }
}

fn tokens(registry: &Registry, range: Range) -> TokenStatistics {
    match registry.token_statistics(range) {
        TokensAnswer::Ready(statistics) => statistics,
        other => panic!("expected token statistics, got {other:?}"),
    }
}

fn totals(statistics: &TokenStatistics) -> Vec<(String, u64)> {
    let mut found = Vec::new();
    for project in &statistics.projects {
        found.push((project.name.clone(), project.figures.total));
        for session in &project.sessions {
            found.push((session.id.clone(), session.figures.total));
            for agent in &session.agents {
                found.push((agent.id.clone(), agent.figures.total));
            }
        }
    }
    found
}

#[test]
fn each_claude_code_row_goes_to_the_agent_whose_transcript_has_its_request_id() {
    let home = Home::new();
    home.line(ORCHARD, S1, None, "ocx-r1", "2026-10-17T08:00:00Z");
    home.line(HARBOR, S2, None, "ocx-r2", "2026-10-17T09:00:00Z");
    home.row(
        "ocx-r1",
        "2026-10-17T08:00:00Z",
        Some("claude"),
        (1000, 0, 0, 10),
    );
    home.row(
        "ocx-r2",
        "2026-10-17T09:00:00Z",
        Some("claude"),
        (500, 0, 0, 5),
    );

    let statistics = tokens(&home.registry(), Range::Today);

    assert_eq!(statistics.total, 1515, "every Claude Code row of Today");
    assert_eq!(
        totals(&statistics),
        vec![
            ("Orchard".to_string(), 1010),
            (S1.to_string(), 1010),
            (S1.to_string(), 1010),
            ("Harbor".to_string(), 505),
            (S2.to_string(), 505),
            (S2.to_string(), 505),
        ],
        "projects by total, each into its session and the session's root agent"
    );
}

#[test]
fn a_subagents_requests_are_its_own_at_any_depth_and_in_sessions_the_tree_no_longer_shows() {
    let home = Home::new();
    // Ten days ago: the tree no longer shows this session, the 30-day range counts it.
    home.line(ORCHARD, S1, None, "ocx-root", "2026-10-07T08:00:00Z");
    home.meta(ORCHARD, S1, SCOUT, "Explore", None);
    home.line(
        ORCHARD,
        S1,
        Some(SCOUT),
        "ocx-scout",
        "2026-10-07T08:01:00Z",
    );
    home.meta(ORCHARD, S1, DIGGER, "general-purpose", Some(SCOUT));
    home.line(
        ORCHARD,
        S1,
        Some(DIGGER),
        "ocx-digger",
        "2026-10-07T08:02:00Z",
    );
    home.row(
        "ocx-root",
        "2026-10-07T08:00:00Z",
        Some("claude"),
        (100, 0, 0, 0),
    );
    home.row(
        "ocx-scout",
        "2026-10-07T08:01:00Z",
        Some("claude"),
        (20, 0, 0, 0),
    );
    home.row(
        "ocx-digger",
        "2026-10-07T08:02:00Z",
        Some("claude"),
        (3, 0, 0, 0),
    );

    let statistics = tokens(&home.registry(), Range::ThirtyDays);

    let session = &statistics.projects[0].sessions[0];
    let agents: Vec<_> = session
        .agents
        .iter()
        .map(|agent| {
            let id = agent.id.as_str();
            (agent.kind, agent.name.as_str(), id, agent.figures.total)
        })
        .collect();
    assert_eq!(
        agents,
        vec![
            (AgentKind::Root, "", S1, 100),
            (AgentKind::Subagent, "Explore", SCOUT, 20),
            (AgentKind::Subagent, "general-purpose", DIGGER, 3),
        ],
        "each request counts for the agent that made it, not the one that spawned it"
    );
    assert_eq!(session.figures.total, 123, "the session sums its agents");
}

#[test]
fn a_response_written_as_several_lines_counts_once_for_the_agent_with_its_last_line() {
    let home = Home::new();
    // One response, three content blocks: three lines with the same requestId.
    for at in ["08:00:00", "08:00:01", "08:00:02"] {
        home.line(ORCHARD, S1, None, "ocx-r1", &format!("2026-10-17T{at}Z"));
    }
    // A request whose last line is in the subagent's transcript.
    home.line(ORCHARD, S1, None, "ocx-r2", "2026-10-17T08:05:00Z");
    home.meta(ORCHARD, S1, SCOUT, "Explore", None);
    home.line(ORCHARD, S1, Some(SCOUT), "ocx-r2", "2026-10-17T08:05:01Z");
    home.row(
        "ocx-r1",
        "2026-10-17T08:00:00Z",
        Some("claude"),
        (1000, 0, 0, 10),
    );
    home.row(
        "ocx-r2",
        "2026-10-17T08:05:00Z",
        Some("claude"),
        (40, 0, 0, 2),
    );

    let statistics = tokens(&home.registry(), Range::Today);

    assert_eq!(
        statistics.total, 1052,
        "one row per request, however many lines"
    );
    assert_eq!(
        totals(&statistics),
        vec![
            ("Orchard".to_string(), 1052),
            (S1.to_string(), 1052),
            (S1.to_string(), 1010),
            (SCOUT.to_string(), 42),
        ],
    );
}

#[test]
fn claude_code_rows_matching_no_transcript_go_on_one_unattributed_row_and_other_harnesses_not_at_all(
) {
    let home = Home::new();
    home.line(ORCHARD, S1, None, "ocx-r1", "2026-10-17T08:00:00Z");
    home.row(
        "ocx-r1",
        "2026-10-17T08:00:00Z",
        Some("claude"),
        (1000, 0, 0, 10),
    );
    home.row(
        "ocx-title",
        "2026-10-17T08:01:00Z",
        Some("claude"),
        (300, 0, 0, 7),
    );
    home.row(
        "ocx-title2",
        "2026-10-17T08:02:00Z",
        Some("claude"),
        (90, 0, 0, 3),
    );
    home.row("ocx-codex", "2026-10-17T08:03:00Z", None, (5000, 0, 0, 50));
    home.row(
        "ocx-grok",
        "2026-10-17T08:04:00Z",
        Some("grok"),
        (5000, 0, 0, 50),
    );

    let statistics = tokens(&home.registry(), Range::Today);

    assert_eq!(statistics.total, 1410, "the Claude Code rows only");
    let unattributed = statistics.unattributed.expect("an Unattributed row");
    assert_eq!(unattributed.total, 400, "ocx-title and ocx-title2");
    assert!(
        (unattributed.share - 400.0 / 1410.0).abs() < 1e-9,
        "its share of the range: {}",
        unattributed.share
    );
    assert_eq!(statistics.projects[0].figures.total, 1010);
}

#[test]
fn a_request_copied_into_other_sessions_belongs_to_the_session_that_had_it_first() {
    const S3: &str = "33333333-3333-4333-8333-333333333333";
    let home = Home::new();
    // S2 started first and made ocx-r1; S3 is a later /branch copy of it, and so is
    // S1, whose copy has the same timestamp: the tie goes to the earlier session.
    home.line(HARBOR, S2, None, "ocx-old", "2026-10-17T07:00:00Z");
    home.line(HARBOR, S2, None, "ocx-r1", "2026-10-17T08:00:00Z");
    home.line(ORCHARD, S1, None, "ocx-r1", "2026-10-17T08:00:00Z");
    home.line(ORCHARD, S1, None, "ocx-r2", "2026-10-17T09:00:00Z");
    home.line(ORCHARD, S3, None, "ocx-r1", "2026-10-17T09:30:00Z");
    home.row(
        "ocx-r1",
        "2026-10-17T08:00:00Z",
        Some("claude"),
        (100, 0, 0, 0),
    );

    let statistics = tokens(&home.registry(), Range::Today);

    assert_eq!(
        totals(&statistics),
        vec![
            ("Harbor".to_string(), 100),
            (S2.to_string(), 100),
            (S2.to_string(), 100),
        ],
        "counted once, for the session that had it first"
    );
    let tree = home.registry();
    tree.attribute_tokens();
    let tree = tree.snapshot();
    let roots: Vec<(String, u64)> = tree
        .projects
        .iter()
        .flat_map(|p| p.sessions.iter())
        .map(|s| (s.id.clone(), s.root.tokens))
        .collect();
    assert!(
        roots.contains(&(S2.to_string(), 100)) && roots.iter().all(|(id, n)| id == S2 || *n == 0),
        "the tree agrees: {roots:?}"
    );
}

#[test]
fn raw_request_lines_attribute_nothing_whatever_their_key_order() {
    let home = Home::new();
    home.line(ORCHARD, S1, None, "ocx-r1", "2026-10-17T08:00:00Z");
    // Content-bearing lines, `type` not first: never read, so their ids claim nothing.
    for (kind, request) in [
        ("api-request-blob", "ocx-blob"),
        ("api-request", "ocx-req"),
        ("api-request-shape", "ocx-shape"),
    ] {
        home.append(
            &format!(".claude/projects/{ORCHARD}/{S1}.jsonl"),
            &format!(
                r#"{{"requestId":"{request}","sessionId":"{S1}","type":"{kind}","timestamp":"2026-10-17T08:01:00Z","message":{{"content":"[scrubbed]"}}}}"#
            ),
        );
        home.append(&format!(".claude/projects/{ORCHARD}/{S1}.jsonl"), "\n");
        home.row(
            request,
            "2026-10-17T08:01:00Z",
            Some("claude"),
            (10, 0, 0, 0),
        );
    }
    home.row(
        "ocx-r1",
        "2026-10-17T08:00:00Z",
        Some("claude"),
        (1000, 0, 0, 10),
    );

    let statistics = tokens(&home.registry(), Range::Today);

    assert_eq!(statistics.projects[0].figures.total, 1010, "ocx-r1 only");
    assert_eq!(statistics.unattributed.map(|row| row.total), Some(30));
}

#[test]
fn a_claude_code_row_without_a_request_id_counts_as_unattributed() {
    let home = Home::new();
    home.line(ORCHARD, S1, None, "ocx-r1", "2026-10-17T08:00:00Z");
    home.row(
        "ocx-r1",
        "2026-10-17T08:00:00Z",
        Some("claude"),
        (1000, 0, 0, 10),
    );
    let at = ms("2026-10-17T08:01:00Z");
    home.append(
        ".opencodex/usage.jsonl",
        &format!(
            r#"{{"timestamp":{at},"model":"claude-opus-5-5","surface":"claude","status":200,"usage":{{"inputTokens":40,"outputTokens":2}},"totalTokens":42}}"#
        ),
    );
    home.append(".opencodex/usage.jsonl", "\n");

    let statistics = tokens(&home.registry(), Range::Today);

    assert_eq!(statistics.total, 1052, "the row without a requestId counts");
    assert_eq!(statistics.unattributed.map(|row| row.total), Some(42));
}

#[test]
fn every_row_splits_into_fresh_input_cache_read_cache_write_and_output_summing_to_its_total() {
    let home = Home::new();
    home.line(ORCHARD, S1, None, "ocx-r1", "2026-10-17T08:00:00Z");
    home.line(ORCHARD, S1, None, "ocx-r2", "2026-10-17T08:01:00Z");
    // opencodex's input includes the cache read and the cache write.
    home.row(
        "ocx-r1",
        "2026-10-17T08:00:00Z",
        Some("claude"),
        (1000, 600, 100, 50),
    );
    home.row(
        "ocx-r2",
        "2026-10-17T08:01:00Z",
        Some("claude"),
        (2000, 1500, 0, 100),
    );
    home.row(
        "ocx-u",
        "2026-10-17T08:02:00Z",
        Some("claude"),
        (10, 4, 1, 2),
    );

    let statistics = tokens(&home.registry(), Range::Today);

    let project = statistics.projects[0].figures;
    assert_eq!(
        (
            project.fresh_input,
            project.cache_read,
            project.cache_write,
            project.output
        ),
        (800, 2100, 100, 150),
        "fresh input is input less cache read and cache write: 300 + 500"
    );
    assert_eq!(project.total, 3150);
    assert_eq!(
        project.fresh_input + project.cache_read + project.cache_write + project.output,
        project.total
    );
    let unattributed = statistics.unattributed.unwrap();
    assert_eq!(
        (
            unattributed.fresh_input,
            unattributed.cache_read,
            unattributed.cache_write,
            unattributed.output
        ),
        (5, 4, 1, 2)
    );
    assert_eq!(unattributed.total, 12);
}

#[test]
fn without_an_opencodex_ledger_the_tokens_view_says_opencodex_is_unavailable() {
    let home = Home::new();
    home.line(ORCHARD, S1, None, "ocx-r1", "2026-10-17T08:00:00Z");

    let answer = home.registry().token_statistics(Range::Today);

    assert_eq!(
        serde_json::to_value(&answer).unwrap(),
        serde_json::json!({ "error": "opencodex-unavailable" }),
    );
}

fn ms(rfc3339: &str) -> i64 {
    rfc3339.parse::<DateTime<Utc>>().unwrap().timestamp_millis()
}

#[test]
fn today_is_charted_per_local_hour() {
    let home = Home::new();
    home.row(
        "ocx-1",
        "2026-10-16T21:59:59.999Z",
        Some("claude"),
        (1, 0, 0, 0),
    );
    home.row(
        "ocx-2",
        "2026-10-16T22:00:00Z",
        Some("claude"),
        (2, 0, 0, 0),
    );
    home.row(
        "ocx-3",
        "2026-10-17T08:00:00Z",
        Some("claude"),
        (4, 0, 0, 0),
    );
    home.row(
        "ocx-4",
        "2026-10-17T08:59:59.999Z",
        Some("claude"),
        (8, 0, 0, 0),
    );
    home.row(
        "ocx-5",
        "2026-10-17T09:00:00Z",
        Some("claude"),
        (16, 0, 0, 0),
    );

    let statistics = tokens(&home.registry(), Range::Today);

    assert_eq!(statistics.bucket, BucketSize::Hour);
    assert_eq!(statistics.buckets.len(), 24, "every hour of the local day");
    let bar = |hour: usize| statistics.buckets[hour];
    assert_eq!(bar(0).start, ms("2026-10-16T22:00:00Z"), "local midnight");
    assert_eq!(bar(0).total, 2, "ocx-2; ocx-1 is yesterday");
    assert_eq!(bar(10).start, ms("2026-10-17T08:00:00Z"), "10:00 local");
    assert_eq!(bar(10).total, 12, "ocx-3 and ocx-4");
    assert_eq!(bar(11).total, 16, "ocx-5");
    assert_eq!(bar(23).start, ms("2026-10-17T21:00:00Z"), "23:00 local");
    let charted: u64 = statistics.buckets.iter().map(|bar| bar.total).sum();
    assert_eq!(
        charted, statistics.total,
        "the bars sum to the range's total"
    );
}

#[test]
fn seven_and_thirty_days_are_charted_per_local_calendar_day() {
    let home = Home::new();
    home.row(
        "ocx-1",
        "2026-10-10T21:59:59.999Z",
        Some("claude"),
        (1, 0, 0, 0),
    );
    home.row(
        "ocx-2",
        "2026-10-10T22:00:00Z",
        Some("claude"),
        (2, 0, 0, 0),
    );
    home.row(
        "ocx-3",
        "2026-10-16T21:59:59.999Z",
        Some("claude"),
        (4, 0, 0, 0),
    );
    home.row(
        "ocx-4",
        "2026-10-16T22:00:00Z",
        Some("claude"),
        (8, 0, 0, 0),
    );
    let registry = home.registry();

    let week = tokens(&registry, Range::SevenDays);

    assert_eq!(week.bucket, BucketSize::Day);
    let days: Vec<(i64, u64)> = week
        .buckets
        .iter()
        .map(|bar| (bar.start, bar.total))
        .collect();
    assert_eq!(days.len(), 7, "the last 7 local days including today");
    assert_eq!(days[0], (ms("2026-10-10T22:00:00Z"), 2), "the 11th: ocx-2");
    assert_eq!(days[5], (ms("2026-10-15T22:00:00Z"), 4), "the 16th: ocx-3");
    assert_eq!(days[6], (ms("2026-10-16T22:00:00Z"), 8), "today: ocx-4");

    let month = tokens(&registry, Range::ThirtyDays);

    assert_eq!(month.buckets.len(), 30);
    assert_eq!(
        month.buckets[0].start,
        ms("2026-09-17T22:00:00Z"),
        "the 18th of September"
    );
    assert_eq!(month.total, 15);
}

#[test]
fn todays_total_is_the_opencodex_views_claude_code_tokens_for_today() {
    // The opencodex fixture ledger (opencodex-2.84.0), no transcripts at all.
    let home = Home::new();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/opencodex-2.84.0");
    fs::create_dir_all(home.path().join(".opencodex")).unwrap();
    fs::copy(
        fixture.join("usage.jsonl"),
        home.path().join(".opencodex/usage.jsonl"),
    )
    .unwrap();
    let registry = home.registry();

    let statistics = tokens(&registry, Range::Today);
    let OpencodexAnswer::Ready(opencodex) =
        registry.opencodex_statistics(Range::Today, SurfaceFilter::ClaudeCode)
    else {
        panic!("opencodex statistics");
    };

    let opencodex_total: u64 = opencodex
        .models
        .iter()
        .map(|model| model.fresh_input + model.cached + model.cache_write + model.output)
        .sum();
    assert!(
        opencodex_total > 0,
        "the fixture has Claude Code rows today"
    );
    assert_eq!(statistics.total, opencodex_total);
    assert_eq!(
        statistics.unattributed.map(|row| row.total),
        Some(opencodex_total),
        "with no transcripts, every row is unattributed"
    );
}

#[test]
fn appended_lines_and_rows_are_counted_on_the_next_request() {
    let home = Home::new();
    home.line(ORCHARD, S1, None, "ocx-r1", "2026-10-17T08:00:00Z");
    home.row(
        "ocx-r1",
        "2026-10-17T08:00:00Z",
        Some("claude"),
        (100, 0, 0, 0),
    );
    let registry = home.registry();
    assert_eq!(tokens(&registry, Range::Today).total, 100);

    home.meta(ORCHARD, S1, SCOUT, "Explore", None);
    home.line(ORCHARD, S1, Some(SCOUT), "ocx-r2", "2026-10-17T09:00:00Z");
    home.row(
        "ocx-r2",
        "2026-10-17T09:00:00Z",
        Some("claude"),
        (20, 0, 0, 0),
    );
    let subagents = home
        .path()
        .join(format!(".claude/projects/{ORCHARD}/{S1}/subagents"));
    registry.files_changed(&[subagents]);

    let statistics = tokens(&registry, Range::Today);
    assert_eq!(statistics.total, 120);
    assert_eq!(
        statistics.unattributed, None,
        "the new row is the subagent's"
    );
    assert_eq!(totals(&statistics).last(), Some(&(SCOUT.to_string(), 20)));
}

#[test]
fn the_agent_tree_shows_each_agents_tokens() {
    let home = Home::new();
    home.line(ORCHARD, S1, None, "ocx-r1", "2026-10-17T08:00:00Z");
    home.meta(ORCHARD, S1, SCOUT, "Explore", None);
    home.line(ORCHARD, S1, Some(SCOUT), "ocx-r2", "2026-10-17T08:01:00Z");
    home.row(
        "ocx-r1",
        "2026-10-17T08:00:00Z",
        Some("claude"),
        (100, 0, 0, 1),
    );
    home.row(
        "ocx-r2",
        "2026-10-17T08:01:00Z",
        Some("claude"),
        (20, 0, 0, 2),
    );
    home.row(
        "ocx-r3",
        "2026-10-17T08:02:00Z",
        Some("claude"),
        (5, 0, 0, 0),
    );

    let registry = home.registry();
    let before = registry.snapshot();
    registry.attribute_tokens();
    let tree = registry.snapshot();

    assert_eq!(
        before.projects[0].sessions[0].root.tokens, 0,
        "the tree does not wait for the ledger: tokens follow once attributed"
    );
    let root = &tree.projects[0].sessions[0].root;
    assert_eq!(root.tokens, 101, "the root agent's own requests");
    assert_eq!(
        root.children[0].tokens, 22,
        "the subagent's, apart from its parent's"
    );
}
