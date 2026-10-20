//! The world's fixed text, translated (management-rooms ticket 14; spec: "The console",
//! its last point). Every fixed string the world draws is a key in `packages/i18n`; the
//! page resolves them in its language and sends the whole table through `world_strings`
//! before it first starts a world and whenever the language changes. The controller
//! keeps the latest and replays it to each newly launched world. The world holds no
//! strings of its own: it looks each one up here by its key, and draws nothing for a key
//! the table lacks. Project and model names are data and never go through here.

use std::collections::BTreeMap;
use std::sync::Arc;

use agents::FlagKind;
use bevy::prelude::Resource;

use super::cctv_state::Words;

/// The page's table: each key (`world.text.*` in `packages/i18n`) with its text in the
/// page's language.
pub type Strings = BTreeMap<String, String>;

/// The latest table the page sent. Changed only when it differs, so `lettering` redraws
/// its text images only then.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct WorldStrings(pub Arc<Strings>);

impl WorldStrings {
    /// The text of `key`, or nothing until the page has sent it.
    pub fn get(&self, key: &str) -> &str {
        self.0.get(key).map_or("", String::as_str)
    }

    /// A status line: `words`, then the timer counting from `since` to `now`, if any
    /// ("WAITING ON YOU 04:12", "WRECKED").
    pub fn status(&self, words: Words, since: Option<i64>, now: i64) -> String {
        let words = self.get(key(words));
        match since {
            Some(since) => format!("{words} {}", timer(since, now, self.get(HOURS_MINUTES))),
            None => words.to_string(),
        }
    }
}

/// The keys the world draws: the sign over the CCTV doorway, and the monitor wall's
/// screen with no feed.
pub const CCTV_SIGN: &str = "world.text.cctv-sign";
pub const NO_SIGNAL: &str = "world.text.no-signal";
/// The word before a camera feed's number: "CAM 01".
pub const CAM: &str = "world.text.cam";
/// A flag's words, on flagged screens' tag bars, the focus
/// monitors and the ticker; the agent a focus monitor shows ("OPUS-5.5 / root"); and a
/// timer's units past an hour ("1 H 05 MIN").
pub const WAITING_ON_YOU: &str = "world.text.waiting-on-you";
pub const ASKING_YOU_SOMETHING: &str = "world.text.asking-you-something";
pub const ROOT_AGENT: &str = "world.text.root-agent";
pub const FOCUS_WHO: &str = "world.text.focus-who";
pub const HOURS_MINUTES: &str = "world.text.hours-minutes";
/// The console: its list's heading, or what it reads with
/// nobody flagged, and the labels in front of its three controls.
pub const SELECT_TARGET: &str = "world.text.select-target";
pub const ALL_AGENTS_WORKING: &str = "world.text.all-agents-working";
pub const KILL: &str = "world.text.kill";
pub const WORK_HARDER: &str = "world.text.work-harder";
pub const SIT_IN: &str = "world.text.sit-in";
/// Work harder's progress on its agent's console row.
pub const LIGHTNING_STRIKE: &str = "world.text.lightning-strike";
pub const DONE: &str = "world.text.done";
/// Kill's progress on its agent's console row, and a wreck's.
pub const KILL_SQUAD_EN_ROUTE: &str = "world.text.kill-squad-en-route";
pub const TARGET_NEUTRALISED: &str = "world.text.target-neutralised";
pub const WRECKED: &str = "world.text.wrecked";

/// The page's key for words the state machine names: the console's heading and rows,
/// and the focus monitors' and the ticker's status lines.
pub fn key(words: Words) -> &'static str {
    match words {
        Words::SelectTarget => SELECT_TARGET,
        Words::AllAgentsWorking => ALL_AGENTS_WORKING,
        Words::Flag(FlagKind::WaitingOnYou) => WAITING_ON_YOU,
        Words::Flag(FlagKind::AskingYouSomething) => ASKING_YOU_SOMETHING,
        Words::LightningStrike => LIGHTNING_STRIKE,
        Words::Done => DONE,
        Words::KillSquadEnRoute => KILL_SQUAD_EN_ROUTE,
        Words::TargetNeutralised => TARGET_NEUTRALISED,
        Words::Wrecked => WRECKED,
    }
}

/// `template` with each `{{name}}` in it replaced by its value, in whatever order the
/// page's language puts them; a name with no value stays as it is.
pub fn fill(template: &str, values: &[(&str, &str)]) -> String {
    values
        .iter()
        .fold(template.to_string(), |text, (name, value)| {
            text.replace(&format!("{{{{{name}}}}}"), value)
        })
}

/// A flag's timer, `since` to `now` (milliseconds since the epoch), as the focus monitors,
/// the ticker and the console show it: minutes and seconds under an hour ("04:12"), then
/// hours and minutes in the page's words (`hours_minutes`, a [`fill`] template with
/// `hours` and `minutes`): "1 H 05 MIN".
pub fn timer(since: i64, now: i64, hours_minutes: &str) -> String {
    let secs = (now - since).max(0) / 1000;
    if secs < 3600 {
        format!("{:02}:{:02}", secs / 60, secs % 60)
    } else {
        let (hours, minutes) = ((secs / 3600).to_string(), format!("{:02}", secs / 60 % 60));
        fill(hours_minutes, &[("hours", &hours), ("minutes", &minutes)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_flags_timer_reads_minutes_and_seconds_then_hours_and_minutes_in_the_pages_words() {
        let units = "{{hours}} H {{minutes}} MIN";
        let at = |secs: i64| timer(1_000_000, 1_000_000 + secs * 1000 + 999, units);
        assert_eq!(at(0), "00:00");
        assert_eq!(at(4 * 60 + 12), "04:12");
        assert_eq!(at(59 * 60 + 59), "59:59");
        assert_eq!(at(3600), "1 H 00 MIN");
        assert_eq!(at(3600 + 5 * 60 + 30), "1 H 05 MIN");
        assert_eq!(at(26 * 3600), "26 H 00 MIN");
        // The page's words set the order too.
        assert_eq!(
            timer(
                1_000_000,
                1_000_000 + 3_600_000,
                "{{hours}} Std. {{minutes}} Min."
            ),
            "1 Std. 00 Min."
        );
        assert_eq!(
            timer(
                1_000_000,
                1_000_000 + 3_900_000,
                "{{minutes}} min past {{hours}} h"
            ),
            "05 min past 1 h"
        );
        // A `since` ahead of the world's clock reads zero, not a negative time.
        assert_eq!(timer(5_000, 1_000, units), "00:00");
    }

    #[test]
    fn a_template_takes_each_value_in_the_pages_order_and_keeps_unknown_names() {
        assert_eq!(
            fill(
                "{{model}} / {{agent}}",
                &[("model", "OPUS-5.5"), ("agent", "root")]
            ),
            "OPUS-5.5 / root"
        );
        assert_eq!(
            fill(
                "{{agent}} ({{model}})",
                &[("model", "OPUS-5.5"), ("agent", "root")]
            ),
            "root (OPUS-5.5)"
        );
        assert_eq!(
            fill("{{other}} {{agent}}", &[("agent", "root")]),
            "{{other}} root"
        );
    }
}
