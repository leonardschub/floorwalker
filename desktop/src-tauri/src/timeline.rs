//! The scripted timeline (spec: "The scripted timeline"), debug builds only:
//! `WORLD_DEBUG_TIMELINE=<file>` replaces the registry's agent tree with a JSON file of
//! timed snapshots, played through the same path as real data to the world and the
//! panel (`agents.rs`). A release build compiles none of this and reads no switch.
//! Sample timelines live in `desktop/src-tauri/timelines/`.
//!
//! The file is `{ "steps": [step, ...] }`; each step is the whole tree from its time
//! on:
//!
//! ```json
//! { "at": 4000, "projects": [
//!   { "id": "-home-dev-Projects-Alpha", "name": "Alpha", "sessions": [
//!     { "id": "alpha-1", "title": "Plan it", "started": 4000, "live": true,
//!       "root": { "model": "Opus 5.5", "modelId": "claude-opus-5-5", "state": "busy",
//!                 "currentTool": "Bash",
//!                 "children": [{ "id": "a1", "name": "Explore", "state": "running" }] } } ] } ] }
//! ```
//!
//! Times (`at`, `started`, `lastActivity`) are milliseconds after the timeline starts
//! playing; a session's may be negative, to have started or finished before it. A
//! session's `started` and `lastActivity` default to its step's `at`, `live` to true,
//! `title` and `root` to none and a busy (or, not live, finished) root agent. As in the
//! registry, a finished session whose last activity is more than 24 hours before its
//! step is left out of the tree, and so is a project left with no session. A step may
//! be `"status": "loading"`.
//!
//! A session may be `"attachable": true`, a background session the user can sit in,
//! with the `"shortId"` to attach by (by default its id's first eight characters).
//! Neither holds while it is not live, so a step that turns an attachable session
//! `"live": false` scripts it going dormant, which the registry reports as ended.
//!
//! A root agent may also be `"state": "waiting"` and carry a flag, which is never
//! worked out from its state here, only scripted: `"flag": { "kind":
//! "asking-you-something", "since": 3000 }` or `{ "kind": "waiting-on-you" }`, its
//! `since` in the same time as the rest and defaulting to the step's `at`. The registry
//! flags only live root agents that are idle or waiting.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use agents::{
    Agent, AgentKind, AgentState, AgentTreeSnapshot, Flag, FlagKind, Project, Session, TreeStatus,
};
use serde::Deserialize;

/// Finished sessions stay in the tree this long after their last activity, as in the
/// registry.
const RECENT_MS: i64 = 24 * 60 * 60 * 1000;

/// The file `WORLD_DEBUG_TIMELINE` names, if set.
pub fn from_env() -> Option<PathBuf> {
    std::env::var_os("WORLD_DEBUG_TIMELINE")
        .filter(|file| !file.is_empty())
        .map(PathBuf::from)
}

