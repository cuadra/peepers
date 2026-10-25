//! The interface — a translation of the comp's artboards.
//!
//! A slim header (brand left, `?` shortcut right) sits above the image
//! area; it hides in fullscreen. Image-area layers, back to front:
//! 1. the image (canvas; pan/zoom transform, drag + wheel)
//! 2. minimap + zoom label (bottom-right, only when zoomed)
//! 3. count/operator buffer or position badge (top-left)
//! 4. launch hint (bottom-left)
//! 5. image info panel (`i`, bottom-left)
//! 6. motion sheet (`?`, centred, dims the whole window)

use crate::app::{App, Message};
use crate::files;
use crate::motion::{Geometry, View};
use crate::theme::{self, fade, line, rgba, MONO, MUTED, SANS, SERIF, SERIF_ITALIC, TEXT, TEXT_2};
use iced::advanced::image::FilterMethod;
use iced::alignment::{Horizontal, Vertical};
use iced::mouse;
use iced::widget::canvas::{self, Canvas, Event, Frame, Geometry as CanvasGeometry, Path, Stroke};
use iced::widget::image::Handle;
use iced::widget::text::{LineHeight, Span};
use iced::widget::{column, container, rich_text, row, stack, text, Column, Row, Space};
use iced::{border, Background, Border, Color, Element, Font, Length, Padding, Point, Rectangle, Renderer, Shadow, Size, Theme, Vector};

const EDGE: f32 = 28.0;
/// Header height in logical px (43 + 1px hairline).
pub const HEADER_H: f32 = 44.0;

pub fn view(app: &App) -> Element<'_, Message> {
    let now = app.now;
    let accent = app.cfg.accent;
    let mut layers: Vec<Element<Message>> = vec![];

    // 1 · image
    let geo = app.geometry();
    let shown = app.shown_view();
    // Choose the bitmap: the screen-sized copy while the full one would be
    // minified (no mipmaps on the GPU), the full one once zoomed in.
    let (handle, px_per_texel) = match (app.current(), geo) {
        (Some((b, _)), Some(g)) => {
            let rect_w = shown.zoom * g.fw;
            let full = rect_w / b.main.width as f32;
            match &b.screen {
                Some(s) if full < 0.75 => (Some(s.handle.clone()), rect_w / s.width as f32),
                _ => (Some(b.main.handle.clone()), full),
            }
        }
        _ => (None, 1.0),
    };
    let status = if app.files.is_empty() {
        Some("No images here".to_string())
    } else { app.errors.get(&app.idx).map(|e| format!("Can't open {} — {}", file_name(app), short_err(e))) };
    layers.push(
        Canvas::new(Stage { handle, geo, view: shown, nearest: px_per_texel >= 3.0, status })
            .width(Length::Fill)
            .height(Length::Fill)
            .into(),
    );

    // 2 · minimap
    if let (Some((b, meta)), Some(g)) = (app.current(), geo) {
        if app.mini.shown(now) {
            let a = app.mini.alpha(now);
            layers.push(minimap(b.thumb.clone(), meta.width as f32, meta.height as f32, g, shown, accent, a));
        }
    }

    // 3 · buffer badge (pending count / operator) or position toast
    if let Some(buf) = app.motions.buffer() {
        layers.push(badge(buf, accent, 1.0));
    } else if app.toast.shown(now) && app.files.len() > 1 {
        layers.push(badge(app.toast_text.clone(), theme::MUTED, app.toast.alpha(now)));
    }

    // 4 · hint
    if app.hint.shown(now) && !app.info.on && !app.help.on {
        layers.push(hint(app.hint.alpha(now)));
    }

    // 5 · info
    if app.info.shown(now) && !app.help.on {
        if let Some((_, meta)) = app.current() {
            layers.push(info_panel(meta, shown.zoom, accent, app.info.alpha(now), app.vw));
        }
    }

    let image_area = stack(layers).width(Length::Fill).height(Length::Fill);
    let body: Element<Message> = if app.header_visible() {
        column![header(accent), image_area].into()
    } else {
        image_area.into()
    };

    // 6 · help — over the whole window, header included
    let mut root: Vec<Element<Message>> = vec![body];
    if app.help.shown(now) {
        root.push(help_sheet(accent, app.help.alpha(now), app.vw));
    }

    container(stack(root).width(Length::Fill).height(Length::Fill))
        .style(|_| container::Style { background: Some(Background::Color(theme::BG)), ..Default::default() })
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

