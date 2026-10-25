//! Application state, the decode scheduler and the event loop.
//!
//! The UI thread only ever does bookkeeping: decoding happens on worker
//! threads and arrives back as `Message::Loaded`.

use crate::config::Config;
use crate::decode::{self, Decoded, Loaded, Meta, Tier};
use crate::motion::{self, Effect, Geometry, Motions, View};
use iced::futures::channel::oneshot;
use iced::keyboard::{self, key::Named, Key};
use iced::widget::image::{self as img, Allocation};
use iced::{window, Size, Subscription, Task};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub const VIEW_ANIM: Duration = Duration::from_millis(380);
const FADE_IN: Duration = Duration::from_millis(220);
const FADE_OUT: Duration = Duration::from_millis(160);
const HINT_FOR: Duration = Duration::from_millis(4000);
const TOAST_FOR: Duration = Duration::from_millis(1100);
/// Keep frames flowing briefly after an animation ends, so the final
/// state (e.g. a fade at exactly 0) is actually drawn and presented.
const GRACE: Duration = Duration::from_millis(120);

#[derive(Debug, Clone)]
pub enum Message {
    Key(keyboard::Event),
    Resized(window::Id, Size),
    Monitor(Option<Size>, f32),
    Loaded(Tier, Box<Result<Loaded, (usize, String)>>),
    /// A bitmap finished uploading to the GPU (or failed to).
    Uploaded(usize, iced::advanced::image::Id, Option<Allocation>),
    Tick(#[allow(dead_code)] Instant),
    HideHint,
    HideToast(u64),
    Drag(f32, f32),
    DragEnd,
    Wheel { lines: f32, cx: f32, cy: f32 },
}

/// A boolean overlay with an eased opacity.
#[derive(Debug, Clone, Copy)]
pub struct Fade {
    pub on: bool,
    from: f32,
    at: Option<Instant>,
}

impl Fade {
    pub fn new(on: bool) -> Self {
        Fade { on, from: if on { 1.0 } else { 0.0 }, at: None }
    }
    pub fn set(&mut self, on: bool, now: Instant) {
        if self.on != on {
            self.from = self.alpha(now);
            self.on = on;
            self.at = Some(now);
        }
    }
    fn dur(&self) -> Duration {
        if self.on { FADE_IN } else { FADE_OUT }
    }
    pub fn alpha(&self, now: Instant) -> f32 {
        let target = if self.on { 1.0 } else { 0.0 };
        match self.at {
            None => target,
            Some(at) => {
                let t = now.saturating_duration_since(at).as_secs_f32() / self.dur().as_secs_f32();
                self.from + (target - self.from) * motion::ease(t.min(1.0))
            }
        }
    }
    pub fn animating(&self, now: Instant) -> bool {
        self.at.map(|at| now.saturating_duration_since(at) < self.dur() + GRACE).unwrap_or(false)
    }
    /// Visible at all (on, or still fading out).
    pub fn shown(&self, now: Instant) -> bool {
        self.on || self.animating(now)
    }
}

/// The bitmaps for one file: the main image, an optional screen-sized copy
/// and the minimap thumbnail.
pub struct Bundle {
    pub main: Decoded,
    pub screen: Option<Decoded>,
    pub thumb: img::Handle,
    /// GPU uploads still outstanding.
    waiting: usize,
    /// Holding the allocations keeps the textures resident on the GPU.
    gpu: Vec<Allocation>,
}

impl Bundle {
    fn handles(&self) -> Vec<img::Handle> {
        let mut v = vec![self.main.handle.clone(), self.thumb.clone()];
        if let Some(s) = &self.screen {
            v.push(s.handle.clone());
        }
        v
    }
    fn owns(&self, id: iced::advanced::image::Id) -> bool {
        self.handles().iter().any(|h| h.id() == id)
    }
    fn bytes(&self) -> usize {
        self.main.bytes + self.screen.as_ref().map(|s| s.bytes).unwrap_or(0) + 360 * 240 * 4
    }
}

/// One file in the cache.
///
/// `shown` is what gets drawn. A bundle arriving from a decoder waits in
/// `next` until every GPU upload has completed, so a swap (first paint, or
/// preview → full resolution) never shows an empty or half-uploaded frame.
struct Entry {
    shown: Option<Bundle>,
    next: Option<Bundle>,
    meta: Meta,
    tick: u64,
}

impl Entry {
    fn tier(&self) -> Option<Tier> {
        [&self.shown, &self.next].iter().filter_map(|b| b.as_ref().map(|b| b.main.tier)).max()
    }
    fn bytes(&self) -> usize {
        [&self.shown, &self.next].iter().filter_map(|b| b.as_ref().map(Bundle::bytes)).sum()
    }
}

pub struct Options {
    pub files: Vec<PathBuf>,
    pub start: usize,
    pub config: Config,
    pub keys: Vec<String>,
}

pub struct App {
    pub cfg: Config,
    pub files: Vec<PathBuf>,
    pub idx: usize,
    cache: HashMap<usize, Entry>,
    cache_bytes: usize,
    tick: u64,
    inflight: HashMap<usize, Tier>,
    pub errors: HashMap<usize, String>,
    max_jobs: usize,
    preview_edge: u32,

