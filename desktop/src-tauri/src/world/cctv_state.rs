//! The CCTV state machine, pure and free of Bevy beside the floor plan: everything the
//! CCTV room decides. Bevy only draws what it says (`cctv`, `feeds`, `focus`, `console`,
//! `punishments`, `wrecks`, `flag_markers`). It is the world's second test seam; the
//! tests below drive it with floor plans built in code and assert on its outputs.
//!
//! - **Inputs:** [`CctvState::plan`] for each new floor plan, [`CctvState::click`] for a
//!   click resolved to a [`Target`], [`CctvState::advance`] for the passing of time and
//!   [`CctvState::set_effects`] for the Punishment effects setting. Time is the world's
//!   clock in milliseconds since the epoch, like a flag's `since`, given every update
//!   whether or not anything is drawn, so a punishment ends the same way seen or not.
//! - **Outputs** read the state for the moment asked: the [`Wall`] and its refresh
//!   turns, the focus monitors ([`CctvState::focus_views`]) and their turns, the ticker,
//!   the [`Console`] (words as [`Words`], never strings), the [`Punishment`]s playing,
//!   which robots are charred or wrecked, and which carry a marker.
//! - **Punishments:** one per agent at a time, any number of agents at once. Each is a
//!   [`Kind`] with a table of phases; its phase is a function of the clock alone. Each new
//!   plan keeps or clears it ([`keep`]) against its agent's [`Status`], and a kept one
//!   follows its desk wherever the plan puts it.
//! - **Real sessions:** a punishment asks its agent's real Claude Code session for one
//!   [`SessionAction`], once, at its step ([`Kind::session_action`]), with Punishment
//!   effects on or off: work harder at the bolt, for "WORK HARDER" to be sent into the
//!   session as a prompt (ADR-0014); a kill at the blast, for the session to be ended
//!   (ADR-0013). One cleared before its step never asks. The world hands each
//!   [`SessionRequest`] ([`CctvState::session_requests`]) to the shell through its
//!   reports; it never reaches into the OS itself.
//! - **Kill and wrecks:** KILL fires only while the flip cover is up ([`COVER_UP`]). Once
//!   fired, a kill goes through whatever the agent's state or flag does; only its session
//!   ending or its desk going before the blast stops it (ADR-0013). At the blast it asks,
//!   once, for its session to be ended for real. From
//!   the blast on the agent's robot is a wreck, which lasts until its agent's state or
//!   flag changes, the agent finishes or its desk goes. From the moment KILL fires until
//!   the wreck clears the agent is killed: off the ticker, out of the focus ranking (a
//!   kill playing still holds a focus monitor), without a flag colour on its feed or a
//!   focus monitor, and without a marker; its console row stays, WRECKED once the kill is
//!   over, selectable with both buttons disabled.
//!
//! Where the screens hang is `cctv_geometry`'s.

use std::collections::HashMap;

use agents::{AgentState, Flag, FlagKind};

use super::cctv_geometry;
use super::floor_plan::{Desk, DeskId, FloorPlan, Room};
use super::marker::{self, Marker};
use super::squad::{self, Trooper};

/// Feeds to a row of the monitor wall up to twelve rooms, and its most rows: past twelve
/// it stays three rows high and gains a column per three more rooms.
const COLUMNS: usize = 4;
const MAX_ROWS: usize = 3;

/// Wall feeds refreshed each frame: one, so twelve feeds update five times a second at
/// 60 frames a second. Measured on the owner's Ice Lake iGPU: a
/// feed of a 12-desk room costs about 5 ms a frame there, nearly all of it drawing its
/// 250-odd meshes one by one, so two a frame missed the 16.7 ms budget and one fits.
const REFRESH_PER_FRAME: usize = 1;

/// The fewest frames between two refreshes of one wall feed: four, so with fewer feeds
/// than that the wall rests between its turns instead of redrawing a feed every frame.
/// Owner-run-1: one room of 41 desks, its feed drawn every frame (nearly a thousand
/// meshes one by one), cost the CCTV room a fifth of its frame rate; every fourth frame
/// is 15 refreshes a second at 60 frames a second and 5 at 20.
const FEED_SPACING: usize = 4;

/// Frames between two refreshes of one focus monitor: eight, so each draws 7.5 times a
/// second at 60 frames a second; the second draws four frames after the first, so they
/// never both draw in one frame and at most one focus picture is drawn in any four
/// frames. Measured on the owner's Ice Lake iGPU: the frame is
/// already bound by the GPU there, a 640 × 360 close-up every other frame cost 8–10 ms a
/// frame and a 320 × 180 one 5.5 ms, so the focus monitors draw 320 × 180 half as often.
const FOCUS_EVERY: usize = 8;

/// While the CCTV room is seen only through its doorway from the corridor, the wall's
/// turns go every other frame: there the portal draws the room on top of the corridor,
/// and that view sat right at the 16.7 ms line with a feed every frame.
const DOORWAY_EVERY: usize = 2;

/// Rows the console's screen lists under its heading (`cctv_console.py`'s 1.24 × 0.66 m
/// glass: a heading and six rows of a readable size).
pub const CONSOLE_ROWS: usize = 6;

/// What a click hit, as the world's picking resolves it (`picking`, `aim`): the CCTV
/// room's clickable things.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// A console row, by the agent it lists.
    Row(DeskId),
    /// A wall feed, by its room's project id.
    Feed(String),
    /// Focus monitor 0 (the left, "1" to the user) or 1.
    Focus(usize),
    /// The flip cover over KILL, the KILL button and the WORK HARDER bar.
    Cover,
    Kill,
    WorkHarder,
    /// The SIT IN bar (sit in): the seat's, never a punishment's.
    SitIn,
}

/// Words the console shows, as which of the page's words they are, never the words
/// themselves (the world looks each up in the page's table, `strings`). Tickets 19–21 add
/// the punishments' progress and WRECKED.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Words {
    SelectTarget,
    AllAgentsWorking,
    /// A flag's words: "WAITING ON YOU", "ASKING YOU SOMETHING".
    Flag(FlagKind),
    /// Work harder's progress on its agent's row: "LIGHTNING STRIKE" while
    /// the bolt plays, then "DONE" for about two seconds.
    LightningStrike,
    Done,
    /// Kill's progress: "KILL SQUAD EN ROUTE" until the blast, then
    /// "TARGET NEUTRALISED" for about two seconds, then "WRECKED" while the wreck stays.
    KillSquadEnRoute,
    TargetNeutralised,
    Wrecked,
}

/// How a console button looks: dark with nobody selected, lit while it can fire on the
/// selected agent, disabled while a punishment plays on it or it is a wreck.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Dark,
    Lit,
    Disabled,
}

/// The flip cover over KILL: down; up, so the button beneath can fire, for
/// [`COVER_UP`] after the click that lifted it; or locked down while a punishment plays
/// on the selected agent or it is a wreck.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cover {
    Down,
    Up,
    Locked,
}

/// How long the flip cover stays up after the click that lifts it, in milliseconds.
pub const COVER_UP: i64 = 3_000;

/// An agent's state and flag, as a floor plan gives them: what a punishment (or a wreck)
/// began with, and what [`keep`] compares the agent's current one against.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    pub state: AgentState,
    pub flag: Option<Flag>,
}

/// Keep or clear (spec: "Punishment and wreck rules over time"): whether work harder (or
/// a wreck) whose agent had state and flag `began` when it began stays,
/// given the agent's state and flag `now`, or none when its desk is gone. Any difference
/// clears it: a change of state (busy, waiting, idle, finished), of flag kind, or of
/// `since` (a new waiting reason restarts it).
pub fn keep(began: &Status, now: Option<&Status>) -> bool {
    now == Some(began)
}

/// A punishment: *work harder* or *kill*.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    WorkHarder,
    Kill,
}

/// What the CCTV room asks of an agent's real Claude Code session: the shell does it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionAction {
    /// Work harder's bolt: "WORK HARDER" sent into the session as a prompt (ADR-0014).
    WorkHarder,
    /// A kill's blast: the session ended (ADR-0013).
    End,
}

/// A [`SessionAction`] on the session `session` (a root agent's: only root agents are
/// flagged, so only they are punished).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionRequest {
    pub session: String,
    pub action: SessionAction,
}

/// A punishment's phase, for the world to draw its effects and the console its words.
/// Work harder (spec: "Punishments in motion"): blue arcs crackle on the ceiling over
/// the robot; a forked bolt strikes its head with a room-wide flash; the robot is
/// charred, sparking and smoking; its paint returns with one wisp of smoke; then the
/// row reads DONE while nothing more is drawn.
///
/// Kill: the squad appears in the room's doorway on its interior side and
/// runs in (breach), runs along the aisles to the desk (advance), two point at the robot
/// while one crouches and plants a charge on its chest (surround), runs back out through
/// the doorway and vanishes there (fall back), and the charge blows (boom): from then on
/// the robot is a wreck ([`CctvState::wrecked`]). The squad's moves are [`squad`]'s.
///
/// With Punishment effects off, every phase but DONE is reported as
/// [`Phase::Still`]: no crackle, bolt, flash, squad or blast is drawn, while the timings,
/// the console's words and the end states stay ([`CctvState::charred`] for work harder's
/// charred second, [`CctvState::wrecked`] from the kill's blast on).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Crackle,
    Strike,
    Charred,
    Wisp,
    Done,
    Breach,
    Advance,
    Surround,
    FallBack,
    Boom,
    /// Punishment effects off: nothing is drawn for the punishment meanwhile.
    Still,
}

impl Phase {
    /// The phase reported for this step of a punishment: the step itself with effects
    /// on, and with them off, [`Phase::Still`] for every step that is only a show.
    fn shown(self, effects: bool) -> Phase {
        if effects || self == Phase::Done {
            self
        } else {
            Phase::Still
        }
    }
}

/// Work harder's phases and how long each lasts, in milliseconds: about two seconds of
/// lightning (half a second of crackle, the bolt, a second charred, the wisp), then
/// DONE for two.
const WORK_HARDER: [(Phase, i64); 5] = [
    (Phase::Crackle, 500),
    (Phase::Strike, 300),
    (Phase::Charred, 1000),
    (Phase::Wisp, 400),
    (Phase::Done, 2000),
];

/// Kill's phases and how long each lasts, in milliseconds: about six seconds of the
/// squad (in through the doorway, up the aisles, round the desk, back out), then the
/// blast and TARGET NEUTRALISED for two; about eight in all.
pub(crate) const KILL: [(Phase, i64); 5] = [
    (Phase::Breach, 600),
    (Phase::Advance, 2400),
    (Phase::Surround, 1400),
    (Phase::FallBack, 1800),
    (Phase::Boom, 2000),
];

impl Kind {
    /// Its phases in order, with how long each lasts.
    fn phases(self) -> &'static [(Phase, i64)] {
        match self {
            Kind::WorkHarder => &WORK_HARDER,
            Kind::Kill => &KILL,
        }
    }

    /// How long it lasts in all, DONE included, in milliseconds.
    pub fn lasts(self) -> i64 {
        self.phases().iter().map(|&(_, lasts)| lasts).sum()
    }

    /// What it asks of its agent's real session, once, and the step at which it asks
    /// (the same moment with Punishment effects off): work harder at the bolt, a kill at
    /// the blast.
    pub fn session_action(self) -> (Phase, SessionAction) {
        match self {
            Kind::WorkHarder => (Phase::Strike, SessionAction::WorkHarder),
            Kind::Kill => (Phase::Boom, SessionAction::End),
        }
    }

    /// Whether a punishment at step `at` (none: over) has reached `step` of its own.
    fn reached(self, at: Option<Phase>, step: Phase) -> bool {
        let Some(at) = at else {
            return true;
        };
        let index = |phase| self.phases().iter().position(|&(p, _)| p == phase);
        index(at) >= index(step)
    }

    /// Its phase `elapsed` milliseconds after it began, and how far through it (0 to 1),
    /// or none once it is over.
    fn phase_at(self, elapsed: i64) -> Option<(Phase, f32)> {
        let mut start = 0;
        for &(phase, lasts) in self.phases() {
            if elapsed < start + lasts {
                let into = (elapsed - start).max(0);
                return Some((phase, into as f32 / lasts as f32));
            }
            start += lasts;
        }
        None
    }
}

/// A punishment in progress on one agent (spec: "The CCTV state machine", its state).
#[derive(Clone, Debug, PartialEq)]
pub struct Punishment {
    /// Its agent: the desk it belongs to (by session and agent, never a floor position)
    /// and where that desk is in the last plan, which it follows if desks repack.
    pub agent: Flagged,
    pub kind: Kind,
    /// When it began, on the world's clock.
    pub began: i64,
    /// The agent's state and flag when it began: any change clears work harder
    /// ([`keep`]); a kill goes on (see [`kill_goes_on`]).
    pub began_with: Status,
    /// Its phase now as the world draws it (see [`Phase`] for Punishment effects off),
    /// and how far through it (0 to 1).
    pub phase: Phase,
    pub progress: f32,
    /// Its step now on its own timeline, whatever the setting: what the console's words,
    /// the charring and the wreck follow.
    step: Phase,
    /// Whether it has reached its step and asked its agent's session for its
    /// [`SessionAction`] (for a kill: whether it has blown).
    asked: bool,
}

impl Punishment {
    /// The words its agent's console row shows meanwhile, whatever the setting.
    pub fn words(&self) -> Words {
        match self.step {
            Phase::Crackle | Phase::Strike | Phase::Charred | Phase::Wisp => Words::LightningStrike,
            Phase::Done => Words::Done,
            Phase::Breach | Phase::Advance | Phase::Surround | Phase::FallBack => {
                Words::KillSquadEnRoute
            }
            Phase::Boom => Words::TargetNeutralised,
            // Never a step of its own; the words of the punishment's first steps.
            Phase::Still => match self.kind {
                Kind::WorkHarder => Words::LightningStrike,
                Kind::Kill => Words::KillSquadEnRoute,
            },
        }
    }

    /// A kill's squad at this moment: each robot in the room now, on its way
    /// to (or back from) the agent's desk as the last plan has it, so it re-routes if
    /// desks repack. None for work harder.
    pub fn squad(&self) -> Vec<Trooper> {
        match self.kind {
            Kind::Kill => squad::squad(
                self.agent.rows,
                self.agent.row,
                self.agent.column,
                self.phase,
                self.progress,
            ),
            Kind::WorkHarder => Vec::new(),
        }
    }
}

/// The console as the state machine says it looks now.
#[derive(Clone, Debug, PartialEq)]
pub struct Console {
    /// "SELECT TARGET", or "ALL AGENTS WORKING" with nobody flagged.
    pub heading: Words,
    pub rows: Vec<ConsoleRow>,
    /// Flagged agents that do not fit on the screen.
    pub hidden: usize,
    pub kill: Button,
    pub work_harder: Button,
    /// The flip cover over KILL.
    pub cover: Cover,
}

/// One console row: the agent (its room, model and flag), whether it is the selected
/// one, its words and the time its timer counts from, if it shows one.
#[derive(Clone, Debug, PartialEq)]
pub struct ConsoleRow {
    pub agent: Flagged,
    pub selected: bool,
    pub words: Words,
    pub timer: Option<i64>,
}

/// What one focus monitor shows: its agent; the flag colour its picture is washed and
/// framed in, none for a killed agent; and its status line's words, with the time its
/// timer counts from if it shows one.
#[derive(Clone, Debug, PartialEq)]
pub struct FocusView {
    pub agent: Flagged,
    pub flag: Option<FlagKind>,
    pub words: Words,
    pub timer: Option<i64>,
    /// While a punishment plays on its agent: shown wide, the whole desk with the ceiling
    /// over it and the aisles round it, and drawn every frame, so the show is seen whole
    /// (`cctv_geometry::punishment_camera`); otherwise the close-up of the face.
    pub wide: bool,
}

/// The CCTV room's state: everything it decides, from the inputs it was given.
#[derive(Clone, Debug)]
pub struct CctvState {
    wall: Wall,
    /// Every flagged agent, most urgent first (see [`CctvState::focus`]).
    flagged: Vec<Flagged>,
    /// The next feed to refresh, counted along the wall.
    turn: usize,
    /// Frames into the wall's round of turns, which lasts at least [`FEED_SPACING`].
    round_frame: usize,
    /// How many wall feeds refresh each frame.
    per_frame: usize,
    /// Frames counted for the focus monitors' turns.
    focus_frame: usize,
    /// Through the doorway, a turn every this many frames, and frames counted there.
    doorway_every: usize,
    doorway_frames: usize,
    /// The agent the user picked to punish: nobody until a click, and nobody
    /// again once its agent has no flag.
    selected: Option<DeskId>,
    /// The world's clock as last given ([`CctvState::advance`], [`CctvState::click`]),
    /// in milliseconds since the epoch.
    now: i64,
    /// Every agent's state and flag in the last plan, for a punishment to begin with.
    statuses: HashMap<DeskId, Status>,
    /// Punishments in progress, newest first, at most one per agent.
    punishments: Vec<Punishment>,
    /// Until when the flip cover stays up, if it is.
    cover_up_until: Option<i64>,
    /// What kills left behind, each kept until its agent changes.
    wrecks: Vec<Wreck>,
    /// The markers the last plan asks for (`marker`'s), before kills take any away.
    markers: HashMap<DeskId, Marker>,
    /// The Punishment effects setting: on by default.
    effects: bool,
    /// What punishments have asked of real sessions since
    /// [`CctvState::session_requests`] last took it.
    requests: Vec<SessionRequest>,
}

