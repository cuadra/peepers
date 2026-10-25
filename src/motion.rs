//! View math and the vim-style key-sequence engine.
//!
//! This module is pure (no UI, no I/O) so the behaviour specified in the
//! design comp can be unit-tested directly. All distances are logical pixels.
//!
//! Model (mirrors the comp's `Main.dc.html` logic):
//! * The image is first *fitted* into the viewport, leaving `MARGIN` on each
//!   side → fitted size `FW × FH`.
//! * The displayed image is `zoom × (FW, FH)`, centred on the viewport and
//!   offset by `(px, py)`.
//! * Pan is clamped so the image edge can travel at most `MARGIN` past the
//!   viewport edge: `lim = max(0, (z·FW − VW)/2 + MARGIN)`.

use std::collections::HashMap;

/// Discrete zoom stops, relative to "fit". Identical to the comp.
pub const ZOOMS: [f32; 7] = [1.0, 1.5, 2.0, 3.0, 4.0, 6.0, 8.0];
/// Breathing room around the fitted image (comp: 40px at 1440×900).
pub const MARGIN: f32 = 40.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub zoom: f32,
    pub px: f32,
    pub py: f32,
}

impl View {
    pub const FIT: View = View { zoom: 1.0, px: 0.0, py: 0.0 };

    pub fn lerp(a: View, b: View, t: f32) -> View {
        View {
            zoom: a.zoom + (b.zoom - a.zoom) * t,
            px: a.px + (b.px - a.px) * t,
            py: a.py + (b.py - a.py) * t,
        }
    }
}

/// Viewport + fitted image geometry for the current frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geometry {
    pub vw: f32,
    pub vh: f32,
    /// Fitted image width/height at zoom 1.
    pub fw: f32,
    pub fh: f32,
    pub margin: f32,
}

impl Geometry {
    /// Fit an `iw × ih` image into a `vw × vh` viewport.
    /// `upscale = false` keeps small images at their natural size.
    pub fn fit(vw: f32, vh: f32, iw: f32, ih: f32, upscale: bool) -> Geometry {
        // Shrink the margin gracefully in small tiled panes.
        let margin = MARGIN.min(0.045 * vw.min(vh)).max(0.0);
        let aw = (vw - 2.0 * margin).max(1.0);
        let ah = (vh - 2.0 * margin).max(1.0);
        let (iw, ih) = (iw.max(1.0), ih.max(1.0));
        let mut s = (aw / iw).min(ah / ih);
        if !upscale {
            s = s.min(1.0);
        }
        Geometry { vw, vh, fw: iw * s, fh: ih * s, margin }
    }

    /// Maximum |px|, |py| at a given zoom.
    pub fn limits(&self, zoom: f32) -> (f32, f32) {
        (
            ((zoom * self.fw - self.vw) / 2.0 + self.margin).max(0.0),
            ((zoom * self.fh - self.vh) / 2.0 + self.margin).max(0.0),
        )
    }

    pub fn clamp(&self, v: View) -> View {
        let (lx, ly) = self.limits(v.zoom);
        View { zoom: v.zoom, px: v.px.clamp(-lx, lx), py: v.py.clamp(-ly, ly) }
    }

    /// Screen rectangle (x, y, w, h) of the image for a view.
    pub fn image_rect(&self, v: View) -> (f32, f32, f32, f32) {
        let w = v.zoom * self.fw;
        let h = v.zoom * self.fh;
        (self.vw / 2.0 + v.px - w / 2.0, self.vh / 2.0 + v.py - h / 2.0, w, h)
    }

    /// The visible part of the image as fractions (left, top, w, h) — used by
    /// the minimap. Same formula as the comp.
    pub fn visible_fraction(&self, v: View) -> (f32, f32, f32, f32) {
        let dw = v.zoom * self.fw;
        let dh = v.zoom * self.fh;
        let fw = (self.vw / dw).min(1.0);
        let fh = (self.vh / dh).min(1.0);
        (0.5 - fw / 2.0 - v.px / dw, 0.5 - fh / 2.0 - v.py / dh, fw, fh)
    }
}

