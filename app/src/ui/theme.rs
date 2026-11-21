//! Dynamic colour: pick the main colour of the album art and build a
//! Material-style tonal palette (CSS custom properties) from it.

/// Most prominent *vivid* colour of an image, or a muted average when the
/// image has no strong colour (black-and-white covers, etc.).
pub fn seed_from_image(img: &image::DynamicImage) -> [u8; 3] {
    let small = img.thumbnail(48, 48).to_rgb8();
    let mut bins = [(0f64, [0f64; 3]); 36];
    let mut avg = [0f64; 3];
    let mut n = 0f64;
    for p in small.pixels() {
        let [r, g, b] = p.0;
        avg[0] += r as f64;
        avg[1] += g as f64;
        avg[2] += b as f64;
        n += 1.0;
        let (h, s, v) = rgb_to_hsv(r, g, b);
        if s < 0.2 || v < 0.18 {
            continue;
        }
        let w = s * v * v;
        let bin = &mut bins[((h / 10.0) as usize).min(35)];
        bin.0 += w;
        bin.1[0] += r as f64 * w;
        bin.1[1] += g as f64 * w;
        bin.1[2] += b as f64 * w;
    }
    // Merge each bin with its neighbours so a hue split across two bins wins.
    let best = (0..36)
        .max_by(|&a, &b| {
            let score =
                |i: usize| bins[(i + 35) % 36].0 * 0.5 + bins[i].0 + bins[(i + 1) % 36].0 * 0.5;
            score(a).total_cmp(&score(b))
        })
        .unwrap_or(0);
    let total: f64 = bins.iter().map(|b| b.0).sum();
    if bins[best].0 > 0.0 && total > n * 0.02 {
        let (w, c) = bins[best];
        [(c[0] / w) as u8, (c[1] / w) as u8, (c[2] / w) as u8]
    } else if n > 0.0 {
        [(avg[0] / n) as u8, (avg[1] / n) as u8, (avg[2] / n) as u8]
    } else {
        [128, 128, 128]
    }
}

fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f64, f64, f64) {
    let (r, g, b) = (r as f64 / 255.0, g as f64 / 255.0, b as f64 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d == 0.0 {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    let h = if h < 0.0 { h + 360.0 } else { h };
    let s = if max == 0.0 { 0.0 } else { d / max };
    (h, s, max)
}

/// Hue (degrees) and saturation (0..1) of an RGB colour.
fn hue_sat(rgb: [u8; 3]) -> (f64, f64) {
    let (r, g, b) = (
        rgb[0] as f64 / 255.0,
        rgb[1] as f64 / 255.0,
        rgb[2] as f64 / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    let d = max - min;
    let s = if d == 0.0 {
        0.0
    } else {
        d / (1.0 - (2.0 * l - 1.0).abs())
    };
    (rgb_to_hsv(rgb[0], rgb[1], rgb[2]).0, s.clamp(0.0, 1.0))
}

fn hsl(h: f64, s: f64, l: f64) -> String {
    format!(
        "hsl({:.0} {:.0}% {:.1}%)",
        h.rem_euclid(360.0),
        (s * 100.0).clamp(0.0, 100.0),
        l
    )
}

pub fn parse_hex(hex: &str) -> Option<[u8; 3]> {
    let h = hex.trim().trim_start_matches('#');
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// CSS custom properties for one scheme.
pub fn palette(seed: [u8; 3], dark: bool) -> String {
    let (h, s0) = hue_sat(seed);
    // Keep chroma in a pleasant range: muted seeds stay calm, loud seeds
    // are tamed so text stays readable.
    let s = (s0 * 0.85).clamp(0.06, 0.62);
    let n = (s * 0.28).clamp(0.04, 0.14); // neutral surfaces
    let nv = (s * 0.4).clamp(0.06, 0.2); // neutral variant
                                         // Tertiary hue: +60° like Material, but dark yellows look muddy, so
                                         // steer that range the other way round the wheel.
    let t = {
        let t = (h + 60.0).rem_euclid(360.0);
        if (40.0..=100.0).contains(&t) {
            (h - 50.0).rem_euclid(360.0)
        } else {
            t
        }
    };
    let vars: Vec<(&str, String)> = if dark {
        vec![
            ("primary", hsl(h, s.max(0.35), 78.0)),
            ("on-primary", hsl(h, s, 18.0)),
            ("primary-container", hsl(h, s * 0.75, 30.0)),
            ("on-primary-container", hsl(h, s.max(0.4), 90.0)),
            ("secondary-container", hsl(h, s * 0.35, 25.0)),
            ("on-secondary-container", hsl(h, s * 0.4, 88.0)),
            ("tertiary-container", hsl(t, (s * 0.8).max(0.3), 29.0)),
            ("on-tertiary-container", hsl(t, (s * 0.8).max(0.35), 90.0)),
            ("surface", hsl(h, n, 6.5)),
            ("surface-dim", hsl(h, n, 5.0)),
            ("surface-container-low", hsl(h, n, 9.0)),
            ("surface-container", hsl(h, n, 11.5)),
            ("surface-container-high", hsl(h, n, 15.0)),
            ("surface-container-highest", hsl(h, n, 19.0)),
            ("on-surface", hsl(h, n, 91.0)),
            ("on-surface-variant", hsl(h, nv, 74.0)),
            ("outline", hsl(h, nv, 45.0)),
            ("outline-variant", hsl(h, nv, 24.0)),
            ("error", "hsl(6 90% 74%)".into()),
            ("ok", "hsl(140 45% 66%)".into()),
            ("scrim", "rgba(0,0,0,.55)".into()),
        ]
    } else {
        vec![
            ("primary", hsl(h, s.max(0.35), 36.0)),
            ("on-primary", "#ffffff".into()),
            ("primary-container", hsl(h, s.max(0.35), 89.0)),
            ("on-primary-container", hsl(h, s, 14.0)),
            ("secondary-container", hsl(h, s * 0.4, 89.0)),
            ("on-secondary-container", hsl(h, s * 0.4, 15.0)),
            ("tertiary-container", hsl(t, (s * 0.8).max(0.35), 88.0)),
            ("on-tertiary-container", hsl(t, s * 0.5, 15.0)),
            ("surface", hsl(h, n * 1.5, 98.0)),
            ("surface-dim", hsl(h, n, 87.0)),
            ("surface-container-low", hsl(h, n * 1.5, 96.0)),
            ("surface-container", hsl(h, n * 1.5, 94.0)),
            ("surface-container-high", hsl(h, n * 1.5, 92.0)),
            ("surface-container-highest", hsl(h, n * 1.5, 90.0)),
            ("on-surface", hsl(h, n, 11.0)),
            ("on-surface-variant", hsl(h, nv, 30.0)),
            ("outline", hsl(h, nv, 50.0)),
            ("outline-variant", hsl(h, nv, 80.0)),
            ("error", "hsl(4 70% 45%)".into()),
            ("ok", "hsl(140 55% 32%)".into()),
            ("scrim", "rgba(0,0,0,.35)".into()),
        ]
    };
    let mut css = String::new();
    for (k, v) in vars {
        css.push_str(&format!("--{k}:{v};"));
    }
    css.push_str(if dark {
        "color-scheme:dark;"
    } else {
        "color-scheme:light;"
    });
    css
}

/// A `<style>` body applying the palette for the chosen theme mode.
pub fn theme_css(seed: [u8; 3], mode: &str) -> String {
    match mode {
        "dark" => format!(":root{{{}}}", palette(seed, true)),
        "light" => format!(":root{{{}}}", palette(seed, false)),
        _ => format!(
            ":root{{{}}}@media (prefers-color-scheme: dark){{:root{{{}}}}}",
            palette(seed, false),
            palette(seed, true)
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_vivid_colour_over_dark_background() {
        let mut img = image::RgbImage::from_pixel(60, 60, image::Rgb([10, 10, 12]));
        for y in 20..40 {
            for x in 20..40 {
                img.put_pixel(x, y, image::Rgb([230, 80, 60]));
            }
        }
        let seed = seed_from_image(&image::DynamicImage::ImageRgb8(img));
        assert!(seed[0] > 200 && seed[1] < 120, "{seed:?}");
    }

    #[test]
    fn grey_image_gives_neutral_seed() {
        let img = image::RgbImage::from_pixel(10, 10, image::Rgb([120, 120, 120]));
        let seed = seed_from_image(&image::DynamicImage::ImageRgb8(img));
        // Neutral: all channels equal, close to the input (resizing may round).
        assert!(
            seed[0] == seed[1] && seed[1] == seed[2] && seed[0].abs_diff(120) <= 2,
            "{seed:?}"
        );
        assert!(palette(seed, true).contains("--surface:"));
    }
}
