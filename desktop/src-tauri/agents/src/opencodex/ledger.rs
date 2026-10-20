//! opencodex's usage ledger (`~/.opencodex/usage.jsonl`): one JSON row per logical
//! request, append-only. It has no published schema; its fields are additive and older
//! rows omit many of them, so each field is read leniently and a line that is not a
//! usage row is skipped. Only the fields the statistics views need are kept; nothing
//! else of a row (route decisions, protocol traces, upstream error text) is kept.
//!
//! The ledger is tailed: each read starts where the last stopped. A last line without
//! a newline is uncommitted (opencodex's own scanner treats it so) and waits. A file
//! that shrank was trimmed or replaced, and is read again from the start.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

use serde::Deserialize;

/// Which harness sent a request: the row's `surface`.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Surface {
    /// `claude`
    ClaudeCode,
    /// No `surface`.
    Codex,
    /// `claude-desktop`
    ClaudeDesktop,
    /// `grok`
    GrokBuild,
    /// A surface this version does not know.
    Other(String),
}

impl Surface {
    /// The harnesses this version knows, in the order the panel lists them.
    pub fn known() -> [Surface; 4] {
        [
            Surface::ClaudeCode,
            Surface::Codex,
            Surface::ClaudeDesktop,
            Surface::GrokBuild,
        ]
    }

    /// The harness's id, which is also its surface filter's.
    pub fn id(&self) -> &str {
        match self {
            Surface::ClaudeCode => "claude-code",
            Surface::Codex => "codex",
            Surface::ClaudeDesktop => "claude-desktop",
            Surface::GrokBuild => "grok-build",
            Surface::Other(surface) => surface,
        }
    }

    /// The harness's display name: the one place the panel's harness names come from.
    /// Product names are data, not translations. ("Surface" is opencodex's word for
    /// what the panel calls a harness, kept here because it names the ledger field.)
    pub fn name(&self) -> &str {
        match self {
            Surface::ClaudeCode => "Claude Code",
            Surface::Codex => "Codex",
            Surface::ClaudeDesktop => "Claude Desktop",
            Surface::GrokBuild => "Grok Build",
            Surface::Other(surface) => surface,
        }
    }

    fn from_field(surface: Option<&str>) -> Self {
        match surface {
            None => Surface::Codex,
            Some("claude") => Surface::ClaudeCode,
            Some("claude-desktop") => Surface::ClaudeDesktop,
            Some("grok") => Surface::GrokBuild,
            Some(other) => Surface::Other(other.to_string()),
        }
    }
}

/// A request's tokens as opencodex counts them. `input` includes both cache reads and
/// cache writes, and `total` is input + output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    /// `cacheReadInputTokens`, part of `input`.
    pub cache_read: u64,
    /// `cacheCreationInputTokens`, part of `input`.
    pub cache_write: u64,
    /// `totalTokens`.
    pub total: u64,
}

impl Tokens {
    /// Input read neither from nor into the cache: `input − cache read − cache write`.
    /// The one definition every view uses, so fresh input, cache read, cache write and
    /// output sum to the total.
    pub fn fresh_input(&self) -> u64 {
        self.input
            .saturating_sub(self.cache_read)
            .saturating_sub(self.cache_write)
    }
}

/// The provider of a row that names none, as opencodex itself writes it.
const UNKNOWN_PROVIDER: &str = "unknown";

/// One logical request, as the statistics views need it. A row without a `requestId`
/// or a `provider` still counts: it is just never attributed to an agent, and its
/// provider reads "unknown".
#[derive(Clone, Debug)]
pub struct UsageRow {
    /// The join key into Claude Code's transcripts (`requestId`); none when the row
    /// has none.
    pub request_id: Option<String>,
    /// When the request was made, in milliseconds since the epoch.
    pub timestamp: i64,
    pub surface: Surface,
    pub provider: String,
    /// `servedModel`, falling back to `model`.
    pub served_model: String,
    /// The HTTP status.
    pub status: u16,
    /// Whether a failure field (`failureCause`, `failureStage`, `errorCode`) is set.
    pub failure: bool,
    /// Physical attempts; more than one means opencodex failed over.
    pub attempts: u32,
    pub duration_ms: Option<u64>,
    /// Time to first token.
    pub first_output_ms: Option<u64>,
    /// The generation window, both relative to the request's start.
    pub gen_start_ms: Option<u64>,
    pub last_output_ms: Option<u64>,
    /// `None` when opencodex has no token counts for the request.
    pub tokens: Option<Tokens>,
}

impl UsageRow {
    /// A non-2xx status or a failure field set.
    pub fn is_error(&self) -> bool {
        !(200..300).contains(&self.status) || self.failure
    }

    /// More than one attempt.
    pub fn failed_over(&self) -> bool {
        self.attempts > 1
    }