    pub win: Option<window::Id>,
    pub vw: f32,
    pub vh: f32,
    /// Window height; `vh` is this minus the header.
    wh: f32,
    pub now: Instant,

    pub motions: Motions,
    pub target: View,
    anim: Option<(View, Instant)>,

    pub info: Fade,
    pub help: Fade,
    pub hint: Fade,
    pub mini: Fade,
    pub toast: Fade,
    pub toast_text: String,
    toast_gen: u64,
    /// A final "settle" tick is scheduled (see `update`).
    settle_pending: bool,
    fullscreen: bool,

    pending_keys: Vec<String>,
}

impl App {
    pub fn new(o: Options) -> (App, Task<Message>) {
        let now = Instant::now();
        let jobs = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2).clamp(2, 4);
        // The header already shows the `?` shortcut; the hint is for header-less mode.
        let show_hint = o.config.show_hint && !o.config.show_header;
        let mut app = App {
            motions: { let mut m = Motions::default(); m.pan_step = o.config.pan_step; m },
            cfg: o.config,
            idx: o.start.min(o.files.len().saturating_sub(1)),
            files: o.files,
            cache: HashMap::new(),
            cache_bytes: 0,
            tick: 0,
            inflight: HashMap::new(),
            errors: HashMap::new(),
            max_jobs: jobs,
            preview_edge: 2560,
            win: None,
            vw: 1440.0,
            vh: 900.0,
            wh: 900.0,
            now,
            target: View::FIT,
            anim: None,
            info: Fade::new(false),
            help: Fade::new(false),
            hint: Fade::new(show_hint),
            mini: Fade::new(false),
            toast: Fade::new(false),
            toast_text: String::new(),
            toast_gen: 0,
            settle_pending: false,
            fullscreen: false,
            pending_keys: o.keys,
        };
        app.relayout();
        let size = window::latest().and_then(|id| window::size(id).map(move |s| Message::Resized(id, s)));
        let monitor = window::latest().and_then(|id| {
            window::monitor_size(id)
                .then(move |m| window::scale_factor(id).map(move |s| Message::Monitor(m, s)))
        });
        let mut tasks = vec![size, monitor, app.pump()];
        if show_hint {
            tasks.push(delay(HINT_FOR, Message::HideHint));
        }
        (app, Task::batch(tasks))
    }

    pub fn header_visible(&self) -> bool {
        self.cfg.show_header && !self.fullscreen
    }

    /// Recompute the image viewport after a resize or header change.
    fn relayout(&mut self) {
        let header = if self.header_visible() { crate::ui::HEADER_H } else { 0.0 };
        self.vh = (self.wh - header).max(1.0);
        if let Some(g) = self.geometry() {
            self.target = g.clamp(self.target);
        }
    }

    pub fn title(&self) -> String {
        match self.files.get(self.idx).and_then(|p| p.file_name()) {
            Some(n) => format!("{} — peepers", n.to_string_lossy()),
            None => "peepers".into(),
        }
    }

    // ───────────────────────── read access for the view ─────────────────────────

    pub fn current(&self) -> Option<(&Bundle, &Meta)> {
        self.cache.get(&self.idx).and_then(|e| e.shown.as_ref().map(|b| (b, &e.meta)))
    }

    pub fn geometry(&self) -> Option<Geometry> {
        self.cache
            .get(&self.idx)
            .map(|e| &e.meta)
            .map(|m| Geometry::fit(self.vw, self.vh, m.width as f32, m.height as f32, self.cfg.upscale))
    }

    /// The view as drawn this frame (mid-animation if one is running).
    pub fn shown_view(&self) -> View {
        match self.anim {
            Some((from, at)) => {
                let t = self.now.saturating_duration_since(at).as_secs_f32() / VIEW_ANIM.as_secs_f32();
                if t >= 1.0 { self.target } else { View::lerp(from, self.target, motion::ease_view(t)) }
            }
            None => self.target,
        }
    }

