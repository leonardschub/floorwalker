//! The sessions under `~/.claude/projects`, kept current as files grow.
//!
//! A project is a `projects/<project>` directory; a session is a root transcript
//! `<session-id>.jsonl` in it whose name is a UUID. That rule, the Python Agent SDK's,
//! also leaves out the set-aside copies `<id>.orphaned-<ts>-<suffix>.jsonl` and
//! `<id>.jsonl.superseded-<ts>`. Subagent transcripts live in `<session-id>/subagents/`
//! and are not sessions: they are read, with their `.meta.json`, as the agents of the
//! session. Nothing else under `~/.claude` is opened here: not `history.jsonl`,
//! `file-history/` or `tool-results/`.
//!
//! Each transcript keeps its reader, so a changed file is read only from where the last
//! read stopped (ticket 14), and what an agent is doing (its model, its current tool) is
//! derived again from the reader each time it is asked for, so a tool's name clears
//! as soon as its result is read. [`Sessions::changed`] routes a changed path to what
//! it belongs to: a whole project, a root transcript, or a file in a session's own
//! folder (its `custom-title.json`, a subagent's transcript or meta).

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use crate::model;
use crate::transcript::Transcript;

pub(crate) struct FoundSession {
    pub id: String,
    pub title: Option<String>,
    pub cwd: Option<String>,
    /// Milliseconds since the epoch: the first record, else the last activity.
    pub started: i64,
    /// Milliseconds since the epoch: the latest record, else the file's mtime.
    pub last_activity: i64,
    /// What the root agent is doing.
    pub activity: Activity,
    /// Read by [`Sessions::read_subagents`], only for the sessions the tree shows.
    pub subagents: Vec<FoundSubagent>,
}

/// One `subagents/agent-<id>.meta.json`, with what its transcript says.
pub(crate) struct FoundSubagent {
    pub id: String,
    /// The meta's `agentType`, its display name.
    pub agent_type: String,
    /// The meta's `parentAgentId`: the spawning subagent; none for the root.
    pub parent: Option<String>,
    /// The meta carries `finished: true` once the subagent is done.
    pub finished: bool,
    /// Its first record, in milliseconds since the epoch: none before it has one.
    pub started: Option<i64>,
    /// What it is doing, from `agent-<id>.jsonl`.
    pub activity: Activity,
}

/// The agent that made an API request: a session's root agent, or one of its subagents.
/// Owned, so attribution can be read under the registry's lock and used after it is
/// let go.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Owner {
    pub project: String,
    pub session: String,
    /// The subagent's id; none for the root agent.
    pub agent: Option<String>,
}

/// What an agent's transcript says it is doing.
#[derive(Default)]
pub(crate) struct Activity {
    /// The display name of the model that answered last.
    pub model: Option<String>,
    /// Its id, without opencodex's routing prefix.
    pub model_id: Option<String>,
    /// Its latest tool call without a result yet.
    pub current_tool: Option<String>,
}

impl Activity {
    fn of(transcript: &Transcript) -> Self {
        Activity {
            model: transcript.model.as_deref().map(model::display_name),
            model_id: transcript
                .model
                .as_deref()
                .map(|m| model::id(m).to_string()),
            current_tool: transcript.current_tool().map(str::to_string),
        }
    }
}

/// One root transcript and its session folder, as read so far.
#[derive(Default)]
struct Tracked {
    transcript: Transcript,
    title_file: Option<String>,
    /// The transcript's mtime in milliseconds, for a transcript without timestamps.
    modified: Option<i64>,
    /// The subagents by id; none until the session is first shown.
    subagents: Option<BTreeMap<String, TrackedSubagent>>,
}

/// One subagent's meta and transcript, as read so far.
#[derive(Default)]
struct TrackedSubagent {
    meta: Option<Meta>,
    transcript: Transcript,
}

impl Tracked {
    fn found(&self, id: &str) -> Option<FoundSession> {
        let transcript = &self.transcript;
        let title = self
            .title_file
            .clone()
            .or_else(|| transcript.custom_title.clone())
            .or_else(|| transcript.ai_title.clone());
        if transcript.sidechain || (title.is_none() && !transcript.has_conversation) {
            return None;
        }
        let last_activity = transcript.last_activity.or(self.modified)?;
        Some(FoundSession {
            id: id.to_string(),
            title,
            cwd: transcript.cwd.clone(),
            started: transcript.first_activity.unwrap_or(last_activity),
            last_activity,
            activity: Activity::of(transcript),
            subagents: self.found_subagents(),
        })
    }

