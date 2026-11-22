//! The icons Keevoro shows by the clock, one for each of its moods (see
//! `keevoro_api::glance::Mood`), made here from the logo so that they are
//! checked in and can be made again: `cargo xtask tray-icons` writes them
//! and `cargo xtask tray-icons --check` (and `cargo test`) fails when the
//! files differ from what this makes.
//!
//! The mood is in the shape, never the colour alone: calm is the logo by
//! itself, and the others carry a round badge at the bottom right with a
//! sign in it (an arrow going up while working, two bars while paused, an
//! exclamation mark when something needs the person's say), cut out from
//! the logo by a clear ring so it reads at sixteen pixels.
//!
//! Two sets are made:
//!
//! - `<mood>.png`, 32 pixels, in colour, from `design/keevoro-icon.png`,
//!   for Windows' notification area and Linux's tray;
//! - `<mood>-mac.png`, 44 pixels (the menu bar's 22 points, doubled), in
//!   black and clear only, drawn from the logo's shapes, which a Mac shows
//!   as a template image in the menu bar's own colour, light or dark.

use anyhow::{bail, Context, Result};
use image::{imageops, Rgba, RgbaImage};
use std::path::Path;

/// The logo the coloured icons are made from.
pub const LOGO: &str = "design/keevoro-icon.png";

/// Where the icons go.
pub const FOLDER: &str = "src-tauri/icons/tray";

/// Each mood, by the name its files have.
const MOODS: [Mood; 4] = [Mood::Calm, Mood::Working, Mood::Paused, Mood::NeedsYou];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mood {
    Calm,
    Working,
    Paused,
    NeedsYou,
}

impl Mood {
    fn name(self) -> &'static str {
        match self {
            Self::Calm => "calm",
            Self::Working => "working",
            Self::Paused => "paused",
            Self::NeedsYou => "needs-you",
        }
    }

    /// The badge's colour in the coloured set: the logo's own pine for
    /// working, a quiet grey for paused, its sun for needing the person.
    fn colour(self) -> Option<[u8; 3]> {
        match self {
            Self::Calm => None,
            Self::Working => Some([0x1d, 0x5c, 0x4b]),
            Self::Paused => Some([0x5f, 0x63, 0x68]),
            Self::NeedsYou => Some([0xd9, 0x5f, 0x0e]),
        }
    }

    /// Whether a point of the badge, from -1 to 1 across it, is in its sign.
    fn sign(self, u: f32, v: f32) -> bool {
        match self {
            Self::Calm => false,
            // An arrow going up: a shaft, and a head that widens downwards.
            Self::Working => {
                let shaft = u.abs() < 0.17 && (-0.1..0.62).contains(&v);
                let head = (-0.66..0.0).contains(&v) && u.abs() < 0.62 * (v + 0.66) / 0.66;
                shaft || head
            }
            Self::Paused => (u - 0.28).abs() < 0.14 && v.abs() < 0.5 || (u + 0.28).abs() < 0.14 && v.abs() < 0.5,
            Self::NeedsYou => {
                let bar = u.abs() < 0.15 && (-0.62..0.16).contains(&v);
                let dot = u * u + (v - 0.44) * (v - 0.44) < 0.17 * 0.17;
                bar || dot
            }
        }
    }
}

/// The badge: its centre and radius, and the clear ring around it, as
/// shares of the icon's width.
const BADGE: (f32, f32) = (0.74, 0.74);
const BADGE_RADIUS: f32 = 0.25;
const RING_RADIUS: f32 = 0.31;

/// Samples taken across each pixel, each way, for smooth edges.
const SAMPLES: u32 = 4;

/// How much of a pixel is inside a shape given in shares of the icon's
/// width, from 0 to 1.
fn coverage(size: u32, x: u32, y: u32, inside: &dyn Fn(f32, f32) -> bool) -> f32 {
    let mut hits = 0;
    for sy in 0..SAMPLES {
        for sx in 0..SAMPLES {
            let px = (x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) / size as f32;
            let py = (y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) / size as f32;
            if inside(px, py) {
                hits += 1;
            }
        }
    }
    hits as f32 / (SAMPLES * SAMPLES) as f32
}