#[derive(Deserialize)]
pub struct Timeline {
    steps: Vec<Step>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Step {
    at: u64,
    #[serde(default)]
    status: Status,
    #[serde(default)]
    projects: Vec<StepProject>,
}

#[derive(Deserialize, Default, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum Status {
    Loading,
    #[default]
    Ready,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StepProject {
    id: String,
    name: Option<String>,
    #[serde(default)]
    sessions: Vec<StepSession>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StepSession {
    id: String,
    title: Option<String>,
    started: Option<i64>,
    last_activity: Option<i64>,
    #[serde(default = "yes")]
    live: bool,
    #[serde(default)]
    attachable: bool,
    short_id: Option<String>,
    root: Option<StepAgent>,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct StepAgent {
    /// A subagent's id; a root agent's is its session's.
    id: Option<String>,
    #[serde(default)]
    name: String,
    model: Option<String>,
    model_id: Option<String>,
    state: Option<State>,
    current_tool: Option<String>,
    #[serde(default)]
    tokens: u64,
    flag: Option<StepFlag>,
    #[serde(default)]
    children: Vec<StepAgent>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StepFlag {
    kind: StepFlagKind,
    since: Option<i64>,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "kebab-case")]
enum StepFlagKind {
    WaitingOnYou,
    AskingYouSomething,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum State {
    Busy,
    Waiting,
    Idle,
    Running,
    Finished,
}

impl Timeline {
    pub fn read(file: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(file).map_err(|error| error.to_string())?;
        let timeline: Timeline = serde_json::from_str(&text).map_err(|error| error.to_string())?;
        if timeline
            .steps
            .windows(2)
            .any(|pair| pair[1].at < pair[0].at)
        {
            return Err("the steps are not in time order".into());
        }
        Ok(timeline)
    }

    /// Plays the timeline from now: hands `sink` each step's tree at its time, then
    /// returns.
    pub fn play(&self, sink: impl Fn(AgentTreeSnapshot)) {
        let began = Instant::now();
        let origin = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as i64);
        for (n, step) in self.steps.iter().enumerate() {
            let due = began + Duration::from_millis(step.at);
            if let Some(wait) = due.checked_duration_since(Instant::now()) {
                std::thread::sleep(wait);
            }
            eprintln!(
                "[timeline] step {} of {} at {} ms: {} projects",
                n + 1,
                self.steps.len(),
                step.at,
                step.projects.len()
            );
            sink(step.tree(origin));
        }
        eprintln!("[timeline] played to its end; the last tree stays");
    }
}

#[cfg(test)]
impl Timeline {
    /// Each step's tree, with its time, played from the epoch.
    pub fn trees(&self) -> Vec<(u64, AgentTreeSnapshot)> {
        self.steps
            .iter()
            .map(|step| (step.at, step.tree(0)))
            .collect()
    }
}

impl Step {
    /// The tree this step stands for, with times counted from `origin` (milliseconds
    /// since the epoch).
    fn tree(&self, origin: i64) -> AgentTreeSnapshot {
        let at = |ms: i64| origin + ms;
        let now = self.at as i64;
        match self.status {
            Status::Loading => AgentTreeSnapshot::loading(),
            Status::Ready => AgentTreeSnapshot {
                status: TreeStatus::Ready,
                projects: self
                    .projects
                    .iter()
                    .map(|project| Project {
                        id: project.id.clone(),
                        name: project.name.clone().unwrap_or_else(|| project.id.clone()),
                        sessions: project
                            .sessions
                            .iter()
                            // As the registry does: a finished session stays in the tree
                            // for 24 hours after its last activity.
                            .filter(|session| {
                                session.live
                                    || now - session.last_activity.unwrap_or(now) <= RECENT_MS
                            })
                            .map(|session| Session {
                                id: session.id.clone(),
                                title: session.title.clone(),
                                live: session.live,
                                attachable: session.live && session.attachable,
                                short_id: session.short_id(),
                                started: at(session.started.unwrap_or(now)),
                                last_activity: at(session.last_activity.unwrap_or(now)),
                                root: session.root.as_ref().map_or_else(
                                    || {
                                        StepAgent::default().agent(
                                            &session.id,
                                            true,
                                            session.live,
                                            (origin, now),
                                        )
                                    },
                                    |root| {
                                        root.agent(&session.id, true, session.live, (origin, now))
                                    },
                                ),
                            })
                            .collect(),
                    })
                    // And a project with no session left is not in it either.
                    .filter(|project| !project.sessions.is_empty())
                    .collect(),
            },
        }
    }
}

impl StepSession {
    /// The short id to attach by while live and attachable: as scripted, or the session
    /// id's first eight characters, as Claude Code makes them.
    fn short_id(&self) -> Option<String> {
        (self.live && self.attachable).then(|| {
            self.short_id
                .clone()
                .unwrap_or_else(|| self.id.chars().take(8).collect())
        })
    }
}

impl StepAgent {
    /// The agent this stands for, in a step at `now` (milliseconds after the timeline's
    /// start) of a timeline played from `origin` (milliseconds since the epoch).
    fn agent(&self, session: &str, root: bool, live: bool, (origin, now): (i64, i64)) -> Agent {
        let state = match (self.state, root, live) {
            (Some(State::Busy), ..) => AgentState::Busy,
            (Some(State::Waiting), ..) => AgentState::Waiting,
            (Some(State::Idle), ..) => AgentState::Idle,
            (Some(State::Running), ..) => AgentState::Running,
            (Some(State::Finished), ..) | (None, _, false) => AgentState::Finished,
            (None, true, true) => AgentState::Busy,
            (None, false, true) => AgentState::Running,
        };
        Agent {
            id: if root {
                session.to_string()
            } else {
                self.id.clone().unwrap_or_default()
            },
            kind: if root {
                AgentKind::Root
            } else {
                AgentKind::Subagent
            },
            name: self.name.clone(),
            model: self.model.clone(),
            model_id: self.model_id.clone(),
            state,
            current_tool: self.current_tool.clone(),
            tokens: self.tokens,
            flag: self.flag.as_ref().map(|flag| Flag {
                kind: match flag.kind {
                    StepFlagKind::WaitingOnYou => FlagKind::WaitingOnYou,
                    StepFlagKind::AskingYouSomething => FlagKind::AskingYouSomething,
                },
                since: origin + flag.since.unwrap_or(now),
            }),
            children: self
                .children
                .iter()
                .map(|child| child.agent(session, false, live, (origin, now)))
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_becomes_the_whole_tree_with_defaults_filled_in() {
        let timeline: Timeline = serde_json::from_str(
            r#"{ "steps": [
                { "at": 0, "status": "loading" },
                { "at": 2000, "projects": [
                    { "id": "-p", "name": "P", "sessions": [
                        { "id": "s1", "started": 500,
                          "root": { "model": "Opus 5.5", "children": [
                            { "id": "a1", "name": "Explore", "state": "finished" },
                            { "id": "a2", "name": "Plan" } ] } },
                        { "id": "s2", "live": false } ] } ] }
            ] }"#,
        )
        .unwrap();
        assert_eq!(timeline.steps[0].tree(1_000), AgentTreeSnapshot::loading());

        let tree = timeline.steps[1].tree(1_000);
        assert_eq!(tree.status, TreeStatus::Ready);
        let sessions = &tree.projects[0].sessions;
        assert_eq!(
            (sessions[0].started, sessions[0].last_activity),
            (1_500, 3_000)
        );
        assert!(sessions[0].live);
        assert_eq!(sessions[0].root.id, "s1");
        assert_eq!(sessions[0].root.state, AgentState::Busy);
        let children = &sessions[0].root.children;
        assert_eq!(children[0].state, AgentState::Finished);
        assert_eq!(
            (children[1].id.as_str(), children[1].state),
            ("a2", AgentState::Running)
        );
        assert_eq!(
            sessions[1].started, 3_000,
            "started defaults to the step's time"
        );
        assert_eq!(sessions[1].root.state, AgentState::Finished);
    }

    #[test]
    fn scripted_root_agents_can_wait_and_be_flagged() {
        let timeline: Timeline = serde_json::from_str(
            r#"{ "steps": [ { "at": 5000, "projects": [
                { "id": "-p", "sessions": [
                    { "id": "asking", "root": { "state": "waiting",
                      "flag": { "kind": "asking-you-something", "since": 3000 } } },
                    { "id": "idle", "lastActivity": 1000, "root": { "state": "idle",
                      "flag": { "kind": "waiting-on-you" } } },
                    { "id": "busy" } ] } ] } ] }"#,
        )
        .unwrap();
        let tree = timeline.steps[0].tree(100_000);
        let root = |n: usize| &tree.projects[0].sessions[n].root;

