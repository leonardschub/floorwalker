//! The CCTV room's console (management-rooms ticket 18; spec: "The console"). The CCTV
//! state machine (`cctv_state::CctvState::console`) says what it shows; this draws it on
//! the `cctv_console` piece, found by its node names as its scene spawns:
//!
//! - **The glass top screen** (`console_screen`) lists the flagged agents under "SELECT
//!   TARGET" (room, model, flag and timer per row, in the flag's colour), or reads "ALL
//!   AGENTS WORKING". The selected row is highlighted, and a count of the rows that do
//!   not fit stands right of the heading. Every text is a live text (`lettering::Live`),
//!   redrawn when its words change (timers, once a second), with words from the page's
//!   table (`strings`); project and model names are data.
//! - **Clickable:** each shown row selects its agent; the flip cover, the KILL button,
//!   the WORK HARDER bar and the SIT IN bar are targets of their own (`aim`). The slab is not: its bounds
//!   rise to its top's back edge over the whole screen, and nothing clickable stands
//!   behind it.
//! - **The controls** are lit or dark (their materials dimmed) as the state machine says
//!   their buttons are: lit while they can fire on the selected agent, dark
//!   with nobody selected or while a punishment plays on it. SIT IN is never dimmed
//!   (agent-screens spec: "SIT IN on the CCTV console"). Their labels ("SIT IN", "KILL",
//!   "WORK HARDER") come from the page's table.
//! - **The flip cover** over KILL swings open about its hinge while the state machine
//!   says it is up, so a click aimed at the button then hits the button, and
//!   closes again when it drops or is locked.

use std::f32::consts::FRAC_PI_2;

use bevy::gltf::GltfMaterialName;
use bevy::light::NotShadowCaster;
use bevy::prelude::*;

use super::aim::Clickable;
use super::cctv_state::{Button, Console, ConsoleRow, Cover, Target, CONSOLE_ROWS};
use super::clock::WorldClock;
use super::feeds::{self, Cctv};
use super::lettering::{Lettered, Lettering, Live, LiveTexts, Wording};
use super::pieces::Kit;
use super::strings::{self, WorldStrings};
use super::surfaces::{Dim, SwapMaterials};

/// The console's parts, by their node names (`cctv_console.py`).
const SCREEN: &str = "console_screen";
const COVER: &str = "console_cover";
/// How far the cover turns open about its hinge, its local X (`cctv_console.py`).
const COVER_OPEN: f32 = -1.9;
const KILL: &str = "console_kill_button";
const WORK_HARDER: &str = "console_work_harder";
const SIT_IN: &str = "console_sit_in";
const LABELS: [(&str, Wording); 3] = [
    ("console_sit_in_label", Wording::SitIn),
    ("console_kill_label", Wording::Kill),
    ("console_work_harder_label", Wording::WorkHarder),
];

/// The screen's glass, 1.24 × 0.66 m, in its node's frame (local X across, +Y out of the
/// glass, -Z up the slope towards the wall): its face is 4 mm out. Texts lie just above
/// it, the highlight between, all as wide as [`TEXT_WIDTH`].
const GLASS_DEPTH: f32 = 0.66;
const TEXT_WIDTH: f32 = 1.16;
const TEXT_OUT: f32 = 0.0056;
const HIGHLIGHT_OUT: f32 = 0.0048;
const HIGHLIGHT: Vec2 = Vec2::new(1.2, 0.07);
/// A line's height: a live text's 1024 × 48 image over [`TEXT_WIDTH`]; the count's image
/// is 192 wide.
const LINE: f32 = TEXT_WIDTH * 48.0 / 1024.0;
const MORE_WIDTH: f32 = TEXT_WIDTH * 192.0 / 1024.0;
/// The heading's line near the glass's top edge, and the rows' below it.
const HEADING_AT: f32 = -GLASS_DEPTH / 2.0 + 0.045;
const FIRST_ROW_AT: f32 = HEADING_AT + 0.085;
const ROW_STEP: f32 = 0.075;
/// What separates a row's fields.
const FIELD: &str = "  ::  ";
/// The heading's colour, and the highlight's: the CCTV room's cyan, linear.
const CYAN: LinearRgba = LinearRgba::rgb(0.1, 0.9, 1.3);
const HIGHLIGHT_COLOUR: LinearRgba = LinearRgba::new(0.0, 0.55, 0.75, 0.4);
/// A selected row's text: near white over the highlight.
const SELECTED_TEXT: LinearRgba = LinearRgba::rgb(1.6, 1.7, 1.7);
/// How much brighter than the flag colour a row's text is drawn, so the tone mapping
/// keeps it bright on the dark glass.
const ROW_GAIN: f32 = 1.6;