    fn animating(&self) -> bool {
        let n = self.now;
        self.anim.is_some()
            || [self.info, self.help, self.hint, self.mini, self.toast].iter().any(|f| f.animating(n))
    }

    // ───────────────────────── update ─────────────────────────

    pub fn update(&mut self, msg: Message) -> Task<Message> {
        self.now = Instant::now();
        let task = match msg {
            Message::Key(ev) => self.on_key(ev),
            Message::Resized(id, size) => {
                self.win = Some(id);
                self.vw = size.width;
                self.wh = size.height;
                self.relayout();
                Task::none()
            }
            Message::Monitor(size, scale) => {
                if let Some(s) = size {
                    let edge = (s.width.max(s.height) * scale.max(1.0)) as u32;
                    self.preview_edge = edge.clamp(1280, 8192);
                }
                Task::none()
            }
            Message::Loaded(tier, res) => self.on_loaded(tier, *res),
            Message::Uploaded(i, id, alloc) => {
                if let Some(e) = self.cache.get_mut(&i) {
                    if let Some(b) = e.next.as_mut().filter(|b| b.owns(id)) {
                        b.waiting = b.waiting.saturating_sub(1);
                        b.gpu.extend(alloc);
                        if b.waiting == 0 {
                            e.shown = e.next.take();
                            self.cache_bytes = self.cache.values().map(Entry::bytes).sum();
                        }
                    }
                }
                Task::batch([self.pump(), self.replay_keys()])
            }
            Message::Tick(_) => {
                self.settle_pending = false;
                if let Some((_, at)) = self.anim {
                    if self.now.saturating_duration_since(at) >= VIEW_ANIM + GRACE {
                        self.anim = None;
                    }
                }
                self.replay_keys()
            }
            Message::HideHint => {
                self.hint.set(false, self.now);
                Task::none()
            }
            Message::HideToast(gen) => {
                if gen == self.toast_gen {
                    self.toast.set(false, self.now);
                }
                Task::none()
            }
            Message::Drag(dx, dy) => {
                if let Some(g) = self.geometry() {
                    let v = self.shown_view();
                    self.anim = None;
                    self.target = g.clamp(View { px: v.px + dx, py: v.py + dy, ..v });
                    self.after_view_change();
                }
                Task::none()
            }
            Message::DragEnd => Task::none(),
            Message::Wheel { lines, cx, cy } => {
                if let Some(g) = self.geometry() {
                    let cur = self.target;
                    let z1 = (cur.zoom * 1.25f32.powf(lines)).clamp(motion::ZOOMS[0], *motion::ZOOMS.last().unwrap());
                    let r = z1 / cur.zoom;
                    // Keep the point under the cursor fixed.
                    let ox = cx - self.vw / 2.0;
                    let oy = cy - self.vh / 2.0;
                    let next = g.clamp(View { zoom: z1, px: ox + (cur.px - ox) * r, py: oy + (cur.py - oy) * r });
                    self.go(next);
                }
                Task::none()
            }
        };
        // Frame events can stop arriving a hair before an animation ends,
        // leaving a fade stuck at a sliver of opacity. Guarantee one update
        // after the longest animation so every fade lands on its final value.
        if self.animating() && !self.settle_pending {
            self.settle_pending = true;
            return Task::batch([task, delay(VIEW_ANIM + GRACE + Duration::from_millis(40), Message::Tick(self.now))]);
        }
        task
    }

    fn go(&mut self, v: View) {
        let from = self.shown_view();
        if from != v {
            self.anim = Some((from, self.now));
        }
        self.target = v;
        self.after_view_change();
    }

    fn after_view_change(&mut self) {
        self.mini.set(self.target.zoom > 1.001, self.now);
    }

    fn on_key(&mut self, ev: keyboard::Event) -> Task<Message> {
        if std::env::var_os("PEEPERS_DEBUG").is_some() {
            eprintln!("peepers: key {ev:?}");
        }
        let keyboard::Event::KeyPressed { modified_key, modifiers, .. } = ev else {
            return Task::none();
        };
        if modifiers.control() || modifiers.alt() || modifiers.logo() {
            return Task::none();
        }
        let Some(name) = key_name(&modified_key) else { return Task::none() };
        self.handle_key(&name)
    }