    fn found_subagents(&self) -> Vec<FoundSubagent> {
        let Some(subagents) = &self.subagents else {
            return Vec::new();
        };
        let mut found: Vec<FoundSubagent> = subagents
            .iter()
            .filter_map(|(id, subagent)| {
                let meta = subagent.meta.as_ref()?;
                Some(FoundSubagent {
                    id: id.clone(),
                    agent_type: meta.agent_type.clone(),
                    parent: meta
                        .parent_agent_id
                        .clone()
                        .filter(|parent| !parent.is_empty()),
                    finished: meta.finished == Some(true),
                    started: subagent.transcript.first_activity,
                    activity: Activity::of(&subagent.transcript),
                })
            })
            .collect();
        // In the order they started; one that has not written yet comes last.
        found.sort_by_key(|agent| (agent.started.is_none(), agent.started, agent.id.clone()));
        found
    }

    /// Reads every subagent in `folder` (`<session>/subagents`): what was appended to
    /// the transcripts already followed, the rest from the start.
    fn read_subagent_folder(&mut self, folder: &Path) {
        let ids: Vec<String> = fs::read_dir(folder)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|file| {
                subagent_file(file.file_name().to_str()?).map(|(id, _)| id.to_string())
            })
            .collect();
        let subagents = self.subagents.get_or_insert_with(BTreeMap::new);
        subagents.retain(|id, _| ids.contains(id));
        for id in ids {
            let subagent = subagents.entry(id.clone()).or_default();
            subagent.meta = read_meta(&folder.join(format!("agent-{id}.meta.json")));
            // A subagent whose transcript is not there yet has simply done nothing.
            let _ = subagent
                .transcript
                .read_new(&folder.join(format!("agent-{id}.jsonl")));
        }
    }
}

#[derive(Deserialize)]
struct Meta {
    #[serde(rename = "agentType")]
    agent_type: String,
    #[serde(rename = "parentAgentId", default)]
    parent_agent_id: Option<String>,
    #[serde(default)]
    finished: Option<bool>,
}

fn read_meta(path: &Path) -> Option<Meta> {
    serde_json::from_slice::<Meta>(&fs::read(path).ok()?).ok()
}

/// A subagent's file name: `agent-<id>.jsonl` (its transcript, `false`) or
/// `agent-<id>.meta.json` (its meta, `true`).
fn subagent_file(name: &str) -> Option<(&str, bool)> {
    let rest = name.strip_prefix("agent-")?;
    if let Some(id) = rest.strip_suffix(".meta.json") {
        return Some((id, true));
    }
    rest.strip_suffix(".jsonl").map(|id| (id, false))
}

pub(crate) struct Sessions {
    /// `~/.claude/projects`
    root: PathBuf,
    /// Project folder name, then session id.
    tracked: BTreeMap<String, BTreeMap<String, Tracked>>,
}

impl Sessions {
    /// Sessions under `root`, none read yet.
    pub fn new(root: &Path) -> Self {
        Sessions {
            root: root.to_path_buf(),
            tracked: BTreeMap::new(),
        }
    }

    /// Reads every project: the whole tree on the first call, then only what changed.
    pub fn scan(&mut self) {
        let Ok(entries) = fs::read_dir(&self.root) else {
            self.tracked.clear();
            return;
        };
        let projects: Vec<String> = entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
            .filter_map(|entry| entry.file_name().to_str().map(str::to_string))
            .collect();
        self.tracked.retain(|project, _| projects.contains(project));
        for project in projects {
            self.scan_project(&project);
        }
    }

