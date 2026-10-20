//! The agents inside a session (ticket 13): subagents nested under whoever spawned
//! them, each with its model and the tool it is running now. Driven, like
//! `registry.rs`, through the public seam with a fixture home, a fixed clock and a
//! stubbed liveness source.
//!
//! The fixture is `claude-code-2.1.302/projects/-home-dev-Projects-Orchard`: one root
//! session (65537283…, root model `claude-opus-5-5`, last activity 10-16 21:54 UTC)
//! with four captured background subagents, all finished:
//!
//! | agent id          | agentType       | parent            | message.model                         |
//! |-------------------|-----------------|-------------------|---------------------------------------|
//! | a1317fd98b265dbc2 | ocx-gpt-6-luna  | root              | ocx-claude-native--gpt-6-luna         |
//! | a1b52a3f5dbbcf977 | Explore         | a1317fd98b265dbc2 | claude-haiku-5-5                      |
//! | a5faa0728b343d6d3 | ocx-gpt-6-1-sol | root              | ocx-claude-native--gpt-6.1-sol        |
//! | a1e79d4f41af80e3a | ocx-gpt-6-astra | root              | ocx-claude-native--gpt-6-astra, then one `<synthetic>` |

use std::fs;
use std::path::{Path, PathBuf};

use agents::{
    Agent, AgentKind, AgentState, AgentTreeSnapshot, Clock, LiveSession, Liveness, LivenessError,
    Registry, RootStatus, Sources,
};
use chrono::{DateTime, Utc};

const ORCHARD: &str = "65537283-8305-4b67-abb5-97e88ac877df";
const LUNA: &str = "a1317fd98b265dbc2";
const EXPLORE: &str = "a1b52a3f5dbbcf977";
const SOL: &str = "a5faa0728b343d6d3";
const ASTRA: &str = "a1e79d4f41af80e3a";

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn orchard_home() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    copy_tree(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/claude-code-2.1.302/projects"),
        &home.path().join(".claude/projects"),
    );
    home
}

fn subagents_dir(home: &Path) -> PathBuf {
    home.join(".claude/projects/-home-dev-Projects-Orchard")
        .join(ORCHARD)
        .join("subagents")
}

struct FixedClock;

impl Clock for FixedClock {
    fn now(&self) -> DateTime<Utc> {
        "2026-10-17T10:00:00Z".parse().unwrap()
    }
}

struct StubLiveness(Vec<LiveSession>);

impl Liveness for StubLiveness {
    fn live_sessions(&self) -> Result<Vec<LiveSession>, LivenessError> {
        Ok(self.0.clone())
    }
}

fn live() -> StubLiveness {
    StubLiveness(vec![LiveSession {
        session_id: ORCHARD.to_string(),
        status: RootStatus::Busy,
        pid: None,
        short_id: None,
        cwd: None,
        started: None,
    }])
}

fn not_live() -> StubLiveness {
    StubLiveness(Vec::new())
}

fn built(home: &Path, liveness: StubLiveness) -> AgentTreeSnapshot {
    let registry = Registry::new(
        Sources::in_home(home),
        Box::new(FixedClock),
        Box::new(liveness),
    );
    registry.build();
    registry.snapshot()
}

fn root(tree: &AgentTreeSnapshot) -> &Agent {
    &tree
        .projects
        .iter()
        .flat_map(|project| &project.sessions)
        .find(|session| session.id == ORCHARD)
        .expect("the Orchard session")
        .root
}

fn find<'a>(agent: &'a Agent, id: &str) -> Option<&'a Agent> {
    if agent.id == id {
        return Some(agent);
    }
    agent.children.iter().find_map(|child| find(child, id))
}

fn agent<'a>(tree: &'a AgentTreeSnapshot, id: &str) -> &'a Agent {
    find(root(tree), id).unwrap_or_else(|| panic!("agent {id} in the tree"))
}

/// Removes the `finished` flag from a subagent's meta, as while it still runs.
fn unfinish(home: &Path, id: &str) {
    let path = subagents_dir(home).join(format!("agent-{id}.meta.json"));
    let mut meta: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    meta.as_object_mut().unwrap().remove("finished");
    fs::write(&path, serde_json::to_vec(&meta).unwrap()).unwrap();
}

#[test]
fn subagents_nest_under_the_agent_that_spawned_them() {
    let home = orchard_home();
    let tree = built(home.path(), live());

    let root = root(&tree);
    let children: Vec<(&str, &str)> = root
        .children
        .iter()
        .map(|child| (child.id.as_str(), child.name.as_str()))
        .collect();
    assert_eq!(
        children,
        vec![
            (LUNA, "ocx-gpt-6-luna"),
            (SOL, "ocx-gpt-6-1-sol"),
            (ASTRA, "ocx-gpt-6-astra"),
        ],
        "depth 1: without a parentAgentId, under the root, in the order they started"
    );
    let luna = agent(&tree, LUNA);
    assert_eq!(luna.children.len(), 1);
    let explore = &luna.children[0];
    assert_eq!(
        (explore.id.as_str(), explore.name.as_str()),
        (EXPLORE, "Explore"),
        "depth 2: under its parentAgentId"
    );
    assert!(explore.children.is_empty());
    for id in [LUNA, EXPLORE, SOL, ASTRA] {
        assert_eq!(agent(&tree, id).kind, AgentKind::Subagent, "{id}");
    }
}

