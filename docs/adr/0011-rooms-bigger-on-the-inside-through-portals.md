# Rooms are bigger on the inside, joined to the corridor by render-to-texture portals

Each project's room in the world is bigger on the inside: its doorway sits about 2.5 m from its neighbours' along one side of the corridor, yet the interior behind it is as large as its agents need and grows a row at a time without moving anything else. Every interior lives elsewhere in world space. Its doorway is a two-way portal: a virtual camera inside the room renders into an image shown on the doorway with a screen-space material (the shape of Bevy's `mirror` example), clipped at the doorway plane with the projection's `near_clip_plane`. On the frame the player crosses the doorway plane, the player is moved into the interior's space, so walking through is seamless and nothing loads. Decided with the effort owner on 2026-10-17 ([How desks are laid out](../../.scratch/sweatshop-rooms/issues/04-grilling-how-desks-are-laid-out.md), [How Bevy 0.20 can draw rooms bigger on the inside](../../.scratch/sweatshop-rooms/issues/08-research-portals-in-bevy.md)).

## Considered Options

- **Euclidean rooms side by side:** a growing room would push its neighbours along the corridor, or need gaps sized for the largest room. Rejected by the owner in favour of a short corridor with roomy interiors.
- **Stencil-buffer portals:** cheaper per portal, but Bevy 0.20's 3D depth buffer has no stencil (Bevy #21493, #21358, #25611).
- **`bevy_basic_portals`:** supports 0.20 only on its unpublished `main`, is one-way and visual only, and is licensed NVPLv7+, not MIT/Apache. **`bevy_easy_portals`** is MIT/Apache but stuck on Bevy 0.17. Neither clips at the doorway plane. The portals are written in this repo instead.

## Consequences

- Every live doorway costs about one extra scene draw per frame. Measured on the owner's Ice Lake iGPU ([Measure doorway views on the owner's machine](../../.scratch/sweatshop-rooms/issues/10-task-measure-doorway-views.md)): about 10 ms per live doorway as built, about 1.3 ms with portal shadows off, cropping, no MSAA and half resolution, on top of 13.5 ms for the corridor alone. Not every visible doorway can stay live at 60 fps, so only the nearest doorways are live and the rest show a dim interior. Lights in rooms that aren't live must be removed, not hidden, or their shadow passes keep costing.
- The world's camera systems (`apply_world_rect`, `set_drawing`, `apply_look`) and its first-frame check act on every `Camera3d` today; they must leave portal cameras alone.
- Collision, go-to-desk and "pushed to the nearest free spot" all work in the interior's own space; anything placed by world position must know which space it is in.