    /// Something at `path` changed (written, created, removed or renamed).
    pub fn changed(&mut self, path: &Path) {
        let Ok(relative) = path.strip_prefix(&self.root) else {
            return;
        };
        let parts: Option<Vec<&str>> = relative
            .components()
            .map(|part| match part {
                Component::Normal(name) => name.to_str(),
                _ => None,
            })
            .collect();
        match parts.as_deref() {
            None => {}
            Some([]) => self.scan(),
            Some([project]) => self.scan_project(project),
            Some([project, file]) => {
                if let Some(id) = session_file_id(file) {
                    self.read_transcript(project, id);
                }
            }
            Some([project, id, inside @ ..]) => {
                if is_uuid(id) {
                    self.session_folder_changed(project, id, inside);
                }
            }
        }
    }

    /// Starts following the subagents of each session for which `wanted` holds.
    /// Subagent transcripts can be large, so sessions that will not be shown are
    /// skipped; a session's subagents, once read, are kept current by
    /// [`Sessions::changed`].
    pub fn read_subagents(&mut self, wanted: impl Fn(&FoundSession) -> bool) {
        for (project, sessions) in self.tracked.iter_mut() {
            for (id, tracked) in sessions.iter_mut() {
                if tracked.subagents.is_some()
                    || !tracked.found(id).is_some_and(|found| wanted(&found))
                {
                    continue;
                }
                let folder = self.root.join(project).join(id).join("subagents");
                tracked.read_subagent_folder(&folder);
            }
        }
    }

    fn scan_project(&mut self, project: &str) {
        let Ok(files) = fs::read_dir(self.root.join(project)) else {
            self.tracked.remove(project);
            return;
        };
        let ids: Vec<String> = files
            .flatten()
            .filter_map(|file| session_file_id(file.file_name().to_str()?).map(str::to_string))
            .collect();
        if let Some(sessions) = self.tracked.get_mut(project) {
            sessions.retain(|id, _| ids.contains(id));
        }
        for id in ids {
            self.read_transcript(project, &id);
            self.session_folder_changed(project, &id, &["subagents"]);
        }
    }

    /// Reads what was appended to `<project>/<id>.jsonl`; forgets the session when the
    /// file is gone.
    fn read_transcript(&mut self, project: &str, id: &str) {
        let folder = self.root.join(project);
        let path = folder.join(format!("{id}.jsonl"));
        let sessions = self.tracked.entry(project.to_string()).or_default();
        let new = !sessions.contains_key(id);
        let tracked = sessions.entry(id.to_string()).or_default();
        if tracked.transcript.read_new(&path).is_err() {
            sessions.remove(id);
            if sessions.is_empty() {
                self.tracked.remove(project);
            }
            return;
        }
        tracked.modified = modified_millis(&path);
        if new {
            tracked.title_file = title_file(&folder.join(id).join("custom-title.json"));
        }
    }

    /// A file in the session's own folder `<project>/<id>/` changed. `inside` is its
    /// path within that folder (empty for the folder itself).
    fn session_folder_changed(&mut self, project: &str, id: &str, inside: &[&str]) {
        let folder = self.root.join(project).join(id);
        let Some(tracked) = self
            .tracked
            .get_mut(project)
            .and_then(|sessions| sessions.get_mut(id))
        else {
            return;
        };
        if matches!(inside, [] | ["custom-title.json"]) {
            tracked.title_file = title_file(&folder.join("custom-title.json"));
        }
        // Subagents are followed only once the session has been shown.
        if tracked.subagents.is_none() {
            return;
        }
        let subagent_folder = folder.join("subagents");
        match inside {
            [] | ["subagents"] => tracked.read_subagent_folder(&subagent_folder),
            ["subagents", file] => {
                let (Some((agent, meta)), Some(subagents)) =
                    (subagent_file(file), tracked.subagents.as_mut())
                else {
                    return;
                };
                let subagent = subagents.entry(agent.to_string()).or_default();
                if meta {
                    subagent.meta = read_meta(&subagent_folder.join(file));
                } else {
                    let _ = subagent.transcript.read_new(&subagent_folder.join(file));
                }
            }
            _ => {}
        }
    }

