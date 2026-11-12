//! Highlights and underlines, and the page geometry they're expressed in.
//!
//! Marks live in **PDF user space**: points, with the origin at the *bottom*
//! left of the page and y growing upwards. Screen space is the opposite way up,
//! so the viewer converts at the edges rather than storing screen coordinates —
//! that keeps marks correct across zoom changes and across sessions.

use serde::{Deserialize, Serialize};

/// The part of a selection that falls on one page: one area per line of text.
///
/// A selection is a run of characters in reading order, and reading order
/// crosses pages — so a selection is a list of these rather than a page and a
/// list of areas. Marks are still made one page at a time; this is what a
/// drag hands them.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub page: usize,
    pub areas: Vec<Area>,
}

/// A rectangle in PDF user space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Area {
    pub left: f32,
    pub bottom: f32,
    pub right: f32,
    pub top: f32,
}

impl Area {
    pub fn new(left: f32, bottom: f32, right: f32, top: f32) -> Self {
        Self {
            left: left.min(right),
            bottom: bottom.min(top),
            right: left.max(right),
            top: bottom.max(top),
        }
    }

    pub fn width(&self) -> f32 {
        self.right - self.left
    }

    pub fn height(&self) -> f32 {
        self.top - self.bottom
    }

    pub fn center_y(&self) -> f32 {
        (self.top + self.bottom) / 2.
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.left && x <= self.right && y >= self.bottom && y <= self.top
    }

    /// Smallest area covering both.
    pub fn union(&self, other: &Area) -> Area {
        Area {
            left: self.left.min(other.left),
            bottom: self.bottom.min(other.bottom),
            right: self.right.max(other.right),
            top: self.top.max(other.top),
        }
    }

    /// What's left of this area once `cover` is taken out of it.
    ///
    /// Marks are one box per line, so a new mark can only ever bite a chunk out
    /// of the middle, the start or the end of an old one — this returns the up
    /// to two pieces that survive. An area that isn't on the same line, or
    /// doesn't overlap, comes back unchanged.
    pub fn without(&self, cover: &Area) -> Vec<Area> {
        if !self.intersects(cover) || !self.shares_line_with(cover) {
            return vec![*self];
        }

        // Slivers narrower than this are visual noise, not a mark.
        const MINIMUM_WIDTH: f32 = 1.0;
        let mut pieces = Vec::with_capacity(2);

        if cover.left - self.left > MINIMUM_WIDTH {
            pieces.push(Area {
                right: cover.left,
                ..*self
            });
        }
        if self.right - cover.right > MINIMUM_WIDTH {
            pieces.push(Area {
                left: cover.right,
                ..*self
            });
        }

        pieces
    }

    /// Grown by `amount` on every side — used to give thin marks a fair
    /// hit area for the eraser.
    pub fn expanded(&self, amount: f32) -> Area {
        Area {
            left: self.left - amount,
            bottom: self.bottom - amount,
            right: self.right + amount,
            top: self.top + amount,
        }
    }

    pub fn intersects(&self, other: &Area) -> bool {
        self.left < other.right
            && other.left < self.right
            && self.bottom < other.top
            && other.bottom < self.top
    }

    /// Do the two overlap vertically by more than half of the shorter one? Used
    /// to decide whether two characters belong to the same line of text.
    pub fn shares_line_with(&self, other: &Area) -> bool {
        let overlap = self.top.min(other.top) - self.bottom.max(other.bottom);
        overlap > 0.5 * self.height().min(other.height())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarkKind {
    Highlight,
    Underline,
}

impl MarkKind {
    pub fn label(self) -> &'static str {
        match self {
            MarkKind::Highlight => "Highlight",
            MarkKind::Underline => "Underline",
        }
    }
}

/// One highlight or underline over a run of text on a page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mark {
    /// Unique within its document, and stable for the session so undo and the
    /// eraser can refer to it.
    pub id: u64,
    /// Zero-based page index.
    pub page: usize,
    pub kind: MarkKind,
    pub area: Area,
    /// `0xRRGGBB`.
    pub color: u32,
    /// How strongly the colour is applied, 0.05–1.0. Files written by older
    /// builds have no opacity, so they default to full strength.
    #[serde(default = "full_opacity")]
    pub opacity: f32,
}