// ───────────────────────── header ─────────────────────────

/// Brand mark: two eyes glancing sideways.
struct Eyes {
    accent: Color,
}

impl canvas::Program<Message> for Eyes {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<CanvasGeometry> {
        let mut f = Frame::new(renderer, bounds.size());
        let r = bounds.height / 2.0;
        for cx in [r, bounds.width - r] {
            f.fill(&Path::circle(Point::new(cx, r), r), self.accent);
            f.fill(&Path::circle(Point::new(cx + r * 0.38, r), r * 0.42), theme::BG);
        }
        vec![f.into_geometry()]
    }
}

fn header<'a>(accent: Color) -> Element<'a, Message> {
    let brand = row![
        Canvas::new(Eyes { accent }).width(Length::Fixed(23.0)).height(Length::Fixed(10.0)),
        text("Peepers").font(SERIF).size(21).color(TEXT).line_height(LineHeight::Relative(1.0)),
    ]
    .spacing(10)
    .align_y(Vertical::Center);

    let shortcut = row![kbd_small("?", 1.0), text("Shortcuts").size(13).color(MUTED)]
        .spacing(8)
        .align_y(Vertical::Center);

    let bar = container(
        row![brand, Space::new().width(Length::Fill), shortcut].align_y(Vertical::Center).height(Length::Fill),
    )
    .padding([0, 28])
    .width(Length::Fill)
    .height(Length::Fixed(HEADER_H - 1.0));

    column![bar, divider(1.0)].into()
}

