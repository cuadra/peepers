//! Image decoding, memory-bounded downscaling and metadata extraction.
//!
//! Runs on worker threads only — never on the UI thread.
//!
//! Two resolution tiers keep memory flat when flipping through a folder of
//! 24–100 MP photos:
//! * `Preview` — fitted to the monitor's long edge. Used for prefetched
//!   neighbours, so each costs ≈ one screenful of RGBA instead of the full
//!   bitmap.
//! * `Full` — the image at native size (capped at `FULL_MAX_EDGE` /
//!   `FULL_MAX_PIXELS`), decoded for the image you are actually on so zooming
//!   to 800% shows real pixels.

use fast_image_resize as fir;
use iced::widget::image::Handle;
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use std::io::Cursor;
use std::path::{Path, PathBuf};

pub const FULL_MAX_EDGE: u32 = 16_384;
pub const FULL_MAX_PIXELS: u64 = 160_000_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Tier {
    Preview,
    Full,
}

#[derive(Debug, Clone)]
pub struct Decoded {
    pub handle: Handle,
    /// Size of the decoded bitmap.
    pub width: u32,
    #[allow(dead_code)]
    pub height: u32,
    /// Bytes held by the bitmap (for the cache budget).
    pub bytes: usize,
    /// The tier this bitmap satisfies (`Full` if no downscale was needed).
    pub tier: Tier,
}

/// Everything the `i` panel shows. Strings are display-ready.
#[derive(Debug, Clone, Default)]
pub struct Meta {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub dimensions: String,
    pub file_size: String,
    pub format: String,
    pub camera: String,
    pub lens: String,
    pub exposure: String,
    pub captured: String,
    pub modified: String,
}

#[derive(Debug, Clone)]
pub struct Loaded {
    pub index: usize,
    pub image: Decoded,
    /// Screen-sized copy of a `Full` bitmap. The GPU samples without
    /// mipmaps, so drawing a 6000px bitmap at 1230px would shimmer and alias;
    /// below ~0.75 texels/pixel the view draws this one instead.
    pub screen: Option<Decoded>,
    /// 360×240-bounded thumbnail for the minimap (2× its CSS size).
    pub thumb: Handle,
    pub meta: Meta,
}

const DASH: &str = "—";

pub fn load(index: usize, path: &Path, tier: Tier, preview_edge: u32) -> Result<Loaded, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let fs_meta = std::fs::metadata(path).ok();

    let mut reader = ImageReader::new(Cursor::new(&bytes[..]))
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    if reader.format().is_none() {
        if let Ok(f) = ImageFormat::from_path(path) {
            reader.set_format(f);
        }
    }
    let format = reader.format();
    let mut limits = Limits::default();
    limits.max_alloc = Some(4 << 30);
    reader.limits(limits);

    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let icc = decoder.icc_profile().ok().flatten();
    let mut img = DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    img.apply_orientation(orientation);

    let (ow, oh) = (img.width(), img.height());
    // Lenient: many real files carry minor EXIF quirks; keep what parses.
    let exif = exif::Reader::new()
        .continue_on_error(true)
        .read_from_container(&mut Cursor::new(&bytes[..]))
        .or_else(|e| e.distill_partial_result(|_| {}))
        .ok();
    drop(bytes);

    // Pick the target size for this tier.
    let (tw, th) = target_size(ow, oh, tier, preview_edge);
    let tier_out = if (tw, th) == full_size(ow, oh) { Tier::Full } else { tier };

    let rgba = img.into_rgba8();
    let (w, h, pixels) = if (tw, th) == (ow, oh) {
        (ow, oh, rgba.into_raw())
    } else {
        resize(rgba.into_raw(), ow, oh, tw, th)?
    };
    let size = pixels.len();

    // Derived copies are made from the decoded bitmap (cheap vs. decoding).
    let screen = {
        let (sw, sh) = scaled(w, h, (preview_edge as f64 / w.max(h) as f64).min(1.0));
        if tier_out == Tier::Full && (sw as f64) < w as f64 * 0.75 {
            let (sw, sh, px) = resize(pixels.clone(), w, h, sw, sh)?;
            let bytes = px.len();
            Some(Decoded { handle: Handle::from_rgba(sw, sh, px), width: sw, height: sh, bytes, tier: Tier::Preview })
        } else {
            None
        }
    };
    let thumb = {
        let s = (360.0 / w as f64).min(240.0 / h as f64).min(1.0);
        let (tw, th) = scaled(w, h, s);
        let (tw, th, px) = if (tw, th) == (w, h) { (w, h, pixels.clone()) } else { resize(pixels.clone(), w, h, tw, th)? };
        Handle::from_rgba(tw, th, px)
    };

    let meta = Meta {
        path: path.to_path_buf(),
        width: ow,
        height: oh,
        dimensions: format!("{ow} × {oh} px"),
        file_size: fs_meta.as_ref().map(|m| human_size(m.len())).unwrap_or_else(|| DASH.into()),
        format: format_label(format, icc.as_deref(), exif.as_ref()),
        camera: camera(exif.as_ref()),
        lens: lens(exif.as_ref()),
        exposure: exposure(exif.as_ref()),
        captured: captured(exif.as_ref()),
        modified: fs_meta
            .and_then(|m| m.modified().ok())
            .map(date_from_system)
            .unwrap_or_else(|| DASH.into()),
    };

    Ok(Loaded {
        index,
        image: Decoded { handle: Handle::from_rgba(w, h, pixels), width: w, height: h, bytes: size, tier: tier_out },
        screen,
        thumb,
        meta,
    })
}

