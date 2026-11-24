use image::imageops::FilterType;
use kwhale_core::{KwhaleCore, KwhaleCoverPalette, Rgb};
use slint::{Image, Rgba8Pixel, SharedPixelBuffer};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};

const MAX_CACHE_ENTRIES: usize = 256;
const MAX_CACHE_BYTES: usize = 96 * 1024 * 1024;

#[derive(Default)]
struct CacheInner {
    map: HashMap<String, DecodedCover>,
    order: VecDeque<String>,
    total_bytes: usize,
}

#[derive(Clone, Default)]
pub struct CoverMemoryCache {
    decoded: Arc<Mutex<CacheInner>>,
}

#[derive(Clone)]
pub struct DecodedCover {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub dominant: Rgb,
}

impl DecodedCover {
    pub fn to_slint_image(&self) -> Image {
        let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&self.rgba, self.width, self.height);
        Image::from_rgba8(buf)
    }

    pub fn byte_len(&self) -> usize {
        self.rgba.len()
    }

    /// Downscales a decoded cover cleanly with Lanczos3 without ever upscaling.
    pub fn resized_lanczos(&self, target_size: u32) -> Self {
        let side = self.width.min(self.height).max(1);
        let out_size = target_size.min(side).max(1);
        if self.width == out_size && self.height == out_size {
            return self.clone();
        }
        let Some(img) = image::RgbaImage::from_raw(self.width, self.height, self.rgba.clone()) else {
            return self.clone();
        };
        let resized = image::DynamicImage::ImageRgba8(img)
            .resize_exact(out_size, out_size, FilterType::Lanczos3)
            .to_rgba8();
        Self {
            width: out_size,
            height: out_size,
            rgba: resized.into_raw(),
            dominant: self.dominant,
        }
    }
}

impl CoverMemoryCache {
    pub fn get(&self, key: &str) -> Option<DecodedCover> {
        let mut guard = self.decoded.lock().ok()?;
        let val = guard.map.get(key).cloned()?;
        if let Some(pos) = guard.order.iter().position(|k| k == key) {
            guard.order.remove(pos);
            guard.order.push_back(key.to_string());
        }
        Some(val)
    }

    pub fn put(&self, key: String, val: DecodedCover) {
        if let Ok(mut guard) = self.decoded.lock() {
            let new_bytes = val.byte_len();
            if let Some(prev) = guard.map.remove(&key) {
                guard.total_bytes = guard.total_bytes.saturating_sub(prev.byte_len());
                if let Some(pos) = guard.order.iter().position(|k| k == &key) {
                    guard.order.remove(pos);
                }
            }
            while !guard.order.is_empty()
                && (guard.map.len() >= MAX_CACHE_ENTRIES
                    || guard.total_bytes.saturating_add(new_bytes) > MAX_CACHE_BYTES)
            {
                if let Some(oldest) = guard.order.pop_front() {
                    if let Some(evicted) = guard.map.remove(&oldest) {
                        guard.total_bytes = guard.total_bytes.saturating_sub(evicted.byte_len());
                    }
                }
            }
            guard.total_bytes = guard.total_bytes.saturating_add(new_bytes);
            guard.order.push_back(key.clone());
            guard.map.insert(key, val);
        }
    }
}

pub fn make_placeholder_image(seed: &str, dark: bool, size: u32, corner_radius: f32) -> Image {
    let rgba = KwhaleCoverPalette::placeholder_rgba(seed, dark, size, corner_radius);
    let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&rgba, size, size);
    Image::from_rgba8(buf)
}

/// Generates a wide 2:1 atmospheric backdrop artwork buffer for the Hero Continue Card.
pub fn make_hero_backdrop_image(seed: &str, accent: Rgb, dark: bool, width: u32, height: u32, radius: f32) -> Image {
    let rgba = render_hero_backdrop_rgba(seed, accent, dark, width, height, radius);
    let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&rgba, width, height);
    Image::from_rgba8(buf)
}