    /// Which agent made each API request, by `requestId`, over the sessions for which
    /// `kept(project, session id)` holds. The same request can sit in several sessions
    /// (a `/branch` or fork copies the lines it branched from), so it belongs to the
    /// session that had it first: the one with its earliest line, then the one that
    /// started first, then the lowest session id, then project. Within that session
    /// it is the agent whose transcript has the request's last line (a root transcript
    /// may repeat a subagent's lines); a tie goes to the subagent. A subagent counts
    /// only once its meta is read (it has no name before). The rule depends on nothing
    /// but the files, so every view and every read agrees.
    pub fn owners(&self, kept: impl Fn(&str, &str) -> bool) -> HashMap<String, Owner> {
        /// How a session ranks for a request: lowest wins.
        type SessionRank<'a> = (i64, i64, &'a str, &'a str);
        let mut owners: HashMap<&str, (SessionRank, Owner)> = HashMap::new();
        for (project, sessions) in &self.tracked {
            for (session, tracked) in sessions {
                if !kept(project, session) {
                    continue;
                }
                // Within the session: request → (first line, last line, agent).
                let mut here: HashMap<&str, (i64, i64, Option<&str>)> = HashMap::new();
                let subagents = tracked
                    .subagents
                    .iter()
                    .flatten()
                    .filter(|(_, subagent)| subagent.meta.is_some())
                    .map(|(agent, subagent)| (Some(agent.as_str()), &subagent.transcript));
                for (agent, transcript) in
                    std::iter::once((None, &tracked.transcript)).chain(subagents)
                {
                    for (request, lines) in &transcript.requests {
                        let held = here.entry(request).or_insert((i64::MAX, i64::MIN, agent));
                        held.0 = held.0.min(lines.first);
                        if lines.last > held.1 || (lines.last == held.1 && agent.is_some()) {
                            (held.1, held.2) = (lines.last, agent);
                        }
                    }
                }
                let started = tracked.transcript.first_activity.unwrap_or(i64::MAX);
                for (request, (first, _, agent)) in here {
                    let rank = (first, started, session.as_str(), project.as_str());
                    if owners.get(request).is_none_or(|(held, _)| rank < *held) {
                        let owner = Owner {
                            project: project.clone(),
                            session: session.clone(),
                            agent: agent.map(str::to_string),
                        };
                        owners.insert(request, (rank, owner));
                    }
                }
            }
        }
        owners
            .into_iter()
            .map(|(request, (_, owner))| (request.to_string(), owner))
            .collect()
    }

    /// The sessions found, by project. A session id found in more than one project keeps
    /// only its newest copy.
    pub fn found(&self) -> BTreeMap<String, Vec<FoundSession>> {
        let found = self
            .tracked
            .iter()
            .map(|(project, sessions)| {
                let found = sessions
                    .iter()
                    .filter_map(|(id, tracked)| tracked.found(id))
                    .collect();
                (project.clone(), found)
            })
            .collect();
        keep_newest_copy(found)
    }
}

fn modified_millis(path: &Path) -> Option<i64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    let since_epoch = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
    i64::try_from(since_epoch.as_millis()).ok()
}

#[derive(Deserialize)]
struct TitleFile {
    #[serde(rename = "customTitle")]
    custom_title: String,
}

fn title_file(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let title = serde_json::from_slice::<TitleFile>(&bytes)
        .ok()?
        .custom_title;
    (!title.is_empty()).then_some(title)
}

/// A session id found in more than one project keeps only its newest copy.
fn keep_newest_copy(
    mut found: BTreeMap<String, Vec<FoundSession>>,
) -> BTreeMap<String, Vec<FoundSession>> {
    let mut newest: BTreeMap<String, i64> = BTreeMap::new();
    for session in found.values().flatten() {
        let entry = newest.entry(session.id.clone()).or_insert(i64::MIN);
        *entry = (*entry).max(session.last_activity);
    }
    for sessions in found.values_mut() {
        sessions.retain(|session| newest[&session.id] == session.last_activity);
    }
    found.retain(|_, sessions| !sessions.is_empty());
    found
}

/// The session id of a root transcript's file name `<uuid>.jsonl`.
fn session_file_id(name: &str) -> Option<&str> {
    name.strip_suffix(".jsonl").filter(|id| is_uuid(id))
}

/// `8-4-4-4-12` hex digits, as Claude Code names its session files.
fn is_uuid(name: &str) -> bool {
    let groups: Vec<&str> = name.split('-').collect();
    groups.len() == 5
        && groups.iter().zip([8, 4, 4, 4, 12]).all(|(group, length)| {
            group.len() == length && group.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
}
