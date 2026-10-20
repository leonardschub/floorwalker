//! The Tokens view's figures (spec "Statistics", "Tokens view"; ticket 17): where Claude
//! Code's tokens went, by project → session → agent, over a range in local time.
//!
//! Every number is opencodex's: the ledger rows of the `claude` surface. Claude Code's
//! transcripts only say which agent made each row, by `requestId` (the agent whose
//! transcript has the request's last line, `claude_dir::Sessions::owners`). Rows that
//! match no transcript go on one *Unattributed* row. A row's total splits into fresh
//! input, cache read, cache write and output, which sum to it. No cost anywhere.

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use crate::claude_dir::{FoundSession, Owner};
use crate::opencodex::{OpencodexUnavailable, Surface, Tokens, UsageRow};
use crate::snapshot::AgentKind;

/// A total and what it is made of, with its share of the range's total.
#[derive(Serialize, Clone, Copy, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TokenFigures {
    /// opencodex's `totalTokens`.
    pub total: u64,
    /// Of the range's total, 0 to 1.
    pub share: f64,
    /// Input read neither from nor into the cache.
    pub fresh_input: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub output: u64,
}

impl TokenFigures {
    /// Adds one request. Fresh input is [`Tokens::fresh_input`], the opencodex view's
    /// definition too, so the four kinds sum to the total.
    fn add(&mut self, tokens: &Tokens) {
        self.total += tokens.total;
        self.cache_read += tokens.cache_read;
        self.cache_write += tokens.cache_write;
        self.output += tokens.output;
        self.fresh_input += tokens.fresh_input();
    }

    fn merge(&mut self, other: &TokenFigures) {
        self.total += other.total;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
        self.output += other.output;
        self.fresh_input += other.fresh_input;
    }

    fn with_share(mut self, of: u64) -> Self {
        self.share = if of == 0 {
            0.0
        } else {
            self.total as f64 / of as f64
        };
        self
    }
}

