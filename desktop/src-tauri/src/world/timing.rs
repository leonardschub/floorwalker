//! Where a frame's time goes, for measuring (sweatshop-rooms ticket 27): debug builds
//! with `WORLD_DEBUG_TIMING=1` mark the main schedule, the extraction and each stage of
//! the render schedule, and the engine logs the mean time between the marks with each
//! frame-times line. `acquire` holds the wait for a swapchain image (the GPU's backlog),
//! `render` the encoding and submission, `present` the present and cleanup.

use std::sync::Mutex;
use std::time::Instant;

use bevy::prelude::*;
use bevy::render::{ExtractSchedule, Render, RenderApp, RenderSystems};

/// The marks of the frame being drawn, in order.
static MARKS: Mutex<Vec<(&'static str, Instant)>> = Mutex::new(Vec::new());

/// Per stage: total ms and frames, over the current log period.
static TOTALS: Mutex<Vec<(&'static str, f64, u32)>> = Mutex::new(Vec::new());

fn mark(name: &'static str) {
    if let Ok(mut marks) = MARKS.lock() {
        marks.push((name, Instant::now()));
    }
}

/// Starts a frame's marks: call right before `app.update()`.
pub fn begin() {
    if let Ok(mut marks) = MARKS.lock() {
        marks.clear();
        marks.push(("begin", Instant::now()));
    }
}

/// The stages of the frame drawn last, as one line.
pub fn last_frame() -> String {
    let Ok(marks) = MARKS.lock() else {
        return String::new();
    };
    marks
        .windows(2)
        .map(|pair| {
            format!(
                "{} {:.1}",
                pair[0].0,
                (pair[1].1 - pair[0].1).as_secs_f64() * 1000.0
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Ends a frame's marks: call right after `app.update()`. Each stage is the time from its
/// mark to the next one's.
pub fn end() {
    mark("end");
    let (Ok(marks), Ok(mut totals)) = (MARKS.lock(), TOTALS.lock()) else {
        return;
    };
    for pair in marks.windows(2) {
        let (name, at) = pair[0];
        let ms = (pair[1].1 - at).as_secs_f64() * 1000.0;
        match totals.iter_mut().find(|t| t.0 == name) {
            Some(total) => {
                total.1 += ms;
                total.2 += 1;
            }
            None => totals.push((name, ms, 1)),
        }
    }
}

/// The mean ms per stage since the last call, as one line.
pub fn take() -> Option<String> {
    let mut totals = TOTALS.lock().ok()?;
    if totals.is_empty() {
        return None;
    }
    let line = totals
        .iter()
        .map(|(name, ms, n)| format!("{name} {:.2}", ms / f64::from(*n)))
        .collect::<Vec<_>>()
        .join(", ");
    totals.clear();
    let (meshes, seen) = meshes();
    Some(format!(
        "stage means (ms): {line}; mesh entities {meshes}, {seen} in view"
    ))
}

/// Mesh entities, and how many of them some active camera sees, last frame.
static MESHES: Mutex<(usize, usize)> = Mutex::new((0, 0));

/// The mesh counts, for the log.
pub fn meshes() -> (usize, usize) {
    MESHES.lock().map(|m| *m).unwrap_or_default()
}

fn count_meshes(meshes: Query<&ViewVisibility, With<Mesh3d>>) {
    let seen = meshes.iter().filter(|v| v.get()).count();
    if let Ok(mut counts) = MESHES.lock() {
        *counts = (meshes.iter().len(), seen);
    }
}

/// Adds the marks to `app` and its render app.
pub fn add(app: &mut App) {
    app.add_systems(First, || mark("main"))
        .add_systems(Last, (count_meshes, || mark("main end")).chain());
    let Some(render) = app.get_sub_app_mut(RenderApp) else {
        return;
    };
    render
        .add_systems(ExtractSchedule, || mark("extract"))
        .add_systems(
            Render,
            (
                (|| mark("render world"))
                    .before(RenderSystems::ExtractCommands)
                    .before(RenderSystems::PrepareAssets),
                (|| mark("assets"))
                    .after(RenderSystems::ExtractCommands)
                    .before(RenderSystems::PrepareAssets),
                (|| mark("views"))
                    .after(RenderSystems::PrepareMeshes)
                    .before(RenderSystems::CreateViews),
                (|| mark("acquire"))
                    .after(RenderSystems::Specialize)
                    .before(RenderSystems::PrepareViews),
                (|| mark("queue"))
                    .after(RenderSystems::PrepareViews)
                    .before(RenderSystems::Queue),
                (|| mark("sort"))
                    .after(RenderSystems::QueueSweep)
                    .before(RenderSystems::PhaseSort),
                (|| mark("prepare"))
                    .after(RenderSystems::PhaseSort)
                    .before(RenderSystems::Prepare),
                (|| mark("render"))
                    .after(RenderSystems::PrepareBindGroups)
                    .before(RenderSystems::Render),
                (|| mark("present"))
                    .after(RenderSystems::Render)
                    .before(RenderSystems::Cleanup),
            ),
        );
}