/// Generates a full-bleed atmospheric radial/diagonal scrim for the Immersive Now Playing screen.
pub fn make_player_backdrop_image(dominant: Rgb, accent: Rgb, _dark: bool, width: u32, height: u32) -> Image {
    let w = width.max(1);
    let h = height.max(1);
    // Immersive Now Playing screen uses a dark atmospheric canvas with luminous album-art glow
    let base_top = dominant.blend(Rgb::from_u32(0x131124), 0.72);
    let base_bot = accent.blend(Rgb::from_u32(0x0D0C18), 0.80);
    let glow_a = dominant.blend(accent, 0.45);
    let glow_b = accent;

    let mut rgba = vec![0u8; (w * h * 4) as usize];
    let w_f = w as f32;
    let h_f = h as f32;

    for y in 0..h {
        let ty = (y as f32 + 0.5) / h_f;
        for x in 0..w {
            let tx = (x as f32 + 0.5) / w_f;
            let mut col = base_top.blend(base_bot, (ty * 0.65 + tx * 0.35).clamp(0.0, 1.0));

            let d1 = (((tx - 0.28) * 1.15).powi(2) + (ty - 0.38).powi(2)).sqrt();
            let g1 = (1.0 - d1 * 1.45).clamp(0.0, 1.0).powi(2);
            col = col.blend(glow_a, g1 * 0.46);

            let d2 = (((tx - 0.74) * 1.10).powi(2) + (ty - 0.62).powi(2)).sqrt();
            let g2 = (1.0 - d2 * 1.55).clamp(0.0, 1.0).powi(2);
            col = col.blend(glow_b, g2 * 0.36);

            let idx = ((y * w + x) * 4) as usize;
            rgba[idx] = col.r;
            rgba[idx + 1] = col.g;
            rgba[idx + 2] = col.b;
            rgba[idx + 3] = 255;
        }
    }

    let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&rgba, w, h);
    Image::from_rgba8(buf)
}

/// Samples a pixel using bilinear interpolation from RGBA8 buffer for smooth upscaling/backdrop synthesis.
fn sample_bilinear_rgba(rgba: &[u8], w: u32, h: u32, u: f32, v: f32) -> Rgb {
    let sx = (u.clamp(0.0, 1.0) * (w.saturating_sub(1)) as f32).clamp(0.0, (w - 1) as f32);
    let sy = (v.clamp(0.0, 1.0) * (h.saturating_sub(1)) as f32).clamp(0.0, (h - 1) as f32);
    let x0 = sx.floor() as u32;
    let y0 = sy.floor() as u32;
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let fx = sx - x0 as f32;
    let fy = sy - y0 as f32;

    let read = |px: u32, py: u32| -> [f32; 3] {
        let i = ((py * w + px) * 4) as usize;
        [rgba[i] as f32, rgba[i + 1] as f32, rgba[i + 2] as f32]
    };
    let c00 = read(x0, y0);
    let c10 = read(x1, y0);
    let c01 = read(x0, y1);
    let c11 = read(x1, y1);
    let mix = |k: usize| -> u8 {
        let top = c00[k] + (c10[k] - c00[k]) * fx;
        let bot = c01[k] + (c11[k] - c01[k]) * fx;
        (top + (bot - top) * fy).round().clamp(0.0, 255.0) as u8
    };
    Rgb { r: mix(0), g: mix(1), b: mix(2) }
}

/// Fast separable box blur over a small RGB8 buffer for smooth atmospheric Hero backdrops.
fn box_blur_rgb(src: &[u8], w: usize, h: usize, radius: usize) -> Vec<u8> {
    if w == 0 || h == 0 || radius == 0 {
        return src.to_vec();
    }
    let mut tmp = vec![0u8; src.len()];
    let mut dst = vec![0u8; src.len()];
    let win = (radius * 2 + 1) as u32;
    // Horizontal pass
    for y in 0..h {
        for x in 0..w {
            let (mut sr, mut sg, mut sb) = (0u32, 0u32, 0u32);
            for dx in -(radius as isize)..=(radius as isize) {
                let sx = (x as isize + dx).clamp(0, (w - 1) as isize) as usize;
                let idx = (y * w + sx) * 3;
                sr += src[idx] as u32;
                sg += src[idx + 1] as u32;
                sb += src[idx + 2] as u32;
            }
            let out = (y * w + x) * 3;
            tmp[out] = (sr / win) as u8;
            tmp[out + 1] = (sg / win) as u8;
            tmp[out + 2] = (sb / win) as u8;
        }
    }
    // Vertical pass
    for y in 0..h {
        for x in 0..w {
            let (mut sr, mut sg, mut sb) = (0u32, 0u32, 0u32);
            for dy in -(radius as isize)..=(radius as isize) {
                let sy = (y as isize + dy).clamp(0, (h - 1) as isize) as usize;
                let idx = (sy * w + x) * 3;
                sr += tmp[idx] as u32;
                sg += tmp[idx + 1] as u32;
                sb += tmp[idx + 2] as u32;
            }
            let out = (y * w + x) * 3;
            dst[out] = (sr / win) as u8;
            dst[out + 1] = (sg / win) as u8;
            dst[out + 2] = (sb / win) as u8;
        }
    }
    dst
}

