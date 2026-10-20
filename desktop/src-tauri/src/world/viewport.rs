//! The camera viewport from the world view's rectangle (spec: "Camera viewport").

use super::controller::Rect;

/// Clamps the reported world view rectangle to a `width` × `height` drawable surface
/// (all physical pixels). The page can report a rectangle a step ahead of the surface
/// during a resize or scale change, and a viewport outside its target is invalid.
/// `None` while the surface has no area.
pub fn clamp(rect: Rect, width: u32, height: u32) -> Option<Rect> {
    if width == 0 || height == 0 {
        return None;
    }
    let x = rect.x.min(width - 1);
    let y = rect.y.min(height - 1);
    Some(Rect {
        x,
        y,
        w: rect.w.clamp(1, width - x),
        h: rect.h.clamp(1, height - y),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const fn rect(x: u32, y: u32, w: u32, h: u32) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn a_rectangle_inside_the_surface_is_kept() {
        assert_eq!(
            clamp(rect(0, 64, 1760, 1592), 2200, 1720),
            Some(rect(0, 64, 1760, 1592))
        );
    }

    #[test]
    fn a_rectangle_reported_ahead_of_a_shrinking_surface_is_cut_to_it() {
        // The spike saw 1917x1294 at dpr 2 reported before the surface settled.
        assert_eq!(
            clamp(rect(0, 64, 1917, 1294), 1276, 742),
            Some(rect(0, 64, 1276, 678))
        );
    }

    #[test]
    fn a_rectangle_starting_outside_the_surface_keeps_one_pixel_inside() {
        assert_eq!(
            clamp(rect(900, 700, 50, 50), 800, 600),
            Some(rect(799, 599, 1, 1))
        );
    }

    #[test]
    fn an_empty_rectangle_still_gives_a_valid_viewport() {
        assert_eq!(
            clamp(rect(10, 10, 0, 0), 800, 600),
            Some(rect(10, 10, 1, 1))
        );
    }

    #[test]
    fn a_surface_without_area_has_no_viewport() {
        assert_eq!(clamp(rect(0, 0, 100, 100), 0, 600), None);
    }
}