fn full_size(w: u32, h: u32) -> (u32, u32) {
    let mut s = (FULL_MAX_EDGE as f64 / w.max(h) as f64).min(1.0);
    let px = w as u64 * h as u64;
    if px > FULL_MAX_PIXELS {
        s = s.min((FULL_MAX_PIXELS as f64 / px as f64).sqrt());
    }
    scaled(w, h, s)
}

fn target_size(w: u32, h: u32, tier: Tier, preview_edge: u32) -> (u32, u32) {
    let full = full_size(w, h);
    match tier {
        Tier::Full => full,
        Tier::Preview => {
            let s = (preview_edge as f64 / w.max(h) as f64).min(1.0);
            let p = scaled(w, h, s);
            if p.0 * p.1 < full.0 * full.1 { p } else { full }
        }
    }
}

fn scaled(w: u32, h: u32, s: f64) -> (u32, u32) {
    if s >= 1.0 {
        (w, h)
    } else {
        (((w as f64 * s).round() as u32).max(1), ((h as f64 * s).round() as u32).max(1))
    }
}

fn resize(src: Vec<u8>, w: u32, h: u32, tw: u32, th: u32) -> Result<(u32, u32, Vec<u8>), String> {
    let src = fir::images::Image::from_vec_u8(w, h, src, fir::PixelType::U8x4).map_err(|e| e.to_string())?;
    let mut dst = fir::images::Image::new(tw, th, fir::PixelType::U8x4);
    let opts = fir::ResizeOptions::new().resize_alg(fir::ResizeAlg::Convolution(fir::FilterType::CatmullRom));
    fir::Resizer::new().resize(&src, &mut dst, &opts).map_err(|e| e.to_string())?;
    Ok((tw, th, dst.into_vec()))
}

// ───────────────────────── metadata formatting ─────────────────────────

pub fn human_size(n: u64) -> String {
    let n = n as f64;
    if n >= 1e9 {
        format!("{:.1} GB", n / 1e9)
    } else if n >= 1e6 {
        format!("{:.1} MB", n / 1e6)
    } else if n >= 1e3 {
        format!("{:.0} KB", n / 1e3)
    } else {
        format!("{n} B")
    }
}