/// Generates a Hero Continue Card backdrop using an aspect-ratio-preserving blurred cover wash
/// so the standalone 1:1 square artwork inside the card never appears stretched on wide banners.
pub fn make_hero_backdrop_from_cover(
    cover_rgba: &[u8],
    cov_w: u32,
    cov_h: u32,
    dominant: Rgb,
    accent: Rgb,
    dark: bool,
    width: u32,
    height: u32,
) -> Image {
    let w = width.max(1);
    let h = height.max(1);
    if cov_w == 0 || cov_h == 0 || cover_rgba.len() < (cov_w * cov_h * 4) as usize {
        return make_hero_backdrop_image("cover", accent, dark, w, h, 0.0);
    }

    // Step 1: aspect-preserving center cover crop at compact blur resolution (never stretches 1:1 horizontally)
    let bw = 180usize;
    let bh = ((bw as f32 * h as f32 / w as f32).round() as usize).max(40);
    let target_aspect = bw as f32 / bh as f32;
    let src_aspect = cov_w as f32 / cov_h as f32;
    let (u_scale, v_scale) = if target_aspect > src_aspect {
        (1.0_f32, src_aspect / target_aspect)
    } else {
        (target_aspect / src_aspect, 1.0_f32)
    };
    let mut small = vec![0u8; bw * bh * 3];
    for y in 0..bh {
        let ty = (y as f32 + 0.5) / bh as f32;
        let cov_v = 0.5 + (ty - 0.5) * v_scale;
        for x in 0..bw {
            let tx = (x as f32 + 0.5) / bw as f32;
            let cov_u = 0.5 + (tx - 0.5) * u_scale;
            let c = sample_bilinear_rgba(cover_rgba, cov_w, cov_h, cov_u, cov_v);
            let idx = (y * bw + x) * 3;
            small[idx] = c.r;
            small[idx + 1] = c.g;
            small[idx + 2] = c.b;
        }
    }
    let blurred = box_blur_rgb(&small, bw, bh, 9);

    // Step 2: upscale blurred color field cleanly + overlay readable dark gradient scrim for typography
    let deep_scrim = Rgb::from_u32(if dark { 0x10101D } else { 0x181629 });
    let tint_anchor = dominant.blend(accent, 0.25).blend(deep_scrim, 0.58);
    let mut out = vec![255u8; (w * h * 4) as usize];
    let w_f = w as f32;
    let h_f = h as f32;

    for y in 0..h {
        let ty = (y as f32 + 0.5) / h_f;
        let fy = (ty * (bh - 1) as f32).clamp(0.0, (bh - 1) as f32);
        let y0 = fy.floor() as usize;
        let y1 = (y0 + 1).min(bh - 1);
        let wy = fy - y0 as f32;

        for x in 0..w {
            let tx = (x as f32 + 0.5) / w_f;
            let fx = (tx * (bw - 1) as f32).clamp(0.0, (bw - 1) as f32);
            let x0 = fx.floor() as usize;
            let x1 = (x0 + 1).min(bw - 1);
            let wx = fx - x0 as f32;

            let p00 = (y0 * bw + x0) * 3;
            let p10 = (y0 * bw + x1) * 3;
            let p01 = (y1 * bw + x0) * 3;
            let p11 = (y1 * bw + x1) * 3;
            let sample = |k: usize| -> u8 {
                let top = blurred[p00 + k] as f32 * (1.0 - wx) + blurred[p10 + k] as f32 * wx;
                let bot = blurred[p01 + k] as f32 * (1.0 - wx) + blurred[p11 + k] as f32 * wx;
                (top * (1.0 - wy) + bot * wy).round().clamp(0.0, 255.0) as u8
            };
            let wash = Rgb { r: sample(0), g: sample(1), b: sample(2) };
            let dark_left = (1.0 - (tx / 0.72)).clamp(0.0, 1.0) * 0.44;
            let dark_bot = ((ty - 0.42) / 0.58).clamp(0.0, 1.0) * 0.34;
            let mut col = wash.blend(tint_anchor, 0.42);
            col = col.blend(deep_scrim, (0.24 + dark_left + dark_bot).clamp(0.24, 0.78));

            let idx = ((y * w + x) * 4) as usize;
            out[idx] = col.r;
            out[idx + 1] = col.g;
            out[idx + 2] = col.b;
            out[idx + 3] = 255;
        }
    }

    let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&out, w, h);
    Image::from_rgba8(buf)
}