    pub fn handle_key(&mut self, name: &str) -> Task<Message> {
        if self.hint.on {
            self.hint.set(false, self.now);
        }
        let geo = self.geometry().unwrap_or(Geometry::fit(self.vw, self.vh, 1.0, 1.0, true));
        let cur = self.target;
        let (effect, _) = self.motions.key(name, &self.cfg.keymap, &geo, cur);
        match effect {
            Effect::None => Task::none(),
            Effect::View(v) => {
                if self.geometry().is_some() {
                    self.go(v);
                }
                Task::none()
            }
            Effect::ToggleInfo => {
                let on = !self.info.on;
                self.info.set(on, self.now);
                self.help.set(false, self.now);
                Task::none()
            }
            Effect::ToggleHelp => {
                let on = !self.help.on;
                self.help.set(on, self.now);
                Task::none()
            }
            Effect::Escape => {
                if self.help.on || self.info.on {
                    self.help.set(false, self.now);
                    self.info.set(false, self.now);
                } else if self.geometry().is_some() && self.target != View::FIT {
                    self.motions.remember(self.target);
                    self.go(View::FIT);
                }
                Task::none()
            }
            Effect::Step(d) => {
                let n = self.files.len() as i64;
                if n == 0 {
                    return Task::none();
                }
                let i = (self.idx as i64 + d).clamp(0, n - 1) as usize;
                self.show(i)
            }
            Effect::Goto(k) => self.show(k.saturating_sub(1).min(self.files.len().saturating_sub(1))),
            Effect::Ends(last) => self.show(if last { self.files.len().saturating_sub(1) } else { 0 }),
            Effect::Fullscreen => {
                self.fullscreen = !self.fullscreen;
                self.relayout();
                let mode = if self.fullscreen { window::Mode::Fullscreen } else { window::Mode::Windowed };
                match self.win {
                    Some(id) => window::set_mode(id, mode),
                    None => Task::none(),
                }
            }
            Effect::Quit => iced::exit(),
        }
    }

    /// Switch to file `i`: instant (zero-latency), view resets to fit.
    fn show(&mut self, i: usize) -> Task<Message> {
        if self.files.is_empty() {
            return Task::none();
        }
        let changed = i != self.idx;
        self.idx = i;
        self.motions.on_image_change();
        self.anim = None;
        self.target = View::FIT;
        self.mini = Fade::new(false);
        // Position badge: "12 / 48".
        self.toast_gen += 1;
        self.toast_text = format!("{} / {}", i + 1, self.files.len());
        self.toast.set(true, self.now);
        let hide = delay(TOAST_FOR, Message::HideToast(self.toast_gen));
        if changed {
            self.touch(i);
        }
        Task::batch([self.pump(), hide])
    }

    fn touch(&mut self, i: usize) {
        self.tick += 1;
        if let Some(e) = self.cache.get_mut(&i) {
            e.tick = self.tick;
        }
    }

    // ───────────────────────── decode scheduling ─────────────────────────

    /// Work out what should be decoding now, in priority order, and start
    /// as many jobs as the pool allows.
    fn pump(&mut self) -> Task<Message> {
        if self.files.is_empty() {
            return Task::none();
        }
        let n = self.files.len() as i64;
        let mut wanted: Vec<(usize, Tier)> = vec![(self.idx, Tier::Full)];
        for d in 1..=self.cfg.prefetch as i64 {
            for s in [d, -d] {
                let j = self.idx as i64 + s;
                if (0..n).contains(&j) {
                    wanted.push((j as usize, Tier::Preview));
                }
            }
        }

        let mut tasks = vec![];
        for (i, tier) in wanted {
            if self.inflight.len() >= self.max_jobs {
                break;
            }
            if self.errors.contains_key(&i) {
                continue;
            }
            if self.cache.get(&i).and_then(Entry::tier).map(|t| t >= tier).unwrap_or(false) {
                continue;
            }
            // A job for this file is running: let it land first.
            if self.inflight.contains_key(&i) {
                continue;
            }
            self.inflight.insert(i, tier);
            let path = self.files[i].clone();
            let edge = self.preview_edge;
            tasks.push(Task::perform(
                on_worker(move || decode::load(i, &path, tier, edge).map_err(|e| (i, e))),
                move |r| Message::Loaded(tier, Box::new(r)),
            ));
        }
        Task::batch(tasks)
    }