/// Every bindable action. Names are what `config.toml` uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    PanLeft,
    PanRight,
    PanUp,
    PanDown,
    HalfLeft,
    HalfRight,
    HalfUp,
    HalfDown,
    EdgeLeft,
    EdgeRight,
    EdgeBottom,
    ZoomIn,
    ZoomOut,
    Fit,
    ToggleInfo,
    ToggleHelp,
    Cancel,
    Next,
    Prev,
    First,
    Last,
    Fullscreen,
    Quit,
    /// Operator prefixes (wait for a second key).
    PrefixG,
    PrefixZ,
    PrefixMark,
    PrefixJump,
}

impl Action {
    pub fn from_name(s: &str) -> Option<Action> {
        use Action::*;
        Some(match s {
            "pan_left" => PanLeft,
            "pan_right" => PanRight,
            "pan_up" => PanUp,
            "pan_down" => PanDown,
            "half_left" => HalfLeft,
            "half_right" => HalfRight,
            "half_up" => HalfUp,
            "half_down" => HalfDown,
            "edge_left" => EdgeLeft,
            "edge_right" => EdgeRight,
            "edge_bottom" => EdgeBottom,
            "zoom_in" => ZoomIn,
            "zoom_out" => ZoomOut,
            "fit" => Fit,
            "info" => ToggleInfo,
            "help" => ToggleHelp,
            "cancel" => Cancel,
            "next" => Next,
            "prev" => Prev,
            "first" => First,
            "last" => Last,
            "fullscreen" => Fullscreen,
            "quit" => Quit,
            "prefix_g" => PrefixG,
            "prefix_z" => PrefixZ,
            "mark" => PrefixMark,
            "jump" => PrefixJump,
            _ => return None,
        })
    }
}

/// Default bindings: `h`/`l` previous/next image, `j`/`k` zoom in/out,
/// `H J K L` pan, plus the comp's other motions, `n`/`N`, `f` and `q`.
pub fn default_keymap() -> HashMap<String, Action> {
    use Action::*;
    let pairs: &[(&str, Action)] = &[
        // hjkl: browse and zoom.  HJKL: pan.
        ("h", Prev),
        ("l", Next),
        ("j", ZoomIn),
        ("k", ZoomOut),
        ("H", PanLeft),
        ("J", PanDown),
        ("K", PanUp),
        ("L", PanRight),
        ("Left", PanLeft),
        ("Right", PanRight),
        ("Up", PanUp),
        ("Down", PanDown),
        ("b", HalfLeft),
        ("w", HalfRight),
        ("{", HalfUp),
        ("}", HalfDown),
        ("0", EdgeLeft),
        ("$", EdgeRight),
        ("G", EdgeBottom),
        ("+", ZoomIn),
        ("=", ZoomIn),
        ("-", ZoomOut),
        ("i", ToggleInfo),
        ("?", ToggleHelp),
        ("Escape", Cancel),
        ("n", Next),
        ("N", Prev),
        ("Space", Next),
        ("Backspace", Prev),
        ("PageDown", Next),
        ("PageUp", Prev),
        ("Home", First),
        ("End", Last),
        ("f", Fullscreen),
        ("q", Quit),
        ("g", PrefixG),
        ("m", PrefixMark),
        ("'", PrefixJump),
    ];
    pairs.iter().map(|(k, a)| (k.to_string(), *a)).collect()
}

/// Side effects the engine asks the app to perform.
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    None,
    /// Move to a new target view (animated).
    View(View),
    ToggleInfo,
    ToggleHelp,
    /// Esc: close overlays if any are open, else reset the view to fit.
    Escape,
    /// Step through the file list by a signed count.
    Step(i64),
    /// Go to a 1-based file number (`12G`, `12gg`).
    Goto(usize),
    /// Jump to the first (`false`) or last (`true`) file.
    Ends(bool),
    Fullscreen,
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pending {
    None,
    G,
    Z,
    Mark,
    Jump,
}