/// Builds a sharp 2x2 cover collage (or resized single cover) from decoded track covers.
pub fn make_collage_cover(covers: &[DecodedCover], target_size: u32) -> DecodedCover {
    let size = target_size.max(4);
    if covers.is_empty() {
        let rgba = KwhaleCoverPalette::placeholder_rgba("mix", true, size, 0.0);
        return DecodedCover {
            width: size,
            height: size,
            rgba,
            dominant: Rgb::from_u32(0x9085FF),
        };
    }
    if covers.len() == 1 {
        return covers[0].resized_lanczos(size);
    }

    let half = (size / 2).max(2);
    let cells = [
        covers[0].resized_lanczos(half),
        covers[1 % covers.len()].resized_lanczos(half),
        covers[2 % covers.len()].resized_lanczos(half),
        covers[3 % covers.len()].resized_lanczos(half),
    ];

    let mut out = vec![255u8; (size * size * 4) as usize];
    let mut sum_r: u64 = 0;
    let mut sum_g: u64 = 0;
    let mut sum_b: u64 = 0;
    let offsets = [(0u32, 0u32), (half, 0u32), (0u32, half), (half, half)];

    for (cell, (ox, oy)) in cells.iter().zip(offsets.iter()) {
        for cy in 0..half {
            let dy = oy + cy;
            if dy >= size {
                continue;
            }
            for cx in 0..half {
                let dx = ox + cx;
                if dx >= size {
                    continue;
                }
                let sidx = ((cy * half + cx) * 4) as usize;
                let didx = ((dy * size + dx) * 4) as usize;
                let (r, g, b, a) = (
                    cell.rgba[sidx],
                    cell.rgba[sidx + 1],
                    cell.rgba[sidx + 2],
                    cell.rgba[sidx + 3],
                );
                out[didx] = r;
                out[didx + 1] = g;
                out[didx + 2] = b;
                out[didx + 3] = a;
                sum_r += r as u64;
                sum_g += g as u64;
                sum_b += b as u64;
            }
        }
    }

    let count = (size as u64) * (size as u64);
    DecodedCover {
        width: size,
        height: size,
        rgba: out,
        dominant: Rgb {
            r: (sum_r / count) as u8,
            g: (sum_g / count) as u8,
            b: (sum_b / count) as u8,
        },
    }
}

pub fn render_hero_backdrop_rgba(
    seed: &str,
    accent: Rgb,
    dark: bool,
    width: u32,
    height: u32,
    corner_radius: f32,
) -> Vec<u8> {
    let width = width.max(1);
    let height = height.max(1);
    let (c1, c2) = KwhaleCoverPalette::gradient(seed, dark);
    let sky_top = c1.blend(Rgb::from_u32(0x282158), 0.52);
    let sky_mid = c2.blend(accent, 0.42);
    let sunset = Rgb::from_u32(0xEB6B88).blend(accent, 0.22);
    let horizon_gold = Rgb::from_u32(0xF59F76);
    let scrim = Rgb::from_u32(if dark { 0x151426 } else { 0x242238 });

    let mut out = vec![0u8; (width * height * 4) as usize];
    let w_f = width as f32;
    let h_f = height as f32;
    let hx = w_f * 0.5;
    let hy = h_f * 0.5;
    let r = corner_radius.clamp(0.0, hx.min(hy));

    for y in 0..height {
        let fy = y as f32 + 0.5;
        let ty = fy / h_f;
        for x in 0..width {
            let fx = x as f32 + 0.5;
            let tx = fx / w_f;

            let alpha = if r > 0.0 {
                let qx = (fx - hx).abs() - (hx - r);
                let qy = (fy - hy).abs() - (hy - r);
                let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
                let inside = qx.max(qy).min(0.0);
                let dist = outside + inside - r;
                (0.5 - dist).clamp(0.0, 1.0)
            } else {
                1.0
            };

            let base = sky_top.blend(sky_mid, (tx * 0.52 + ty * 0.48).clamp(0.0, 1.0));
            let sun_dist = (((tx - 0.72) * 1.35).powi(2) + (ty - 0.62).powi(2)).sqrt();
            let sun_glow = (1.0 - sun_dist * 1.6).clamp(0.0, 1.0);
            let mut col = base.blend(sunset, sun_glow * 0.84);
            let core_glow = (1.0 - sun_dist * 3.0).clamp(0.0, 1.0);
            col = col.blend(horizon_gold, core_glow * 0.62);

            // Soft sunset cloud bands
            let cloud = ((tx * 9.5 + ty * 5.0).sin() * 0.5 + 0.5)
                * ((tx * 4.2 - ty * 2.1).cos() * 0.5 + 0.5);
            if ty < 0.70 {
                col = col.blend(sunset, cloud * 0.24 * (1.0 - ty * 0.8));
            }

            // Natural distant hillside + gentle city roofline silhouette (no stark pillar)
            let b1 = if (0.50..0.55).contains(&tx) { 0.62 } else { 1.0 };
            let b2 = if (0.58..0.62).contains(&tx) { 0.68 } else { 1.0 };
            let b3 = if (0.64..0.70).contains(&tx) { 0.72 } else { 1.0 };
            let hills = 0.78
                + 0.045 * (tx * 11.0).sin()
                + 0.025 * (tx * 29.0).cos();
            let skyline = hills.min(b1).min(b2).min(b3);
            let sky_mask = ((ty - skyline) * 35.0).clamp(0.0, 1.0);
            col = col.blend(Rgb::from_u32(0x16142A), sky_mask * 0.78);

            // Left-side & bottom readability scrims so white text + timestamps stay crisp
            let left_scrim = (1.0 - tx * 1.25).clamp(0.0, 1.0) * 0.68;
            let bottom_scrim = ((ty - 0.45) * 1.25).clamp(0.0, 1.0) * 0.56;
            col = col.blend(scrim, (left_scrim + bottom_scrim).clamp(0.0, 0.88));

            let idx = ((y * width + x) * 4) as usize;
            out[idx] = col.r;
            out[idx + 1] = col.g;
            out[idx + 2] = col.b;
            out[idx + 3] = (alpha * 255.0).round() as u8;
        }
    }
    out
}