fn format_label(f: Option<ImageFormat>, icc: Option<&[u8]>, exif: Option<&exif::Exif>) -> String {
    let name = match f {
        Some(ImageFormat::Jpeg) => "JPEG",
        Some(ImageFormat::Png) => "PNG",
        Some(ImageFormat::Gif) => "GIF",
        Some(ImageFormat::WebP) => "WebP",
        Some(ImageFormat::Tiff) => "TIFF",
        Some(ImageFormat::Bmp) => "BMP",
        Some(ImageFormat::Ico) => "ICO",
        Some(ImageFormat::Pnm) => "PNM",
        Some(ImageFormat::Qoi) => "QOI",
        Some(ImageFormat::Tga) => "TGA",
        Some(ImageFormat::Hdr) => "Radiance HDR",
        Some(ImageFormat::OpenExr) => "OpenEXR",
        Some(ImageFormat::Avif) => "AVIF",
        _ => "Image",
    };
    let space = icc.and_then(icc_description).or_else(|| {
        exif.and_then(|e| e.get_field(exif::Tag::ColorSpace, exif::In::PRIMARY))
            .and_then(|f| f.value.get_uint(0))
            .and_then(|v| (v == 1).then(|| "sRGB".to_string()))
    });
    match space {
        Some(s) => format!("{name} · {s}"),
        None => name.to_string(),
    }
}