/// What a kill leaves at its agent's desk once it is over: a wreck, which belongs to the
/// agent like the kill did, follows its desk and is kept or cleared by [`keep`] against
/// the agent's state and flag when the kill ended.
#[derive(Clone, Debug, PartialEq)]
pub struct Wreck {
    pub agent: Flagged,
    pub began_with: Status,
}

impl Default for CctvState {
    fn default() -> Self {
        Self {
            wall: Wall::default(),
            flagged: Vec::new(),
            turn: 0,
            round_frame: 0,
            per_frame: REFRESH_PER_FRAME,
            focus_frame: 0,
            doorway_every: DOORWAY_EVERY,
            doorway_frames: 0,
            selected: None,
            now: 0,
            statuses: HashMap::new(),
            punishments: Vec::new(),
            cover_up_until: None,
            wrecks: Vec::new(),
            markers: HashMap::new(),
            effects: true,
            requests: Vec::new(),
        }
    }
}

/// The monitor wall: a grid `columns` wide and `rows` high, filled left to right then
/// top to bottom in the corridor's order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Wall {
    pub columns: usize,
    pub rows: usize,
    pub feeds: Vec<Feed>,
}

/// One project room's feed on the wall.
#[derive(Clone, Debug, PartialEq)]
pub struct Feed {
    /// The room's project id, which keys its feed camera; its name, for the overlay; and
    /// its slot, whose render layer the feed camera sees.
    pub project: String,
    pub name: String,
    pub slot: usize,
    /// Its CAM number: the room's corridor position, from 1, which is also its place on
    /// the wall.
    pub cam: usize,
    /// Its place on the wall, from the top left.
    pub column: usize,
    pub row: usize,
    /// Its flag colour: the flag of its room's flagged agents, red (*asking you
    /// something*) over amber (*waiting on you*) when it has both; none when nobody in
    /// the room is flagged.
    pub flag: Option<FlagKind>,
}

/// A flagged agent, as the focus monitors and the ticker show it.
#[derive(Clone, Debug, PartialEq)]
pub struct Flagged {
    pub desk: DeskId,
    /// Its room: the project id, which keys the room's feed; its name; its slot, whose
    /// render layer a focus camera sees; and its CAM number.
    pub project: String,
    pub name: String,
    pub slot: usize,
    pub cam: usize,
    /// Its desk's place in the room, for a focus camera to stand in front of its face,
    /// and the room's rows (its depth), for the kill squad's way to it.
    pub row: usize,
    pub column: usize,
    pub rows: usize,
    /// Its model's short name ([`short_model`]); a name, never translated.
    pub model: String,
    pub kind: FlagKind,
    /// Since when it has been flagged, in milliseconds since the epoch.
    pub since: i64,
}

impl Wall {
    /// Whether the wall shows one NO SIGNAL screen: no project is open.
    pub fn no_signal(&self) -> bool {
        self.feeds.is_empty()
    }

    /// Columns past the narrow wall's four: each widens the CCTV room by one column,
    /// half on each side (a `cctv_extension` strip each), so it stays centred.
    pub fn extra_columns(&self) -> usize {
        self.columns.saturating_sub(COLUMNS)
    }

    /// Half the CCTV room's inside width for this wall: its side walls' inner faces
    /// stand this far either side of the doorway.
    pub fn half_width(&self) -> f32 {
        cctv_geometry::room_half_width(self.extra_columns())
    }
}

/// [`Wall::extra_columns`] and [`Wall::half_width`] of the wall `rooms` open rooms make,
/// for `collision` and the room's shell (`cctv`), which follow the plan.
pub fn room_extra_columns(rooms: usize) -> usize {
    columns(rooms).saturating_sub(COLUMNS)
}

pub fn room_half_width(rooms: usize) -> f32 {
    Wall {
        columns: columns(rooms),
        ..Wall::default()
    }
    .half_width()
}

impl CctvState {
    /// A new floor plan: the wall follows it alone, every open room in corridor order.
    pub fn plan(&mut self, plan: &FloorPlan) {
        let columns = columns(plan.rooms.len());
        let feeds: Vec<Feed> = plan
            .rooms
            .iter()
            .enumerate()
            .map(|(n, room)| Feed {
                project: room.project.clone(),
                name: room.name.clone(),
                slot: room.slot,
                cam: n + 1,
                column: n % columns,
                row: n / columns,
                // Coloured below, once the punishments and wrecks are kept or cleared.
                flag: None,
            })
            .collect();
        self.wall = Wall {
            columns,
            rows: feeds.len().div_ceil(columns),
            feeds,
        };
        self.flagged = ranked(plan);
        self.markers = marker::of_plan(plan);
        self.statuses = plan
            .rooms
            .iter()
            .flat_map(|room| &room.desks)
            .map(|desk| {
                let status = Status {
                    state: desk.state,
                    flag: desk.flag,
                };
                (desk.id.clone(), status)
            })
            .collect();
        if self.selected_agent().is_none() {
            self.select(None);
        }
        // Keep or clear each punishment and wreck against its agent now, and move a kept
        // one to wherever its desk is in this plan.
        self.punishments.retain_mut(|p| {
            let (kind, asked, began_with) = (p.kind, p.asked, p.began_with);
            follow(plan, &mut p.agent, |now| match kind {
                Kind::Kill => kill_goes_on(asked, now),
                Kind::WorkHarder => keep(&began_with, now),
            })
        });
        self.wrecks
            .retain_mut(|w| follow(plan, &mut w.agent, |now| keep(&w.began_with, now)));
        self.recolour();
    }

    /// Gives each feed its room's flag colour from the ranking, so a killed agent no
    /// longer counts: red if anyone there asks something, else amber if
    /// anyone waits on the user, else none.
    fn recolour(&mut self) {
        let colours: Vec<Option<FlagKind>> = self
            .wall
            .feeds
            .iter()
            .map(|feed| {
                let mut kinds = self
                    .ranking()
                    .filter(|f| f.project == feed.project)
                    .map(|f| f.kind);
                let first = kinds.next()?;
                if first == FlagKind::AskingYouSomething
                    || kinds.any(|kind| kind == FlagKind::AskingYouSomething)
                {
                    Some(FlagKind::AskingYouSomething)
                } else {
                    Some(first)
                }
            })
            .collect();
        for (feed, flag) in self.wall.feeds.iter_mut().zip(colours) {
            feed.flag = flag;
        }
    }

    /// Selects `desk` (or nobody): a change of selection drops the flip cover.
    fn select(&mut self, desk: Option<DeskId>) {
        if self.selected != desk {
            self.cover_up_until = None;
        }
        self.selected = desk;
    }

    /// A click, resolved by the world's picking to what it hit, at `now` (milliseconds
    /// since the epoch, for the flip cover's three seconds).
    /// A console row, a flagged wall feed (its room's top-ranked flagged agent) or a
    /// focus monitor selects that agent; only flagged agents can be selected, so anything
    /// else leaves the selection as it is. WORK HARDER fires on the selection; the first
    /// click on the cover lifts it, and KILL fires only while it is up.
    pub fn click(&mut self, target: &Target, now: i64) {
        self.advance(now);
        let picked = match target {
            Target::Row(desk) => self.flagged.iter().find(|f| &f.desk == desk),
            // The ranking's order: the room's first is its top-ranked (never a killed
            // agent, which no longer colours the feed).
            Target::Feed(project) => self.ranking().find(|f| &f.project == project),
            Target::Focus(n) => self.focus().get(*n).copied().flatten(),
            Target::WorkHarder => {
                self.punish(Kind::WorkHarder);
                None
            }
            // The first click lifts the cover, if KILL could fire now.
            Target::Cover => {
                if self.cover_up_until.is_none() && self.console().kill == Button::Lit {
                    self.cover_up_until = Some(now + COVER_UP);
                }
                None
            }
            // The button beneath fires only while the cover is up.
            Target::Kill => {
                if self.cover_up_until.is_some() {
                    self.punish(Kind::Kill);
                }
                None
            }
            // The world sits the user in [`CctvState::sit_in`]; nothing here changes.
            Target::SitIn => None,
        };
        if let Some(desk) = picked.map(|agent| agent.desk.clone()) {
            self.select(Some(desk));
        }
    }

    /// Starts a punishment of `kind` on the selected agent, now, unless one already
    /// plays on it: one per agent at a time.
    fn punish(&mut self, kind: Kind) {
        let Some(agent) = self.selected_agent().cloned() else {
            return;
        };
        if self.punishment_of(&agent.desk).is_some() || self.wreck_of(&agent.desk).is_some() {
            return;
        }
        let Some(status) = self.statuses.get(&agent.desk).copied() else {
            return;
        };
        let (step, progress) = kind.phase_at(0).expect("a punishment's first phase");
        self.punishments.insert(
            0,
            Punishment {
                agent,
                kind,
                began: self.now,
                began_with: status,
                phase: step.shown(self.effects),
                progress,
                step,
                asked: false,
            },
        );
        // Locked down while it plays, and down after.
        self.cover_up_until = None;
        // A kill takes its agent off its feed's colour at once.
        self.recolour();
    }

    /// The wreck at `desk`, if any.
    fn wreck_of(&self, desk: &DeskId) -> Option<&Wreck> {
        self.wrecks.iter().find(|w| &w.agent.desk == desk)
    }

    /// The punishment playing on `desk`, if any.
    fn punishment_of(&self, desk: &DeskId) -> Option<&Punishment> {
        self.punishments.iter().find(|p| &p.agent.desk == desk)
    }

    /// Whether the agent at `desk` is killed: from the moment KILL fires on it until its
    /// wreck clears.
    fn killed(&self, desk: &DeskId) -> bool {
        self.wreck_of(desk).is_some()
            || self
                .punishment_of(desk)
                .is_some_and(|p| p.kind == Kind::Kill)
    }

    /// The flagged agents most urgent first, without the killed ones: the focus
    /// monitors' ranking and the ticker (spec: "Focus monitors and the ticker", wrecks).
    fn ranking(&self) -> impl Iterator<Item = &Flagged> {
        self.flagged.iter().filter(|f| !self.killed(&f.desk))
    }

    /// The passing of time: the world's clock is now `now` (milliseconds since the
    /// epoch). Each punishment moves on to its phase for that moment, and one that has
    /// run its course ends. It runs on this clock alone, whether or not anything is drawn.
    /// One reaching the step at which it asks something of its agent's session (or over
    /// without that step seen) asks, once: work harder at the bolt, a kill at its blast.
    /// A kill that is over leaves its wreck, kept against the agent as it is
    /// then, unless its session has ended; the flip cover drops once its time is up.
    pub fn advance(&mut self, now: i64) {
        self.now = now;
        let (wrecks, requests, statuses) = (&mut self.wrecks, &mut self.requests, &self.statuses);
        let effects = self.effects;
        self.punishments.retain_mut(|p| {
            let at = p.kind.phase_at(now - p.began);
            let (step, action) = p.kind.session_action();
            if !p.asked && p.kind.reached(at.map(|(at, _)| at), step) {
                p.asked = true;
                requests.push(SessionRequest {
                    session: p.agent.desk.session.clone(),
                    action,
                });
            }
            match at {
                Some((step, progress)) => {
                    p.step = step;
                    p.phase = step.shown(effects);
                    p.progress = progress;
                    true
                }
                None => {
                    let live = statuses
                        .get(&p.agent.desk)
                        .filter(|status| status.state != AgentState::Finished);
                    if let (Kind::Kill, Some(&status)) = (p.kind, live) {
                        wrecks.push(Wreck {
                            agent: p.agent.clone(),
                            began_with: status,
                        });
                    }
                    false
                }
            }
        });
        if self.cover_up_until.is_some_and(|until| now >= until) {
            self.cover_up_until = None;
        }
    }

    /// The Punishment effects setting, applied at once, to punishments
    /// already playing too: off, only DONE is reported of their phases and the rest is
    /// [`Phase::Still`]; their timings, words and end states stay.
    pub fn set_effects(&mut self, on: bool) {
        self.effects = on;
        self.advance(self.now);
    }

    /// The Punishment effects setting as last given.
    pub fn effects(&self) -> bool {
        self.effects
    }

    /// The agents whose robot is charred now: work harder's charred second, with
    /// Punishment effects on or off. The world chars each one's robot.
    pub fn charred(&self) -> Vec<&Flagged> {
        self.punishments
            .iter()
            .filter(|p| p.step == Phase::Charred)
            .map(|p| &p.agent)
            .collect()
    }

    /// The agents whose robot is a wreck now: from a kill's blast on, then while its
    /// wreck stays, with Punishment effects on or off. The world hides each one's robot
    /// and lays its wreck at its desk.
    pub fn wrecked(&self) -> Vec<&Flagged> {
        let blasting = self
            .punishments
            .iter()
            .filter(|p| p.step == Phase::Boom)
            .map(|p| &p.agent);
        blasting
            .chain(self.wrecks.iter().map(|w| &w.agent))
            .collect()
    }

    /// The robots that carry a flag marker now, by desk, and how (spec: "Flag markers in
    /// project rooms"): every flagged root agent's, but a killed one's, from the moment
    /// KILL fires until its wreck clears; then a fresh one if it is flagged. Work harder
    /// leaves it on. `flag_markers` draws what this says.
    pub fn markers(&self) -> HashMap<DeskId, Marker> {
        self.markers
            .iter()
            .filter(|(desk, _)| !self.killed(desk))
            .map(|(desk, marker)| (desk.clone(), *marker))
            .collect()
    }

    /// What punishments have asked of real sessions since the last call, each once, at
    /// its step (with Punishment effects on or off): the world hands them to the shell,
    /// which does them (ADR-0013, ADR-0014). A punishment cleared before its step never
    /// asks.
    pub fn session_requests(&mut self) -> Vec<SessionRequest> {
        std::mem::take(&mut self.requests)
    }

    /// The world's clock as last given, in milliseconds since the epoch.
    pub fn now(&self) -> i64 {
        self.now
    }

    /// The punishments in progress, newest first, each with its phase and progress for
    /// the world to draw its effects, on its agent's desk as the last plan has it.
    pub fn punishments(&self) -> &[Punishment] {
        &self.punishments
    }

    /// The selected agent, if any.
    pub fn selected(&self) -> Option<&DeskId> {
        self.selected.as_ref()
    }

    /// Who SIT IN sits the user in: the selected agent while it is flagged, punished or
    /// not. Asking changes nothing (agent-screens spec: "SIT IN on the CCTV console").
    pub fn sit_in(&self) -> Option<&DeskId> {
        self.selected_agent().map(|agent| &agent.desk)
    }

    /// The selected agent as the ranking has it, while it is flagged.
    fn selected_agent(&self) -> Option<&Flagged> {
        let selected = self.selected.as_ref()?;
        self.flagged.iter().find(|f| &f.desk == selected)
    }

    /// The two focus monitors' agents, each none for NO SIGNAL: punishments playing,
    /// newest first, so a kill holds one while it plays; then the selected
    /// agent (a WRECKED row too, if the user picks it); then the flagged agents but the
    /// killed ones, *asking you something* before *waiting on you*, then the
    /// longest-waited first, then in corridor and desk order; each shown once.
    pub fn focus(&self) -> [Option<&Flagged>; 2] {
        let punished = self.punishments.iter().map(|p| &p.agent);
        let selected = self.selected_agent();
        let mut seen: Vec<&DeskId> = Vec::with_capacity(2);
        let mut order = punished.chain(selected).chain(self.ranking()).filter(|f| {
            let fresh = !seen.contains(&&f.desk);
            if fresh {
                seen.push(&f.desk);
            }
            fresh
        });
        [order.next(), order.next()]
    }