fn file_name(app: &App) -> String {
    app.files.get(app.idx).and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

fn short_err(e: &str) -> String {
    let e = e.rsplit(": ").next().unwrap_or(e);
    e.chars().take(80).collect()
}

// ───────────────────────── 1 · the image ─────────────────────────

struct Stage {
    handle: Option<Handle>,
    geo: Option<Geometry>,
    view: View,
    nearest: bool,
    status: Option<String>,
}

#[derive(Default)]
struct StageState {
    drag: Option<Point>,
}

impl canvas::Program<Message> for Stage {
    type State = StageState;

    fn update(
        &self,
        state: &mut StageState,
        event: &Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        match event {
            Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                state.drag = cursor.position_in(bounds);
                state.drag.map(|_| canvas::Action::capture())
            }
            Event::Mouse(mouse::Event::CursorMoved { .. }) => {
                let (Some(from), Some(to)) = (state.drag, cursor.position_in(bounds)) else { return None };
                state.drag = Some(to);
                Some(canvas::Action::publish(Message::Drag(to.x - from.x, to.y - from.y)).and_capture())
            }
            Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.drag.take().map(|_| canvas::Action::publish(Message::DragEnd))
            }
            Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let p = cursor.position_in(bounds)?;
                let lines = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => *y,
                    mouse::ScrollDelta::Pixels { y, .. } => y / 60.0,
                };
                Some(canvas::Action::publish(Message::Wheel { lines, cx: p.x, cy: p.y }).and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &StageState,
        renderer: &Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<CanvasGeometry> {
        let mut frame = Frame::new(renderer, bounds.size());
        if let (Some(h), Some(g)) = (&self.handle, self.geo) {
            let (x, y, w, hgt) = g.image_rect(self.view);
            let mut img = canvas::Image::new(h.clone())
                .filter_method(if self.nearest { FilterMethod::Nearest } else { FilterMethod::Linear })
                .snap(false);
            img.border_radius = border::Radius::default();
            frame.draw_image(Rectangle::new(Point::new(x, y), Size::new(w, hgt)), img);
        }
        if let Some(s) = &self.status {
            frame.fill_text(canvas::Text {
                content: s.clone(),
                position: frame.center(),
                color: MUTED,
                size: 15.0.into(),
                font: SANS,
                align_x: iced::widget::text::Alignment::Center,
                align_y: Vertical::Center,
                ..Default::default()
            });
        }
        let _ = state;
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(&self, state: &StageState, _b: Rectangle, _c: mouse::Cursor) -> mouse::Interaction {
        if state.drag.is_some() { mouse::Interaction::Grabbing } else { mouse::Interaction::default() }
    }
}

// ───────────────────────── 2 · minimap ─────────────────────────

/// Thumbnail layer (images draw above geometry within a layer, so the
/// viewport overlay lives in its own canvas stacked on top).
struct Thumb {
    handle: Handle,
    alpha: f32,
}

impl canvas::Program<Message> for Thumb {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<CanvasGeometry> {
        let mut f = Frame::new(renderer, bounds.size());
        let mut img = canvas::Image::new(self.handle.clone()).opacity(self.alpha);
        img.border_radius = border::Radius::from(6.0);
        f.draw_image(Rectangle::new(Point::ORIGIN, bounds.size()), img);
        vec![f.into_geometry()]
    }
}

struct Mini {
    frac: (f32, f32, f32, f32),
    accent: Color,
    alpha: f32,
}

impl canvas::Program<Message> for Mini {
    type State = ();

    fn draw(&self, _: &(), renderer: &Renderer, _: &Theme, bounds: Rectangle, _: mouse::Cursor) -> Vec<CanvasGeometry> {
        let mut f = Frame::new(renderer, bounds.size());
        let (w, h) = (bounds.width, bounds.height);

        // Visible-area rectangle, clipped to the thumbnail.
        let (l, t, fw, fh) = self.frac;
        let x0 = (l * w).clamp(0.0, w);
        let y0 = (t * h).clamp(0.0, h);
        let x1 = ((l + fw) * w).clamp(0.0, w);
        let y1 = ((t + fh) * h).clamp(0.0, h);

        // Dim outside it (the comp's 999px box-shadow), honouring the 6px radius.
        let dim = Path::new(|b| {
            b.rounded_rectangle(Point::ORIGIN, bounds.size(), 6.0.into());
            b.rectangle(Point::new(x0, y0), Size::new(x1 - x0, y1 - y0));
        });
        f.fill(
            &dim,
            canvas::Fill { style: canvas::Style::Solid(fade(rgba(8, 8, 7, 0.5), self.alpha)), rule: canvas::fill::Rule::EvenOdd },
        );
        let r = Path::rounded_rectangle(
            Point::new(x0 + 0.75, y0 + 0.75),
            Size::new((x1 - x0 - 1.5).max(0.0), (y1 - y0 - 1.5).max(0.0)),
            2.0.into(),
        );
        f.stroke(&r, Stroke::default().with_color(fade(self.accent, self.alpha)).with_width(1.5));
        // 1px hairline ring.
        let ring = Path::rounded_rectangle(Point::new(0.5, 0.5), Size::new(w - 1.0, h - 1.0), 5.5.into());
        f.stroke(&ring, Stroke::default().with_color(fade(line(0.16), self.alpha)).with_width(1.0));
        vec![f.into_geometry()]
    }
}

fn minimap<'a>(handle: Handle, iw: f32, ih: f32, g: Geometry, v: View, accent: Color, a: f32) -> Element<'a, Message> {
    // Comp: 180×120 for a 3:2 photo — fit other aspect ratios in that box.
    let s = (180.0 / iw).min(120.0 / ih);
    let (mw, mh) = ((iw * s).round(), (ih * s).round());
    let map = container(stack![
        Canvas::new(Thumb { handle, alpha: a }).width(Length::Fixed(mw)).height(Length::Fixed(mh)),
        Canvas::new(Mini { frac: g.visible_fraction(v), accent, alpha: a })
            .width(Length::Fixed(mw))
            .height(Length::Fixed(mh)),
    ])
    .style(move |_| container::Style {
        border: Border { radius: 6.0.into(), ..Default::default() },
        shadow: Shadow { color: fade(rgba(0, 0, 0, 0.5), a), offset: Vector::new(0.0, 18.0), blur_radius: 40.0 },
        ..Default::default()
    });
    let label = text(format!("{}%", (v.zoom * 100.0).round() as i32)).font(MONO).size(11).color(fade(MUTED, a));
    let col = column![map, label].spacing(10).align_x(Horizontal::Right);
    container(col)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding { top: 0.0, right: EDGE, bottom: EDGE - 6.0 * (1.0 - a), left: 0.0 })
        .align_x(Horizontal::Right)
        .align_y(Vertical::Bottom)
        .into()
}

// ───────────────────────── 3 · badge ─────────────────────────

fn badge<'a>(s: String, color: Color, a: f32) -> Element<'a, Message> {
    let chip = container(text(s).font(MONO).size(13).color(fade(color, a)))
        .height(30)
        .padding([0, 12])
        .center_y(30)
        .align_x(Horizontal::Center)
        .style(move |_| container::Style {
            background: Some(Background::Color(fade(rgba(14, 13, 12, 0.82), a))),
            border: Border { color: fade(color, a), width: 1.0, radius: 8.0.into() },
            ..Default::default()
        });
    let chip = container(chip).width(Length::Shrink);
    container(row![chip, Space::new().width(Length::Fixed(0.0))].height(30))
        .padding(EDGE)
        .width(Length::Fill)
        .height(Length::Fill)
        .into()
}

// ───────────────────────── 4 · hint ─────────────────────────

fn kbd_small<'a>(k: &str, a: f32) -> Element<'a, Message> {
    container(text(k.to_string()).font(MONO).size(11).color(fade(TEXT, a)).line_height(LineHeight::Absolute(22.0.into())))
        .height(22)
        .width(Length::Shrink)
        .padding([0, 6])
        .align_x(Horizontal::Center)
        .style(move |_| container::Style {
            border: Border { color: fade(line(0.18), a), width: 1.0, radius: 5.0.into() },
            ..Default::default()
        })
        .into()
}

fn hint<'a>(a: f32) -> Element<'a, Message> {
    let t = |s: &str| text(s.to_string()).size(13).color(fade(MUTED, a));
    let r = row![kbd_small("?", a), t("motions"), Space::new().width(Length::Fixed(8.0)), kbd_small("i", a), t("info")]
        .spacing(8)
        .align_y(Vertical::Center);
    container(r)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(EDGE)
        .align_x(Horizontal::Left)
        .align_y(Vertical::Bottom)
        .into()
}

// ───────────────────────── helpers ─────────────────────────

/// Letter-spaced mono label (CSS `letter-spacing`), which iced text lacks.
fn tracked<'a>(s: &str, size: f32, tracking_em: f32, color: Color) -> Element<'a, Message> {
    let mut r = Row::new().spacing(size * tracking_em);
    for ch in s.chars() {
        r = r.push(text(ch.to_string()).font(MONO).size(size).color(color));
    }
    r.into()
}

fn panel_style(bg: Color, radius: f32, shadow_y: f32, shadow_blur: f32, shadow_a: f32, a: f32) -> container::Style {
    container::Style {
        background: Some(Background::Color(fade(bg, a))),
        border: Border { color: fade(line(0.09), a), width: 1.0, radius: radius.into() },
        shadow: Shadow {
            color: fade(Color::from_rgba(0.0, 0.0, 0.0, shadow_a), a),
            offset: Vector::new(0.0, shadow_y),
            blur_radius: shadow_blur,
        },
        ..Default::default()
    }
}

fn divider<'a>(a: f32) -> Element<'a, Message> {
    container(Space::new())
        .width(Length::Fill)
        .height(1)
        .style(move |_| container::Style { background: Some(Background::Color(fade(line(0.09), a))), ..Default::default() })
        .into()
}

// ───────────────────────── 5 · info panel ─────────────────────────

fn info_panel<'a>(m: &crate::decode::Meta, zoom: f32, accent: Color, a: f32, vw: f32) -> Element<'a, Message> {
    let name = m.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (name[..i].to_string(), name[i..].to_string()),
        _ => (name.clone(), String::new()),
    };
    let title = rich_text![
        Span::<(), Font>::new(stem).font(SERIF).size(46).color(fade(TEXT, a)),
        Span::<(), Font>::new(ext).font(SERIF_ITALIC).size(46).color(fade(MUTED, a)),
    ]
    .line_height(LineHeight::Relative(1.0))
    .wrapping(iced::widget::text::Wrapping::Glyph);

    let head_left = column![tracked(&files::pretty_dir(&m.path), 11.0, 0.16, fade(accent, a)), title].spacing(6);
    let head = row![
        container(head_left).width(Length::Fill),
        text("i to hide").font(MONO).size(11).color(fade(MUTED, a)),
    ]
    .spacing(24)
    .align_y(Vertical::Bottom);

    let groups: [[(&str, String); 3]; 3] = [
        [("DIMENSIONS", m.dimensions.clone()), ("FILE SIZE", m.file_size.clone()), ("FORMAT", m.format.clone())],
        [("CAMERA", m.camera.clone()), ("LENS", m.lens.clone()), ("EXPOSURE", m.exposure.clone())],
        [
            ("CAPTURED", m.captured.clone()),
            ("MODIFIED", m.modified.clone()),
            ("VIEW", format!("{}% of fit", (zoom * 100.0).round() as i32)),
        ],
    ];
    let cols: Vec<Element<Message>> = groups
        .into_iter()
        .map(|g| {
            let mut c = Column::new().spacing(14).width(Length::FillPortion(1));
            for (k, v) in g {
                c = c.push(
                    column![tracked(k, 10.5, 0.16, fade(theme::LABEL, a)), text(v).font(SANS).size(15).color(fade(TEXT, a))]
                        .spacing(3),
                );
            }
            c.into()
        })
        .collect();
    // Narrow tiles: stack the three groups.
    let grid: Element<Message> = if vw < 620.0 {
        Column::with_children(cols).spacing(20).into()
    } else {
        Row::with_children(cols).spacing(32).into()
    };

    let panel = container(column![head, divider(a), grid].spacing(26))
        .padding(Padding { top: 30.0, right: 34.0, bottom: 32.0, left: 34.0 })
        .max_width(760)
        .style(move |_| panel_style(rgba(14, 13, 12, 0.86), 18.0, 30.0, 80.0, 0.45, a));

    container(panel)
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(Padding { top: EDGE, right: EDGE, bottom: EDGE - 6.0 * (1.0 - a), left: EDGE })
        .align_x(Horizontal::Left)
        .align_y(Vertical::Bottom)
        .into()
}

