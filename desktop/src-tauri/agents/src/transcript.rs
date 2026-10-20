//! One session transcript (`projects/<project>/<session-id>.jsonl`), read leniently.
//!
//! Semantics follow the MIT-licensed Python Agent SDK's `sessions.py`: a transcript
//! whose first record is a sidechain is not a session; a user-set title wins over an
//! AI-generated one, the last of each counting; a transcript with neither a title nor
//! any conversation record is metadata only and not listed. The format is internal to
//! Claude Code and changes between versions, so unknown record types and fields are
//! ignored and a line that fails to parse is skipped, never failing the transcript.
//!
//! Only the fields below are kept, the `requestId` of each line among them (ticket 17
//! attributes opencodex's rows to agents by it). Message, tool and prompt content is never
//! deserialised into anything (of a content block only its type, id and tool name are
//! read). Lines of the raw-content types (`api-request-blob`, `api-request`,
//! `api-request-shape`, `file-history-*`) are dropped whole, whatever their key order,
//! and the usual spelling of a blob line before it is even parsed.
//!
//! The reader keeps its byte offset, so a growing file is read from where it stopped
//! (ticket 14). A last line without a newline is still being written and waits.

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

use chrono::DateTime;
use serde::Deserialize;

use crate::model::SYNTHETIC;

#[derive(Default, Debug)]
pub(crate) struct Transcript {
    offset: u64,
    partial: Vec<u8>,
    seen_first_record: bool,
    pub sidechain: bool,
    pub has_conversation: bool,
    pub cwd: Option<String>,
    pub custom_title: Option<String>,
    pub ai_title: Option<String>,
    /// The earliest record timestamp, in milliseconds since the epoch.
    pub first_activity: Option<i64>,
    /// The latest record timestamp, in milliseconds since the epoch.
    pub last_activity: Option<i64>,
    /// The model of the latest message, as written (`<synthetic>` never counts).
    pub model: Option<String>,
    /// Tool calls without a result yet, oldest first: (tool_use id, tool name).
    pending_tools: Vec<(String, String)>,
    /// The `requestId` of each API request in this transcript, with the timestamps of
    /// its first and last line (one response is written as several lines), in
    /// milliseconds since the epoch: how opencodex's rows are attributed to agents
    /// (ticket 17).
    pub requests: HashMap<String, RequestLines>,
}

/// When a request's lines in one transcript were written; a line without a
/// timestamp counts as neither first nor last.
#[derive(Clone, Copy, Debug)]
pub(crate) struct RequestLines {
    pub first: i64,
    pub last: i64,
}

/// The fields a record may carry that the registry uses. Each is read leniently: a
/// value of an unexpected type counts as absent rather than failing the line.
#[derive(Deserialize)]
struct Record {
    #[serde(rename = "type", default, deserialize_with = "lenient")]
    kind: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    timestamp: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    cwd: Option<String>,
    #[serde(rename = "isSidechain", default, deserialize_with = "lenient")]
    is_sidechain: Option<bool>,
    #[serde(rename = "customTitle", default, deserialize_with = "lenient")]
    custom_title: Option<String>,
    #[serde(rename = "aiTitle", default, deserialize_with = "lenient")]
    ai_title: Option<String>,
    #[serde(rename = "requestId", default, deserialize_with = "lenient")]
    request_id: Option<String>,
}

/// The part of a `user` or `assistant` record's `message` the registry uses. Parsed
/// on its own, so a message of an unexpected shape costs only these fields.
#[derive(Deserialize)]
struct MessageRecord {
    message: Message,
}

#[derive(Deserialize)]
struct Message {
    /// On an assistant message, the model that answered.
    #[serde(default, deserialize_with = "lenient")]
    model: Option<String>,
    /// A list of blocks; a plain-text message has none of interest.
    #[serde(default, deserialize_with = "lenient_blocks")]
    content: Vec<Block>,
}

#[derive(Deserialize)]
struct Block {
    #[serde(rename = "type", default, deserialize_with = "lenient")]
    kind: Option<String>,
    /// A `tool_use` block's id.
    #[serde(default, deserialize_with = "lenient")]
    id: Option<String>,
    /// A `tool_use` block's tool name.
    #[serde(default, deserialize_with = "lenient")]
    name: Option<String>,
    /// The `tool_use` a `tool_result` block answers.
    #[serde(default, deserialize_with = "lenient")]
    tool_use_id: Option<String>,
}