    /// What the two focus monitors show, each none for NO SIGNAL: [`CctvState::focus`]'s
    /// agents, each with the flag colour its screen and frame take and its status line. A
    /// killed agent, from the moment KILL fires until its wreck clears, has no flag
    /// colour, and its status line reads its console row's words without a timer: the
    /// kill's progress, then WRECKED.
    pub fn focus_views(&self) -> [Option<FocusView>; 2] {
        self.focus().map(|agent| {
            let agent = agent?;
            let (flag, (words, timer)) = if self.killed(&agent.desk) {
                (None, self.row_words(agent))
            } else {
                (
                    Some(agent.kind),
                    (Words::Flag(agent.kind), Some(agent.since)),
                )
            };
            Some(FocusView {
                wide: self.punishment_of(&agent.desk).is_some(),
                agent: agent.clone(),
                flag,
                words,
                timer,
            })
        })
    }

    /// An agent's console words, and the time its timer counts from if it shows one: a
    /// punishment's progress while one plays, WRECKED over a wreck, else its flag.
    fn row_words(&self, agent: &Flagged) -> (Words, Option<i64>) {
        match self.punishment_of(&agent.desk) {
            Some(punishment) => (punishment.words(), None),
            None if self.wreck_of(&agent.desk).is_some() => (Words::Wrecked, None),
            None => (Words::Flag(agent.kind), Some(agent.since)),
        }
    }

    /// The console's screen and buttons: the flagged agents under "SELECT
    /// TARGET", most urgent first as on the ticker, the selected one highlighted, or
    /// "ALL AGENTS WORKING" with nobody flagged. Both buttons are dark until an agent is
    /// selected. The screen holds [`CONSOLE_ROWS`] rows; an agent selected past them takes
    /// the last row's place, and the rest are counted.
    pub fn console(&self) -> Console {
        let selected = self.selected_agent();
        let mut shown: Vec<&Flagged> = self.flagged.iter().take(CONSOLE_ROWS).collect();
        if let Some(agent) = selected {
            if !shown.iter().any(|f| f.desk == agent.desk) {
                shown.pop();
                shown.push(agent);
            }
        }
        let rows: Vec<ConsoleRow> = shown
            .into_iter()
            .map(|agent| {
                let (words, timer) = self.row_words(agent);
                ConsoleRow {
                    agent: agent.clone(),
                    selected: Some(&agent.desk) == self.selected.as_ref(),
                    words,
                    timer,
                }
            })
            .collect();
        let button = match selected {
            None => Button::Dark,
            Some(agent)
                if self.punishment_of(&agent.desk).is_some()
                    || self.wreck_of(&agent.desk).is_some() =>
            {
                Button::Disabled
            }
            Some(_) => Button::Lit,
        };
        let cover = match (button, self.cover_up_until) {
            (Button::Disabled, _) => Cover::Locked,
            (Button::Lit, Some(_)) => Cover::Up,
            _ => Cover::Down,
        };
        Console {
            heading: if self.flagged.is_empty() {
                Words::AllAgentsWorking
            } else {
                Words::SelectTarget
            },
            hidden: self.flagged.len() - rows.len(),
            rows,
            kill: button,
            work_harder: button,
            cover,
        }
    }

    /// The ticker's entries: every flagged agent, most urgent first, in the focus
    /// monitors' order, but for the killed ones (from KILL until the wreck clears).
    pub fn ticker(&self) -> Vec<&Flagged> {
        self.ranking().collect()
    }

    /// The monitor wall as the last plan lays it out.
    pub fn wall(&self) -> &Wall {
        &self.wall
    }

    /// How many wall feeds refresh each frame (a debug switch's, for measuring).
    pub fn set_refresh_per_frame(&mut self, count: usize) {
        self.per_frame = count;
    }

    /// Through the doorway, a turn every `every` frames (a debug switch's, for measuring).
    pub fn set_doorway_every(&mut self, every: usize) {
        self.doorway_every = every.max(1);
    }

    /// This frame's turn while the CCTV room is seen only through its doorway from the
    /// corridor: [`CctvState::refresh`]'s every [`DOORWAY_EVERY`] frames, none between.
    pub fn refresh_through_doorway(&mut self) -> Vec<String> {
        let due = self.doorway_frames.is_multiple_of(self.doorway_every);
        self.doorway_frames = (self.doorway_frames + 1) % self.doorway_every;
        if due {
            self.refresh()
        } else {
            Vec::new()
        }
    }

    /// This frame's turn: the projects whose feeds draw now, in turns along the wall, so
    /// each feed refreshes as often as every other and the cost per frame stays flat
    /// however many feeds there are, and no feed more often than every
    /// [`FEED_SPACING`] frames: with fewer feeds, some frames draw none. Call it once per
    /// drawn frame while the CCTV room is seen; the other feeds keep their last picture.
    pub fn refresh(&mut self) -> Vec<String> {
        let feeds = &self.wall.feeds;
        if feeds.is_empty() || self.per_frame == 0 {
            return Vec::new();
        }
        let busy = feeds.len().div_ceil(self.per_frame);
        let round = busy.max(FEED_SPACING);
        // A wall that shrank goes on from where its shorter round is now.
        let frame = self.round_frame % round;
        self.round_frame = (frame + 1) % round;
        if frame >= busy {
            return Vec::new();
        }
        let count = self.per_frame.min(feeds.len());
        let due = (0..count)
            .map(|k| feeds[(self.turn + k) % feeds.len()].project.clone())
            .collect();
        self.turn = (self.turn + count) % feeds.len();
        due
    }

    /// This frame's turn of the focus monitors: whether each one's camera draws now. Each
    /// with an agent draws every [`FOCUS_EVERY`] frames, the two never in the same frame,
    /// but one showing a punishment draws every frame while it plays: its bolt lasts 300 ms,
    /// shorter than a turn at the frame rates the CCTV room gets. One showing NO SIGNAL
    /// never draws. Call it once per drawn frame while the CCTV room is seen, like
    /// [`CctvState::refresh`].
    pub fn refresh_focus(&mut self) -> [bool; 2] {
        let frame = self.focus_frame;
        self.focus_frame = (frame + 1) % FOCUS_EVERY;
        let focus = self.focus();
        [0, 1].map(|n| {
            focus[n].is_some_and(|agent| {
                frame == n * FOCUS_EVERY / 2 || self.punishment_of(&agent.desk).is_some()
            })
        })
    }
}

/// Whether a kill goes on, given its agent's state and flag `now`, or none when its desk
/// is gone (ADR-0013): once fired, whatever the agent's state or flag does, until its
/// desk goes, or its session ends before the blast. From the blast on, the session
/// ending is the kill's own doing, and TARGET NEUTRALISED plays out.
pub fn kill_goes_on(blasted: bool, now: Option<&Status>) -> bool {
    now.is_some_and(|now| blasted || now.state != AgentState::Finished)
}

/// Keep or clear a punishment's or wreck's `agent` against `plan`, as `keeps` says of its
/// agent's state and flag now (none when its desk is gone); a kept one moves to wherever
/// its desk is in the plan.
fn follow(
    plan: &FloorPlan,
    agent: &mut Flagged,
    keeps: impl FnOnce(Option<&Status>) -> bool,
) -> bool {
    let found = plan.rooms.iter().enumerate().find_map(|(n, room)| {
        let desk = room.desks.iter().find(|d| d.id == agent.desk)?;
        Some((n, room, desk))
    });
    let now = found.map(|(_, _, desk)| Status {
        state: desk.state,
        flag: desk.flag,
    });
    if !keeps(now.as_ref()) {
        return false;
    }
    if let Some((n, room, desk)) = found {
        let flag = desk.flag.unwrap_or(Flag {
            kind: agent.kind,
            since: agent.since,
        });
        *agent = flagged(n, room, desk, flag);
    }
    true
}

/// Every flagged agent of `plan`, most urgent first: *asking you something* before
/// *waiting on you*, then the earliest `since`, then corridor and desk order.
fn ranked(plan: &FloorPlan) -> Vec<Flagged> {
    let mut flagged: Vec<Flagged> = plan
        .rooms
        .iter()
        .enumerate()
        .flat_map(|(n, room)| {
            room.desks
                .iter()
                .filter_map(move |desk| Some(flagged(n, room, desk, desk.flag?)))
        })
        .collect();
    // A stable sort keeps corridor and desk order among equals.
    flagged.sort_by_key(|f| (f.kind != FlagKind::AskingYouSomething, f.since));
    flagged
}

/// The agent at `desk` in `room`, the `n`th of the corridor, as flagged with `flag`.
fn flagged(n: usize, room: &Room, desk: &Desk, flag: Flag) -> Flagged {
    Flagged {
        desk: desk.id.clone(),
        project: room.project.clone(),
        name: room.name.clone(),
        slot: room.slot,
        cam: n + 1,
        row: desk.row,
        column: desk.column,
        rows: room.rows,
        model: short_model(desk.model.as_deref(), desk.model_id.as_deref()),
        kind: flag.kind,
        since: flag.since,
    }
}

/// A model's short name, as the focus monitors and the ticker write it: its display name
/// (or its id, for a model the registry does not know) in capitals, words joined by "-"
/// ("Opus 5.5" is "OPUS-5.5"); empty before the agent has answered.
pub fn short_model(model: Option<&str>, id: Option<&str>) -> String {
    model
        .or(id)
        .unwrap_or_default()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("-")
        .to_uppercase()
}

