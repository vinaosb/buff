//! 2-D transform: position + rotation + scale.
//!
//! Builder-style chaining: `Transform::new().translate(x,
//! y).rotate(radians)`. The three fns match the T16 spec's "Transform
//! (3): new, translate, rotate" cap exactly; scaling is exposed via
//! the public `scale` field (no `scale()` fn needed — keeps the count
//! under the 40-fn cap).

use std::fmt;

/// 2-D affine transform: translation + rotation + non-uniform scale.
///
/// Stored as three independent fields (NOT a 3×3 matrix) so the
/// builder methods are cheap and the math is obvious. The draw
/// pipeline composes them at render time:
///
/// ```text
///   screen = T(position) × R(rotation) × S(scale) × local_vertex
/// ```
///
/// Rotation is in **radians** (counter-clockwise). Scale defaults to
/// `(1.0, 1.0)`. Position defaults to the origin `(0.0, 0.0)`.
///
/// # Example
///
/// ```no_run
/// use buff_game::Transform;
///
/// let t = Transform::new()
///     .translate(100.0, 200.0)
///     .rotate(std::f32::consts::FRAC_PI_2);  // 90° CCW
/// assert_eq!(t.position, (100.0, 200.0));
/// ```

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// Translation in pixels (x right, y down — screen-space).
    pub position: (f32, f32),
    /// Rotation in radians (counter-clockwise around `position`).
    pub rotation: f32,
    /// Non-uniform scale (sx, sy). `(1.0, 1.0)` is identity.
    pub scale: (f32, f32),
}

impl Transform {
    /// Identity transform: position (0, 0), rotation 0, scale (1, 1).
    pub fn new() -> Self {
        Self::default()
    }

    /// Builder: returns a new `Transform` with `position` shifted by
    /// `(dx, dy)`. Consumes self for chaining. Rotation + scale
    /// are preserved unchanged.
    pub fn translate(mut self, dx: f32, dy: f32) -> Self {
        self.position.0 += dx;
        self.position.1 += dy;
        self
    }

    /// Builder: returns a new `Transform` with `rotation` advanced
    /// by `radians`. Consumes self for chaining. Position + scale
    /// are preserved unchanged.
    pub fn rotate(mut self, radians: f32) -> Self {
        self.rotation += radians;
        self
    }
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            position: (0.0, 0.0),
            rotation: 0.0,
            scale: (1.0, 1.0),
        }
    }
}

impl fmt::Display for Transform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Transform(pos=({:.1},{:.1}), rot={:.3}rad, scale=({:.1},{:.1}))",
            self.position.0, self.position.1, self.rotation, self.scale.0, self.scale.1,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_is_identity() {
        let t = Transform::new();
        assert_eq!(t.position, (0.0, 0.0));
        assert_eq!(t.rotation, 0.0);
        assert_eq!(t.scale, (1.0, 1.0));
    }

    #[test]
    fn translate_adds_to_position() {
        let t = Transform::new().translate(10.0, 20.0);
        assert_eq!(t.position, (10.0, 20.0));
        let t2 = t.translate(5.0, -5.0);
        assert_eq!(t2.position, (15.0, 15.0));
    }

    #[test]
    fn rotate_adds_radians() {
        let t = Transform::new().rotate(std::f32::consts::FRAC_PI_2);
        assert!((t.rotation - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        let t2 = t.rotate(std::f32::consts::FRAC_PI_2);
        assert!((t2.rotation - std::f32::consts::PI).abs() < 1e-6);
    }

    #[test]
    fn builder_chain_preserves_unchanged_fields() {
        let t = Transform::new().translate(5.0, 5.0).rotate(1.0);
        assert_eq!(t.position, (5.0, 5.0));
        assert!((t.rotation - 1.0).abs() < 1e-6);
        assert_eq!(t.scale, (1.0, 1.0)); // unchanged
    }

    #[test]
    fn default_equals_new() {
        assert_eq!(Transform::default(), Transform::new());
    }
}