fn in_circle(x: f32, y: f32, centre: (f32, f32), r: f32) -> bool {
    let (dx, dy) = (x - centre.0, y - centre.1);
    dx * dx + dy * dy < r * r
}

/// Whether a point of the badge is in its sign, in the badge's own terms.
fn in_sign(mood: Mood, x: f32, y: f32) -> bool {
    mood.sign((x - BADGE.0) / BADGE_RADIUS, (y - BADGE.1) / BADGE_RADIUS)
}

/// The badge laid over an icon: the ring cleared, the disc filled with
/// `fill`, its sign `sign`.
fn badge(icon: &mut RgbaImage, mood: Mood, fill: [u8; 3], sign: Option<[u8; 3]>) {
    let size = icon.width();
    for y in 0..size {
        for x in 0..size {
            let ring = coverage(size, x, y, &|px, py| in_circle(px, py, BADGE, RING_RADIUS));
            let disc = coverage(size, x, y, &|px, py| in_circle(px, py, BADGE, BADGE_RADIUS));
            let marked = coverage(size, x, y, &|px, py| {
                in_circle(px, py, BADGE, BADGE_RADIUS) && in_sign(mood, px, py)
            });
            let pixel = icon.get_pixel_mut(x, y);
            // What was there fades out across the ring.
            pixel.0[3] = (f32::from(pixel.0[3]) * (1.0 - ring)).round() as u8;
            if disc > 0.0 {
                let (colour, alpha) = match sign {
                    // A coloured sign on a coloured disc.
                    Some(sign) => {
                        let mix = |a: u8, b: u8| (f32::from(a) * (disc - marked) + f32::from(b) * marked) / disc;
                        (
                            [mix(fill[0], sign[0]), mix(fill[1], sign[1]), mix(fill[2], sign[2])],
                            disc,
                        )
                    }
                    // A clear sign cut out of the disc.
                    None => (
                        [f32::from(fill[0]), f32::from(fill[1]), f32::from(fill[2])],
                        disc - marked,
                    ),
                };
                let under = f32::from(pixel.0[3]) / 255.0;
                let out = alpha + under * (1.0 - alpha);
                if out > 0.0 {
                    for c in 0..3 {
                        let blended = (colour[c] * alpha + f32::from(pixel.0[c]) * under * (1.0 - alpha)) / out;
                        pixel.0[c] = blended.round().clamp(0.0, 255.0) as u8;
                    }
                }
                pixel.0[3] = (out * 255.0).round() as u8;
            }
        }
    }
}

/// A coloured icon, from the logo.
fn coloured(logo: &RgbaImage, mood: Mood) -> RgbaImage {
    let mut icon = imageops::resize(logo, 32, 32, imageops::FilterType::Lanczos3);
    if let Some(fill) = mood.colour() {
        badge(&mut icon, mood, fill, Some([0xff, 0xff, 0xff]));
    }
    icon
}

/// Whether a point is inside a rounded rectangle.
fn in_rounded(x: f32, y: f32, (left, top, right, bottom): (f32, f32, f32, f32), r: f32) -> bool {
    if x < left || x > right || y < top || y > bottom {
        return false;
    }
    // The nearest point of the rectangle the corners are rounded around.
    let nearest = (x.clamp(left + r, right - r), y.clamp(top + r, bottom - r));
    let (dx, dy) = (x - nearest.0, y - nearest.1);
    dx * dx + dy * dy <= r * r
}

/// The logo's shapes, as one colour: the front card's frame, its sun and
/// its hills, and the edge of the card behind.
fn in_logo_shape(x: f32, y: f32) -> bool {
    let front = (0.06, 0.12, 0.78, 0.82);
    let back = (0.24, 0.26, 0.94, 0.94);
    let stroke = 0.075;
    let inner = (front.0 + stroke, front.1 + stroke, front.2 - stroke, front.3 - stroke);
    let in_front = in_rounded(x, y, front, 0.14);
    let frame = in_front && !in_rounded(x, y, inner, 0.08);
    let sun = in_circle(x, y, (0.53, 0.38), 0.11);
    let hill_top = 0.6 - 0.07 * ((x - 0.12) * 5.2).sin();
    let hills = in_rounded(x, y, inner, 0.08) && y > hill_top;
    let back_edge = !in_rounded(
        x,
        y,
        (front.0 - 0.05, front.1 - 0.05, front.2 + 0.05, front.3 + 0.05),
        0.18,
    ) && in_rounded(x, y, back, 0.14)
        && !in_rounded(
            x,
            y,
            (back.0 + stroke, back.1 + stroke, back.2 - stroke, back.3 - stroke),
            0.08,
        );
    frame || (in_front && (sun || hills)) || back_edge
}