fn decode_any_dynamic_image(bytes: &[u8]) -> Option<image::DynamicImage> {
    if let Ok(img) = image::load_from_memory(bytes) {
        return Some(img);
    }
    if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        let mut decoder = image_webp::WebPDecoder::new(std::io::Cursor::new(bytes)).ok()?;
        let (w, h) = decoder.dimensions();
        let out_len = decoder.output_buffer_size()?;
        let mut buf = vec![0u8; out_len];
        decoder.read_image(&mut buf).ok()?;
        return if decoder.has_alpha() {
            image::RgbaImage::from_raw(w, h, buf).map(image::DynamicImage::ImageRgba8)
        } else {
            image::RgbImage::from_raw(w, h, buf).map(image::DynamicImage::ImageRgb8)
        };
    }
    None
}

pub fn decode_rounded_rgba(bytes: &[u8], target_size: u32, corner_radius: f32) -> Option<DecodedCover> {
    let img = decode_any_dynamic_image(bytes)?;
    let (w, h) = (img.width(), img.height());
    let side = w.min(h).max(1);
    let x0 = (w - side) / 2;
    let y0 = (h - side) / 2;
    // Never upscale covers: keep native square resolution if source <= target_size,
    // and downscale larger artwork cleanly with high-acutance Lanczos3.
    let out_size = target_size.max(1).min(side);
    let rgba_img = if x0 == 0 && y0 == 0 && w == side && h == side && out_size == side {
        img.into_rgba8()
    } else {
        let cropped = img.crop_imm(x0, y0, side, side);
        if out_size == side {
            cropped.into_rgba8()
        } else {
            cropped.resize_exact(out_size, out_size, FilterType::Lanczos3).to_rgba8()
        }
    };

    let mut sum_r: u64 = 0;
    let mut sum_g: u64 = 0;
    let mut sum_b: u64 = 0;
    let mut rgba = rgba_img.into_raw();
    let count = (out_size as u64) * (out_size as u64);

    if corner_radius <= 0.0 {
        for px in rgba.chunks_exact(4) {
            sum_r += px[0] as u64;
            sum_g += px[1] as u64;
            sum_b += px[2] as u64;
        }
    } else {
        let s = out_size as f32;
        let half = s * 0.5;
        let r = corner_radius.clamp(0.0, half);
        for y in 0..out_size {
            let fy = y as f32 + 0.5;
            for x in 0..out_size {
                let fx = x as f32 + 0.5;
                let idx = ((y * out_size + x) * 4) as usize;
                sum_r += rgba[idx] as u64;
                sum_g += rgba[idx + 1] as u64;
                sum_b += rgba[idx + 2] as u64;

                let qx = (fx - half).abs() - (half - r);
                let qy = (fy - half).abs() - (half - r);
                let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
                let inside = qx.max(qy).min(0.0);
                let dist = outside + inside - r;
                let mask = (0.5 - dist).clamp(0.0, 1.0);
                let a = (rgba[idx + 3] as f32 / 255.0) * mask;
                rgba[idx + 3] = (a * 255.0).round() as u8;
            }
        }
    }

    let dominant = if count > 0 {
        Rgb {
            r: (sum_r / count) as u8,
            g: (sum_g / count) as u8,
            b: (sum_b / count) as u8,
        }
    } else {
        Rgb::from_u32(0x9085FF)
    };

    Some(DecodedCover {
        width: out_size,
        height: out_size,
        rgba,
        dominant,
    })
}