/// Key-sequence state: count buffer, pending operator, marks, previous view.
#[derive(Debug, Clone)]
pub struct Motions {
    count: String,
    pending: Pending,
    pending_key: String,
    marks: HashMap<char, View>,
    prev: Option<View>,
    pub pan_step: f32,
}

impl Default for Motions {
    fn default() -> Self {
        Motions {
            count: String::new(),
            pending: Pending::None,
            pending_key: String::new(),
            marks: HashMap::new(),
            prev: None,
            pan_step: 90.0,
        }
    }
}

impl Motions {
    /// Text for the top-left buffer badge (`"3"`, `"g"`, `"12m"`), if any.
    pub fn buffer(&self) -> Option<String> {
        if self.count.is_empty() && self.pending == Pending::None {
            None
        } else {
            Some(format!("{}{}", self.count, self.pending_key))
        }
    }

    fn reset(&mut self) {
        self.count.clear();
        self.pending = Pending::None;
        self.pending_key.clear();
    }

    /// Feed one key (already normalised: a single character like `"$"`, or
    /// a named key like `"Escape"`). Returns the effect plus whether the key
    /// was consumed.
    pub fn key(
        &mut self,
        key: &str,
        keymap: &HashMap<String, Action>,
        geo: &Geometry,
        cur: View,
    ) -> (Effect, bool) {
        let n = self.count.parse::<u32>().ok().map(|n| n.max(1)).unwrap_or(1) as f32;
        let has_count = !self.count.is_empty();
        let single = {
            let mut c = key.chars();
            match (c.next(), c.next()) {
                (Some(ch), None) => Some(ch),
                _ => None,
            }
        };

        // Second key of an operator.
        match self.pending {
            Pending::None => {}
            Pending::Mark => {
                if let Some(ch) = single.filter(|c| c.is_ascii_lowercase()) {
                    self.marks.insert(ch, cur);
                }
                self.reset();
                return (Effect::None, true);
            }
            Pending::Jump => {
                let target = if key == "'" {
                    self.prev
                } else {
                    single.and_then(|c| self.marks.get(&c).copied())
                };
                self.reset();
                return match target {
                    Some(v) => {
                        self.prev = Some(cur);
                        (Effect::View(geo.clamp(v)), true)
                    }
                    None => (Effect::None, true),
                };
            }
            Pending::G => {
                let had = has_count;
                self.reset();
                if key == "g" {
                    // `gg` = top edge; with a count, `5gg` = go to file 5.
                    if had {
                        return (Effect::Goto(n as usize), true);
                    }
                    let (_, ly) = geo.limits(cur.zoom);
                    return (Effect::View(geo.clamp(View { py: ly, ..cur })), true);
                }
                return (Effect::None, true);
            }
            Pending::Z => {
                self.reset();
                if key == "z" {
                    self.prev = Some(cur);
                    return (Effect::View(View::FIT), true);
                }
                return (Effect::None, true);
            }
        }

        // Count digits (0 only continues an existing count).
        if let Some(ch) = single {
            if ch.is_ascii_digit() && (ch != '0' || has_count) && self.count.len() < 6 {
                self.count.push(ch);
                return (Effect::None, true);
            }
        }

        let Some(action) = keymap.get(key).copied() else {
            let consumed = has_count;
            self.reset();
            return (Effect::None, consumed);
        };

        use Action::*;
        let step = self.pan_step;
        let (lx, ly) = geo.limits(cur.zoom);
        let pan = |dx: f32, dy: f32| Effect::View(geo.clamp(View { px: cur.px + dx, py: cur.py + dy, ..cur }));
        let zoom_to = |z1: f32| {
            let r = z1 / cur.zoom;
            Effect::View(geo.clamp(View { zoom: z1, px: cur.px * r, py: cur.py * r }))
        };
        let zi = ZOOMS.iter().position(|z| (*z - cur.zoom).abs() < 1e-3);

        let effect = match action {
            PanLeft => pan(step * n, 0.0),
            PanRight => pan(-step * n, 0.0),
            PanUp => pan(0.0, step * n),
            PanDown => pan(0.0, -step * n),
            HalfLeft => pan(geo.vw / 2.0 * n, 0.0),
            HalfRight => pan(-geo.vw / 2.0 * n, 0.0),
            HalfUp => pan(0.0, geo.vh / 2.0 * n),
            HalfDown => pan(0.0, -geo.vh / 2.0 * n),
            EdgeLeft => Effect::View(geo.clamp(View { px: lx, ..cur })),
            EdgeRight => Effect::View(geo.clamp(View { px: -lx, ..cur })),
            EdgeBottom => {
                if has_count {
                    // `12G` = go to file 12, like a line number.
                    Effect::Goto(n as usize)
                } else {
                    Effect::View(geo.clamp(View { py: -ly, ..cur }))
                }
            }
            ZoomIn => {
                let i = zi.unwrap_or_else(|| ZOOMS.iter().rposition(|z| *z < cur.zoom).unwrap_or(0));
                zoom_to(ZOOMS[(i + n as usize).min(ZOOMS.len() - 1)])
            }
            ZoomOut => {
                let i = zi.unwrap_or_else(|| ZOOMS.iter().position(|z| *z > cur.zoom).unwrap_or(ZOOMS.len() - 1));
                zoom_to(ZOOMS[i.saturating_sub(n as usize)])
            }
            Fit => {
                self.prev = Some(cur);
                Effect::View(View::FIT)
            }
            ToggleInfo => Effect::ToggleInfo,
            ToggleHelp => Effect::ToggleHelp,
            // Esc: first cancels a pending count; otherwise the app closes an
            // open overlay or, with none open, resets the view to fit.
            Cancel if has_count => Effect::None,
            Cancel => Effect::Escape,
            Next => Effect::Step(n as i64),
            Prev => Effect::Step(-(n as i64)),
            First => Effect::Ends(false),
            Last => Effect::Ends(true),
            Fullscreen => Effect::Fullscreen,
            Quit => Effect::Quit,
            PrefixG | PrefixZ | PrefixMark | PrefixJump => {
                self.pending = match action {
                    PrefixG => Pending::G,
                    PrefixZ => Pending::Z,
                    PrefixMark => Pending::Mark,
                    _ => Pending::Jump,
                };
                self.pending_key = key.to_string();
                // Keep the count for the operator.
                return (Effect::None, true);
            }
        };
        self.reset();
        (effect, true)
    }