/// Read the human-readable description tag from an ICC profile.
pub fn icc_description(icc: &[u8]) -> Option<String> {
    let be32 = |o: usize| icc.get(o..o + 4).map(|b| u32::from_be_bytes([b[0], b[1], b[2], b[3]]) as usize);
    let count = be32(128)?;
    for i in 0..count.min(256) {
        let e = 132 + i * 12;
        if icc.get(e..e + 4)? != b"desc" {
            continue;
        }
        let off = be32(e + 4)?;
        let ty = icc.get(off..off + 4)?;
        let s = if ty == b"desc" {
            let n = be32(off + 8)?;
            let raw = icc.get(off + 12..off + 12 + n)?;
            String::from_utf8_lossy(raw).trim_end_matches('\0').to_string()
        } else if ty == b"mluc" {
            let rec_off = be32(off + 16 + 8)?;
            let len = be32(off + 16 + 4)?;
            let raw = icc.get(off + rec_off..off + rec_off + len)?;
            let u: Vec<u16> = raw.chunks_exact(2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
            String::from_utf16_lossy(&u).trim_end_matches('\0').to_string()
        } else {
            return None;
        };
        let s = s.trim().to_string();
        // "sRGB IEC61966-2.1" → "sRGB"; keep short names like "Display P3".
        return Some(if s.starts_with("sRGB") { "sRGB".into() } else { s }).filter(|s| !s.is_empty());
    }
    None
}

fn ascii(e: Option<&exif::Exif>, tag: exif::Tag) -> Option<String> {
    let f = e?.get_field(tag, exif::In::PRIMARY)?;
    match &f.value {
        exif::Value::Ascii(v) => {
            let s: String = v.iter().map(|b| String::from_utf8_lossy(b).to_string()).collect::<Vec<_>>().join(" ");
            let s = s.trim_matches(|c: char| c == '\0' || c.is_whitespace()).to_string();
            (!s.is_empty()).then_some(s)
        }
        _ => None,
    }
}

fn rational(e: Option<&exif::Exif>, tag: exif::Tag) -> Option<f64> {
    let f = e?.get_field(tag, exif::In::PRIMARY)?;
    match &f.value {
        exif::Value::Rational(v) => v.first().map(|r| r.to_f64()),
        exif::Value::SRational(v) => v.first().map(|r| r.to_f64()),
        _ => None,
    }
    .filter(|v| v.is_finite())
}

fn camera(e: Option<&exif::Exif>) -> String {
    let make = ascii(e, exif::Tag::Make);
    let model = ascii(e, exif::Tag::Model);
    match (make, model) {
        (Some(mk), Some(md)) => {
            let first = mk.split_whitespace().next().unwrap_or("").to_lowercase();
            if md.to_lowercase().starts_with(&first) { md } else { format!("{} {md}", title_case(&mk)) }
        }
        (None, Some(md)) => md,
        (Some(mk), None) => title_case(&mk),
        _ => DASH.into(),
    }
}

fn title_case(s: &str) -> String {
    // "NIKON CORPORATION" → "Nikon Corporation"; leave mixed case alone.
    if s.chars().any(|c| c.is_lowercase()) {
        return s.to_string();
    }
    s.split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            c.next().map(|f| f.to_string() + &c.as_str().to_lowercase()).unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn fnum(v: f64) -> String {
    if (v - v.round()).abs() < 0.05 { format!("{:.0}", v) } else { format!("{:.1}", v) }
}

fn lens(e: Option<&exif::Exif>) -> String {
    if let Some(l) = ascii(e, exif::Tag::LensModel) {
        return l;
    }
    let focal = rational(e, exif::Tag::FocalLength);
    let max_ap = rational(e, exif::Tag::MaxApertureValue).map(|apex| 2f64.powf(apex / 2.0));
    match (focal, max_ap) {
        (Some(f), Some(a)) => format!("{} mm ƒ/{}", fnum(f), fnum(a)),
        (Some(f), None) => format!("{} mm", fnum(f)),
        _ => DASH.into(),
    }
}

fn exposure(e: Option<&exif::Exif>) -> String {
    let mut parts = vec![];
    if let Some(t) = rational(e, exif::Tag::ExposureTime).filter(|t| *t > 0.0) {
        parts.push(if t < 1.0 { format!("1/{} s", (1.0 / t).round()) } else { format!("{} s", fnum(t)) });
    }
    if let Some(f) = rational(e, exif::Tag::FNumber).filter(|f| *f > 0.0) {
        parts.push(format!("ƒ/{}", fnum(f)));
    }
    if let Some(iso) = e
        .and_then(|e| e.get_field(exif::Tag::PhotographicSensitivity, exif::In::PRIMARY))
        .and_then(|f| f.value.get_uint(0))
    {
        parts.push(format!("ISO {iso}"));
    }
    if parts.is_empty() { DASH.into() } else { parts.join(" · ") }
}

const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

fn captured(e: Option<&exif::Exif>) -> String {
    let Some(s) = ascii(e, exif::Tag::DateTimeOriginal).or_else(|| ascii(e, exif::Tag::DateTime)) else {
        return DASH.into();
    };
    // "2026:10:12 18:42:00"
    let n: Vec<u32> = s.split(|c: char| !c.is_ascii_digit()).filter_map(|p| p.parse().ok()).collect();
    if n.len() < 5 || !(1..=12).contains(&n[1]) {
        return s;
    }
    let (h, m) = (n[3], n[4]);
    let (h12, ap) = match h {
        0 => (12, "AM"),
        1..=11 => (h, "AM"),
        12 => (12, "PM"),
        _ => (h - 12, "PM"),
    };
    format!("{} {}, {} · {}:{:02} {}", MONTHS[(n[1] - 1) as usize], n[2], n[0], h12, m, ap)
}

fn date_from_system(t: std::time::SystemTime) -> String {
    let secs = match t.duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs() as libc::time_t,
        Err(_) => return DASH.into(),
    };
    // SAFETY: localtime_r writes into the provided struct only.
    let tm = unsafe {
        let mut tm: libc::tm = std::mem::zeroed();
        if libc::localtime_r(&secs, &mut tm).is_null() {
            return DASH.into();
        }
        tm
    };
    format!("{} {}, {}", MONTHS[tm.tm_mon.clamp(0, 11) as usize], tm.tm_mday, tm.tm_year + 1900)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(human_size(14_200_000), "14.2 MB");
        assert_eq!(target_size(6000, 4000, Tier::Preview, 2560), (2560, 1707));
        assert_eq!(target_size(6000, 4000, Tier::Full, 2560), (6000, 4000));
        assert_eq!(target_size(800, 600, Tier::Preview, 2560), (800, 600));
        let (w, h) = target_size(20000, 20000, Tier::Full, 2560);
        assert!(w as u64 * h as u64 <= FULL_MAX_PIXELS && w <= FULL_MAX_EDGE);
    }

    #[test]
    fn exif_shapes() {
        assert_eq!(fnum(8.0), "8");
        assert_eq!(fnum(1.8), "1.8");
    }
}

