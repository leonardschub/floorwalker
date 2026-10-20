//! The statistics views' commands (spec "The data contract", "Statistics"): opencodex,
//! Of the month and Tokens. The figures come from the `agents` crate's registry; each
//! command only runs its request off the async workers (reading a month of ledger the
//! first time takes a moment) and keeps opencodex's ledger watched while the view
//! asks, so the open view asks again when the ledger grows.
//!
//! The commands are declared in `build.rs`'s app manifest and granted only to `main`.
//! The page's side is `web/src/agents/tauri-client.ts`.

use agents::{OfTheMonthAnswer, OpencodexAnswer, Range, Registry, SurfaceFilter, TokensAnswer};
use tauri::{AppHandle, Manager};

use crate::agents::Agents;

/// Runs `ask` against the registry off the async workers, renewing the ledger watch;
/// `failed` is the error the page reads if the task itself fails.
async fn statistics<R: Send + 'static>(
    app: AppHandle,
    failed: &'static str,
    ask: impl FnOnce(&Registry) -> R + Send + 'static,
) -> Result<R, String> {
    tauri::async_runtime::spawn_blocking(move || app.state::<Agents>().statistics(ask))
        .await
        .map_err(|_| failed.to_string())
}

/// **opencodex statistics**: usage limits, the summary line and one row per served
/// model for `range` (local time) and `surface`, or `{ error: 'opencodex-unavailable' }`.
#[tauri::command]
pub async fn agents_opencodex_statistics(
    app: AppHandle,
    range: Range,
    surface: SurfaceFilter,
) -> Result<OpencodexAnswer, String> {
    statistics(app, "opencodex-statistics-failed", move |registry| {
        registry.opencodex_statistics(range, surface)
    })
    .await
}

/// **Of the month**: the model and the harness with the most total tokens this local
/// calendar month, with shares, runners-up and last month's winners, or
/// `{ error: 'opencodex-unavailable' }`.
#[tauri::command]
pub async fn agents_of_the_month(app: AppHandle) -> Result<OfTheMonthAnswer, String> {
    statistics(app, "of-the-month-failed", |registry| {
        registry.of_the_month()
    })
    .await
}

/// **token statistics**: Claude Code's tokens for `range` (local time) by project,
/// session and agent, with the Unattributed row and the chart, or
/// `{ error: 'opencodex-unavailable' }`. It needs the agent registry built, so it starts
/// that build if the tree has not, and waits for it; the first request also reads the
/// subagents of older sessions.
#[tauri::command]
pub async fn agents_token_statistics(app: AppHandle, range: Range) -> Result<TokensAnswer, String> {
    app.state::<Agents>().start_first_build(&app);
    statistics(app, "token-statistics-failed", move |registry| {
        registry.token_statistics(range)
    })
    .await
}
