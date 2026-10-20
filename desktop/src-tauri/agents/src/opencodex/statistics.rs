//! The opencodex view's figures (spec "Statistics", "opencodex view"): usage limits,
//! a summary line of requests, errors and failovers, and one row per served model.
//! View-ready, all surfaces unless filtered, and no cost anywhere.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::ledger::{Surface, UsageRow};
use super::quota::UsageLimit;

/// Which harness's requests count: all, or one surface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SurfaceFilter {
    All,
    ClaudeCode,
    Codex,
    ClaudeDesktop,
    GrokBuild,
}

impl SurfaceFilter {
    fn admits(self, surface: &Surface) -> bool {
        match self {
            SurfaceFilter::All => true,
            SurfaceFilter::ClaudeCode => *surface == Surface::ClaudeCode,
            SurfaceFilter::Codex => *surface == Surface::Codex,
            SurfaceFilter::ClaudeDesktop => *surface == Surface::ClaudeDesktop,
            SurfaceFilter::GrokBuild => *surface == Surface::GrokBuild,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub requests: u64,
    /// Non-2xx, or a failure field set.
    pub errors: u64,
    /// Requests with more than one attempt.
    pub failovers: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelFigures {
    /// The served model's id (`servedModel`, falling back to `model`).
    pub model: String,
    /// Its display name ("Opus 5.5"), or the id when it has none.
    pub name: String,
    /// The provider that served most of its requests.
    pub provider: String,
    pub requests: u64,
    /// Errors over requests, 0 to 1.
    pub error_rate: f64,
    /// Median time to first token, over the requests that report one.
    pub median_first_token_ms: Option<f64>,
    /// Median output tokens per second over the generation window.
    pub median_tokens_per_second: Option<f64>,
    /// [`Tokens::fresh_input`](super::Tokens::fresh_input): input less cache read and
    /// cache write, so the four kinds sum to the total.
    pub fresh_input: u64,
    pub output: u64,
    /// Cache read.
    pub cached: u64,
    pub cache_write: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpencodexStatistics {
    pub limits: Vec<UsageLimit>,
    pub summary: Summary,
    /// Most requests first.
    pub models: Vec<ModelFigures>,
    /// The harnesses the view's filter offers, by name.
    pub harnesses: Vec<Harness>,
}

/// A harness the filter offers: its filter id and display name.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Harness {
    pub id: String,
    pub name: String,
}

/// opencodex is not installed, unreadable, or has no ledger.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum OpencodexUnavailable {
    #[serde(rename = "opencodex-unavailable")]
    OpencodexUnavailable,
}

/// The statistics command's answer: the figures, or `{ "error": "opencodex-unavailable" }`.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum OpencodexAnswer {
    Ready(OpencodexStatistics),
    Unavailable { error: OpencodexUnavailable },
}

impl OpencodexAnswer {
    pub fn unavailable() -> Self {
        OpencodexAnswer::Unavailable {
            error: OpencodexUnavailable::OpencodexUnavailable,
        }
    }
}

#[derive(Default)]
struct Tally<'a> {
    requests: u64,
    errors: u64,
    providers: HashMap<&'a str, (u64, i64)>,
    first_token: Vec<f64>,
    tokens_per_second: Vec<f64>,
    fresh_input: u64,
    output: u64,
    cached: u64,
    cache_write: u64,
}

/// The figures for the rows from `start` (ms) on that `surface` admits.
pub(crate) fn statistics(
    rows: &[UsageRow],
    start: i64,
    surface: SurfaceFilter,
    limits: Vec<UsageLimit>,
) -> OpencodexStatistics {
    let mut summary = Summary {
        requests: 0,
        errors: 0,
        failovers: 0,
    };
    let mut models: HashMap<&str, Tally> = HashMap::new();
    for row in rows
        .iter()
        .filter(|row| row.timestamp >= start && surface.admits(&row.surface))
    {
        summary.requests += 1;
        summary.errors += row.is_error() as u64;
        summary.failovers += row.failed_over() as u64;

        let tally = models.entry(&row.served_model).or_default();
        tally.requests += 1;
        tally.errors += row.is_error() as u64;
        let provider = tally.providers.entry(&row.provider).or_default();
        provider.0 += 1;
        provider.1 = provider.1.max(row.timestamp);
        tally
            .first_token
            .extend(row.first_output_ms.map(|ms| ms as f64));
        tally.tokens_per_second.extend(row.tokens_per_second());
        if let Some(tokens) = row.tokens {
            tally.fresh_input += tokens.fresh_input();
            tally.output += tokens.output;
            tally.cached += tokens.cache_read;
            tally.cache_write += tokens.cache_write;
        }
    }

    let mut models: Vec<ModelFigures> = models
        .into_iter()
        .map(|(model, mut tally)| ModelFigures {
            model: model.to_string(),
            name: crate::model::display_name(model),
            provider: tally
                .providers
                .iter()
                .max_by_key(|(_, (count, last))| (*count, *last))
                .map(|(provider, _)| provider.to_string())
                .unwrap_or_default(),
            requests: tally.requests,
            error_rate: tally.errors as f64 / tally.requests as f64,
            median_first_token_ms: median(&mut tally.first_token),
            median_tokens_per_second: median(&mut tally.tokens_per_second),
            fresh_input: tally.fresh_input,
            output: tally.output,
            cached: tally.cached,
            cache_write: tally.cache_write,
        })
        .collect();
    models.sort_by(|a, b| b.requests.cmp(&a.requests).then(a.model.cmp(&b.model)));

    OpencodexStatistics {
        limits,
        summary,
        models,
        harnesses: Surface::known()
            .iter()
            .map(|surface| Harness {
                id: surface.id().to_string(),
                name: surface.name().to_string(),
            })
            .collect(),
    }
}

fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    Some(if values.len() % 2 == 0 {
        (values[middle - 1] + values[middle]) / 2.0
    } else {
        values[middle]
    })
}