/// Cuts a transcript right after its last `tool_use`, as while that tool still runs,
/// and returns the lines cut off (its result and everything after it).
fn cut_after_last_tool_use(path: &Path) -> String {
    let text = fs::read_to_string(path).unwrap();
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let last = lines
        .iter()
        .rposition(|line| line.contains(r#""type":"tool_use""#))
        .unwrap();
    fs::write(path, lines[..=last].concat()).unwrap();
    lines[last + 1..].concat()
}

#[test]
fn an_agent_shows_the_tool_it_runs_until_the_result_arrives() {
    let home = orchard_home();
    unfinish(home.path(), EXPLORE);
    let transcript = subagents_dir(home.path()).join(format!("agent-{EXPLORE}.jsonl"));
    let rest = cut_after_last_tool_use(&transcript);

    let tree = built(home.path(), live());
    assert_eq!(agent(&tree, EXPLORE).current_tool.as_deref(), Some("Bash"));

    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&transcript)
        .unwrap();
    std::io::Write::write_all(&mut file, rest.as_bytes()).unwrap();
    let tree = built(home.path(), live());
    assert_eq!(
        agent(&tree, EXPLORE).current_tool,
        None,
        "the result has arrived"
    );
}

#[test]
fn the_root_agent_shows_its_current_tool_too() {
    let home = orchard_home();
    let transcript = home
        .path()
        .join(".claude/projects/-home-dev-Projects-Orchard")
        .join(format!("{ORCHARD}.jsonl"));
    cut_after_last_tool_use(&transcript);

    let tree = built(home.path(), live());
    assert_eq!(root(&tree).current_tool.as_deref(), Some("Bash"));
    let tree = built(home.path(), not_live());
    assert_eq!(
        root(&tree).current_tool,
        None,
        "a finished agent runs nothing, even if a result never came"
    );
}

#[test]
fn every_agent_shows_the_model_that_answered_it() {
    let home = orchard_home();
    let tree = built(home.path(), live());

    let model = |id| agent(&tree, id).model.as_deref();
    assert_eq!(root(&tree).model.as_deref(), Some("Opus 5.5"));
    assert_eq!(model(EXPLORE), Some("Haiku 5.5"));
    assert_eq!(
        model(SOL),
        Some("GPT-6.1 Sol"),
        "routed: ocx-claude-native-- is stripped, not the requested placeholder"
    );
    assert_eq!(model(LUNA), Some("GPT-6 Luna"));
    assert_eq!(
        model(ASTRA),
        Some("GPT-6 Astra"),
        "its last message is <synthetic>, which is not a model"
    );
}

#[test]
fn every_agent_carries_its_models_id_without_the_routing_prefix() {
    let home = orchard_home();
    let tree = built(home.path(), live());

    let id = |agent: &agents::Agent| agent.model_id.clone();
    assert_eq!(id(root(&tree)).as_deref(), Some("claude-opus-5-5"));
    assert_eq!(
        id(agent(&tree, EXPLORE)).as_deref(),
        Some("claude-haiku-5-5")
    );
    assert_eq!(
        id(agent(&tree, SOL)).as_deref(),
        Some("gpt-6.1-sol"),
        "routed: ocx-claude-native-- is stripped"
    );
    assert_eq!(
        id(agent(&tree, ASTRA)).as_deref(),
        Some("gpt-6-astra"),
        "<synthetic> is not a model"
    );
    for agent in [root(&tree), agent(&tree, EXPLORE), agent(&tree, SOL)] {
        assert_eq!(agent.model.is_some(), agent.model_id.is_some());
    }
}

#[test]
fn an_unknown_model_shows_as_its_id() {
    let home = orchard_home();
    let transcript = subagents_dir(home.path()).join(format!("agent-{SOL}.jsonl"));
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(&transcript)
        .unwrap();
    std::io::Write::write_all(
        &mut file,
        br#"{"type":"assistant","timestamp":"2026-10-16T13:51:00.000Z","message":{"model":"ocx-claude-native--mystery-7","content":[]}}
"#,
    )
    .unwrap();

    let tree = built(home.path(), live());
    assert_eq!(agent(&tree, SOL).model.as_deref(), Some("mystery-7"));
    assert_eq!(agent(&tree, SOL).model_id.as_deref(), Some("mystery-7"));
}

#[test]
fn subagents_carry_no_message_text_tool_input_or_tool_output() {
    // As in registry.rs: every content string in the fixture is "[scrubbed]".
    let home = orchard_home();
    unfinish(home.path(), EXPLORE);
    cut_after_last_tool_use(&subagents_dir(home.path()).join(format!("agent-{EXPLORE}.jsonl")));
    let tree = built(home.path(), live());
    let payload = serde_json::to_string(&tree).unwrap();

    assert!(payload.contains(r#""currentTool":"Bash""#), "{payload}");
    assert!(!payload.contains("[scrubbed]"), "{payload}");
}

#[test]
fn a_subagent_runs_while_its_meta_lacks_finished_and_its_session_is_live() {
    let home = orchard_home();
    unfinish(home.path(), EXPLORE);

    let tree = built(home.path(), live());
    assert_eq!(agent(&tree, EXPLORE).state, AgentState::Running);
    assert_eq!(
        agent(&tree, LUNA).state,
        AgentState::Finished,
        "its meta says finished"
    );

    let tree = built(home.path(), not_live());
    assert_eq!(
        agent(&tree, EXPLORE).state,
        AgentState::Finished,
        "a subagent of a session that has ended is finished, whatever its meta says"
    );
}
