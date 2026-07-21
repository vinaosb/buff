//! RGBA color value (8 bits per channel).

/// An 8-bit-per-channel RGBA color.
///
/// The default alpha is 255 (fully opaque). Constructed via the
/// associated functions [`Color::rgb`], [`Color::rgba`],
/// [`Color::black`], [`Color::white`], [`Color::gray`].
///
/// Maps to `image::Rgba<u8>` at the FFI boundary. The struct is
/// `Copy + Clone + Debug + PartialEq + Eq + Send + Sync` so it can
/// cross `spawn` boundaries per T4 FFI guide R4 (Thread Safety).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    r: u8,
    g: u8,
    b: u8,
    a: u8,
}

impl Color {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b, a: 255 }
    }

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Color { r, g, b, a }
    }

    pub const fn black() -> Self {
        Color {
            r: 0,
            g: 0,
            b: 0,
            a: 255,
        }
    }

    pub const fn white() -> Self {
        Color {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        }
    }

    pub const fn gray(value: u8) -> Self {
        Color {
            r: value,
            g: value,
            b: value,
            a: 255,
        }
    }

    #[inline]
    pub const fn r(self) -> u8 {
        self.r
    }

    #[inline]
    pub const fn g(self) -> u8 {
        self.g
    }

    #[inline]
    pub const fn b(self) -> u8 {
        self.b
    }

    #[inline]
    pub const fn a(self) -> u8 {
        self.a
    }

    /// Rec. 601 luma: `0.299 R + 0.587 G + 0.114 B` (ITU-R BT.601-7).
    /// Used by [`crate::Image::grayscale`] when converting to single-
    /// channel luma. The coefficient ordering matches `image::Luma`.
    #[inline]
    pub fn luma(self) -> u8 {
        let yf = 0.299_f32 * self.r as f32 + 0.587_f32 * self.g as f32 + 0.114_f32 * self.b as f32;
        yf.round().clamp(0.0, 255.0) as u8
    }

    pub(crate) fn to_rgba(self) -> image::Rgba<u8> {
        image::Rgba([self.r, self.g, self.b, self.a])
    }

    pub(crate) fn from_rgba(px: image::Rgba<u8>) -> Self {
        Color {
            r: px[0],
            g: px[1],
            b: px[2],
            a: px[3],
        }
    }
}

impl Default for Color {
    fn default() -> Self {
        Color::black()
    }
}