pub fn spawn_fetch_cover_decoded<F>(
    rt: &tokio::runtime::Handle,
    core: Arc<KwhaleCore>,
    cache: CoverMemoryCache,
    image_id: String,
    tier: &'static str,
    target_size: u32,
    corner_radius: f32,
    on_ready: F,
) where
    F: FnOnce(DecodedCover) + Send + 'static,
{
    let cache_key = format!("{image_id}:{tier}:{target_size}:{corner_radius:.1}");
    if let Some(hit) = cache.get(&cache_key) {
        let _ = slint::invoke_from_event_loop(move || {
            on_ready(hit);
        });
        return;
    }

    rt.spawn(async move {
        let bytes = match core.get_or_fetch_image(&image_id, tier).await {
            Ok((_, b)) => b,
            Err(err) => {
                tracing::warn!(image_id = %image_id, tier, error = %err, "failed to fetch cover image");
                return;
            }
        };
        let bytes_len = bytes.len();
        let decoded = tokio::task::spawn_blocking(move || {
            decode_rounded_rgba(&bytes, target_size, corner_radius)
        })
        .await
        .ok()
        .flatten();
        if let Some(cov) = decoded {
            cache.put(cache_key, cov.clone());
            let _ = slint::invoke_from_event_loop(move || {
                on_ready(cov);
            });
        } else {
            tracing::warn!(image_id = %image_id, tier, bytes = bytes_len, "failed to decode cover image");
        }
    });
}

pub fn spawn_fetch_cover<F>(
    rt: &tokio::runtime::Handle,
    core: Arc<KwhaleCore>,
    cache: CoverMemoryCache,
    image_id: String,
    tier: &'static str,
    target_size: u32,
    corner_radius: f32,
    on_ready: F,
) where
    F: FnOnce(Image, Rgb) + Send + 'static,
{
    spawn_fetch_cover_decoded(rt, core, cache, image_id, tier, target_size, corner_radius, move |cov| {
        on_ready(cov.to_slint_image(), cov.dominant);
    });
}

#[derive(Debug, Clone, Copy)]
pub struct WidgetPalette {
    pub primary_glow: Rgb,
    pub secondary_glow: Rgb,
    pub artist_tint: Rgb,
}

fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let rf = r as f32 / 255.0;
    let gf = g as f32 / 255.0;
    let bf = b as f32 / 255.0;
    let max = rf.max(gf).max(bf);
    let min = rf.min(gf).min(bf);
    let l = (max + min) * 0.5;
    let d = max - min;
    if d < 1e-6 {
        return (0.0, 0.0, l);
    }
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let mut h = if (max - rf).abs() < 1e-6 {
        (gf - bf) / d + if gf < bf { 6.0 } else { 0.0 }
    } else if (max - gf).abs() < 1e-6 {
        (bf - rf) / d + 2.0
    } else {
        (rf - gf) / d + 4.0
    };
    h *= 60.0;
    (h, s, l)
}