    fn on_loaded(&mut self, tier: Tier, res: Result<Loaded, (usize, String)>) -> Task<Message> {
        match res {
            Ok(l) => {
                self.inflight.remove(&l.index);
                let _ = tier;
                let i = l.index;
                let near = (i as i64 - self.idx as i64).unsigned_abs() as usize <= self.cfg.prefetch;
                let better = self.cache.get(&i).and_then(Entry::tier).map(|t| l.image.tier > t).unwrap_or(true);
                if near && better {
                    // Push everything to the GPU now, off the critical path,
                    // so switching to this file later draws on the next frame.
                    let mut bundle =
                        Bundle { main: l.image, screen: l.screen, thumb: l.thumb, waiting: 0, gpu: Vec::new() };
                    let handles = bundle.handles();
                    bundle.waiting = handles.len();
                    self.tick += 1;
                    let tick = self.tick;
                    let e = self.cache.entry(i).or_insert_with(|| Entry { shown: None, next: None, meta: l.meta, tick });
                    e.next = Some(bundle);
                    self.cache_bytes = self.cache.values().map(Entry::bytes).sum();
                    self.evict();
                    let uploads = handles.into_iter().map(move |h| {
                        let id = h.id();
                        img::allocate(h).map(move |r| Message::Uploaded(i, id, r.ok()))
                    });
                    return Task::batch(uploads.chain([self.pump()]));
                }
            }
            Err((i, e)) => {
                self.inflight.remove(&i);
                eprintln!("peepers: {e}");
                self.errors.insert(i, e);
            }
        }
        Task::batch([self.pump(), self.replay_keys()])
    }

    /// Drop the least useful bitmaps until under budget. The current image is
    /// never evicted; far-away images go first, then least-recently used.
    fn evict(&mut self) {
        while self.cache_bytes > self.cfg.cache_bytes {
            let idx = self.idx as i64;
            let victim = self
                .cache
                .iter()
                .filter(|(i, _)| **i != self.idx)
                .max_by_key(|(i, e)| ((**i as i64 - idx).abs(), u64::MAX - e.tick))
                .map(|(i, _)| *i);
            match victim {
                Some(v) => {
                    if let Some(e) = self.cache.remove(&v) {
                        self.cache_bytes -= e.bytes();
                    }
                }
                None => break,
            }
        }
    }

    // ───────────────────────── scripted keys (testing) ─────────────────────────

    /// `--keys` replays a key sequence once the first image is on screen.
    fn replay_keys(&mut self) -> Task<Message> {
        if self.pending_keys.is_empty() || self.win.is_none() {
            return Task::none();
        }
        let ready = self
            .cache
            .get(&self.idx)
            .map(|e| e.next.is_none() && e.shown.as_ref().map(|b| b.main.tier == Tier::Full).unwrap_or(false))
            .unwrap_or(false)
            || self.errors.contains_key(&self.idx)
            || self.files.is_empty();
        if !ready {
            return Task::none();
        }
        let keys = std::mem::take(&mut self.pending_keys);
        let tasks: Vec<_> = keys.iter().map(|k| self.handle_key(k)).collect();
        Task::batch(tasks)
    }

    // ───────────────────────── subscriptions ─────────────────────────

    pub fn subscription(&self) -> Subscription<Message> {
        let mut subs = vec![
            // All keyboard events, captured or not: no widget here takes text
            // input, so every key belongs to the motion engine.
            iced::event::listen_with(|event, _status, _id| match event {
                iced::Event::Keyboard(k) => Some(Message::Key(k)),
                _ => None,
            }),
            window::resize_events().map(|(id, s)| Message::Resized(id, s)),
        ];
        if self.animating() {
            subs.push(window::frames().map(Message::Tick));
        }
        Subscription::batch(subs)
    }
}

/// Normalise a key to the names used by the keymap.
pub fn key_name(k: &Key) -> Option<String> {
    Some(match k {
        Key::Character(c) => c.to_string(),
        Key::Named(n) => match n {
            Named::Escape => "Escape",
            Named::Space => "Space",
            Named::Backspace => "Backspace",
            Named::Enter => "Enter",
            Named::Tab => "Tab",
            Named::ArrowLeft => "Left",
            Named::ArrowRight => "Right",
            Named::ArrowUp => "Up",
            Named::ArrowDown => "Down",
            Named::PageUp => "PageUp",
            Named::PageDown => "PageDown",
            Named::Home => "Home",
            Named::End => "End",
            Named::F11 => "F11",
            _ => return None,
        }
        .to_string(),
        _ => return None,
    })
}

/// Run blocking work on a dedicated thread and await it from the runtime.
fn on_worker<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> impl std::future::Future<Output = T> {
    let (tx, rx) = oneshot::channel();
    std::thread::Builder::new()
        .name("peepers-decode".into())
        .spawn(move || {
            let _ = tx.send(f());
        })
        .expect("spawn decode thread");
    async move { rx.await.expect("decode thread dropped") }
}

fn delay(d: Duration, msg: Message) -> Task<Message> {
    Task::perform(
        on_worker(move || {
            std::thread::sleep(d);
        }),
        move |_| msg.clone(),
    )
}