fn full_opacity() -> f32 {
    1.0
}

/// The transparency steps offered in the colour dropdown.
pub const OPACITY_STEPS: &[f32] = &[0.2, 0.4, 0.6, 0.8, 1.0];

impl Mark {
    /// Alpha written into the PDF annotation, so other viewers see roughly what
    /// RustedPdf draws.
    pub fn alpha(&self) -> u8 {
        let base = match self.kind {
            MarkKind::Highlight => 200.,
            MarkKind::Underline => 255.,
        };
        (base * self.opacity.clamp(0.05, 1.0)).round() as u8
    }

    /// The rectangle actually painted: a highlight covers the text, an
    /// underline is a rule along the baseline.
    pub fn painted_area(&self) -> Area {
        match self.kind {
            MarkKind::Highlight => self.area,
            MarkKind::Underline => Area {
                left: self.area.left,
                right: self.area.right,
                bottom: self.area.bottom - 0.5,
                top: self.area.bottom + 1.5,
            },
        }
    }

    /// Hit area for the eraser — an underline's painted rule is only 2pt tall,
    /// which is unfairly hard to click.
    pub fn hit_area(&self) -> Area {
        match self.kind {
            MarkKind::Highlight => self.area,
            MarkKind::Underline => self.painted_area().expanded(3.),
        }
    }
}

/// The colours offered in the toolbar. Any `0xRRGGBB` works; these are just the
/// presets.
pub const PRESET_COLORS: &[(&str, u32)] = &[
    ("Yellow", 0xffd400),
    ("Green", 0x5ad469),
    ("Blue", 0x4aa8ff),
    ("Pink", 0xff7ab6),
    ("Orange", 0xffa14a),
    ("Red", 0xe5484d),
];

/// Hue steps in the full picker.
const PICKER_HUES: usize = 12;
/// Lightness steps, light to dark.
const PICKER_TINTS: &[f32] = &[0.88, 0.76, 0.64, 0.5];

/// The full colour grid: every hue at four strengths, plus a neutral column.
///
/// Highlights are multiplied into the page, so a light tint reads as a
/// highlighter and a dark one as ink — both are useful, which is why the grid
/// spans the range rather than offering pastels only.
pub fn picker_colors() -> Vec<u32> {
    let mut colors = Vec::with_capacity(PICKER_HUES * PICKER_TINTS.len() + 4);

    for tint in PICKER_TINTS {
        for step in 0..PICKER_HUES {
            let hue = step as f32 * (360. / PICKER_HUES as f32);
            colors.push(hsl_to_rgb(hue, 0.95, *tint));
        }
    }

    colors.extend([0xffffff, 0xc8c8c8, 0x8c8c8c, 0x303030]);
    colors
}

fn hsl_to_rgb(hue: f32, saturation: f32, lightness: f32) -> u32 {
    let chroma = (1. - (2. * lightness - 1.).abs()) * saturation;
    let sector = hue / 60.;
    let second = chroma * (1. - (sector % 2. - 1.).abs());
    let match_value = lightness - chroma / 2.;

    let (red, green, blue) = match sector as u32 {
        0 => (chroma, second, 0.),
        1 => (second, chroma, 0.),
        2 => (0., chroma, second),
        3 => (0., second, chroma),
        4 => (second, 0., chroma),
        _ => (chroma, 0., second),
    };

    let channel = |value: f32| (((value + match_value) * 255.).round().clamp(0., 255.)) as u32;
    channel(red) << 16 | channel(green) << 8 | channel(blue)
}