// ───────────────────────── 6 · motion sheet ─────────────────────────

/// (keys, description)
type HelpRow = (&'static [&'static str], &'static str);

pub const HELP: &[(&str, &[HelpRow])] = &[
    ("PAN", &[(&["H", "J", "K", "L"], "Nudge the view"), (&["b", "w"], "Half a screen left / right"), (&["{", "}"], "Half a screen up / down")]),
    ("EDGES", &[(&["0", "$"], "Left / right edge"), (&["g", "g"], "Top edge"), (&["G"], "Bottom edge")]),
    ("ZOOM", &[(&["j", "k"], "In / out one step"), (&["3", "j"], "Three steps at once"), (&["Esc"], "Reset to fit")]),
    ("REMEMBER", &[(&["m", "a"], "Save this view as a"), (&["'", "a"], "Return to view a"), (&["'", "'"], "Previous view")]),
    ("OVERLAYS", &[(&["i"], "Image information"), (&["?"], "This sheet"), (&["Esc"], "Close overlay")]),
    ("FILES", &[(&["h", "l"], "Previous / next image"), (&["5", "G"], "Go to image five"), (&["q"], "Quit")]),
];

fn kbd<'a>(k: &str, a: f32) -> Element<'a, Message> {
    container(text(k.to_string()).font(MONO).size(12).color(fade(TEXT, a)).line_height(LineHeight::Absolute(26.0.into())))
        .height(26)
        .width(Length::Fixed(kbd_width(k)))
        .align_x(Horizontal::Center)
        .style(move |_| container::Style {
            background: Some(Background::Color(fade(line(0.06), a))),
            border: Border { color: fade(line(0.14), a), width: 1.0, radius: 6.0.into() },
            ..Default::default()
        })
        .into()
}