/// One text on the screen: its quad, its material and the words it shows.
struct Text {
    quad: Entity,
    material: Handle<StandardMaterial>,
    words: Option<String>,
}

/// The console screen's texts and highlight, once its node has spawned.
#[derive(Resource, Default)]
struct Screen {
    heading: Option<Text>,
    more: Option<Text>,
    rows: Vec<Text>,
    /// What each row's quad selects, as last set.
    targets: Vec<Option<Target>>,
    highlight: Option<Entity>,
    /// The console as last logged.
    logged: Option<String>,
}

pub struct ConsolePlugin;

impl Plugin for ConsolePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Screen>()
            .add_systems(Update, (find_parts, draw_console.in_set(LiveTexts)).chain())
            .add_systems(
                PostUpdate,
                (find_controls, light_controls)
                    .chain()
                    .before(SwapMaterials),
            )
            .add_systems(
                PostUpdate,
                swing_cover.before(bevy::transform::TransformSystems::Propagate),
            );
    }
}

/// A text quad lying on the glass, facing out of it, its top towards the wall.
fn on_glass(x: f32, out: f32, z: f32) -> Transform {
    Transform::from_xyz(x, out, z).with_rotation(Quat::from_rotation_x(-FRAC_PI_2))
}

/// A material for a live text, drawn as it is (unlit), letters only.
fn text_material(colour: LinearRgba) -> StandardMaterial {
    StandardMaterial {
        base_color: colour.into(),
        unlit: true,
        fog_enabled: false,
        alpha_mode: AlphaMode::Premultiplied,
        ..default()
    }
}

/// Finds the console's parts as its scene spawns: marks what clicks can hit, puts the
/// texts and highlight on its screen and the labels in front of its controls.
fn find_parts(
    mut commands: Commands,
    parts: Query<(Entity, &Name, &Transform), Added<Name>>,
    mut screen: ResMut<Screen>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    for (part, name, at) in &parts {
        if name.as_str() == COVER {
            commands.entity(part).insert(Hinge(at.rotation));
        }
        let target = match name.as_str() {
            COVER => Some(Some(Target::Cover)),
            KILL => Some(Some(Target::Kill)),
            WORK_HARDER => Some(Some(Target::WorkHarder)),
            SIT_IN => Some(Some(Target::SitIn)),
            _ => None,
        };
        if let Some(target) = target {
            commands.entity(part).insert(Clickable(target));
        }
        if let Some((_, wording)) = LABELS.iter().find(|(label, _)| name.as_str() == *label) {
            let size = wording.size();
            commands.spawn((
                Lettered(*wording),
                Mesh3d(meshes.add(Rectangle::new(size.x, size.y))),
                on_glass(0.0, 0.001, 0.0),
                NotShadowCaster,
                ChildOf(part),
            ));
        }
        if name.as_str() != SCREEN || screen.heading.is_some() {
            continue;
        }
        let line = meshes.add(Rectangle::new(TEXT_WIDTH, LINE));
        let mut text = |commands: &mut Commands, mesh: &Handle<Mesh>, at: Transform| {
            let material = materials.add(text_material(CYAN));
            let quad = commands
                .spawn((
                    Mesh3d(mesh.clone()),
                    MeshMaterial3d(material.clone()),
                    at,
                    NotShadowCaster,
                    Visibility::Hidden,
                    ChildOf(part),
                ))
                .id();
            Text {
                quad,
                material,
                words: None,
            }
        };
        screen.heading = Some(text(
            &mut commands,
            &line,
            on_glass(0.0, TEXT_OUT, HEADING_AT),
        ));
        let more = meshes.add(Rectangle::new(MORE_WIDTH, LINE));
        screen.more = Some(text(
            &mut commands,
            &more,
            on_glass((TEXT_WIDTH - MORE_WIDTH) / 2.0, TEXT_OUT, HEADING_AT),
        ));
        screen.rows = (0..CONSOLE_ROWS)
            .map(|n| {
                let row = text(&mut commands, &line, on_glass(0.0, TEXT_OUT, row_at(n)));
                commands.entity(row.quad).insert(Clickable(None));
                row
            })
            .collect();
        screen.targets = vec![None; CONSOLE_ROWS];
        let highlight = materials.add(StandardMaterial {
            base_color: HIGHLIGHT_COLOUR.into(),
            unlit: true,
            fog_enabled: false,
            alpha_mode: AlphaMode::Blend,
            ..default()
        });
        screen.highlight = Some(
            commands
                .spawn((
                    Mesh3d(meshes.add(Rectangle::new(HIGHLIGHT.x, HIGHLIGHT.y))),
                    MeshMaterial3d(highlight),
                    on_glass(0.0, HIGHLIGHT_OUT, row_at(0)),
                    NotShadowCaster,
                    Visibility::Hidden,
                    ChildOf(part),
                ))
                .id(),
        );
        eprintln!("[world] console screen found");
    }
}