/// A Mac menu-bar icon: black and clear only.
fn template(mood: Mood) -> RgbaImage {
    let size = 44;
    let mut icon = RgbaImage::from_fn(size, size, |x, y| {
        let alpha = coverage(size, x, y, &in_logo_shape);
        Rgba([0, 0, 0, (alpha * 255.0).round() as u8])
    });
    if mood != Mood::Calm {
        badge(&mut icon, mood, [0, 0, 0], None);
    }
    icon
}

/// Every icon: its file name and what it holds.
pub fn make(root: &Path) -> Result<Vec<(String, RgbaImage)>> {
    let logo = image::open(root.join(LOGO))
        .with_context(|| format!("Couldn't read {LOGO}"))?
        .to_rgba8();
    let mut icons = Vec::new();
    for mood in MOODS {
        icons.push((format!("{}.png", mood.name()), coloured(&logo, mood)));
        icons.push((format!("{}-mac.png", mood.name()), template(mood)));
    }
    Ok(icons)
}

/// Writes every icon into [`FOLDER`].
pub fn write(root: &Path) -> Result<()> {
    let folder = root.join(FOLDER);
    std::fs::create_dir_all(&folder)?;
    for (name, icon) in make(root)? {
        icon.save(folder.join(&name))
            .with_context(|| format!("Couldn't write {name}"))?;
    }
    Ok(())
}

/// The icons in [`FOLDER`] that are not what [`make`] makes, by name.
pub fn out_of_date(root: &Path) -> Result<Vec<String>> {
    let mut stale = Vec::new();
    for (name, icon) in make(root)? {
        match image::open(root.join(FOLDER).join(&name)) {
            Ok(kept) if kept.to_rgba8() == icon => {}
            _ => stale.push(name),
        }
    }
    Ok(stale)
}

/// Fails, naming them, when the icons kept are not what is made.
pub fn check(root: &Path) -> Result<()> {
    let stale = out_of_date(root)?;
    if !stale.is_empty() {
        bail!(
            "{} out of date in {FOLDER}. Run `cargo xtask tray-icons` and commit them.",
            stale.join(", ")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
    }

    #[test]
    fn the_tray_icons_kept_are_the_ones_made_from_the_logo() {
        assert_eq!(
            out_of_date(&root()).unwrap(),
            Vec::<String>::new(),
            "run `cargo xtask tray-icons` and commit them"
        );
    }

    #[test]
    fn each_mood_differs_in_shape_not_only_colour() {
        let icons = make(&root()).unwrap();
        let shape = |name: &str| -> Vec<bool> {
            let icon = &icons.iter().find(|(n, _)| n == name).unwrap().1;
            // What is drawn at all, and what is light within it: a shape a
            // person who cannot tell colours apart still sees.
            icon.pixels()
                .flat_map(|p| {
                    let light = u32::from(p.0[0]) + u32::from(p.0[1]) + u32::from(p.0[2]) > 600;
                    [p.0[3] > 128, p.0[3] > 128 && light]
                })
                .collect()
        };
        for set in ["", "-mac"] {
            let shapes: Vec<Vec<bool>> = MOODS.iter().map(|m| shape(&format!("{}{set}.png", m.name()))).collect();
            for (i, a) in shapes.iter().enumerate() {
                for b in &shapes[i + 1..] {
                    assert_ne!(a, b, "two moods have the same shape in the {set:?} set");
                }
            }
        }
    }

    #[test]
    fn the_mac_icons_are_black_and_clear_only() {
        for (name, icon) in make(&root()).unwrap() {
            if name.ends_with("-mac.png") {
                assert_eq!(icon.dimensions(), (44, 44));
                assert!(icon.pixels().all(|p| p.0[..3] == [0, 0, 0]), "{name}");
            } else {
                assert_eq!(icon.dimensions(), (32, 32));
            }
        }
    }
}