/// CSS: `min-width: 26px; padding: 0 7px` with 12px JetBrains Mono (0.6em advance).
fn kbd_width(k: &str) -> f32 {
    (k.chars().count() as f32 * 7.2 + 14.0).max(26.0).round()
}

fn help_sheet<'a>(accent: Color, a: f32, vw: f32) -> Element<'a, Message> {
    let title = rich_text![
        Span::<(), Font>::new("Motions").font(SERIF).size(44).color(fade(TEXT, a)),
        Span::<(), Font>::new(", only.").font(SERIF_ITALIC).size(44).color(fade(accent, a)),
    ]
    .line_height(LineHeight::Relative(1.0));
    let head = row![
        container(title).width(Length::Fill),
        text("count + motion · Esc to close").font(MONO).size(11).color(fade(MUTED, a)),
    ]
    .spacing(24)
    .align_y(Vertical::Bottom);

    let per_row = if vw >= 980.0 { 3 } else if vw >= 640.0 { 2 } else { 1 };
    let mut grid = Column::new().spacing(30);
    for chunk in HELP.chunks(per_row) {
        let mut r = Row::new().spacing(40);
        for (title, rows) in chunk {
            let mut g = Column::new().spacing(12).width(Length::FillPortion(1));
            g = g.push(tracked(title, 10.5, 0.16, fade(theme::LABEL, a)));
            for (keys, label) in rows.iter() {
                let kw: f32 = keys.iter().map(|k| kbd_width(k)).sum::<f32>() + 4.0 * (keys.len() as f32 - 1.0);
                let mut kr = Row::new().spacing(4);
                for k in keys.iter() {
                    kr = kr.push(kbd(k, a));
                }
                g = g.push(
                    row![
                        container(kr).width(Length::Fixed(kw.max(84.0))),
                        text(label.to_string()).font(SANS).size(14).color(fade(TEXT_2, a)),
                    ]
                    .spacing(14)
                    .align_y(Vertical::Center),
                );
            }
            r = r.push(g);
        }
        for _ in chunk.len()..per_row {
            r = r.push(Space::new().width(Length::FillPortion(1)));
        }
        grid = grid.push(r);
    }

    let panel = container(column![head, grid].spacing(34))
        .padding(Padding { top: 44.0, right: 48.0, bottom: 48.0, left: 48.0 })
        .width(Length::Fixed(920f32.min(vw - 48.0).max(200.0)))
        .style(move |_| panel_style(rgba(20, 19, 17, 0.94), 20.0, 40.0, 120.0, 0.55, a));

    container(container(panel).padding(Padding { top: 12.0 * (1.0 - a), ..Padding::ZERO }))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(24)
        .center_x(Length::Fill)
        .center_y(Length::Fill)
        .style(move |_| container::Style {
            background: Some(Background::Color(fade(rgba(8, 8, 7, 0.86), a))),
            ..Default::default()
        })
        .into()
}

