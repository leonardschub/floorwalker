//! Picking what the player aims at (management-rooms ticket 18; spec: "Clicks reach the
//! world"): our own ray-against-boxes code, like collision, with no picking crate. The
//! ray goes from the eye through the middle of the view; each clickable thing is a box
//! turned any way (a screen hangs turned on the curved wall, the console's top slopes),
//! and the nearest box the ray enters wins. Pure: the world turns its clickable meshes
//! into [`Boxed`]s and hands them over with what each one stands for.

use bevy::math::Vec3;

/// A ray: where it starts and the way it goes (any length but zero).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: Vec3,
    pub direction: Vec3,
}

/// A box turned any way: its centre, its three axes (unit length, at right angles) and
/// how far it reaches along each, either side of the centre.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Boxed {
    pub centre: Vec3,
    pub axes: [Vec3; 3],
    pub half: [f32; 3],
}

impl Boxed {
    /// A box along the world's axes.
    #[cfg(test)]
    pub fn aligned(centre: Vec3, half: [f32; 3]) -> Self {
        Self {
            centre,
            axes: [Vec3::X, Vec3::Y, Vec3::Z],
            half,
        }
    }

    /// How far along `ray` (in multiples of its direction) it enters this box, or none if
    /// it misses it or the box is behind it. A ray that starts inside enters at once.
    pub fn hit(&self, ray: &Ray) -> Option<f32> {
        let from = ray.origin - self.centre;
        let (mut near, mut far) = (0.0_f32, f32::INFINITY);
        for (axis, half) in self.axes.iter().zip(self.half) {
            let at = from.dot(*axis);
            let speed = ray.direction.dot(*axis);
            if speed.abs() < 1e-9 {
                // Parallel to this pair of faces: inside the slab or never.
                if at.abs() > half {
                    return None;
                }
                continue;
            }
            let (a, b) = ((-half - at) / speed, (half - at) / speed);
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if near > far {
                return None;
            }
        }
        Some(near)
    }
}

/// What the ray enters first among `boxes`, each with what it stands for; none if it
/// enters none.
pub fn nearest<T>(ray: &Ray, boxes: impl IntoIterator<Item = (T, Boxed)>) -> Option<T> {
    boxes
        .into_iter()
        .filter_map(|(what, boxed)| Some((boxed.hit(ray)?, what)))
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, what)| what)
}