    /// Record a view for `''` (used when the app resets the view itself).
    pub fn remember(&mut self, v: View) {
        self.prev = Some(v);
    }

    /// Forget marks that only make sense for one image's geometry.
    pub fn on_image_change(&mut self) {
        self.reset();
        self.prev = None;
    }
}

/// CSS `cubic-bezier(x1, y1, x2, y2)` easing, solved numerically.
pub fn cubic_bezier(x1: f32, y1: f32, x2: f32, y2: f32, x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }
    let bez = |t: f32, a: f32, b: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    let dbez = |t: f32, a: f32, b: f32| {
        let u = 1.0 - t;
        3.0 * u * u * a + 6.0 * u * t * (b - a) + 3.0 * t * t * (1.0 - b)
    };
    // Newton–Raphson, falling back to bisection.
    let mut t = x;
    for _ in 0..8 {
        let err = bez(t, x1, x2) - x;
        if err.abs() < 1e-5 {
            return bez(t, y1, y2);
        }
        let d = dbez(t, x1, x2);
        if d.abs() < 1e-6 {
            break;
        }
        t = (t - err / d).clamp(0.0, 1.0);
    }
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..30 {
        t = (lo + hi) / 2.0;
        if bez(t, x1, x2) < x {
            lo = t;
        } else {
            hi = t;
        }
    }
    bez(t, y1, y2)
}

