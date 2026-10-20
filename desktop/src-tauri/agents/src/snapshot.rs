//! What the panel receives (spec "The data contract"): metadata only. No type here has
//! a field for message text, tool input or tool output, so none can reach the webview.
//! The shapes mirror `web/src/agents/client.ts`.

use serde::Serialize;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentTreeSnapshot {
    /// `Loading` while the registry's first build runs.
    pub status: TreeStatus,
    pub projects: Vec<Project>,
}

impl AgentTreeSnapshot {
    pub fn loading() -> Self {
        AgentTreeSnapshot {
            status: TreeStatus::Loading,
            projects: Vec::new(),
        }
    }
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TreeStatus {
    Loading,
    Ready,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    /// The `projects/<project>` directory name.
    pub id: String,
    /// The last path segment of the session's working directory, or the directory name.
    pub name: String,
    pub sessions: Vec<Session>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    pub id: String,
    /// From `custom-title.json`, else the last custom title, else the last AI title.
    pub title: Option<String>,
    pub live: bool,
    /// Whether the user can sit in: a live background session with a process, which
    /// `claude attach <short id>` opens.
    pub attachable: bool,
    /// The short id to attach by, only while attachable.
    pub short_id: Option<String>,
    /// The start: the transcript's first activity, in milliseconds since the epoch.
    pub started: i64,
    /// The last activity, in milliseconds since the epoch.
    pub last_activity: i64,
    pub root: Agent,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    /// The session id for a root agent, the agent id for a subagent.
    pub id: String,
    pub kind: AgentKind,
    /// A subagent's `agentType`; empty for a root agent, which the panel names itself.
    pub name: String,
    /// The model's display name, or none before the agent has answered.
    pub model: Option<String>,
    /// The model's id (`message.model` without opencodex's `ocx-claude-native--`), or none
    /// before the agent has answered: what the world tells the model's company by.
    pub model_id: Option<String>,
    pub state: AgentState,
    /// The name of the tool the agent is running now, never its input.
    pub current_tool: Option<String>,
    pub tokens: u64,
    /// Set only on a live root agent that is idle or waiting: whether it waits on the
    /// user, and since when. The registry's answer, so the world and the panel agree.
    pub flag: Option<Flag>,
    pub children: Vec<Agent>,
}

/// A live root agent the user is holding up (glossary: **flag**).
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flag {
    pub kind: FlagKind,
    /// In milliseconds since the epoch: the last activity for *waiting on you*; when the
    /// registry first saw the session waiting for this reason for *asking you something*.
    pub since: i64,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "kebab-case")]
pub enum FlagKind {
    /// Idle after its turn, or blocked on a dialog: amber.
    WaitingOnYou,
    /// A question, a permission prompt or another request: red.
    AskingYouSomething,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    Root,
    Subagent,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AgentState {
    Busy,
    /// A live root agent blocked on the user (Claude Code's `waiting`).
    Waiting,
    Idle,
    Running,
    Finished,
}