/// Where row `n`'s line lies, up the glass.
fn row_at(n: usize) -> f32 {
    FIRST_ROW_AT + ROW_STEP * n as f32
}

/// A row's line: its room, its model, its words and its timer.
fn row_text(row: &ConsoleRow, strings: &WorldStrings, now: i64) -> String {
    let status = strings.status(row.words, row.timer, now);
    [row.agent.name.as_str(), row.agent.model.as_str(), &status]
        .into_iter()
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>()
        .join(FIELD)
}

/// Draws the console as the state machine says: the heading, the rows with their
/// timers and colours, the highlight on the selected row, and what each row selects.
#[allow(clippy::too_many_arguments)]
fn draw_console(
    mut commands: Commands,
    kit: Option<Res<Kit>>,
    cctv: Res<Cctv>,
    clock: Res<WorldClock>,
    strings: Res<WorldStrings>,
    mut screen: ResMut<Screen>,
    mut lettering: ResMut<Lettering>,
    mut images: ResMut<Assets<Image>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut transforms: Query<&mut Transform>,
) {
    let Some(kit) = kit else { return };
    if screen.heading.is_none() {
        return;
    }
    let console = cctv.0.console();
    let now = clock.0;
    let heading = strings.get(strings::key(console.heading)).to_string();
    let more = if console.hidden > 0 {
        format!("+{}", console.hidden)
    } else {
        String::new()
    };
    let Screen {
        heading: Some(heading_text),
        more: Some(more_text),
        rows,
        targets,
        highlight,
        logged,
    } = &mut *screen
    else {
        return;
    };
    let mut to = Letter {
        kit: &kit,
        commands: &mut commands,
        lettering: &mut lettering,
        images: &mut images,
        materials: &mut materials,
    };
    to.letter(heading_text, Live::ConsoleHeading, &heading, CYAN);
    to.letter(more_text, Live::ConsoleMore, &more, CYAN);
    for (n, (text, target)) in rows.iter_mut().zip(targets.iter_mut()).enumerate() {
        let row = console.rows.get(n);
        let words = row.map_or(String::new(), |row| row_text(row, &strings, now));
        let colour = row.map_or(CYAN, |row| {
            if row.selected {
                SELECTED_TEXT
            } else {
                let [r, g, b] = feeds::flag_colour(row.agent.kind).to_array();
                LinearRgba::rgb(r * ROW_GAIN, g * ROW_GAIN, b * ROW_GAIN)
            }
        });
        to.letter(text, Live::ConsoleRow(n), &words, colour);
        let want = row.map(|row| Target::Row(row.agent.desk.clone()));
        if *target != want {
            to.commands
                .entity(text.quad)
                .insert(Clickable(want.clone()));
            *target = want;
        }
    }
    if let Some(highlight) = *highlight {
        let selected = console.rows.iter().position(|row| row.selected);
        if let Some(n) = selected {
            if let Ok(mut at) = transforms.get_mut(highlight) {
                if at.translation.z != row_at(n) {
                    at.translation.z = row_at(n);
                }
            }
        }
        to.commands.entity(highlight).insert(if selected.is_some() {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
    }
    let summary = describe(&console);
    if logged.as_ref() != Some(&summary) {
        eprintln!("[world] console: {summary}");
        *logged = Some(summary);
    }
}

/// What lettering a screen text needs.
struct Letter<'a, 'w, 's> {
    kit: &'a Kit,
    commands: &'a mut Commands<'w, 's>,
    lettering: &'a mut Lettering,
    images: &'a mut Assets<Image>,
    materials: &'a mut Assets<StandardMaterial>,
}

impl Letter<'_, '_, '_> {
    /// Shows `words` on `text` as live text `which`, in `colour`, hidden while empty;
    /// only what changed is redone.
    fn letter(&mut self, text: &mut Text, which: Live, words: &str, colour: LinearRgba) {
        if text.words.as_deref() != Some(words) {
            let Some(live) =
                self.lettering
                    .live(which, words, self.kit, self.commands, self.images)
            else {
                return;
            };
            if let Some(mut material) = self.materials.get_mut(&text.material) {
                material.base_color_texture = Some(live.image);
            }
            self.commands.entity(text.quad).insert(if words.is_empty() {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            });
            text.words = Some(words.to_string());
        }
        let colour = Color::from(colour);
        if self
            .materials
            .get(&text.material)
            .is_some_and(|m| m.base_color != colour)
        {
            if let Some(mut material) = self.materials.get_mut(&text.material) {
                material.base_color = colour;
            }
        }
    }
}