/// The comp's view transition: `.38s cubic-bezier(.2,.7,.1,1)`.
pub fn ease_view(t: f32) -> f32 {
    cubic_bezier(0.2, 0.7, 0.1, 1.0, t)
}

/// CSS `ease`, used by the overlay fade.
pub fn ease(t: f32) -> f32 {
    cubic_bezier(0.25, 0.1, 0.25, 1.0, t)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The comp's artboard: 1440×900 viewport, 3:2 photo → 1230×820 frame.
    fn comp() -> Geometry {
        Geometry::fit(1440.0, 900.0, 6000.0, 4000.0, true)
    }

    fn press(m: &mut Motions, keys: &[&str], mut v: View) -> (View, Vec<Effect>) {
        let km = default_keymap();
        let g = comp();
        let mut fx = vec![];
        for k in keys {
            let (e, _) = m.key(k, &km, &g, v);
            if let Effect::View(nv) = e {
                v = nv;
            }
            fx.push(e);
        }
        (v, fx)
    }

    #[test]
    fn fit_matches_comp_frame() {
        let g = comp();
        assert!((g.fw - 1230.0).abs() < 0.5, "{}", g.fw);
        assert!((g.fh - 820.0).abs() < 0.5, "{}", g.fh);
    }

    #[test]
    fn limits_match_comp() {
        let g = comp();
        // lim at 4×: x = (4920-1440)/2+40 = 1780, y = (3280-900)/2+40 = 1230
        let (lx, ly) = g.limits(4.0);
        assert!((lx - 1780.0).abs() < 1.0 && (ly - 1230.0).abs() < 1.0);
        // At fit there is still 40px of play vertically… but comp clamps ≥0.
        assert_eq!(g.limits(1.0).0, 0.0);
    }

    #[test]
    fn counts_and_pan() {
        let mut m = Motions::default();
        let start = View { zoom: 4.0, px: 0.0, py: 0.0 };
        let (v, _) = press(&mut m, &["3", "Right"], start);
        assert_eq!(v.px, -270.0);
        let (v, _) = press(&mut m, &["Left"], v);
        assert_eq!(v.px, -180.0);
        assert!(m.buffer().is_none());
    }

    #[test]
    fn escape_cancels_count_first() {
        let mut m = Motions::default();
        let (_, fx) = press(&mut m, &["3", "Escape"], View { zoom: 2.0, px: 0.0, py: 0.0 });
        assert_eq!(fx[1], Effect::None);
        assert!(m.buffer().is_none());
        let (_, fx) = press(&mut m, &["z", "z"], View { zoom: 2.0, px: 0.0, py: 0.0 });
        assert_eq!(fx, vec![Effect::None, Effect::None], "zz no longer bound");
    }

    #[test]
    fn hjkl_browse_and_zoom() {
        let mut m = Motions::default();
        let (_, fx) = press(&mut m, &["l"], View::FIT);
        assert_eq!(fx[0], Effect::Step(1));
        let (_, fx) = press(&mut m, &["2", "h"], View::FIT);
        assert_eq!(fx[1], Effect::Step(-2));
        let (v, _) = press(&mut m, &["j"], View::FIT);
        assert_eq!(v.zoom, 1.5);
        let (v, _) = press(&mut m, &["3", "j"], v);
        assert_eq!(v.zoom, 4.0);
        let (v, _) = press(&mut m, &["k"], v);
        assert_eq!(v.zoom, 3.0);
    }

    #[test]
    fn shift_hjkl_pan() {
        let mut m = Motions::default();
        let z4 = View { zoom: 4.0, px: 0.0, py: 0.0 };
        let (v, _) = press(&mut m, &["L"], z4);
        assert_eq!((v.px, v.py), (-90.0, 0.0));
        let (v, _) = press(&mut m, &["H", "H"], v);
        assert_eq!(v.px, 90.0);
        let (v, _) = press(&mut m, &["J"], v);
        assert_eq!(v.py, -90.0);
        let (v, _) = press(&mut m, &["2", "K"], v);
        assert_eq!(v.py, 90.0);
    }

    #[test]
    fn zoom_steps_keep_centre() {
        let mut m = Motions::default();
        let (v, _) = press(&mut m, &["+"], View { zoom: 1.0, px: 0.0, py: 0.0 });
        assert_eq!(v.zoom, 1.5);
        let (v, _) = press(&mut m, &["3", "+"], v);
        assert_eq!(v.zoom, 4.0);
        let v2 = View { px: -400.0, ..v };
        let (v3, _) = press(&mut m, &["-"], v2);
        assert_eq!(v3.zoom, 3.0);
        assert!((v3.px - -300.0).abs() < 1e-3);
        let (v4, _) = press(&mut m, &["9", "-"], v3);
        assert_eq!(v4.zoom, 1.0);
    }

    #[test]
    fn edges_and_gg() {
        let mut m = Motions::default();
        let z = View { zoom: 4.0, px: 0.0, py: 0.0 };
        let (v, _) = press(&mut m, &["$"], z);
        assert_eq!(v.px, -1780.0);
        let (v, _) = press(&mut m, &["0"], v);
        assert_eq!(v.px, 1780.0);
        let (v, _) = press(&mut m, &["g"], v);
        assert_eq!(m.buffer().as_deref(), Some("g"));
        let (v, _) = press(&mut m, &["g"], v);
        assert_eq!(v.py, 1230.0);
        let (v, _) = press(&mut m, &["G"], v);
        assert_eq!(v.py, -1230.0);
    }

    #[test]
    fn zero_extends_count() {
        let mut m = Motions::default();
        let (_, _) = press(&mut m, &["1", "0"], View::FIT);
        assert_eq!(m.buffer().as_deref(), Some("10"));
        let (_, fx) = press(&mut m, &["n"], View::FIT);
        assert_eq!(fx[0], Effect::Step(10));
    }

    #[test]
    fn marks_and_previous() {
        let mut m = Motions::default();
        let a = View { zoom: 4.0, px: -560.0, py: -640.0 };
        let (v, _) = press(&mut m, &["m", "a"], a);
        let (_, fx) = press(&mut m, &["Escape"], v);
        assert_eq!(fx[0], Effect::Escape);
        m.remember(v);
        let v = View::FIT;
        let (v, _) = press(&mut m, &["'", "a"], v);
        assert_eq!(v, a);
        let (v, _) = press(&mut m, &["'", "'"], v);
        assert_eq!(v, View::FIT);
    }

    #[test]
    fn absolute_file_jump() {
        let mut m = Motions::default();
        let (_, fx) = press(&mut m, &["1", "2", "G"], View::FIT);
        assert_eq!(fx[2], Effect::Goto(12));
    }

    #[test]
    fn minimap_fraction_matches_comp() {
        let g = comp();
        let (l, t, w, h) = g.visible_fraction(View { zoom: 4.0, px: -560.0, py: -640.0 });
        assert!((w - 1440.0 / 4920.0).abs() < 1e-4);
        assert!((h - 900.0 / 3280.0).abs() < 1e-4);
        assert!((l - (0.5 - w / 2.0 + 560.0 / 4920.0)).abs() < 1e-4);
        assert!((t - (0.5 - h / 2.0 + 640.0 / 3280.0)).abs() < 1e-4);
    }

    #[test]
    fn easing_endpoints_and_shape() {
        assert_eq!(ease_view(0.0), 0.0);
        assert_eq!(ease_view(1.0), 1.0);
        // Strong ease-out: well past halfway at t=0.25.
        assert!(ease_view(0.25) > 0.6);
        assert!((ease(0.5) - 0.8024).abs() < 0.01);
    }
}
