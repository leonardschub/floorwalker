//! Model display names (spec "Model display names").
//!
//! The transcript's `message.model` is the model that answered. A request routed
//! through opencodex reads `ocx-claude-native--<id>`; the prefix is stripped so the
//! routed model shows, not the placeholder alias Claude Code asked for. Known ids get a
//! display name; an unknown id shows as it is. Versions stay distinct.

/// The prefix opencodex puts before the model it routed a request to.
const ROUTED_PREFIX: &str = "ocx-claude-native--";

/// What Claude Code writes for a message it made up itself; not a model.
pub(crate) const SYNTHETIC: &str = "<synthetic>";

const KNOWN: &[(&str, &str)] = &[
    ("claude-opus-5-5", "Opus 5.5"),
    ("claude-sonnet-5-5", "Sonnet 5.5"),
    ("claude-haiku-5-5", "Haiku 5.5"),
    ("claude-opus-4-8", "Opus 4.8"),
    ("gpt-6.1-sol", "GPT-6.1 Sol"),
    ("gpt-6-luna", "GPT-6 Luna"),
    ("gpt-6-astra", "GPT-6 Astra"),
];

/// The model's id: `message.model` without opencodex's routing prefix.
pub(crate) fn id(model: &str) -> &str {
    model.strip_prefix(ROUTED_PREFIX).unwrap_or(model)
}

pub(crate) fn display_name(model: &str) -> String {
    let id = id(model);
    KNOWN
        .iter()
        .find(|(known, _)| *known == id)
        .map_or(id, |(_, name)| name)
        .to_string()
}