        assert_eq!(root(0).state, AgentState::Waiting);
        assert_eq!(
            root(0).flag,
            Some(Flag {
                kind: FlagKind::AskingYouSomething,
                since: 103_000
            })
        );
        assert_eq!(root(1).state, AgentState::Idle);
        assert_eq!(
            root(1).flag,
            Some(Flag {
                kind: FlagKind::WaitingOnYou,
                since: 105_000
            }),
            "since defaults to the step's time"
        );
        assert_eq!(root(2).flag, None, "no flag unless scripted");
    }

    #[test]
    fn a_session_finished_more_than_24_hours_ago_is_left_out_as_in_the_registry() {
        const HOUR: i64 = 60 * 60 * 1000;
        let timeline: Timeline = serde_json::from_str(&format!(
            r#"{{ "steps": [ {{ "at": 1000, "projects": [
                {{ "id": "-p", "sessions": [
                    {{ "id": "live" }},
                    {{ "id": "yesterday", "live": false, "started": {}, "lastActivity": {} }},
                    {{ "id": "old", "live": false, "started": {}, "lastActivity": {} }},
                    {{ "id": "long-live", "started": {}, "lastActivity": {} }} ] }},
                {{ "id": "-gone", "sessions": [
                    {{ "id": "older", "live": false, "lastActivity": {} }} ] }} ] }} ] }}"#,
            -30 * HOUR,
            1000 - 24 * HOUR,
            -50 * HOUR,
            1000 - 24 * HOUR - 1,
            -40 * HOUR,
            -30 * HOUR,
            -25 * HOUR,
        ))
        .unwrap();
        let tree = timeline.steps[0].tree(0);
        let ids: Vec<_> = tree.projects[0]
            .sessions
            .iter()
            .map(|s| s.id.as_str())
            .collect();
        // Finished exactly 24 h ago stays; a millisecond more goes; live always stays.
        assert_eq!(ids, ["live", "yesterday", "long-live"]);
        assert_eq!(tree.projects[0].sessions[1].last_activity, 1000 - 24 * HOUR);
        assert_eq!(
            tree.projects.len(),
            1,
            "a project with no session left goes"
        );
    }

    #[test]
    fn a_scripted_attachable_session_can_go_dormant() {
        let timeline: Timeline = serde_json::from_str(
            r#"{ "steps": [
                { "at": 0, "projects": [ { "id": "-p", "sessions": [
                    { "id": "5b6b105b-3c99-4efe-94be-15dfd5b4f052", "attachable": true },
                    { "id": "named", "attachable": true, "shortId": "c0ffee12" },
                    { "id": "terminal" } ] } ] },
                { "at": 4000, "projects": [ { "id": "-p", "sessions": [
                    { "id": "5b6b105b-3c99-4efe-94be-15dfd5b4f052", "attachable": true,
                      "live": false } ] } ] }
            ] }"#,
        )
        .unwrap();
        let trees = timeline.trees();
        let sessions = &trees[0].1.projects[0].sessions;
        assert!(sessions[0].attachable);
        assert_eq!(
            sessions[0].short_id.as_deref(),
            Some("5b6b105b"),
            "the short id defaults to the session id's first eight characters"
        );
        assert_eq!(sessions[1].short_id.as_deref(), Some("c0ffee12"));
        assert!(!sessions[2].attachable, "not attachable unless scripted");
        assert_eq!(sessions[2].short_id, None);

        let dormant = &trees[1].1.projects[0].sessions[0];
        assert!(!dormant.live);
        assert_eq!(dormant.root.state, AgentState::Finished);
        assert!(!dormant.attachable, "a session that is not live never is");
        assert_eq!(dormant.short_id, None);
    }
}