/// What the ray enters first among `boxes` within `reach` metres of its origin (agent
/// screens: a right-click sits in a robot within 4 m); none if it enters none that near.
/// A box entered further away neither counts nor stops the ray.
pub fn nearest_within<T>(
    ray: &Ray,
    boxes: impl IntoIterator<Item = (T, Boxed)>,
    reach: f32,
) -> Option<T> {
    let length = ray.direction.length();
    boxes
        .into_iter()
        .filter_map(|(what, boxed)| Some((boxed.hit(ray)? * length, what)))
        .filter(|(distance, _)| *distance <= reach)
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, what)| what)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ray(origin: [f32; 3], direction: [f32; 3]) -> Ray {
        Ray {
            origin: Vec3::from_array(origin),
            direction: Vec3::from_array(direction),
        }
    }

    fn cube(centre: [f32; 3], half: f32) -> Boxed {
        Boxed::aligned(Vec3::from_array(centre), [half; 3])
    }

    #[test]
    fn the_nearest_box_the_ray_enters_wins() {
        let forward = ray([0.0, 1.6, 0.0], [0.0, 0.0, -1.0]);
        let boxes = [
            ("far", cube([0.0, 1.6, -7.0], 0.5)),
            ("near", cube([0.0, 1.6, -3.0], 0.5)),
            ("aside", cube([2.0, 1.6, -1.0], 0.5)),
        ];
        assert_eq!(nearest(&forward, boxes), Some("near"));
        // In any order.
        let mut reversed = boxes;
        reversed.reverse();
        assert_eq!(nearest(&forward, reversed), Some("near"));
        // A box overlapping another nearer to the eye wins over it.
        let overlapping = [
            (
                "screen",
                Boxed::aligned(Vec3::new(0.0, 1.6, -3.0), [0.6, 0.3, 0.01]),
            ),
            (
                "row",
                Boxed::aligned(Vec3::new(0.0, 1.6, -2.99), [0.5, 0.05, 0.01]),
            ),
        ];
        assert_eq!(nearest(&forward, overlapping), Some("row"));
    }

    #[test]
    fn within_reach_only_boxes_entered_close_enough_count_and_the_nearest_wins() {
        // The ray's direction is not of unit length: reach is in metres all the same.
        let forward = ray([0.0, 1.6, 0.0], [0.0, 0.0, -2.0]);
        // Faces at 2.5 m and 3.9 m: both within 4 m, the nearer wins.
        let boxes = [
            ("far", cube([0.0, 1.6, -4.4], 0.5)),
            ("near", cube([0.0, 1.6, -3.0], 0.5)),
        ];
        assert_eq!(nearest_within(&forward, boxes, 4.0), Some("near"));
        // A face at 4.1 m is beyond reach, and does not stop the ray either.
        let beyond = [("beyond", cube([0.0, 1.6, -4.6], 0.5))];
        assert_eq!(nearest_within(&forward, beyond, 4.0), None);
        // A face at 3.9 m is within it.
        let just = [("just", cube([0.0, 1.6, -4.4], 0.5))];
        assert_eq!(nearest_within(&forward, just, 4.0), Some("just"));
        // A box the eye stands in is hit at once.
        let around = [("around", cube([0.0, 1.6, 0.0], 0.5))];
        assert_eq!(nearest_within(&forward, around, 4.0), Some("around"));
    }

    #[test]
    fn a_ray_that_passes_beside_or_away_from_every_box_hits_nothing() {
        let boxes = [
            ("a", cube([0.0, 1.6, -3.0], 0.5)),
            ("b", cube([3.0, 0.5, 2.0], 0.2)),
        ];
        // Beside, above, and turned away.
        assert_eq!(
            nearest(&ray([0.0, 1.6, 0.0], [1.0, 0.0, -1.0]), boxes),
            None
        );
        assert_eq!(
            nearest(&ray([0.0, 2.2, 0.0], [0.0, 0.0, -1.0]), boxes),
            None
        );
        assert_eq!(
            nearest(&ray([0.0, 1.6, 0.0], [0.0, 0.0, 1.0]), boxes[..1].to_vec()),
            None
        );
        // Parallel to a pair of faces, outside them.
        assert_eq!(
            nearest(&ray([0.0, 1.0, 0.0], [0.0, 0.0, -1.0]), boxes[..1].to_vec()),
            None
        );
        assert_eq!(
            nearest(
                &ray([0.0, 1.0, 0.0], [0.0, 0.0, -1.0]),
                Vec::<((), Boxed)>::new()
            ),
            None
        );
        // Just inside the edge still hits; just outside misses.
        let edge = |x: f32| nearest(&ray([x, 1.6, 0.0], [0.0, 0.0, -1.0]), boxes[..1].to_vec());
        assert_eq!(edge(0.49), Some("a"));
        assert_eq!(edge(0.51), None);
    }

    #[test]
    fn a_turned_box_is_hit_by_its_own_shape_not_its_bounds() {
        // A thin screen 1.2 m wide turned 45 degrees about the vertical, 3 m ahead.
        let turn = std::f32::consts::FRAC_PI_4;
        let screen = Boxed {
            centre: Vec3::new(0.0, 1.6, -3.0),
            axes: [
                Vec3::new(turn.cos(), 0.0, -turn.sin()),
                Vec3::Y,
                Vec3::new(turn.sin(), 0.0, turn.cos()),
            ],
            half: [0.6, 0.34, 0.01],
        };
        let at = |x: f32, z: f32| {
            let eye = Vec3::new(0.0, 1.6, 0.0);
            let direction = Vec3::new(x, 1.6, z) - eye;
            screen.hit(&Ray {
                origin: eye,
                direction,
            })
        };
        // Its middle and a point along its face near its end.
        assert!(at(0.0, -3.0).is_some());
        assert!(at(0.4, -3.4).is_some());
        // A corner of its axis-aligned bounds that the turned screen does not cover.
        assert!(at(0.4, -2.6).is_none());
        // Entered at the distance of its face, not its centre.
        let t = at(0.0, -3.0).unwrap();
        assert!((2.98..3.0).contains(&(t * 3.0)), "{t}");
    }

    #[test]
    fn a_box_behind_the_eye_is_never_hit_and_one_around_it_is_hit_at_once() {
        let forward = ray([0.0, 1.6, 0.0], [0.0, 0.0, -1.0]);
        assert_eq!(cube([0.0, 1.6, 2.0], 0.5).hit(&forward), None);
        assert_eq!(cube([0.0, 1.6, 0.0], 0.5).hit(&forward), Some(0.0));
    }

    #[test]
    fn something_that_blocks_hides_what_stands_behind_it() {
        // The world passes a box that does nothing as none: nearest it stops the ray.
        let forward = ray([0.0, 1.6, 0.0], [0.0, -0.3, -1.0]);
        let boxes = [
            (Some("feed"), cube([0.0, 0.4, -4.0], 0.3)),
            (
                None,
                Boxed::aligned(Vec3::new(0.0, 0.6, -2.0), [1.3, 0.4, 0.4]),
            ),
        ];
        assert_eq!(nearest(&forward, boxes).flatten(), None);
        assert_eq!(
            nearest(&forward, boxes[..1].to_vec()).flatten(),
            Some("feed")
        );
    }
}