/// What one bar of the chart covers.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BucketSize {
    /// Today: one bar per local hour.
    Hour,
    /// 7 and 30 days: one bar per local calendar day.
    Day,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Bucket {
    /// When the bar's hour or day starts, in milliseconds since the epoch.
    pub start: i64,
    pub total: u64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentTokens {
    /// The session id for a root agent, the agent id for a subagent.
    pub id: String,
    pub kind: AgentKind,
    /// A subagent's `agentType`; empty for a root agent, which the panel names itself.
    pub name: String,
    #[serde(flatten)]
    pub figures: TokenFigures,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SessionTokens {
    pub id: String,
    pub title: Option<String>,
    #[serde(flatten)]
    pub figures: TokenFigures,
    /// The root agent first, then its subagents in the order they started.
    pub agents: Vec<AgentTokens>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectTokens {
    pub id: String,
    pub name: String,
    #[serde(flatten)]
    pub figures: TokenFigures,
    /// Most tokens first.
    pub sessions: Vec<SessionTokens>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TokenStatistics {
    /// Every Claude Code row of the range.
    pub total: u64,
    pub bucket: BucketSize,
    /// The chart: every hour of today, or every day of the range, oldest first.
    pub buckets: Vec<Bucket>,
    /// Most tokens first.
    pub projects: Vec<ProjectTokens>,
    /// The rows that match no transcript; none when there are none.
    pub unattributed: Option<TokenFigures>,
}

/// The Tokens command's answer: the figures, or `{ "error": "opencodex-unavailable" }`.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(untagged)]
pub enum TokensAnswer {
    Ready(TokenStatistics),
    Unavailable { error: OpencodexUnavailable },
}

impl TokensAnswer {
    pub fn unavailable() -> Self {
        TokensAnswer::Unavailable {
            error: OpencodexUnavailable::OpencodexUnavailable,
        }
    }
}

/// An agent, as rows are tallied: project, session, subagent (none for the root).
type AgentKey<'a> = (&'a str, &'a str, Option<&'a str>);

/// The Claude Code rows with tokens, from `start` (ms) on.
fn claude_code_rows(rows: &[UsageRow], start: i64) -> impl Iterator<Item = (&UsageRow, &Tokens)> {
    rows.iter()
        .filter(move |row| row.timestamp >= start && row.surface == Surface::ClaudeCode)
        .filter_map(|row| row.tokens.as_ref().map(|tokens| (row, tokens)))
}

/// Each agent's tokens over every row read, by (session id, agent id): what the agent
/// tree shows beside each agent.
pub(crate) fn per_agent(
    rows: &[UsageRow],
    owners: &HashMap<String, Owner>,
) -> HashMap<(String, String), u64> {
    let mut totals = HashMap::new();
    for (row, tokens) in claude_code_rows(rows, i64::MIN) {
        if let Some(owner) = row.request_id.as_deref().and_then(|id| owners.get(id)) {
            let agent = owner.agent.as_deref().unwrap_or(&owner.session);
            *totals
                .entry((owner.session.clone(), agent.to_string()))
                .or_default() += tokens.total;
        }
    }
    totals
}

/// The view's figures for the rows from `start` on, charted in `buckets` (their
/// starts, oldest first) of `size`.
pub(crate) fn statistics(
    rows: &[UsageRow],
    start: i64,
    size: BucketSize,
    buckets: Vec<i64>,
    owners: &HashMap<String, Owner>,
    found: &BTreeMap<String, Vec<FoundSession>>,
    project_name: impl Fn(&str, &[FoundSession]) -> String,
) -> TokenStatistics {
    let mut total = 0;
    let mut chart: Vec<Bucket> = buckets
        .into_iter()
        .map(|start| Bucket { start, total: 0 })
        .collect();
    let mut agents: HashMap<AgentKey, TokenFigures> = HashMap::new();
    let mut unattributed = TokenFigures::default();
    for (row, tokens) in claude_code_rows(rows, start) {
        total += tokens.total;
        let bar = chart.partition_point(|bucket| bucket.start <= row.timestamp);
        if let Some(bucket) = bar.checked_sub(1).and_then(|bar| chart.get_mut(bar)) {
            bucket.total += tokens.total;
        }
        match row.request_id.as_deref().and_then(|id| owners.get(id)) {
            Some(owner) => agents
                .entry((&*owner.project, &*owner.session, owner.agent.as_deref()))
                .or_default()
                .add(tokens),
            None => unattributed.add(tokens),
        }
    }

    let mut projects = Vec::new();
    for (project_id, sessions_found) in found {
        let mut sessions = Vec::new();
        for session in sessions_found {
            let key = (project_id.as_str(), session.id.as_str());
            let mut session_agents = Vec::new();
            if let Some(figures) = agents.remove(&(key.0, key.1, None)) {
                session_agents.push(AgentTokens {
                    id: session.id.clone(),
                    kind: AgentKind::Root,
                    name: String::new(),
                    figures,
                });
            }
            for subagent in &session.subagents {
                if let Some(figures) = agents.remove(&(key.0, key.1, Some(subagent.id.as_str()))) {
                    session_agents.push(AgentTokens {
                        id: subagent.id.clone(),
                        kind: AgentKind::Subagent,
                        name: subagent.agent_type.clone(),
                        figures,
                    });
                }
            }
            if session_agents.is_empty() {
                continue;
            }
            let mut figures = TokenFigures::default();
            for agent in &mut session_agents {
                figures.merge(&agent.figures);
                agent.figures = agent.figures.with_share(total);
            }
            sessions.push(SessionTokens {
                id: session.id.clone(),
                title: session.title.clone(),
                figures: figures.with_share(total),
                agents: session_agents,
            });
        }
        if sessions.is_empty() {
            continue;
        }
        sessions.sort_by(|a, b| b.figures.total.cmp(&a.figures.total));
        let mut figures = TokenFigures::default();
        for session in &sessions {
            figures.merge(&session.figures);
        }
        projects.push(ProjectTokens {
            id: project_id.clone(),
            name: project_name(project_id, sessions_found),
            figures: figures.with_share(total),
            sessions,
        });
    }
    projects.sort_by(|a, b| b.figures.total.cmp(&a.figures.total));

    // An owner the tree does not know (a set-aside copy of a session) is not lost.
    for figures in agents.values() {
        unattributed.merge(figures);
    }
    TokenStatistics {
        total,
        bucket: size,
        buckets: chart,
        projects,
        unattributed: (unattributed != TokenFigures::default())
            .then(|| unattributed.with_share(total)),
    }
}