/// The content blocks, or none when `content` is not a list of objects (a plain string).
fn lenient_blocks<'de, D>(deserializer: D) -> Result<Vec<Block>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Content {
        Blocks(Vec<Block>),
        Other(serde::de::IgnoredAny),
    }
    Ok(match Content::deserialize(deserializer)? {
        Content::Blocks(blocks) => blocks,
        Content::Other(_) => Vec::new(),
    })
}

fn lenient<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).ok())
}

/// Record types that carry raw request or file content and nothing the registry uses:
/// skipped whole, wherever `type` sits in the line.
const CONTENT_TYPES: &[&str] = &[
    "api-request-blob",
    "api-request",
    "api-request-shape",
    "file-history-snapshot",
    "file-history-delta",
];

/// The common spelling of the largest of them, skipped before it is even parsed.
const BLOB_PREFIX: &[u8] = br#"{"type":"api-request-blob""#;

impl Transcript {
    /// Reads whatever was appended since the last read.
    pub fn read_new(&mut self, path: &Path) -> io::Result<()> {
        let mut file = File::open(path)?;
        let length = file.metadata()?.len();
        if length < self.offset {
            // Rewritten in place: start over.
            *self = Transcript::default();
        }
        file.seek(SeekFrom::Start(self.offset))?;
        let mut appended = Vec::new();
        file.read_to_end(&mut appended)?;
        self.offset += appended.len() as u64;
        self.partial.extend_from_slice(&appended);
        let complete = match self.partial.iter().rposition(|&byte| byte == b'\n') {
            Some(end) => end + 1,
            None => return Ok(()),
        };
        let lines: Vec<u8> = self.partial.drain(..complete).collect();
        for line in lines.split(|&byte| byte == b'\n') {
            self.feed(line);
        }
        Ok(())
    }

    fn feed(&mut self, line: &[u8]) {
        let line = line.trim_ascii();
        if line.is_empty() || line.starts_with(BLOB_PREFIX) {
            return;
        }
        // Unknown fields are skipped without being kept, so parsing a content line
        // keeps none of its content; its type then drops it whole.
        let Ok(record) = serde_json::from_slice::<Record>(line) else {
            return;
        };
        if record
            .kind
            .as_deref()
            .is_some_and(|kind| CONTENT_TYPES.contains(&kind))
        {
            return;
        }
        if !self.seen_first_record {
            self.seen_first_record = true;
            self.sidechain = record.is_sidechain == Some(true);
        }
        let at = record
            .timestamp
            .as_deref()
            .and_then(|at| DateTime::parse_from_rfc3339(at).ok())
            .map(|at| at.timestamp_millis());
        if let Some(request) = record.request_id.filter(|id| !id.is_empty()) {
            let lines = self.requests.entry(request).or_insert(RequestLines {
                first: i64::MAX,
                last: i64::MIN,
            });
            if let Some(at) = at {
                lines.first = lines.first.min(at);
                lines.last = lines.last.max(at);
            }
        }
        if let Some(at) = at {
            self.first_activity = Some(self.first_activity.map_or(at, |first| first.min(at)));
            self.last_activity = Some(self.last_activity.map_or(at, |last| last.max(at)));
        }
        if self.cwd.is_none() {
            self.cwd = record.cwd.filter(|cwd| !cwd.is_empty());
        }
        if let Some(title) = record.custom_title.filter(|title| !title.is_empty()) {
            self.custom_title = Some(title);
        }
        if let Some(title) = record.ai_title.filter(|title| !title.is_empty()) {
            self.ai_title = Some(title);
        }
        if matches!(record.kind.as_deref(), Some("user" | "assistant")) {
            self.has_conversation = true;
            if let Ok(record) = serde_json::from_slice::<MessageRecord>(line) {
                self.feed_message(record.message);
            }
        }
    }

    fn feed_message(&mut self, message: Message) {
        if let Some(model) = message
            .model
            .filter(|model| !model.is_empty() && model != SYNTHETIC)
        {
            self.model = Some(model);
        }
        for block in message.content {
            match (
                block.kind.as_deref(),
                block.id,
                block.name,
                block.tool_use_id,
            ) {
                (Some("tool_use"), Some(id), Some(name), _) => self.pending_tools.push((id, name)),
                (Some("tool_result"), _, _, Some(answered)) => {
                    self.pending_tools.retain(|(id, _)| *id != answered)
                }
                _ => {}
            }
        }
    }

    /// The name of the latest tool call that has no result yet; never its input.
    pub fn current_tool(&self) -> Option<&str> {
        self.pending_tools.last().map(|(_, name)| name.as_str())
    }
}