/// The console for the world's log: its heading, its rows (the selected one marked) and
/// its buttons.
fn describe(console: &Console) -> String {
    let rows: Vec<String> = console
        .rows
        .iter()
        .map(|row| {
            format!(
                "{}{} {:?}",
                if row.selected { "*" } else { "" },
                row.agent.desk.agent,
                row.words
            )
        })
        .collect();
    format!(
        "{:?} [{}]{}, kill {:?} (cover {:?}), work harder {:?}",
        console.heading,
        rows.join(", "),
        if console.hidden > 0 {
            format!(" +{}", console.hidden)
        } else {
            String::new()
        },
        console.kill,
        console.cover,
        console.work_harder
    )
}

/// Which console control a mesh belongs to: the KILL button and its cover, or the WORK
/// HARDER bar.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum Control {
    Kill,
    WorkHarder,
}

/// Marks each mesh of the KILL button, its cover and the WORK HARDER bar as its scene
/// spawns, dark to begin with.
fn find_controls(
    mut commands: Commands,
    parts: Query<Entity, Added<GltfMaterialName>>,
    parents: Query<&ChildOf>,
    names: Query<&Name>,
) {
    for part in &parts {
        let control = std::iter::once(part)
            .chain(parents.iter_ancestors(part))
            .filter_map(|up| names.get(up).ok())
            .find_map(|name| match name.as_str() {
                COVER | KILL => Some(Control::Kill),
                WORK_HARDER => Some(Control::WorkHarder),
                _ => None,
            });
        if let Some(control) = control {
            commands.entity(part).insert((control, Dim(true)));
        }
    }
}

/// Draws each control as the state machine says its button is: lit while it can fire on
/// the selected agent, dark (its materials dimmed) with nobody selected or while a
/// punishment plays on the agent.
fn light_controls(cctv: Res<Cctv>, mut parts: Query<(&Control, &mut Dim)>) {
    let console = cctv.0.console();
    for (control, mut dim) in &mut parts {
        let button = match control {
            Control::Kill => console.kill,
            Control::WorkHarder => console.work_harder,
        };
        dim.set_if_neq(Dim(button != Button::Lit));
    }
}

/// The flip cover's turn when closed, as its scene has it.
#[derive(Component)]
struct Hinge(Quat);

/// Swings the flip cover open while the state machine says it is up, closed otherwise.
fn swing_cover(cctv: Res<Cctv>, mut covers: Query<(&Hinge, &mut Transform)>) {
    let open = cctv.0.console().cover == Cover::Up;
    for (hinge, mut at) in &mut covers {
        let want = if open {
            hinge.0 * Quat::from_rotation_x(COVER_OPEN)
        } else {
            hinge.0
        };
        if at.rotation != want {
            at.rotation = want;
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;

    use super::*;

    /// A node of the console's scene by `name`, with one mesh under it as the glTF gives.
    fn node(world: &mut World, name: &str) -> (Entity, Entity) {
        let node = world
            .spawn((Name::new(name.to_string()), Transform::default()))
            .id();
        let mesh = world
            .spawn((GltfMaterialName("a material".into()), ChildOf(node)))
            .id();
        (node, mesh)
    }

    #[test]
    fn the_sit_in_bar_is_clickable_labelled_beneath_and_never_dimmed() {
        let mut world = World::new();
        world.init_resource::<Assets<Mesh>>();
        world.init_resource::<Assets<StandardMaterial>>();
        world.init_resource::<Screen>();
        let (bar, bar_mesh) = node(&mut world, "console_sit_in");
        let (label, _) = node(&mut world, "console_sit_in_label");
        let (work_harder, work_harder_mesh) = node(&mut world, "console_work_harder");
        world.run_system_once(find_parts).unwrap();
        world.run_system_once(find_controls).unwrap();

        assert_eq!(
            world.get::<Clickable>(bar),
            Some(&Clickable(Some(Target::SitIn)))
        );
        let words: Vec<Wording> = world
            .get::<Children>(label)
            .unwrap()
            .iter()
            .filter_map(|child| world.get::<Lettered>(child).map(|l| l.0))
            .collect();
        assert_eq!(words, [Wording::SitIn]);
        // WORK HARDER starts dark; SIT IN is never dimmed.
        assert_eq!(
            world.get::<Clickable>(work_harder),
            Some(&Clickable(Some(Target::WorkHarder)))
        );
        assert!(world.get::<Dim>(work_harder_mesh).is_some());
        assert!(world.get::<Dim>(bar_mesh).is_none());
    }
}