fn hsl_to_rgb(h_deg: f32, s: f32, l: f32) -> Rgb {
    let h = ((h_deg % 360.0) + 360.0) % 360.0 / 360.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - ((h * 6.0) % 2.0 - 1.0).abs());
    let m = l - c * 0.5;
    let (r1, g1, b1) = match (h * 6.0).floor() as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    Rgb {
        r: ((r1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        g: ((g1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
        b: ((b1 + m) * 255.0).round().clamp(0.0, 255.0) as u8,
    }
}

/// Extracts Android PlayerWidget-style primary/secondary radial glows and pastel artist tint.
pub fn extract_widget_palette(cover: Option<(&[u8], u32, u32)>) -> WidgetPalette {
    let Some((rgba, w, h)) = cover else {
        return WidgetPalette {
            primary_glow: Rgb::from_u32(0x422F70),
            secondary_glow: Rgb::from_u32(0x282B5E),
            artist_tint: Rgb::from_u32(0xC5BBE0),
        };
    };
    if w == 0 || h == 0 || rgba.len() < (w * h * 4) as usize {
        return extract_widget_palette(None);
    }

    let step_x = (w / 24).max(1);
    let step_y = (h / 24).max(1);
    let mut best_hue = 265.0_f32;
    let mut best_sat = 0.0_f32;
    let mut best_score = -1.0_f32;
    let (mut sum_r, mut sum_g, mut sum_b, mut count) = (0u64, 0u64, 0u64, 0u64);

    for y in (0..h).step_by(step_y as usize) {
        for x in (0..w).step_by(step_x as usize) {
            let idx = ((y * w + x) * 4) as usize;
            let (r, g, b) = (rgba[idx], rgba[idx + 1], rgba[idx + 2]);
            sum_r += r as u64;
            sum_g += g as u64;
            sum_b += b as u64;
            count += 1;

            let (hue, sat, light) = rgb_to_hsl(r, g, b);
            if (0.12..=0.85).contains(&light) && sat > 0.16 {
                let lw = 1.0 - (light - 0.45).abs();
                let score = sat * 2.2 + lw * 0.5;
                if score > best_score {
                    best_score = score;
                    best_hue = hue;
                    best_sat = sat;
                }
            }
        }
    }

    if best_score < 0.0 && count > 0 {
        let (hue, sat, _) = rgb_to_hsl(
            (sum_r / count) as u8,
            (sum_g / count) as u8,
            (sum_b / count) as u8,
        );
        best_hue = hue;
        best_sat = sat.max(0.18);
    }

    WidgetPalette {
        primary_glow: hsl_to_rgb(best_hue, best_sat.clamp(0.40, 0.88), 0.25),
        secondary_glow: hsl_to_rgb((best_hue + 18.0) % 360.0, best_sat.clamp(0.30, 0.75), 0.21),
        artist_tint: hsl_to_rgb(best_hue, best_sat.clamp(0.24, 0.52), 0.74),
    }
}

/// Renders the atmospheric cover-tinted obsidian gradient backdrop matching Android PlayerWidget.kt.
pub fn make_widget_backdrop_image(pal: WidgetPalette, width: u32, height: u32) -> Image {
    let w = width.max(1);
    let h = height.max(1);
    let w_f = w as f32;
    let h_f = h as f32;
    let base_top = Rgb::from_u32(0x131118).blend(pal.primary_glow, 0.28);
    let base_mid = Rgb::from_u32(0x121017);
    let base_bot = Rgb::from_u32(0x16131D);

    let mut rgba = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        let fy = y as f32 + 0.5;
        let ty = fy / h_f;
        for x in 0..w {
            let fx = x as f32 + 0.5;
            let tx = fx / w_f;
            let diag = (tx + ty) * 0.5;
            let mut col = if diag < 0.55 {
                base_top.blend(base_mid, (diag / 0.55).clamp(0.0, 1.0))
            } else {
                base_mid.blend(base_bot, ((diag - 0.55) / 0.45).clamp(0.0, 1.0))
            };

            let dl = ((fx - w_f * 0.16).powi(2) + (fy - h_f * 0.45).powi(2)).sqrt() / (w_f * 0.46);
            if dl < 1.0 {
                let alpha = if dl < 0.5 {
                    (215.0 - dl * 2.0 * 130.0) / 255.0
                } else {
                    (85.0 * (1.0 - (dl - 0.5) * 2.0)) / 255.0
                };
                col = col.blend(pal.primary_glow, alpha.clamp(0.0, 1.0));
            }

            let dr = ((fx - w_f * 0.90).powi(2) + (fy - h_f * 0.12).powi(2)).sqrt() / (w_f * 0.42);
            if dr < 1.0 {
                let alpha = if dr < 0.55 {
                    (170.0 - (dr / 0.55) * 115.0) / 255.0
                } else {
                    (55.0 * (1.0 - (dr - 0.55) / 0.45)) / 255.0
                };
                col = col.blend(pal.secondary_glow, alpha.clamp(0.0, 1.0));
            }

            let idx = ((y * w + x) * 4) as usize;
            rgba[idx] = col.r;
            rgba[idx + 1] = col.g;
            rgba[idx + 2] = col.b;
            rgba[idx + 3] = 255;
        }
    }

    let buf = SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&rgba, w, h);
    Image::from_rgba8(buf)
}

pub fn load_tray_icon_argb32() -> Vec<ksni::Icon> {
    let png = include_bytes!("../../../assets/icons/kwhale-64.png");
    let Ok(img) = image::load_from_memory(png) else {
        return Vec::new();
    };
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width() as i32, rgba.height() as i32);
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for px in rgba.pixels() {
        data.push(px[3]);
        data.push(px[0]);
        data.push(px[1]);
        data.push(px[2]);
    }
    vec![ksni::Icon { width: w, height: h, data }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_widget_palette_and_builds_backdrop() {
        let mut rgba = vec![0u8; 32 * 32 * 4];
        for px in rgba.chunks_exact_mut(4) {
            px[0] = 210;
            px[1] = 75;
            px[2] = 140;
            px[3] = 255;
        }
        let pal = extract_widget_palette(Some((&rgba, 32, 32)));
        assert!(pal.artist_tint.r > pal.artist_tint.g);
        let _img = make_widget_backdrop_image(pal, 420, 140);
        assert!(!load_tray_icon_argb32().is_empty());
    }

    #[test]
    fn decodes_png_and_webp_covers_with_dominant_color() {
        // 1. Encode a solid-ish RGB image as PNG
        let mut rgba = vec![0u8; 32 * 32 * 4];
        for px in rgba.chunks_exact_mut(4) {
            px[0] = 200;
            px[1] = 80;
            px[2] = 120;
            px[3] = 255;
        }
        let img = image::RgbaImage::from_raw(32, 32, rgba.clone()).unwrap();
        let mut png_bytes = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
            .unwrap();
        let dec_png = decode_rounded_rgba(&png_bytes, 24, 8.0).expect("decode PNG cover");
        assert_eq!((dec_png.width, dec_png.height), (24, 24));
        assert!(dec_png.dominant.r > 180 && dec_png.dominant.b > 100);
        // Rounded corner alpha check
        assert_eq!(dec_png.rgba[3], 0);

        // 2. Encode WebP using image_webp and ensure decode never upscales small sources
        let mut webp_bytes = Vec::new();
        image_webp::WebPEncoder::new(&mut webp_bytes)
            .encode(&rgba, 32, 32, image_webp::ColorType::Rgba8)
            .unwrap();
        let dec_webp = decode_rounded_rgba(&webp_bytes, 160, 0.0).expect("decode WebP cover");
        assert_eq!((dec_webp.width, dec_webp.height), (32, 32));
        assert!(dec_webp.dominant.r > 170);
        assert_eq!(dec_webp.rgba[3], 255);

        // 3. Downscale Lanczos3 keeps exact dimensions and avoids upscaling
        let mini = dec_webp.resized_lanczos(16);
        assert_eq!((mini.width, mini.height), (16, 16));
        assert_eq!(mini.resized_lanczos(128).width, 16);

        // 4. Bilinear hero sampling works smoothly without panics on corner transitions
        let _hero = make_hero_backdrop_from_cover(
            &dec_webp.rgba,
            dec_webp.width,
            dec_webp.height,
            dec_webp.dominant,
            Rgb::from_u32(0x9085FF),
            true,
            160,
            80,
        );

        // 5. 2x2 collage combines 4 distinct quadrant colors sharply
        let solid = |r: u8, g: u8, b: u8| DecodedCover {
            width: 32,
            height: 32,
            rgba: vec![r, g, b, 255].repeat(32 * 32),
            dominant: Rgb { r, g, b },
        };
        let col = make_collage_cover(
            &[solid(255, 0, 0), solid(0, 255, 0), solid(0, 0, 255), solid(255, 255, 0)],
            64,
        );
        assert_eq!((col.width, col.height), (64, 64));
        // top-left pixel is red, top-right pixel is green, bottom-left pixel is blue
        assert_eq!(col.rgba[0..3], [255, 0, 0]);
        assert_eq!(col.rgba[(48 * 4)..(48 * 4 + 3)], [0, 255, 0]);
        assert_eq!(col.rgba[(48 * 64 * 4)..(48 * 64 * 4 + 3)], [0, 0, 255]);
    }

    #[test]
    fn memory_cache_evicts_oldest_under_budget() {
        let cache = CoverMemoryCache::default();
        let make_cov = |len_px: u32| DecodedCover {
            width: len_px,
            height: len_px,
            rgba: vec![42u8; (len_px * len_px * 4) as usize],
            dominant: Rgb::from_u32(0x8877FF),
        };
        // 3000x3000 RGBA ~= 36 MB each => 3rd insert exceeds 96 MB budget and evicts first while keeping bumped second
        cache.put("k1".into(), make_cov(3000));
        cache.put("k2".into(), make_cov(3000));
        assert!(cache.get("k1").is_some()); // bumps k1 ahead of k2 in LRU order
        cache.put("k3".into(), make_cov(3000));
        assert!(cache.get("k2").is_none());
        assert!(cache.get("k1").is_some());
        assert!(cache.get("k3").is_some());
    }
}