    /// Output tokens per second over the generation window, when there is one.
    pub fn tokens_per_second(&self) -> Option<f64> {
        let window = self.last_output_ms?.checked_sub(self.gen_start_ms?)?;
        let output = self.tokens?.output;
        (window > 0 && output > 0).then(|| output as f64 * 1000.0 / window as f64)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Row {
    #[serde(default, deserialize_with = "lenient")]
    request_id: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    timestamp: Option<i64>,
    #[serde(default, deserialize_with = "lenient")]
    surface: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    provider: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    model: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    served_model: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    status: Option<u16>,
    #[serde(default, deserialize_with = "present")]
    failure_cause: bool,
    #[serde(default, deserialize_with = "present")]
    failure_stage: bool,
    #[serde(default, deserialize_with = "present")]
    error_code: bool,
    #[serde(default, deserialize_with = "length")]
    attempts: Option<u32>,
    #[serde(default, deserialize_with = "lenient")]
    duration_ms: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    first_output_ms: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    gen_start_ms: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    last_output_ms: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    usage: Option<RowUsage>,
    #[serde(default, deserialize_with = "lenient")]
    total_tokens: Option<u64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RowUsage {
    #[serde(default, deserialize_with = "lenient")]
    input_tokens: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    output_tokens: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    total_tokens: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    cache_read_input_tokens: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    cached_input_tokens: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    cache_creation_input_tokens: Option<u64>,
}

fn lenient<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::de::DeserializeOwned,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(serde_json::from_value(value).ok())
}

/// Whether a field holds anything but `null`.
fn present<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    Ok(Option::<serde::de::IgnoredAny>::deserialize(deserializer)?.is_some())
}

/// The length of an array field, without keeping its items.
fn length<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<u32>, D::Error> {
    struct Count;
    impl<'de> serde::de::Visitor<'de> for Count {
        type Value = Option<u32>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("any value")
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(
            self,
            mut seq: A,
        ) -> Result<Self::Value, A::Error> {
            let mut count = 0;
            while seq.next_element::<serde::de::IgnoredAny>()?.is_some() {
                count += 1;
            }
            Ok(Some(count))
        }
        fn visit_map<A: serde::de::MapAccess<'de>>(
            self,
            mut map: A,
        ) -> Result<Self::Value, A::Error> {
            while map
                .next_entry::<serde::de::IgnoredAny, serde::de::IgnoredAny>()?
                .is_some()
            {}
            Ok(None)
        }
        fn visit_unit<E>(self) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_i64<E>(self, _: i64) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_u64<E>(self, _: u64) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
            Ok(None)
        }
        fn visit_str<E>(self, _: &str) -> Result<Self::Value, E> {
            Ok(None)
        }
    }
    deserializer.deserialize_any(Count)
}

impl Row {
    fn into_usage_row(self) -> Option<UsageRow> {
        let tokens = self.usage.map(|usage| {
            let input = usage.input_tokens.unwrap_or(0);
            let output = usage.output_tokens.unwrap_or(0);
            Tokens {
                input,
                output,
                cache_read: usage
                    .cache_read_input_tokens
                    .or(usage.cached_input_tokens)
                    .unwrap_or(0),
                cache_write: usage.cache_creation_input_tokens.unwrap_or(0),
                total: self
                    .total_tokens
                    .or(usage.total_tokens)
                    .unwrap_or(input + output),
            }
        });
        let model = self.model.filter(|m| !m.is_empty());
        Some(UsageRow {
            request_id: self.request_id.filter(|id| !id.is_empty()),
            timestamp: self.timestamp?,
            surface: Surface::from_field(self.surface.as_deref()),
            provider: self
                .provider
                .filter(|p| !p.is_empty())
                .unwrap_or_else(|| UNKNOWN_PROVIDER.to_string()),
            served_model: self.served_model.filter(|m| !m.is_empty()).or(model)?,
            status: self.status?,
            failure: self.failure_cause || self.failure_stage || self.error_code,
            attempts: self.attempts.unwrap_or(1).max(1),
            duration_ms: self.duration_ms,
            first_output_ms: self.first_output_ms,
            gen_start_ms: self.gen_start_ms,
            last_output_ms: self.last_output_ms,
            tokens,
        })
    }
}

/// The ledger as read so far.
#[derive(Default, Debug)]
pub struct Ledger {
    /// Bytes of complete lines read so far.
    offset: u64,
    rows: Vec<UsageRow>,
}

impl Ledger {
    /// Every committed row read so far, in file order.
    pub fn rows(&self) -> &[UsageRow] {
        &self.rows
    }

    /// Reads the lines committed since the last read. An error means the ledger is
    /// missing or unreadable; the rows read before are dropped.
    pub fn read_new(&mut self, path: &Path) -> io::Result<()> {
        let result = self.read_appended(path);
        if result.is_err() {
            *self = Ledger::default();
        }
        result
    }

    fn read_appended(&mut self, path: &Path) -> io::Result<()> {
        let mut file = File::open(path)?;
        if file.metadata()?.len() < self.offset {
            *self = Ledger::default();
        }
        file.seek(SeekFrom::Start(self.offset))?;
        let mut reader = BufReader::with_capacity(1 << 16, file);
        let mut line = Vec::new();
        loop {
            line.clear();
            let read = reader.read_until(b'\n', &mut line)?;
            if read == 0 || line.last() != Some(&b'\n') {
                return Ok(());
            }
            self.offset += read as u64;
            if let Some(row) = serde_json::from_slice::<Row>(line.trim_ascii())
                .ok()
                .and_then(Row::into_usage_row)
            {
                self.rows.push(row);
            }
        }
    }
}