/// The monitor wall's columns for `rooms` open rooms: rows of [`COLUMNS`] up to
/// [`MAX_ROWS`] rows, then one more column per [`MAX_ROWS`] more rooms, with no cap.
fn columns(rooms: usize) -> usize {
    COLUMNS.max(rooms.div_ceil(MAX_ROWS))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::floor_plan::{layout, Desk, DeskId};
    use agents::{AgentState, Flag};

    #[test]
    fn with_no_open_project_the_wall_shows_no_signal() {
        let mut cctv = CctvState::default();
        cctv.plan(&FloorPlan::default());
        assert!(cctv.wall().no_signal());
        assert!(cctv.wall().feeds.is_empty());
    }

    fn room(project: &str, slot: usize) -> Room {
        Room {
            project: format!("-{project}"),
            name: project.into(),
            slot,
            rows: layout::MIN_ROWS,
            desks: Vec::new(),
        }
    }

    /// A plan of `count` rooms named `p0`, `p1`, … in corridor order, their slots
    /// scrambled as rooms that opened and closed leave them.
    fn plan_of(count: usize) -> FloorPlan {
        FloorPlan {
            rooms: (0..count)
                .map(|n| room(&format!("p{n}"), (n * 7 + 3) % count.max(1) + 100 * (n % 2)))
                .collect(),
        }
    }

    /// Each feed as (name, CAM, column, row).
    fn feeds(cctv: &CctvState) -> Vec<(String, usize, usize, usize)> {
        cctv.wall()
            .feeds
            .iter()
            .map(|f| (f.name.clone(), f.cam, f.column, f.row))
            .collect()
    }

    #[test]
    fn feeds_fill_rows_of_four_in_corridor_order_left_to_right_then_top_to_bottom() {
        let mut cctv = CctvState::default();
        cctv.plan(&plan_of(1));
        assert!(!cctv.wall().no_signal());
        assert_eq!((cctv.wall().columns, cctv.wall().rows), (4, 1));
        assert_eq!(feeds(&cctv), vec![("p0".into(), 1, 0, 0)]);

        cctv.plan(&plan_of(4));
        assert_eq!((cctv.wall().columns, cctv.wall().rows), (4, 1));
        cctv.plan(&plan_of(5));
        assert_eq!((cctv.wall().columns, cctv.wall().rows), (4, 2));
        assert_eq!(feeds(&cctv)[4], ("p4".into(), 5, 0, 1));
        cctv.plan(&plan_of(9));
        assert_eq!((cctv.wall().columns, cctv.wall().rows), (4, 3));

        cctv.plan(&plan_of(12));
        assert_eq!((cctv.wall().columns, cctv.wall().rows), (4, 3));
        let expected: Vec<_> = [
            (1, 0, 0),
            (2, 1, 0),
            (3, 2, 0),
            (4, 3, 0),
            (5, 0, 1),
            (6, 1, 1),
            (7, 2, 1),
            (8, 3, 1),
            (9, 0, 2),
            (10, 1, 2),
            (11, 2, 2),
            (12, 3, 2),
        ]
        .into_iter()
        .map(|(cam, column, row)| (format!("p{}", cam - 1), cam, column, row))
        .collect();
        assert_eq!(feeds(&cctv), expected);

        // Each feed is its room's: the project and slot it shows, whatever the slot.
        let plan = plan_of(12);
        for (feed, room) in cctv.wall().feeds.iter().zip(&plan.rooms) {
            assert_eq!((&feed.project, feed.slot), (&room.project, room.slot));
        }

        // Rooms closing shrink the wall back, and the rest close up in corridor order.
        cctv.plan(&plan_of(6));
        assert_eq!((cctv.wall().columns, cctv.wall().rows), (4, 2));
        cctv.plan(&FloorPlan::default());
        assert!(cctv.wall().no_signal());
    }

    #[test]
    fn past_twelve_rooms_the_wall_stays_three_rows_high_and_gains_a_column_per_three_rooms() {
        let mut cctv = CctvState::default();
        for (rooms, columns) in [
            (13, 5),
            (14, 5),
            (15, 5),
            (16, 6),
            (18, 6),
            (19, 7),
            (24, 8),
        ] {
            cctv.plan(&plan_of(rooms));
            let wall = cctv.wall();
            assert_eq!((wall.columns, wall.rows), (columns, 3), "{rooms} rooms");
            assert_eq!(wall.feeds.len(), rooms, "every room has its feed at once");
        }
        // Thirteen rooms: rows of five in corridor order, the last row short.
        cctv.plan(&plan_of(13));
        let places: Vec<_> = feeds(&cctv)
            .into_iter()
            .map(|(_, cam, c, r)| (cam, c, r))
            .collect();
        assert_eq!(
            places,
            vec![
                (1, 0, 0),
                (2, 1, 0),
                (3, 2, 0),
                (4, 3, 0),
                (5, 4, 0),
                (6, 0, 1),
                (7, 1, 1),
                (8, 2, 1),
                (9, 3, 1),
                (10, 4, 1),
                (11, 0, 2),
                (12, 1, 2),
                (13, 2, 2),
            ]
        );
        // Twenty-four: eight columns, the last feed bottom right.
        cctv.plan(&plan_of(24));
        assert_eq!(feeds(&cctv)[23], ("p23".into(), 24, 7, 2));
        // Rooms closing narrow it back, down to rows of four again.
        cctv.plan(&plan_of(16));
        assert_eq!((cctv.wall().columns, cctv.wall().rows), (6, 3));
        cctv.plan(&plan_of(12));
        assert_eq!((cctv.wall().columns, cctv.wall().rows), (4, 3));
    }

    #[test]
    fn each_extra_column_widens_the_room_by_a_column_half_on_each_side_and_closing_narrows_it() {
        let mut cctv = CctvState::default();
        // Up to twelve rooms the room is 8 m wide inside (side walls at ±4).
        for rooms in [0, 1, 6, 12] {
            cctv.plan(&plan_of(rooms));
            assert_eq!(cctv.wall().extra_columns(), 0, "{rooms} rooms");
            assert_eq!(cctv.wall().half_width(), 4.0, "{rooms} rooms");
            assert_eq!(room_half_width(rooms), 4.0, "{rooms} rooms");
        }
        // Each column past four adds 4/3 m, 2/3 m on each side.
        for (rooms, extra, half) in [
            (13, 1, 4.0 + 2.0 / 3.0),
            (15, 1, 4.0 + 2.0 / 3.0),
            (16, 2, 4.0 + 4.0 / 3.0),
            (24, 4, 4.0 + 8.0 / 3.0),
            (40, 10, 4.0 + 20.0 / 3.0),
        ] {
            cctv.plan(&plan_of(rooms));
            assert_eq!(cctv.wall().extra_columns(), extra, "{rooms} rooms");
            assert!(
                (cctv.wall().half_width() - half).abs() < 1e-5,
                "{rooms} rooms"
            );
            assert_eq!(room_half_width(rooms), cctv.wall().half_width());
            assert_eq!(room_extra_columns(rooms), extra);
        }
        // Closing rooms narrows it back.
        cctv.plan(&plan_of(14));
        assert!((cctv.wall().half_width() - (4.0 + 2.0 / 3.0)).abs() < 1e-5);
        cctv.plan(&plan_of(3));
        assert_eq!(cctv.wall().half_width(), 4.0);
    }

    /// The projects whose feeds refresh over `frames` frames, frame by frame.
    fn turns(cctv: &mut CctvState, frames: usize) -> Vec<Vec<String>> {
        (0..frames).map(|_| cctv.refresh()).collect()
    }

    fn times(turns: &[Vec<String>], project: &str) -> usize {
        turns.iter().flatten().filter(|p| *p == project).count()
    }

    #[test]
    fn one_feed_refreshes_each_frame_in_turns_so_each_of_twelve_updates_five_times_a_second() {
        let mut cctv = CctvState::default();
        cctv.plan(&plan_of(12));
        let round = turns(&mut cctv, 12);
        assert!(round.iter().all(|frame| frame.len() == 1), "{round:?}");
        for room in &plan_of(12).rooms {
            assert_eq!(times(&round, &room.project), 1, "{round:?}");
        }
        // At 60 frames a second, each of twelve feeds updates 5 times a second.
        let second = turns(&mut cctv, 60);
        for room in &plan_of(12).rooms {
            assert_eq!(times(&second, &room.project), 5);
        }
    }

    #[test]
    fn a_wider_wall_keeps_one_feed_a_frame_so_each_of_twenty_four_updates_less_often() {
        let mut cctv = CctvState::default();
        cctv.plan(&plan_of(24));
        let round = turns(&mut cctv, 24);
        assert!(round.iter().all(|frame| frame.len() == 1), "{round:?}");
        for room in &plan_of(24).rooms {
            assert_eq!(times(&round, &room.project), 1, "{round:?}");
        }
        // At 60 frames a second, each of 24 feeds updates 2.5 times a second.
        let seconds = turns(&mut cctv, 120);
        for room in &plan_of(24).rooms {
            assert_eq!(times(&seconds, &room.project), 5);
        }
    }

    #[test]
    fn seen_only_through_its_doorway_the_wall_refreshes_every_other_frame_still_in_turns() {
        let mut cctv = CctvState::default();
        cctv.plan(&plan_of(24));
        let frames: Vec<_> = (0..48).map(|_| cctv.refresh_through_doorway()).collect();
        // A feed every other frame, never two at once.
        for (n, frame) in frames.iter().enumerate() {
            assert_eq!(frame.len(), (n + 1) % 2, "frame {n}: {frames:?}");
        }
        for room in &plan_of(24).rooms {
            assert_eq!(times(&frames, &room.project), 1, "{frames:?}");
        }
        // Walking in, the turns go on where they were, a feed every frame.
        let inside = turns(&mut cctv, 24);
        assert!(inside.iter().all(|f| f.len() == 1), "{inside:?}");
        assert_eq!(inside[0], vec!["-p0".to_string()]);
        // With no feed a frame, none through the doorway either.
        cctv.set_refresh_per_frame(0);
        assert!((0..4).all(|_| cctv.refresh_through_doorway().is_empty()));
    }

    #[test]
    fn a_short_wall_refreshes_each_feed_every_fourth_frame_at_most_and_no_signal_nothing() {
        // Owner-run-1: one room of 41 desks, its feed redrawn every frame, cost the
        // CCTV room a quarter of its frame rate; every fourth frame is 15 times a second
        // at 60 frames a second, 5 at 20, as the spec's "5–10 times a second" asks.
        let mut cctv = CctvState::default();
        assert!(turns(&mut cctv, 3).iter().all(Vec::is_empty));
        cctv.plan(&plan_of(1));
        let frames = turns(&mut cctv, 8);
        assert_eq!(times(&frames, "-p0"), 2, "{frames:?}");
        assert!(frames.iter().all(|f| f.len() <= 1), "{frames:?}");
        let drawn: Vec<usize> = (0..8).filter(|&n| !frames[n].is_empty()).collect();
        assert_eq!(drawn[1] - drawn[0], 4, "{frames:?}");
        cctv.plan(&plan_of(3));
        let frames = turns(&mut cctv, 8);
        assert!(frames.iter().all(|f| f.len() <= 1), "{frames:?}");
        for room in &plan_of(3).rooms {
            assert_eq!(times(&frames, &room.project), 2, "{frames:?}");
        }
        // From four feeds on, one every frame, as before.
        cctv.plan(&plan_of(4));
        let frames = turns(&mut cctv, 8);
        assert!(frames.iter().all(|f| f.len() == 1), "{frames:?}");
        cctv.plan(&FloorPlan::default());
        assert!(cctv.refresh().is_empty());
    }

    #[test]
    fn a_room_that_opens_gets_its_turn_within_a_round_of_the_wall() {
        let mut cctv = CctvState::default();
        cctv.plan(&plan_of(11));
        turns(&mut cctv, 5);
        cctv.plan(&plan_of(12));
        assert_eq!(times(&turns(&mut cctv, 12), "-p11"), 1);
        cctv.plan(&plan_of(5));
        let frames = turns(&mut cctv, 5);
        for room in &plan_of(5).rooms {
            assert!(times(&frames, &room.project) >= 1, "{frames:?}");
        }
    }

    fn desk(agent: &str, depth: usize, flag: Option<(FlagKind, i64)>) -> Desk {
        Desk {
            id: DeskId {
                session: format!("s-{agent}"),
                agent: if depth == 0 {
                    format!("s-{agent}")
                } else {
                    agent.into()
                },
            },
            row: 0,
            column: 1,
            depth,
            model: Some("Opus 5.5".into()),
            model_id: Some("claude-opus-5-5".into()),
            flag: flag.map(|(kind, since)| Flag { kind, since }),
            // A flagged root agent waits (or is idle); the rest work.
            state: match flag {
                Some((FlagKind::AskingYouSomething, _)) => AgentState::Waiting,
                Some(_) => AgentState::Idle,
                None if depth == 0 => AgentState::Busy,
                None => AgentState::Running,
            },
            short_id: None,
        }
    }

    fn room_with(project: &str, desks: Vec<Desk>) -> Room {
        Room {
            desks,
            ..room(project, 0)
        }
    }

    const ASKING: FlagKind = FlagKind::AskingYouSomething;
    const WAITING: FlagKind = FlagKind::WaitingOnYou;

    #[test]
    fn a_feed_takes_its_flagged_agents_colour_and_red_wins_over_amber() {
        let mut cctv = CctvState::default();
        cctv.plan(&FloorPlan {
            rooms: vec![
                room_with("quiet", vec![desk("busy", 0, None), desk("sub", 1, None)]),
                room_with(
                    "amber",
                    vec![desk("busy", 0, None), desk("w", 0, Some((WAITING, 5)))],
                ),
                room_with(
                    "both",
                    vec![
                        desk("w1", 0, Some((WAITING, 1))),
                        desk("a", 0, Some((ASKING, 9))),
                        desk("w2", 0, Some((WAITING, 2))),
                    ],
                ),
                room_with("red", vec![desk("a", 0, Some((ASKING, 3)))]),
            ],
        });
        let flags: Vec<_> = cctv.wall().feeds.iter().map(|f| f.flag).collect();
        assert_eq!(flags, vec![None, Some(WAITING), Some(ASKING), Some(ASKING)]);

        // The colour follows the next plan: the answered room loses it.
        cctv.plan(&FloorPlan {
            rooms: vec![room_with("red", vec![desk("a", 0, None)])],
        });
        assert_eq!(cctv.wall().feeds[0].flag, None);
    }

    /// The focus monitors' agents, by agent id, or `-` for NO SIGNAL.
    fn focused(cctv: &CctvState) -> [String; 2] {
        cctv.focus()
            .map(|f| f.map_or("-".to_string(), |f| f.desk.agent.clone()))
    }

    #[test]
    fn focus_monitors_show_asking_before_waiting_then_the_longest_waited() {
        let mut cctv = CctvState::default();
        cctv.plan(&FloorPlan {
            rooms: vec![
                room_with("p0", vec![desk("w-long", 0, Some((WAITING, 1_000)))]),
                room_with(
                    "p1",
                    vec![
                        desk("busy", 0, None),
                        desk("a-late", 0, Some((ASKING, 50_000))),
                    ],
                ),
                room_with("p2", vec![desk("a-early", 0, Some((ASKING, 20_000)))]),
            ],
        });
        assert_eq!(focused(&cctv), ["s-a-early", "s-a-late"]);

        // Once one is answered, the longest-waiting agent of the other kind moves up.
        cctv.plan(&FloorPlan {
            rooms: vec![
                room_with("p0", vec![desk("w-long", 0, Some((WAITING, 1_000)))]),
                room_with("p1", vec![desk("a-late", 0, Some((ASKING, 50_000)))]),
                room_with("p2", vec![desk("a-early", 0, None)]),
            ],
        });
        assert_eq!(focused(&cctv), ["s-a-late", "s-w-long"]);

        // Each focus monitor shows what it needs: the room, its feed and the agent's seat,
        // its model's short name and its flag.
        let [Some(first), _] = cctv.focus() else {
            panic!("a focused agent")
        };
        assert_eq!(
            (
                first.project.as_str(),
                first.name.as_str(),
                first.slot,
                first.cam
            ),
            ("-p1", "p1", 0, 2)
        );
        assert_eq!((first.row, first.column), (0, 1));
        assert_eq!(first.model, "OPUS-5.5");
        assert_eq!((first.kind, first.since), (ASKING, 50_000));
    }

    #[test]
    fn equal_waits_go_in_corridor_then_desk_order_and_nobody_shows_twice() {
        let mut cctv = CctvState::default();
        let plan = |rooms| FloorPlan { rooms };
        cctv.plan(&plan(vec![
            room_with(
                "p0",
                vec![desk("b", 0, None), desk("c", 0, Some((WAITING, 7)))],
            ),
            room_with("p1", vec![desk("a", 0, Some((WAITING, 7)))]),
        ]));
        assert_eq!(focused(&cctv), ["s-c", "s-a"]);
        cctv.plan(&plan(vec![
            room_with(
                "p0",
                vec![
                    desk("x", 0, Some((WAITING, 7))),
                    desk("y", 0, Some((WAITING, 7))),
                ],
            ),
            room_with("p1", vec![desk("a", 0, Some((WAITING, 7)))]),
        ]));
        assert_eq!(focused(&cctv), ["s-x", "s-y"]);

        // One flagged agent fills the first monitor only; nobody, neither.
        cctv.plan(&plan(vec![room_with(
            "p0",
            vec![desk("x", 0, Some((ASKING, 7)))],
        )]));
        assert_eq!(focused(&cctv), ["s-x", "-"]);
        cctv.plan(&plan(vec![room_with("p0", vec![desk("x", 0, None)])]));
        assert_eq!(focused(&cctv), ["-", "-"]);
        cctv.plan(&FloorPlan::default());
        assert_eq!(focused(&cctv), ["-", "-"]);
    }

    #[test]
    fn the_ticker_lists_every_flagged_agent_with_project_model_flag_and_time_most_urgent_first() {
        let mut cctv = CctvState::default();
        assert!(cctv.ticker().is_empty());
        let mut sonnet = desk("s", 0, Some((WAITING, 300)));
        sonnet.model = Some("Sonnet 5.5".into());
        let mut unknown = desk("u", 0, Some((ASKING, 900)));
        (unknown.model, unknown.model_id) = (None, Some("gpt-6.1-sol".into()));
        cctv.plan(&FloorPlan {
            rooms: vec![
                room_with("alpha", vec![desk("busy", 0, None), sonnet]),
                room_with(
                    "beta",
                    vec![
                        desk("a", 0, Some((ASKING, 600))),
                        desk("w", 0, Some((WAITING, 100))),
                    ],
                ),
                room_with("gamma", vec![unknown, desk("sub", 1, None)]),
            ],
        });
        let entries: Vec<_> = cctv
            .ticker()
            .iter()
            .map(|f| (f.name.as_str(), f.model.as_str(), f.kind, f.since))
            .collect();
        assert_eq!(
            entries,
            vec![
                ("beta", "OPUS-5.5", ASKING, 600),
                ("gamma", "GPT-6.1-SOL", ASKING, 900),
                ("beta", "OPUS-5.5", WAITING, 100),
                ("alpha", "SONNET-5.5", WAITING, 300),
            ]
        );
        // More than the two focus monitors hold: the ticker leaves none out.
        assert_eq!(focused(&cctv), ["s-a", "s-u"]);

        cctv.plan(&FloorPlan {
            rooms: vec![room_with("alpha", vec![desk("busy", 0, None)])],
        });
        assert!(cctv.ticker().is_empty());
    }

    #[test]
    fn a_models_short_name_is_its_name_in_capitals_with_dashes_or_its_id_or_nothing_yet() {
        assert_eq!(
            short_model(Some("Opus 5.5"), Some("claude-opus-5-5")),
            "OPUS-5.5"
        );
        assert_eq!(short_model(Some(" Haiku  4.5 "), None), "HAIKU-4.5");
        assert_eq!(short_model(None, Some("grok-5")), "GROK-5");
        assert_eq!(short_model(None, None), "");
    }

    #[test]
    fn each_focus_monitor_with_an_agent_refreshes_seven_and_a_half_times_a_second_never_both_in_one_frame(
    ) {
        let mut cctv = CctvState::default();
        let frames =
            |cctv: &mut CctvState, n| (0..n).map(|_| cctv.refresh_focus()).collect::<Vec<_>>();
        assert!(frames(&mut cctv, 8).iter().all(|f| f == &[false, false]));
        cctv.plan(&FloorPlan {
            rooms: vec![room_with(
                "p0",
                vec![
                    desk("a", 0, Some((ASKING, 1))),
                    desk("w", 0, Some((WAITING, 1))),
                ],
            )],
        });
        // Two seconds at 60 frames a second: 15 draws each, never two in one frame and
        // at most one focus draw in any four frames in a row.
        let two = frames(&mut cctv, 120);
        assert!(two.iter().all(|f| !(f[0] && f[1])), "{two:?}");
        assert_eq!(two.iter().filter(|f| f[0]).count(), 15);
        assert_eq!(two.iter().filter(|f| f[1]).count(), 15);
        for window in two.windows(4) {
            assert!(
                window.iter().filter(|f| f[0] || f[1]).count() <= 1,
                "{two:?}"
            );
        }

        // A monitor showing NO SIGNAL draws nothing.
        cctv.plan(&FloorPlan {
            rooms: vec![room_with("p0", vec![desk("a", 0, Some((ASKING, 1)))])],
        });
        let two = frames(&mut cctv, 120);
        assert_eq!(two.iter().filter(|f| f[0]).count(), 15);
        assert!(two.iter().all(|f| !f[1]));
    }

    /// Three rooms: Alpha with a busy agent and one waiting on you since 1 000; Beta with
    /// one asking since 5 000 and one waiting since 2 000; Gamma all busy.
    fn console_plan() -> FloorPlan {
        FloorPlan {
            rooms: vec![
                room_with(
                    "alpha",
                    vec![
                        desk("a-busy", 0, None),
                        desk("a-wait", 0, Some((WAITING, 1_000))),
                    ],
                ),
                room_with(
                    "beta",
                    vec![
                        desk("b-ask", 0, Some((ASKING, 5_000))),
                        desk("b-wait", 0, Some((WAITING, 2_000))),
                    ],
                ),
                room_with("gamma", vec![desk("g-busy", 0, None)]),
            ],
        }
    }

    fn id(agent: &str) -> DeskId {
        DeskId {
            session: format!("s-{agent}"),
            agent: format!("s-{agent}"),
        }
    }

    /// The console's rows as (agent, selected).
    fn rows(cctv: &CctvState) -> Vec<(String, bool)> {
        cctv.console()
            .rows
            .iter()
            .map(|row| (row.agent.desk.agent.clone(), row.selected))
            .collect()
    }

    #[test]
    fn the_console_lists_flagged_agents_under_select_target_and_nothing_is_selected_until_a_click()
    {
        let mut cctv = CctvState::default();
        cctv.plan(&console_plan());
        let console = cctv.console();
        assert_eq!(console.heading, Words::SelectTarget);
        // Every flagged agent, most urgent first, as on the ticker; nobody selected.
        assert_eq!(
            rows(&cctv),
            [
                ("s-b-ask".to_string(), false),
                ("s-a-wait".to_string(), false),
                ("s-b-wait".to_string(), false),
            ]
        );
        // Each row has what it shows: the room, the model, the flag's words and since when.
        let first = &cctv.console().rows[0];
        assert_eq!(
            (first.agent.name.as_str(), first.agent.model.as_str()),
            ("beta", "OPUS-5.5")
        );
        assert_eq!(first.words, Words::Flag(ASKING));
        assert_eq!(first.timer, Some(5_000));
        assert_eq!(cctv.selected(), None);
        assert_eq!(console.kill, Button::Dark);
        assert_eq!(console.work_harder, Button::Dark);
    }

    #[test]
    fn clicking_a_console_row_selects_its_agent_highlights_the_row_and_puts_it_on_focus_monitor_one(
    ) {
        let mut cctv = CctvState::default();
        cctv.plan(&console_plan());
        assert_eq!(focused(&cctv), ["s-b-ask", "s-a-wait"]);
        cctv.click(&Target::Row(id("b-wait")), 10_000);
        assert_eq!(cctv.selected(), Some(&id("b-wait")));
        assert_eq!(
            rows(&cctv),
            [
                ("s-b-ask".to_string(), false),
                ("s-a-wait".to_string(), false),
                ("s-b-wait".to_string(), true),
            ]
        );
        // It leads focus monitor 1; the most urgent of the rest follow, without repeats.
        assert_eq!(focused(&cctv), ["s-b-wait", "s-b-ask"]);
        // With an agent selected, both buttons are ready.
        assert_eq!(cctv.console().kill, Button::Lit);
        assert_eq!(cctv.console().work_harder, Button::Lit);
        // Another row moves the selection.
        cctv.click(&Target::Row(id("a-wait")), 11_000);
        assert_eq!(cctv.selected(), Some(&id("a-wait")));
        assert_eq!(focused(&cctv), ["s-a-wait", "s-b-ask"]);
    }

    #[test]
    fn a_flagged_wall_feed_selects_its_top_ranked_agent_and_an_unflagged_one_selects_nobody() {
        let mut cctv = CctvState::default();
        cctv.plan(&console_plan());
        // Beta has an agent asking and one waiting: the asking one is its top-ranked.
        cctv.click(&Target::Feed("-beta".into()), 0);
        assert_eq!(cctv.selected(), Some(&id("b-ask")));
        cctv.click(&Target::Feed("-alpha".into()), 0);
        assert_eq!(cctv.selected(), Some(&id("a-wait")));
        // Gamma's agents all work: its feed selects nobody and keeps the selection.
        cctv.click(&Target::Feed("-gamma".into()), 0);
        assert_eq!(cctv.selected(), Some(&id("a-wait")));
        // As does a feed of no room on the wall.
        cctv.click(&Target::Feed("-gone".into()), 0);
        assert_eq!(cctv.selected(), Some(&id("a-wait")));
    }

    #[test]
    fn a_focus_monitor_selects_the_agent_it_shows_and_one_showing_no_signal_selects_nobody() {
        let mut cctv = CctvState::default();
        cctv.plan(&console_plan());
        assert_eq!(focused(&cctv), ["s-b-ask", "s-a-wait"]);
        cctv.click(&Target::Focus(1), 0);
        assert_eq!(cctv.selected(), Some(&id("a-wait")));
        // The selected agent moves up to focus monitor 1.
        assert_eq!(focused(&cctv), ["s-a-wait", "s-b-ask"]);
        cctv.click(&Target::Focus(1), 0);
        assert_eq!(cctv.selected(), Some(&id("b-ask")));

        cctv.plan(&FloorPlan {
            rooms: vec![room_with(
                "beta",
                vec![desk("b-ask", 0, Some((ASKING, 5_000)))],
            )],
        });
        assert_eq!(focused(&cctv), ["s-b-ask", "-"]);
        cctv.click(&Target::Focus(1), 0);
        assert_eq!(cctv.selected(), Some(&id("b-ask")));
        cctv.click(&Target::Focus(7), 0);
        assert_eq!(cctv.selected(), Some(&id("b-ask")));
    }

    #[test]
    fn only_flagged_agents_are_selectable() {
        let mut cctv = CctvState::default();
        cctv.plan(&console_plan());
        // A row naming an agent that is busy, or that is nowhere, selects nobody.
        cctv.click(&Target::Row(id("a-busy")), 0);
        cctv.click(&Target::Row(id("nobody")), 0);
        assert_eq!(cctv.selected(), None);
        // The controls select nobody either, and do nothing yet without a selection.
        for target in [Target::Cover, Target::Kill, Target::WorkHarder] {
            cctv.click(&target, 0);
        }
        assert_eq!(cctv.selected(), None);
        assert_eq!(focused(&cctv), ["s-b-ask", "s-a-wait"]);
    }

    #[test]
    fn the_selection_clears_when_its_agent_loses_its_flag_and_stays_while_it_keeps_one() {
        let mut cctv = CctvState::default();
        cctv.plan(&console_plan());
        cctv.click(&Target::Row(id("b-wait")), 0);
        // Its flag changes kind and time: still flagged, still selected.
        let mut plan = console_plan();
        plan.rooms[1].desks[1].flag = Some(Flag {
            kind: ASKING,
            since: 9_000,
        });
        cctv.plan(&plan);
        assert_eq!(cctv.selected(), Some(&id("b-wait")));
        assert_eq!(focused(&cctv), ["s-b-wait", "s-b-ask"]);
        // Answered: the selection clears, and the focus monitors go back to the ranking.
        plan.rooms[1].desks[1].flag = None;
        cctv.plan(&plan);
        assert_eq!(cctv.selected(), None);
        assert!(rows(&cctv).iter().all(|(_, selected)| !selected));
        assert_eq!(focused(&cctv), ["s-b-ask", "s-a-wait"]);
        assert_eq!(cctv.console().kill, Button::Dark);
        // Gone with its room, too.
        cctv.click(&Target::Row(id("a-wait")), 0);
        cctv.plan(&FloorPlan {
            rooms: vec![console_plan().rooms[1].clone()],
        });
        assert_eq!(cctv.selected(), None);
    }

    #[test]
    fn with_nobody_flagged_the_console_reads_all_agents_working_with_both_buttons_dark() {
        let mut cctv = CctvState::default();
        let console = cctv.console();
        assert_eq!(console.heading, Words::AllAgentsWorking);
        assert!(console.rows.is_empty());
        assert_eq!(
            (console.kill, console.work_harder),
            (Button::Dark, Button::Dark)
        );
        cctv.plan(&console_plan());
        cctv.click(&Target::Row(id("a-wait")), 0);
        cctv.plan(&FloorPlan {
            rooms: vec![room_with("gamma", vec![desk("g-busy", 0, None)])],
        });
        let console = cctv.console();
        assert_eq!(console.heading, Words::AllAgentsWorking);
        assert!(console.rows.is_empty());
        assert_eq!(console.hidden, 0);
        assert_eq!(
            (console.kill, console.work_harder),
            (Button::Dark, Button::Dark)
        );
    }

    #[test]
    fn a_long_list_shows_the_first_rows_counts_the_rest_and_keeps_the_selection_in_view() {
        let mut cctv = CctvState::default();
        let desks = (0..9)
            .map(|n| desk(&format!("w{n}"), 0, Some((WAITING, n as i64))))
            .collect();
        cctv.plan(&FloorPlan {
            rooms: vec![room_with("big", desks)],
        });
        let shown: Vec<String> = rows(&cctv).into_iter().map(|(agent, _)| agent).collect();
        assert_eq!(shown.len(), CONSOLE_ROWS);
        assert_eq!(shown[0], "s-w0");
        assert_eq!(cctv.console().hidden, 9 - CONSOLE_ROWS);
        // An agent past the last row, selected from its feed or a focus monitor, takes
        // the last row's place so its highlight shows.
        cctv.click(&Target::Feed("-big".into()), 0);
        cctv.click(&Target::Row(id("w8")), 0);
        assert_eq!(cctv.selected(), Some(&id("w8")));
        let rows = rows(&cctv);
        assert_eq!(rows.len(), CONSOLE_ROWS);
        assert_eq!(rows[CONSOLE_ROWS - 1], ("s-w8".to_string(), true));
        assert_eq!(rows[CONSOLE_ROWS - 2].0, format!("s-w{}", CONSOLE_ROWS - 2));
        assert_eq!(cctv.console().hidden, 9 - CONSOLE_ROWS);
    }

    #[test]
    fn more_feeds_a_frame_close_the_turns_up_and_none_stops_them() {
        let mut cctv = CctvState::default();
        cctv.set_refresh_per_frame(2);
        cctv.plan(&plan_of(12));
        let frames = turns(&mut cctv, 6);
        assert!(
            frames.iter().all(|f| f.len() == 2 && f[0] != f[1]),
            "{frames:?}"
        );
        for room in &plan_of(12).rooms {
            assert_eq!(times(&frames, &room.project), 1);
        }
        cctv.set_refresh_per_frame(0);
        assert!(cctv.refresh().is_empty());
    }

    /// Each punishment playing, newest first, as (agent, kind, phase).
    fn playing(cctv: &CctvState) -> Vec<(String, Kind, Phase)> {
        cctv.punishments()
            .iter()
            .map(|p| (p.agent.desk.agent.clone(), p.kind, p.phase))
            .collect()
    }

    /// The console's row of `agent`: its words and timer.
    fn row_of(cctv: &CctvState, agent: &str) -> (Words, Option<i64>) {
        let console = cctv.console();
        let row = console
            .rows
            .iter()
            .find(|row| row.agent.desk == id(agent))
            .expect("a row");
        (row.words, row.timer)
    }

    #[test]
    fn work_harder_fires_in_one_click_on_the_selected_agent() {
        let mut cctv = CctvState::default();
        cctv.plan(&console_plan());
        // With nobody selected, the bar does nothing.
        cctv.click(&Target::WorkHarder, 10_000);
        assert!(cctv.punishments().is_empty());

        cctv.click(&Target::Row(id("b-wait")), 10_000);
        cctv.click(&Target::WorkHarder, 10_100);
        assert_eq!(
            playing(&cctv),
            [("s-b-wait".to_string(), Kind::WorkHarder, Phase::Crackle)]
        );
        let strike = &cctv.punishments()[0];
        assert_eq!(strike.began, 10_100);
        // Its room and desk, for the world to put the lightning over the robot.
        assert_eq!(
            (strike.agent.project.as_str(), strike.agent.slot),
            ("-beta", 0)
        );
        // The row reads LIGHTNING STRIKE, without a timer; the selection stays.
        assert_eq!(row_of(&cctv, "b-wait"), (Words::LightningStrike, None));
        assert_eq!(cctv.selected(), Some(&id("b-wait")));
        // Nothing else is punished.
        assert_eq!(row_of(&cctv, "b-ask"), (Words::Flag(ASKING), Some(5_000)));
    }

    #[test]
    fn work_harder_plays_about_two_seconds_of_lightning_then_done_for_two_on_the_worlds_clock() {
        let mut cctv = CctvState::default();
        cctv.plan(&console_plan());
        cctv.click(&Target::Row(id("b-wait")), 10_000);
        cctv.click(&Target::WorkHarder, 20_000);
        // Each phase in turn on the world's clock, with the row's words: crackle, the
        // bolt, a second charred, the wisp, then DONE; then the normal row again.
        for (at, phase, words) in [
            (20_000, Phase::Crackle, Words::LightningStrike),
            (20_499, Phase::Crackle, Words::LightningStrike),
            (20_500, Phase::Strike, Words::LightningStrike),
            (20_800, Phase::Charred, Words::LightningStrike),
            (21_799, Phase::Charred, Words::LightningStrike),
            (21_800, Phase::Wisp, Words::LightningStrike),
            (22_200, Phase::Done, Words::Done),
            (24_199, Phase::Done, Words::Done),
        ] {
            cctv.advance(at);
            assert_eq!(
                playing(&cctv),
                [("s-b-wait".to_string(), Kind::WorkHarder, phase)],
                "at {at}"
            );
            assert_eq!(row_of(&cctv, "b-wait"), (words, None), "at {at}");
        }
        // How far through its phase: half way through the second charred.
        cctv.advance(21_300);
        assert!((cctv.punishments()[0].progress - 0.5).abs() < 1e-6);
        cctv.advance(24_200);
        assert!(cctv.punishments().is_empty());
        assert_eq!(row_of(&cctv, "b-wait"), (Words::Flag(WAITING), Some(2_000)));
        // It ends on the clock alone, however long between two moments of it: the world
        // may not have drawn a frame meanwhile (not seen, kept warm or suspended).
        cctv.click(&Target::WorkHarder, 30_000);
        assert_eq!(playing(&cctv).len(), 1);
        cctv.advance(90_000);
        assert!(cctv.punishments().is_empty());
    }

    /// The agents whose robot is charred now.
    fn charred(cctv: &CctvState) -> Vec<String> {
        cctv.charred()
            .iter()
            .map(|agent| agent.desk.agent.clone())
            .collect()
    }

    #[test]
    fn with_effects_off_work_harder_keeps_its_timings_and_words_and_only_chars_the_robot_for_a_second(
    ) {
        let mut cctv = CctvState::default();
        cctv.set_effects(false);
        cctv.plan(&console_plan());
        cctv.click(&Target::Row(id("b-wait")), 10_000);
        cctv.click(&Target::WorkHarder, 20_000);
        // The same moments and words as with effects on, but no crackle, bolt or wisp is
        // reported: the robot sits still, is charred for the second it would be, and is
        // itself again; then DONE.
        for (at, phase, words, chars) in [
            (20_000, Phase::Still, Words::LightningStrike, false),
            (20_500, Phase::Still, Words::LightningStrike, false),
            (20_799, Phase::Still, Words::LightningStrike, false),
            (20_800, Phase::Still, Words::LightningStrike, true),
            (21_799, Phase::Still, Words::LightningStrike, true),
            (21_800, Phase::Still, Words::LightningStrike, false),
            (22_200, Phase::Done, Words::Done, false),
            (24_199, Phase::Done, Words::Done, false),
        ] {
            cctv.advance(at);
            assert_eq!(
                playing(&cctv),
                [("s-b-wait".to_string(), Kind::WorkHarder, phase)],
                "at {at}"
            );
            assert_eq!(row_of(&cctv, "b-wait"), (words, None), "at {at}");
            let robots = if chars { vec!["s-b-wait"] } else { vec![] };
            assert_eq!(charred(&cctv), robots, "at {at}");
            assert_eq!(
                (cctv.console().kill, cctv.console().work_harder),
                (Button::Disabled, Button::Disabled),
                "at {at}"
            );
        }
        // How far through it, as with effects on: half way through the charred second.
        cctv.advance(21_300);
        assert!((cctv.punishments()[0].progress - 0.5).abs() < 1e-6);
        // It ends at the same moment, with the normal row back and nothing charred.
        cctv.advance(24_200);
        assert!(cctv.punishments().is_empty());
        assert!(charred(&cctv).is_empty());
        assert_eq!(row_of(&cctv, "b-wait"), (Words::Flag(WAITING), Some(2_000)));
    }

    #[test]
    fn one_punishment_per_agent_with_its_buttons_disabled_while_it_plays() {
        let mut cctv = CctvState::default();
        cctv.plan(&console_plan());
        cctv.click(&Target::Row(id("b-ask")), 10_000);
        assert_eq!(
            (cctv.console().kill, cctv.console().work_harder),
            (Button::Lit, Button::Lit)
        );
        cctv.click(&Target::WorkHarder, 10_000);
        assert_eq!(
            (cctv.console().kill, cctv.console().work_harder),
            (Button::Disabled, Button::Disabled)
        );
        // A second click on the bar, or one during DONE, starts nothing new.
        cctv.click(&Target::WorkHarder, 11_000);
        cctv.click(&Target::WorkHarder, 13_000);
        assert_eq!(cctv.punishments().len(), 1);
        assert_eq!(cctv.punishments()[0].began, 10_000);
        // Another agent's buttons are ready meanwhile; back on the first, disabled.
        cctv.click(&Target::Row(id("a-wait")), 13_100);
        assert_eq!(cctv.console().work_harder, Button::Lit);
        cctv.click(&Target::Row(id("b-ask")), 13_200);
        assert_eq!(cctv.console().work_harder, Button::Disabled);
        // Once it is over, the agent can be punished again.
        cctv.advance(14_200);
        assert_eq!(
            (cctv.console().kill, cctv.console().work_harder),
            (Button::Lit, Button::Lit)
        );
        cctv.click(&Target::WorkHarder, 14_300);
        assert_eq!(cctv.punishments()[0].began, 14_300);
    }

    #[test]
    fn several_agents_are_punished_at_once_and_punishments_playing_lead_the_focus_monitors_newest_first(
    ) {
        let mut cctv = CctvState::default();
        cctv.plan(&console_plan());
        assert_eq!(focused(&cctv), ["s-b-ask", "s-a-wait"]);
        // The least urgent agent punished first: it leads focus monitor 1 while it plays.
        cctv.click(&Target::Row(id("b-wait")), 10_000);
        cctv.click(&Target::WorkHarder, 10_000);
        cctv.click(&Target::Row(id("a-wait")), 10_500);
        // Then the selection, ahead of the ranking.
        assert_eq!(focused(&cctv), ["s-b-wait", "s-a-wait"]);
        cctv.click(&Target::WorkHarder, 11_000);
        // Two at once, in two rooms: the newest first.
        assert_eq!(
            playing(&cctv),
            [
                ("s-a-wait".to_string(), Kind::WorkHarder, Phase::Crackle),
                ("s-b-wait".to_string(), Kind::WorkHarder, Phase::Charred),
            ]
        );
        assert_eq!(focused(&cctv), ["s-a-wait", "s-b-wait"]);
        assert_eq!(row_of(&cctv, "a-wait").0, Words::LightningStrike);
        assert_eq!(row_of(&cctv, "b-wait").0, Words::LightningStrike);
        // The first ends; the second goes on; the focus monitors follow.
        cctv.advance(14_200);
        assert_eq!(
            playing(&cctv),
            [("s-a-wait".to_string(), Kind::WorkHarder, Phase::Done)]
        );
        assert_eq!(focused(&cctv), ["s-a-wait", "s-b-ask"]);
        cctv.advance(15_200);
        assert!(cctv.punishments().is_empty());
        // Back to the selection, then the ranking.
        assert_eq!(focused(&cctv), ["s-a-wait", "s-b-ask"]);
    }

    #[test]
    fn keep_or_clear_keeps_only_an_agent_whose_state_and_flag_have_not_changed() {
        let status = |state, flag: Option<(FlagKind, i64)>| Status {
            state,
            flag: flag.map(|(kind, since)| Flag { kind, since }),
        };
        let waiting = status(AgentState::Idle, Some((WAITING, 1_000)));
        let dialog = status(AgentState::Waiting, Some((WAITING, 1_000)));
        let asking = status(AgentState::Waiting, Some((ASKING, 5_000)));
        let busy = status(AgentState::Busy, None);
        let finished = status(AgentState::Finished, None);
        // No change keeps it.
        assert!(keep(&waiting, Some(&waiting)));
        assert!(keep(&asking, Some(&asking)));
        // Any change clears it: waiting → busy, asking → busy, waiting ↔ asking …
        assert!(!keep(&waiting, Some(&busy)));
        assert!(!keep(&asking, Some(&busy)));
        assert!(!keep(&waiting, Some(&asking)));
        assert!(!keep(&asking, Some(&waiting)));
        // … idle to blocked on a dialog, the same flag but another state …
        assert!(!keep(&waiting, Some(&dialog)));
        // … a new waiting reason, which restarts *asking you something*'s `since` …
        let new_reason = status(AgentState::Waiting, Some((ASKING, 9_000)));
        assert!(!keep(&asking, Some(&new_reason)));
        // … the agent finishing, and its desk gone.
        assert!(!keep(&waiting, Some(&finished)));
        assert!(!keep(&waiting, None));
    }

    /// The console plan with Beta's waiting agent (`b-wait`) changed by `change`.
    fn with_b_wait(change: impl FnOnce(&mut Desk)) -> FloorPlan {
        let mut plan = console_plan();
        change(&mut plan.rooms[1].desks[1]);
        plan
    }

    /// `b-wait` punished with work harder at 10 000, and the world at 10 600 (the bolt).
    fn striking_b_wait() -> CctvState {
        let mut cctv = CctvState::default();
        cctv.plan(&console_plan());
        cctv.click(&Target::Row(id("b-wait")), 10_000);
        cctv.click(&Target::WorkHarder, 10_000);
        cctv.advance(10_600);
        assert_eq!(playing(&cctv)[0].2, Phase::Strike);
        cctv
    }

    /// What the state machine has asked of real sessions since last asked, as
    /// (session, action).
    fn asked(cctv: &mut CctvState) -> Vec<(String, SessionAction)> {
        cctv.session_requests()
            .into_iter()
            .map(|request| (request.session, request.action))
            .collect()
    }

    #[test]
    fn work_harder_asks_once_at_the_bolt_for_its_words_to_be_sent_into_the_session() {
        let prompt = || vec![("s-b-wait".to_string(), SessionAction::WorkHarder)];
        let mut cctv = CctvState::default();
        cctv.plan(&console_plan());
        cctv.click(&Target::Row(id("b-wait")), 10_000);
        cctv.click(&Target::WorkHarder, 10_000);
        for at in [10_000, 10_250, 10_499] {
            cctv.advance(at);
            assert!(asked(&mut cctv).is_empty(), "at {at}");
        }
        cctv.advance(10_500);
        assert_eq!(asked(&mut cctv), prompt());
        // Once: never again through the charring, the wisp, DONE or after.
        for at in [10_501, 10_800, 12_200, 14_200, 60_000] {
            cctv.advance(at);
            assert!(asked(&mut cctv).is_empty(), "at {at}");
        }
        // With Punishment effects off, at the same moment.
        let mut cctv = CctvState::default();
        cctv.set_effects(false);
        cctv.plan(&console_plan());
        cctv.click(&Target::Row(id("b-wait")), 10_000);
        cctv.click(&Target::WorkHarder, 10_000);
        cctv.advance(10_499);
        assert!(asked(&mut cctv).is_empty());
        cctv.advance(10_500);
        assert_eq!(asked(&mut cctv), prompt());
        // A world that went without a frame past the whole strike still asks, once.
        let mut cctv = CctvState::default();
        cctv.plan(&console_plan());
        cctv.click(&Target::Row(id("b-wait")), 10_000);
        cctv.click(&Target::WorkHarder, 10_000);
        cctv.advance(90_000);
        assert_eq!(asked(&mut cctv), prompt());
        assert!(asked(&mut cctv).is_empty());
        // The next work harder on the same agent asks again, at its own bolt.
        cctv.click(&Target::WorkHarder, 100_000);
        cctv.advance(100_500);
        assert_eq!(asked(&mut cctv), prompt());
        // A kill asks to end the session, never for the prompt.
        let mut cctv = killing_b_wait();
        cctv.advance(60_000);
        assert_eq!(
            asked(&mut cctv),
            [("s-b-wait".to_string(), SessionAction::End)]
        );
    }

    #[test]
    fn a_strike_cleared_before_the_bolt_never_asks_and_one_cleared_after_asks_no_more() {
        // Answered during the crackle: the strike clears, and its bolt never comes.
        let mut cctv = CctvState::default();
        cctv.plan(&console_plan());
        cctv.click(&Target::Row(id("b-wait")), 10_000);
        cctv.click(&Target::WorkHarder, 10_000);
        cctv.advance(10_499);
        cctv.plan(&with_b_wait(|desk| {
            desk.flag = None;
            desk.state = AgentState::Busy;
        }));
        assert!(cctv.punishments().is_empty());
        cctv.advance(60_000);
        assert!(asked(&mut cctv).is_empty());
        // Its desk gone during the crackle: the same.
        let mut cctv = CctvState::default();
        cctv.plan(&console_plan());
        cctv.click(&Target::Row(id("b-wait")), 10_000);
        cctv.click(&Target::WorkHarder, 10_000);
        let mut plan = console_plan();
        plan.rooms[1].desks.remove(1);
        cctv.plan(&plan);
        cctv.advance(60_000);
        assert!(asked(&mut cctv).is_empty());
        // Cleared after the bolt: it asked at the bolt, and only then.
        let mut cctv = striking_b_wait();
        assert_eq!(
            asked(&mut cctv),
            [("s-b-wait".to_string(), SessionAction::WorkHarder)]
        );
        cctv.plan(&with_b_wait(|desk| desk.state = AgentState::Busy));
        cctv.advance(60_000);
        assert!(asked(&mut cctv).is_empty());
    }

    #[test]
    fn a_new_plan_keeps_a_punishment_while_its_agent_is_unchanged_and_clears_it_at_once_on_any_change(
    ) {
        // The same plan again, or one where only other agents change, keeps it.
        let mut cctv = striking_b_wait();
        cctv.plan(&console_plan());
        let mut plan = console_plan();
        plan.rooms[0].desks[1].flag = None;
        plan.rooms[0].desks[1].state = AgentState::Busy;
        cctv.plan(&plan);
        assert_eq!(playing(&cctv).len(), 1);

        let flag = |kind, since| Some(Flag { kind, since });
        type Change = Box<dyn Fn(&mut Desk)>;
        let changes: [(&str, Change); 5] = [
            (
                "waiting → busy",
                Box::new(|d: &mut Desk| {
                    d.state = AgentState::Busy;
                    d.flag = None;
                }),
            ),
            (
                "waiting → asking",
                Box::new(move |d: &mut Desk| {
                    d.state = AgentState::Waiting;
                    d.flag = flag(ASKING, 10_300);
                }),
            ),
            (
                "idle → a dialog, the same flag",
                Box::new(|d: &mut Desk| d.state = AgentState::Waiting),
            ),
            (
                "a new since",
                Box::new(move |d: &mut Desk| d.flag = flag(WAITING, 10_300)),
            ),
            (
                "finished",
                Box::new(|d: &mut Desk| {
                    d.state = AgentState::Finished;
                    d.flag = None;
                }),
            ),
        ];
        for (what, change) in changes {
            let mut cctv = striking_b_wait();
            cctv.plan(&with_b_wait(change));
            // It stops at once: nothing more to draw, and its row is a plain row (or gone).
            assert!(cctv.punishments().is_empty(), "{what}");
            let row = cctv
                .console()
                .rows
                .into_iter()
                .find(|row| row.agent.desk == id("b-wait"));
            assert!(
                row.is_none_or(|row| matches!(row.words, Words::Flag(_))),
                "{what}"
            );
            // It stays cleared: the clock running on brings nothing back.
            cctv.advance(11_000);
            assert!(cctv.punishments().is_empty(), "{what}");
        }

        // Its desk gone with its room.
        let mut cctv = striking_b_wait();
        let mut plan = console_plan();
        plan.rooms.remove(1);
        cctv.plan(&plan);
        assert!(cctv.punishments().is_empty());
        // Or its desk alone.
        let mut cctv = striking_b_wait();
        let mut plan = console_plan();
        plan.rooms[1].desks.remove(1);
        cctv.plan(&plan);
        assert!(cctv.punishments().is_empty());
    }

    #[test]
    fn a_moved_desk_keeps_its_punishment_which_follows_it() {
        let mut cctv = striking_b_wait();
        let punished = &cctv.punishments()[0].agent;
        assert_eq!((punished.cam, punished.row, punished.column), (2, 0, 1));
        // Alpha closes, so Beta moves up the corridor, and Beta's desks repack: the
        // punished agent's desk moves a row back.
        let mut plan = console_plan();
        plan.rooms.remove(0);
        plan.rooms[0].desks[1].row = 1;
        plan.rooms[0].desks[1].column = 3;
        cctv.plan(&plan);
        assert_eq!(
            playing(&cctv),
            [("s-b-wait".to_string(), Kind::WorkHarder, Phase::Strike)]
        );
        let punished = &cctv.punishments()[0];
        assert_eq!(
            (
                punished.agent.cam,
                punished.agent.row,
                punished.agent.column
            ),
            (1, 1, 3)
        );
        assert_eq!(punished.began, 10_000);
        // The focus monitor follows it too.
        let focus = cctv.focus()[0].expect("the punished agent");
        assert_eq!(
            (focus.desk.clone(), focus.row, focus.column),
            (id("b-wait"), 1, 3)
        );
    }

    /// The console's flip cover and KILL button now.
    fn cover(cctv: &CctvState) -> (Cover, Button) {
        let console = cctv.console();
        (console.cover, console.kill)
    }

    #[test]
    fn the_first_click_lifts_the_cover_which_drops_by_itself_after_three_seconds() {
        let mut cctv = CctvState::default();
        cctv.plan(&console_plan());
        // With nobody selected the cover stays down.
        cctv.click(&Target::Cover, 10_000);
        assert_eq!(cover(&cctv), (Cover::Down, Button::Dark));
        cctv.click(&Target::Row(id("b-wait")), 10_000);
        assert_eq!(cover(&cctv), (Cover::Down, Button::Lit));
        cctv.click(&Target::Cover, 11_000);
        assert_eq!(cover(&cctv), (Cover::Up, Button::Lit));
        // Another click on the open cover changes nothing.
        cctv.click(&Target::Cover, 12_000);
        cctv.advance(13_999);
        assert_eq!(cover(&cctv).0, Cover::Up);
        // Three seconds after it lifted, it drops by itself.
        cctv.advance(14_000);
        assert_eq!(cover(&cctv), (Cover::Down, Button::Lit));
        assert!(cctv.punishments().is_empty());
    }

    #[test]
    fn focus_monitor_1_shows_a_punishment_wide_and_every_frame_while_it_plays() {
        // Owner-run-1: a close-up drawn every eighth frame missed the 300 ms bolt at
        // 20 frames a second, and left the ceiling, the squad and the blast out of shot.
        let mut cctv = CctvState::default();
        cctv.plan(&console_plan());
        let frames =
            |cctv: &mut CctvState, n| (0..n).map(|_| cctv.refresh_focus()).collect::<Vec<_>>();
        let wide = |cctv: &CctvState| cctv.focus_views().map(|v| v.is_some_and(|v| v.wide));
        assert_eq!(wide(&cctv), [false, false]);
        cctv.click(&Target::Row(id("b-wait")), 10_000);
        cctv.click(&Target::WorkHarder, 10_100);
        let views = cctv.focus_views();
        assert_eq!(
            views[0].as_ref().map(|v| &v.agent.desk),
            Some(&id("b-wait"))
        );
        assert_eq!(wide(&cctv), [true, false]);
        // Every frame for focus monitor 1; the other keeps its turns.
        let sixteen = frames(&mut cctv, 16);
        assert!(sixteen.iter().all(|f| f[0]), "{sixteen:?}");
        assert_eq!(sixteen.iter().filter(|f| f[1]).count(), 2, "{sixteen:?}");
        // DONE is part of it; once it is over, the close-up and its turns again.
        cctv.advance(10_100 + Kind::WorkHarder.lasts() - 1);
        assert_eq!(wide(&cctv), [true, false]);
        cctv.advance(10_100 + Kind::WorkHarder.lasts());
        assert_eq!(wide(&cctv), [false, false]);
        assert_eq!(frames(&mut cctv, 16).iter().filter(|f| f[0]).count(), 2);

        // A kill too, until its blast is over.
        cctv.click(&Target::Row(id("a-wait")), 20_000);
        cctv.click(&Target::Cover, 20_100);
        cctv.click(&Target::Kill, 20_200);
        assert_eq!(
            cctv.focus_views()[0]
                .as_ref()
                .map(|v| (&v.agent.desk, v.wide)),
            Some((&id("a-wait"), true))
        );
        assert!(frames(&mut cctv, 16).iter().all(|f| f[0]));
        cctv.advance(20_200 + Kind::Kill.lasts());
        assert_eq!(wide(&cctv), [false, false]);
    }

    #[test]
    fn kill_fires_on_the_selected_agent_only_while_the_cover_is_up() {
        let mut cctv = CctvState::default();
        cctv.plan(&console_plan());
        cctv.click(&Target::Row(id("b-wait")), 10_000);
        // Under the closed cover the button fires nothing.
        cctv.click(&Target::Kill, 10_100);
        assert!(cctv.punishments().is_empty());
        // Nor once the cover has dropped again.
        cctv.click(&Target::Cover, 11_000);
        cctv.click(&Target::Kill, 14_000);
        assert!(cctv.punishments().is_empty());
        // Within the three seconds it fires, on the selected agent; the cover is locked
        // down and both buttons are disabled while the kill plays.
        cctv.click(&Target::Cover, 15_000);
        cctv.click(&Target::Kill, 17_999);
        assert_eq!(
            playing(&cctv),
            [("s-b-wait".to_string(), Kind::Kill, Phase::Breach)]
        );
        assert_eq!(cctv.punishments()[0].began, 17_999);
        let console = cctv.console();
        assert_eq!(
            (console.cover, console.kill, console.work_harder),
            (Cover::Locked, Button::Disabled, Button::Disabled)
        );
        // Clicks on the locked cover or the button start nothing more.
        cctv.click(&Target::Cover, 18_100);
        cctv.click(&Target::Kill, 18_200);
        cctv.click(&Target::WorkHarder, 18_300);
        assert_eq!(cctv.punishments().len(), 1);
        assert_eq!(cover(&cctv).0, Cover::Locked);
    }

    #[test]
    fn the_cover_drops_when_the_selection_changes_and_is_locked_while_a_punishment_plays() {
        let mut cctv = CctvState::default();
        cctv.plan(&console_plan());
        cctv.click(&Target::Row(id("b-wait")), 10_000);
        cctv.click(&Target::Cover, 10_000);
        // Selecting the same agent again keeps it up; another agent drops it.
        cctv.click(&Target::Row(id("b-wait")), 10_500);
        assert_eq!(cover(&cctv).0, Cover::Up);
        cctv.click(&Target::Row(id("a-wait")), 11_000);
        assert_eq!(cover(&cctv), (Cover::Down, Button::Lit));
        cctv.click(&Target::Kill, 11_100);
        assert!(cctv.punishments().is_empty());
        // The selection clearing (its agent answered) drops it too.
        cctv.click(&Target::Cover, 12_000);
        assert_eq!(cover(&cctv).0, Cover::Up);
        cctv.plan(&with_a_wait_answered());
        assert_eq!(cover(&cctv), (Cover::Down, Button::Dark));
        // Work harder on the agent locks the cover down while it plays, open or not, and
        // it stays down after.
        cctv.plan(&console_plan());
        cctv.click(&Target::Row(id("b-ask")), 13_000);
        cctv.click(&Target::Cover, 13_000);
        cctv.click(&Target::WorkHarder, 13_500);
        assert_eq!(cover(&cctv), (Cover::Locked, Button::Disabled));
        cctv.click(&Target::Cover, 14_000);
        assert_eq!(cover(&cctv).0, Cover::Locked);
        cctv.advance(13_500 + Kind::WorkHarder.lasts());
        assert_eq!(cover(&cctv), (Cover::Down, Button::Lit));
        // Another agent's cover is free meanwhile.
        cctv.click(&Target::WorkHarder, 20_000);
        cctv.click(&Target::Row(id("a-wait")), 20_100);
        cctv.click(&Target::Cover, 20_200);
        assert_eq!(cover(&cctv), (Cover::Up, Button::Lit));
    }

    /// The console plan with Alpha's waiting agent answered (busy, no flag).
    fn with_a_wait_answered() -> FloorPlan {
        let mut plan = console_plan();
        let desk = &mut plan.rooms[0].desks[1];
        desk.flag = None;
        desk.state = AgentState::Busy;
        plan
    }

    /// `b-wait` killed at 20 000 (the cover lifted just before).
    fn killing_b_wait() -> CctvState {
        let mut cctv = CctvState::default();
        cctv.plan(&console_plan());
        cctv.click(&Target::Row(id("b-wait")), 19_000);
        cctv.click(&Target::Cover, 19_500);
        cctv.click(&Target::Kill, 20_000);
        assert_eq!(playing(&cctv).len(), 1);
        cctv
    }

    /// The agents whose robot is a wreck now.
    fn wrecked(cctv: &CctvState) -> Vec<String> {
        cctv.wrecked()
            .iter()
            .map(|agent| agent.desk.agent.clone())
            .collect()
    }

    #[test]
    fn a_kill_plays_about_eight_seconds_squad_en_route_until_the_blast_then_target_neutralised_then_a_wreck(
    ) {
        let mut cctv = killing_b_wait();
        assert!((7_500..=8_500).contains(&Kind::Kill.lasts()));
        for (at, phase, words) in [
            (20_000, Phase::Breach, Words::KillSquadEnRoute),
            (20_599, Phase::Breach, Words::KillSquadEnRoute),
            (20_600, Phase::Advance, Words::KillSquadEnRoute),
            (22_999, Phase::Advance, Words::KillSquadEnRoute),
            (23_000, Phase::Surround, Words::KillSquadEnRoute),
            (24_399, Phase::Surround, Words::KillSquadEnRoute),
            (24_400, Phase::FallBack, Words::KillSquadEnRoute),
            (26_199, Phase::FallBack, Words::KillSquadEnRoute),
            // The blast: TARGET NEUTRALISED for about two seconds.
            (26_200, Phase::Boom, Words::TargetNeutralised),
            (28_199, Phase::Boom, Words::TargetNeutralised),
        ] {
            cctv.advance(at);
            assert_eq!(
                playing(&cctv),
                [("s-b-wait".to_string(), Kind::Kill, phase)],
                "at {at}"
            );
            assert_eq!(row_of(&cctv, "b-wait"), (words, None), "at {at}");
            // The robot is a wreck from the blast on, never before.
            let wreck = if phase == Phase::Boom {
                vec!["s-b-wait".to_string()]
            } else {
                vec![]
            };
            assert_eq!(wrecked(&cctv), wreck, "at {at}");
        }
        // Then the kill is over and the wreck remains: the row reads WRECKED without a
        // timer, the selection stays, both buttons are disabled and the cover locked.
        cctv.advance(28_200);
        assert!(cctv.punishments().is_empty());
        assert_eq!(wrecked(&cctv), ["s-b-wait"]);
        assert_eq!(row_of(&cctv, "b-wait"), (Words::Wrecked, None));
        assert_eq!(cctv.selected(), Some(&id("b-wait")));
        let console = cctv.console();
        assert_eq!(
            (console.cover, console.kill, console.work_harder),
            (Cover::Locked, Button::Disabled, Button::Disabled)
        );
        // A wreck can't be punished.
        cctv.click(&Target::Cover, 29_000);
        cctv.click(&Target::Kill, 29_100);
        cctv.click(&Target::WorkHarder, 29_200);
        assert!(cctv.punishments().is_empty());
        // It ends the same way however long the world went without a frame.
        let mut cctv = killing_b_wait();
        cctv.advance(60_000);
        assert!(cctv.punishments().is_empty());
        assert_eq!(wrecked(&cctv), ["s-b-wait"]);
    }

    #[test]
    fn with_effects_off_a_kill_shows_no_squad_or_blast_and_the_robot_becomes_a_wreck_at_the_same_moment(
    ) {
        let mut cctv = CctvState::default();
        cctv.set_effects(false);
        cctv.plan(&console_plan());
        cctv.click(&Target::Row(id("b-wait")), 19_000);
        cctv.click(&Target::Cover, 19_500);
        cctv.click(&Target::Kill, 20_000);
        for (at, words, wreck) in [
            (20_000, Words::KillSquadEnRoute, false),
            (20_600, Words::KillSquadEnRoute, false),
            (23_000, Words::KillSquadEnRoute, false),
            (24_400, Words::KillSquadEnRoute, false),
            (26_199, Words::KillSquadEnRoute, false),
            (26_200, Words::TargetNeutralised, true),
            (28_199, Words::TargetNeutralised, true),
        ] {
            cctv.advance(at);
            // No squad and no blast phase reported, ever.
            assert_eq!(
                playing(&cctv),
                [("s-b-wait".to_string(), Kind::Kill, Phase::Still)],
                "at {at}"
            );
            assert!(cctv.punishments()[0].squad().is_empty(), "at {at}");
            assert_eq!(row_of(&cctv, "b-wait"), (words, None), "at {at}");
            let robots = if wreck { vec!["s-b-wait"] } else { vec![] };
            assert_eq!(wrecked(&cctv), robots, "at {at}");
            assert!(charred(&cctv).is_empty(), "at {at}");
        }
        // Over at the same moment, the wreck remaining, WRECKED, locked.
        cctv.advance(28_200);
        assert!(cctv.punishments().is_empty());
        assert_eq!(wrecked(&cctv), ["s-b-wait"]);
        assert_eq!(row_of(&cctv, "b-wait"), (Words::Wrecked, None));
        let console = cctv.console();
        assert_eq!(
            (console.cover, console.kill, console.work_harder),
            (Cover::Locked, Button::Disabled, Button::Disabled)
        );
    }

    #[test]
    fn turning_effects_off_or_on_while_punishments_play_takes_effect_at_once_on_the_same_timeline()
    {
        // A kill and a strike playing when effects go off.
        let mut cctv = killing_b_wait();
        cctv.click(&Target::Row(id("b-ask")), 20_000);
        cctv.click(&Target::WorkHarder, 21_000);
        cctv.advance(22_000);
        assert_eq!(
            playing(&cctv),
            [
                ("s-b-ask".to_string(), Kind::WorkHarder, Phase::Charred),
                ("s-b-wait".to_string(), Kind::Kill, Phase::Advance),
            ]
        );
        let squad = cctv.punishments()[1].squad().len();
        assert!(squad > 0);
        // Off: at once, with no time passing, no squad and no effect phase; the strike's
        // robot stays charred and both keep their progress and words.
        let progress: Vec<f32> = cctv.punishments().iter().map(|p| p.progress).collect();
        cctv.set_effects(false);
        assert_eq!(
            playing(&cctv),
            [
                ("s-b-ask".to_string(), Kind::WorkHarder, Phase::Still),
                ("s-b-wait".to_string(), Kind::Kill, Phase::Still),
            ]
        );
        assert!(cctv.punishments()[1].squad().is_empty());
        assert_eq!(charred(&cctv), ["s-b-ask"]);
        assert_eq!(
            cctv.punishments()
                .iter()
                .map(|p| p.progress)
                .collect::<Vec<_>>(),
            progress
        );
        assert_eq!(row_of(&cctv, "b-ask"), (Words::LightningStrike, None));
        assert_eq!(row_of(&cctv, "b-wait"), (Words::KillSquadEnRoute, None));
        // On again: the squad is back where it is by now, the strike in its phase.
        cctv.set_effects(true);
        assert_eq!(
            playing(&cctv),
            [
                ("s-b-ask".to_string(), Kind::WorkHarder, Phase::Charred),
                ("s-b-wait".to_string(), Kind::Kill, Phase::Advance),
            ]
        );
        assert_eq!(cctv.punishments()[1].squad().len(), squad);
        // Off through the blast: the wreck comes at the same moment, and turned on during
        // the blast, the blast shows for what is left of it.
        cctv.set_effects(false);
        cctv.advance(26_200);
        assert_eq!(wrecked(&cctv), ["s-b-wait"]);
        cctv.set_effects(true);
        cctv.advance(27_200);
        assert_eq!(
            playing(&cctv),
            [("s-b-wait".to_string(), Kind::Kill, Phase::Boom)]
        );
        assert!((cctv.punishments()[0].progress - 0.5).abs() < 1e-6);
        cctv.advance(28_200);
        assert_eq!(wrecked(&cctv), ["s-b-wait"]);
    }

    /// `b-wait`'s session finished: not live any more, no flag. (The floor plan gives a
    /// finished session no desk; the state machine still copes with one.)
    fn finished(desk: &mut Desk) {
        desk.state = AgentState::Finished;
        desk.flag = None;
    }

    /// `b-wait`'s desk gone, alone.
    fn without_b_wait() -> FloorPlan {
        let mut plan = console_plan();
        plan.rooms[1].desks.remove(1);
        plan
    }

    /// The sessions kills have asked to end since last asked.
    fn sessions_to_end(cctv: &mut CctvState) -> Vec<String> {
        cctv.session_requests()
            .into_iter()
            .filter(|request| request.action == SessionAction::End)
            .map(|request| request.session)
            .collect()
    }

    #[test]
    fn a_kill_asks_once_at_the_blast_to_end_its_session() {
        let mut cctv = killing_b_wait();
        for at in [20_000, 23_000, 26_199] {
            cctv.advance(at);
            assert!(sessions_to_end(&mut cctv).is_empty(), "at {at}");
        }
        cctv.advance(26_200);
        assert_eq!(sessions_to_end(&mut cctv), ["s-b-wait"]);
        // Asked once: never again through the blast, the wreck or a click.
        for at in [26_201, 28_199, 28_200, 60_000] {
            cctv.advance(at);
            assert!(sessions_to_end(&mut cctv).is_empty(), "at {at}");
        }
        // With Punishment effects off, at the same moment.
        let mut cctv = CctvState::default();
        cctv.set_effects(false);
        cctv.plan(&console_plan());
        cctv.click(&Target::Row(id("b-wait")), 19_000);
        cctv.click(&Target::Cover, 19_500);
        cctv.click(&Target::Kill, 20_000);
        cctv.advance(26_199);
        assert!(sessions_to_end(&mut cctv).is_empty());
        cctv.advance(26_200);
        assert_eq!(sessions_to_end(&mut cctv), ["s-b-wait"]);
        // A world that went without a frame past the whole kill still asks, once.
        let mut cctv = killing_b_wait();
        cctv.advance(60_000);
        assert_eq!(sessions_to_end(&mut cctv), ["s-b-wait"]);
        assert!(sessions_to_end(&mut cctv).is_empty());
        // Work harder never asks for its session to be ended.
        let mut cctv = striking_b_wait();
        cctv.advance(60_000);
        assert!(sessions_to_end(&mut cctv).is_empty());
    }

    /// What the punishments and the console are, for comparing before and after a click.
    fn punishing(cctv: &CctvState) -> (Vec<Punishment>, Console, Vec<String>) {
        (cctv.punishments().to_vec(), cctv.console(), wrecked(cctv))
    }

    #[test]
    fn sit_in_names_the_selected_flagged_agent_and_never_changes_a_punishment() {
        let mut cctv = CctvState::default();
        cctv.plan(&console_plan());
        // With nobody selected there is nobody to sit in.
        cctv.click(&Target::SitIn, 9_000);
        assert_eq!(cctv.sit_in(), None);
        cctv.click(&Target::Row(id("a-wait")), 9_000);
        assert_eq!(cctv.sit_in(), Some(&id("a-wait")));
        // With the cover up, SIT IN leaves it up.
        cctv.click(&Target::Cover, 9_500);
        let before = punishing(&cctv);
        cctv.click(&Target::SitIn, 9_500);
        assert_eq!(punishing(&cctv), before);
        assert_eq!(cover(&cctv).0, Cover::Up);

        // A kill playing on b-wait and work harder on b-ask: SIT IN is allowed on
        // either, and starts, stops and holds back nothing.
        let mut cctv = killing_b_wait();
        cctv.click(&Target::Row(id("b-ask")), 21_000);
        cctv.click(&Target::WorkHarder, 21_000);
        for agent in ["b-ask", "b-wait"] {
            cctv.click(&Target::Row(id(agent)), 22_000);
            let before = punishing(&cctv);
            cctv.click(&Target::SitIn, 22_000);
            assert_eq!(cctv.sit_in(), Some(&id(agent)));
            assert_eq!(punishing(&cctv), before, "{agent}");
        }
        assert_eq!(playing(&cctv).len(), 2);
        // The kill still blows at its time and asks once to end its session.
        cctv.advance(26_200);
        assert_eq!(sessions_to_end(&mut cctv), ["s-b-wait"]);
        assert_eq!(wrecked(&cctv), ["s-b-wait"]);
    }

    #[test]
    fn once_kill_fires_a_change_of_state_or_flag_no_longer_stops_it() {
        // Answered while the squad advances: the kill plays on, blows, asks to end the
        // session and leaves a wreck.
        let answered = || {
            with_b_wait(|desk| {
                desk.flag = None;
                desk.state = AgentState::Busy;
            })
        };
        let mut cctv = killing_b_wait();
        cctv.advance(21_000);
        cctv.plan(&answered());
        assert_eq!(playing(&cctv)[0].2, Phase::Advance);
        cctv.advance(26_200);
        assert_eq!(playing(&cctv)[0].2, Phase::Boom);
        assert_eq!(sessions_to_end(&mut cctv), ["s-b-wait"]);
        cctv.advance(28_200);
        assert_eq!(wrecked(&cctv), ["s-b-wait"]);
        // The wreck keeps to the agent as it was when the kill ended, and clears on its
        // next change.
        cctv.plan(&answered());
        assert_eq!(wrecked(&cctv), ["s-b-wait"]);
        cctv.plan(&with_b_wait(|desk| desk.state = AgentState::Idle));
        assert!(wrecked(&cctv).is_empty());
        // A change during the blast itself changes nothing either.
        let mut cctv = killing_b_wait();
        cctv.advance(27_000);
        cctv.plan(&with_b_wait(|desk| desk.state = AgentState::Waiting));
        assert_eq!(playing(&cctv)[0].2, Phase::Boom);
        assert_eq!(wrecked(&cctv), ["s-b-wait"]);
    }

    #[test]
    fn a_kill_stops_without_asking_if_its_session_ends_or_its_desk_goes_before_the_blast() {
        for (what, plan) in [
            ("finished", with_b_wait(finished)),
            ("desk gone", without_b_wait()),
        ] {
            let mut cctv = killing_b_wait();
            cctv.advance(26_199);
            cctv.plan(&plan);
            assert!(cctv.punishments().is_empty(), "{what}");
            assert!(wrecked(&cctv).is_empty(), "{what}");
            cctv.advance(40_000);
            assert!(sessions_to_end(&mut cctv).is_empty(), "{what}");
            assert!(wrecked(&cctv).is_empty(), "{what}");
        }
        // Another agent's kill goes on meanwhile.
        let mut cctv = killing_b_wait();
        cctv.advance(21_000);
        let mut plan = console_plan();
        finished(&mut plan.rooms[0].desks[1]);
        cctv.plan(&plan);
        assert_eq!(playing(&cctv)[0].2, Phase::Advance);
    }

    #[test]
    fn a_session_ending_after_the_blast_lets_the_kill_finish_and_leaves_no_wreck() {
        // The session ends, as the blast asked: TARGET NEUTRALISED plays out, then the
        // finished agent has no wreck.
        let mut cctv = killing_b_wait();
        cctv.advance(26_500);
        assert_eq!(sessions_to_end(&mut cctv), ["s-b-wait"]);
        cctv.plan(&with_b_wait(finished));
        assert_eq!(playing(&cctv)[0].2, Phase::Boom);
        assert_eq!(wrecked(&cctv), ["s-b-wait"]);
        cctv.advance(28_200);
        assert!(cctv.punishments().is_empty());
        assert!(wrecked(&cctv).is_empty());
        assert!(sessions_to_end(&mut cctv).is_empty());
        // Its desk going takes the blast with it.
        let mut cctv = killing_b_wait();
        cctv.advance(26_500);
        cctv.plan(&without_b_wait());
        assert!(cctv.punishments().is_empty());
        assert!(wrecked(&cctv).is_empty());
    }

    #[test]
    fn a_kill_re_routes_its_squad_when_the_desk_moves_mid_way() {
        let mut cctv = killing_b_wait();
        cctv.advance(22_000);
        assert_eq!(playing(&cctv)[0].2, Phase::Advance);
        let beside = |cctv: &mut CctvState, row: usize, column: usize| {
            cctv.advance(22_999);
            let squad = cctv.punishments()[0].squad();
            let [x, z] = layout::desk_position(row, column);
            squad.len() == 3 && squad.iter().all(|t| (t.at[0] - x).hypot(t.at[1] - z) < 1.0)
        };
        assert!(beside(&mut cctv, 0, 1));
        // Beta's desks repack: the agent's desk moves a row back and across.
        let mut cctv = killing_b_wait();
        cctv.advance(22_000);
        let mut plan = console_plan();
        plan.rooms[1].desks[1].row = 1;
        plan.rooms[1].desks[1].column = 3;
        cctv.plan(&plan);
        assert!(beside(&mut cctv, 1, 3));
        assert!(!beside(&mut cctv, 0, 1));
        // Work harder has no squad.
        let cctv = striking_b_wait();
        assert!(cctv.punishments()[0].squad().is_empty());
    }

    /// `b-wait` killed at 20 000 and its kill over at 28 200: a wreck.
    fn wrecked_b_wait() -> CctvState {
        let mut cctv = killing_b_wait();
        cctv.advance(28_200);
        assert!(cctv.punishments().is_empty());
        assert_eq!(wrecked(&cctv), ["s-b-wait"]);
        cctv
    }

    #[test]
    fn a_wreck_stays_while_its_agent_is_unchanged_and_clears_at_once_on_any_change() {
        // The same plan again, other agents changing, and time passing keep it.
        let mut cctv = wrecked_b_wait();
        cctv.plan(&console_plan());
        cctv.plan(&with_a_wait_answered());
        cctv.advance(3_600_000);
        assert_eq!(wrecked(&cctv), ["s-b-wait"]);
        assert_eq!(row_of(&cctv, "b-wait"), (Words::Wrecked, None));

        let flag = |kind, since| Some(Flag { kind, since });
        type Change = Box<dyn Fn(&mut Desk)>;
        let changes: [(&str, Change); 5] = [
            (
                "waiting → busy",
                Box::new(|d: &mut Desk| {
                    d.state = AgentState::Busy;
                    d.flag = None;
                }),
            ),
            (
                "waiting → asking",
                Box::new(move |d: &mut Desk| {
                    d.state = AgentState::Waiting;
                    d.flag = flag(ASKING, 30_000);
                }),
            ),
            (
                "idle → a dialog, the same flag",
                Box::new(|d: &mut Desk| d.state = AgentState::Waiting),
            ),
            (
                "a new since",
                Box::new(move |d: &mut Desk| d.flag = flag(WAITING, 30_000)),
            ),
            (
                "finished",
                Box::new(|d: &mut Desk| {
                    d.state = AgentState::Finished;
                    d.flag = None;
                }),
            ),
        ];
        for (what, change) in changes {
            let mut cctv = wrecked_b_wait();
            cctv.plan(&with_b_wait(change));
            // The robot is back at once, and its row (if it is still flagged) is a plain
            // one again.
            assert!(wrecked(&cctv).is_empty(), "{what}");
            let row = cctv
                .console()
                .rows
                .into_iter()
                .find(|row| row.agent.desk == id("b-wait"));
            assert!(
                row.is_none_or(|row| matches!(row.words, Words::Flag(_))),
                "{what}"
            );
            // It stays cleared: the old state coming back brings no wreck back.
            cctv.plan(&console_plan());
            cctv.advance(40_000);
            assert!(wrecked(&cctv).is_empty(), "{what}");
        }

        // Its desk gone with its room, or alone.
        let mut cctv = wrecked_b_wait();
        let mut plan = console_plan();
        plan.rooms.remove(1);
        cctv.plan(&plan);
        assert!(wrecked(&cctv).is_empty());
        let mut cctv = wrecked_b_wait();
        let mut plan = console_plan();
        plan.rooms[1].desks.remove(1);
        cctv.plan(&plan);
        assert!(wrecked(&cctv).is_empty());
    }

    #[test]
    fn a_wreck_moves_with_its_desk_when_desks_repack() {
        let mut cctv = wrecked_b_wait();
        let at = |cctv: &CctvState| {
            let w = cctv.wrecked()[0];
            (w.desk.agent.clone(), w.cam, w.row, w.column)
        };
        assert_eq!(at(&cctv), ("s-b-wait".to_string(), 2, 0, 1));
        // Alpha closes, so Beta moves up the corridor, and its desks repack.
        let mut plan = console_plan();
        plan.rooms.remove(0);
        plan.rooms[0].desks[1].row = 1;
        plan.rooms[0].desks[1].column = 3;
        cctv.plan(&plan);
        assert_eq!(at(&cctv), ("s-b-wait".to_string(), 1, 1, 3));
        assert_eq!(row_of(&cctv, "b-wait"), (Words::Wrecked, None));
    }

    #[test]
    fn a_wrecked_row_can_be_selected_but_both_its_buttons_stay_disabled() {
        let mut cctv = wrecked_b_wait();
        // Select someone else: their buttons light.
        cctv.click(&Target::Row(id("a-wait")), 30_000);
        assert_eq!(cctv.selected(), Some(&id("a-wait")));
        assert_eq!(cover(&cctv), (Cover::Down, Button::Lit));
        // Back to the wreck's row: selected and highlighted, buttons disabled, cover
        // locked, and nothing fires.
        cctv.click(&Target::Row(id("b-wait")), 31_000);
        assert_eq!(cctv.selected(), Some(&id("b-wait")));
        assert!(rows(&cctv).contains(&("s-b-wait".to_string(), true)));
        let console = cctv.console();
        assert_eq!(
            (console.cover, console.kill, console.work_harder),
            (Cover::Locked, Button::Disabled, Button::Disabled)
        );
        cctv.click(&Target::Cover, 31_100);
        cctv.click(&Target::Kill, 31_200);
        cctv.click(&Target::WorkHarder, 31_300);
        assert!(cctv.punishments().is_empty());
        assert_eq!(row_of(&cctv, "b-wait"), (Words::Wrecked, None));
    }

    /// The ticker's entries, by agent id.
    fn ticked(cctv: &CctvState) -> Vec<String> {
        cctv.ticker()
            .iter()
            .map(|agent| agent.desk.agent.clone())
            .collect()
    }

    #[test]
    fn from_kill_until_the_wreck_clears_the_agent_is_off_the_ticker_and_out_of_the_focus_ranking() {
        // Before: b-ask, a-wait, b-wait, most urgent first.
        let mut cctv = CctvState::default();
        cctv.plan(&console_plan());
        assert_eq!(ticked(&cctv), ["s-b-ask", "s-a-wait", "s-b-wait"]);
        // Kill b-wait. The kill holds focus monitor 1 while it plays, the ranking fills
        // the other, and the ticker drops it at once.
        let mut cctv = killing_b_wait();
        assert_eq!(ticked(&cctv), ["s-b-ask", "s-a-wait"]);
        assert_eq!(focused(&cctv), ["s-b-wait", "s-b-ask"]);
        // Once the kill is over, its wreck shows on no focus monitor unless selected:
        // with another agent selected, the ranking never brings it back.
        cctv.advance(28_200);
        cctv.click(&Target::Row(id("a-wait")), 29_000);
        assert_eq!(focused(&cctv), ["s-a-wait", "s-b-ask"]);
        assert_eq!(ticked(&cctv), ["s-b-ask", "s-a-wait"]);
        // Its row stays on the console, WRECKED.
        assert_eq!(row_of(&cctv, "b-wait"), (Words::Wrecked, None));
        // A wall feed of a room whose only other flag is the wreck selects past it.
        cctv.click(&Target::Feed("-beta".into()), 29_500);
        assert_eq!(cctv.selected(), Some(&id("b-ask")));
        // The wreck clears (the agent asks now): back on the ticker and in the ranking.
        cctv.plan(&with_b_wait(|desk| {
            desk.state = AgentState::Waiting;
            desk.flag = Some(Flag {
                kind: ASKING,
                since: 30_000,
            });
        }));
        assert_eq!(ticked(&cctv), ["s-b-ask", "s-b-wait", "s-a-wait"]);
        // Work harder keeps its agent on the ticker.
        let cctv = striking_b_wait();
        assert_eq!(ticked(&cctv), ["s-b-ask", "s-a-wait", "s-b-wait"]);
    }

    /// Each feed's flag colour, by project.
    fn colours(cctv: &CctvState) -> Vec<(String, Option<FlagKind>)> {
        cctv.wall()
            .feeds
            .iter()
            .map(|feed| (feed.name.clone(), feed.flag))
            .collect()
    }

    #[test]
    fn from_kill_until_the_wreck_clears_its_rooms_frame_and_wash_no_longer_count_it() {
        // Alpha's only flag is a-wait; Beta has b-ask and b-wait.
        let mut cctv = CctvState::default();
        cctv.plan(&console_plan());
        let before = vec![
            ("alpha".to_string(), Some(WAITING)),
            ("beta".to_string(), Some(ASKING)),
            ("gamma".to_string(), None),
        ];
        assert_eq!(colours(&cctv), before);
        // Kill a-wait: Alpha's feed loses its colour at once, through the kill, the
        // wreck it leaves and plans that keep it.
        cctv.click(&Target::Row(id("a-wait")), 10_000);
        cctv.click(&Target::Cover, 10_000);
        cctv.click(&Target::Kill, 10_500);
        let killed = vec![
            ("alpha".to_string(), None),
            ("beta".to_string(), Some(ASKING)),
            ("gamma".to_string(), None),
        ];
        assert_eq!(colours(&cctv), killed);
        cctv.advance(30_000);
        assert_eq!(wrecked(&cctv), ["s-a-wait"]);
        cctv.plan(&console_plan());
        assert_eq!(colours(&cctv), killed);
        // Kill b-ask too: Beta turns amber, from b-wait alone.
        cctv.click(&Target::Row(id("b-ask")), 31_000);
        cctv.click(&Target::Cover, 31_000);
        cctv.click(&Target::Kill, 31_500);
        assert_eq!(colours(&cctv)[1], ("beta".to_string(), Some(WAITING)));
        // A new flag mid-way no longer stops the kill (ADR-0013): the colour stays off.
        let mut plan = console_plan();
        plan.rooms[1].desks[0].flag = Some(Flag {
            kind: ASKING,
            since: 32_000,
        });
        cctv.plan(&plan);
        assert_eq!(colours(&cctv)[1], ("beta".to_string(), Some(WAITING)));
        // Alpha's wreck clears as its agent asks: Alpha turns red.
        plan.rooms[0].desks[1].state = AgentState::Waiting;
        plan.rooms[0].desks[1].flag = Some(Flag {
            kind: ASKING,
            since: 33_000,
        });
        cctv.plan(&plan);
        assert!(wrecked(&cctv).is_empty());
        assert_eq!(colours(&cctv)[0], ("alpha".to_string(), Some(ASKING)));
        // Work harder leaves the colour alone.
        let cctv = striking_b_wait();
        assert_eq!(colours(&cctv), before);
    }

    /// What each focus monitor shows: agent id, flag colour, status words and timer.
    fn shown(cctv: &CctvState) -> Vec<(String, Option<FlagKind>, Words, Option<i64>)> {
        cctv.focus_views()
            .into_iter()
            .flatten()
            .map(|view| (view.agent.desk.agent, view.flag, view.words, view.timer))
            .collect()
    }

    #[test]
    fn from_kill_until_the_wreck_clears_a_focus_monitor_shows_its_agent_without_a_flag() {
        // Before: the ranking's two, each in its flag's colour with its timer.
        let mut cctv = CctvState::default();
        cctv.plan(&console_plan());
        assert_eq!(
            shown(&cctv),
            [
                (
                    "s-b-ask".to_string(),
                    Some(ASKING),
                    Words::Flag(ASKING),
                    Some(5_000)
                ),
                (
                    "s-a-wait".to_string(),
                    Some(WAITING),
                    Words::Flag(WAITING),
                    Some(1_000)
                ),
            ]
        );
        // The kill playing on monitor 1: no flag colour, the kill's progress, no timer.
        let mut cctv = killing_b_wait();
        assert_eq!(
            shown(&cctv)[0],
            ("s-b-wait".to_string(), None, Words::KillSquadEnRoute, None)
        );
        cctv.advance(26_200);
        assert_eq!(
            shown(&cctv)[0],
            ("s-b-wait".to_string(), None, Words::TargetNeutralised, None)
        );
        // The wreck, selected: still no flag colour, WRECKED.
        cctv.advance(28_200);
        cctv.click(&Target::Row(id("b-wait")), 29_000);
        assert_eq!(
            shown(&cctv)[0],
            ("s-b-wait".to_string(), None, Words::Wrecked, None)
        );
        // Work harder leaves the flag on.
        let cctv = striking_b_wait();
        assert_eq!(
            shown(&cctv)[0],
            (
                "s-b-wait".to_string(),
                Some(WAITING),
                Words::Flag(WAITING),
                Some(2_000)
            )
        );
        // The wreck clears as its agent asks: its flag is back.
        let mut cctv = wrecked_b_wait();
        cctv.click(&Target::Row(id("b-wait")), 29_000);
        cctv.plan(&with_b_wait(|desk| {
            desk.state = AgentState::Waiting;
            desk.flag = Some(Flag {
                kind: ASKING,
                since: 30_000,
            });
        }));
        assert_eq!(
            shown(&cctv)[0],
            (
                "s-b-wait".to_string(),
                Some(ASKING),
                Words::Flag(ASKING),
                Some(30_000)
            )
        );
    }

    /// The robots carrying a marker, by agent id, with its kind, sorted.
    fn marked(cctv: &CctvState) -> Vec<(String, FlagKind)> {
        let mut marked: Vec<_> = cctv
            .markers()
            .into_iter()
            .map(|(desk, marker)| (desk.agent, marker.kind))
            .collect();
        marked.sort_by(|a, b| a.0.cmp(&b.0));
        marked
    }

    #[test]
    fn a_killed_robot_carries_no_marker_until_its_wreck_clears_and_work_harder_leaves_it_on() {
        let all = vec![
            ("s-a-wait".to_string(), WAITING),
            ("s-b-ask".to_string(), ASKING),
            ("s-b-wait".to_string(), WAITING),
        ];
        let mut cctv = CctvState::default();
        cctv.plan(&console_plan());
        assert_eq!(marked(&cctv), all);
        // Work harder leaves it on, through the strike and after.
        let mut cctv = striking_b_wait();
        assert_eq!(marked(&cctv), all);
        cctv.advance(20_000);
        assert_eq!(marked(&cctv), all);
        // KILL takes it away the moment it fires, and it stays off through the kill and
        // over the wreck.
        let without = vec![
            ("s-a-wait".to_string(), WAITING),
            ("s-b-ask".to_string(), ASKING),
        ];
        let mut cctv = killing_b_wait();
        assert_eq!(marked(&cctv), without);
        cctv.advance(27_000);
        assert_eq!(marked(&cctv), without);
        cctv.advance(28_200);
        cctv.plan(&console_plan());
        assert_eq!(marked(&cctv), without);
        // The wreck clears as the agent asks: a fresh marker, in its flag's colour now.
        cctv.plan(&with_b_wait(|desk| {
            desk.state = AgentState::Waiting;
            desk.flag = Some(Flag {
                kind: ASKING,
                since: 30_000,
            });
        }));
        assert!(wrecked(&cctv).is_empty());
        assert!(marked(&cctv).contains(&("s-b-wait".to_string(), ASKING)));
        // Cleared by going busy instead: no marker, since it has no flag.
        let mut cctv = wrecked_b_wait();
        cctv.plan(&with_b_wait(|desk| {
            desk.state = AgentState::Busy;
            desk.flag = None;
        }));
        assert_eq!(marked(&cctv), without);
        // A new flag mid-way no longer stops the kill (ADR-0013): still no marker.
        let mut cctv = killing_b_wait();
        cctv.advance(21_000);
        cctv.plan(&with_b_wait(|desk| {
            desk.flag = Some(Flag {
                kind: WAITING,
                since: 21_000,
            })
        }));
        assert_eq!(marked(&cctv), without);
    }
}
