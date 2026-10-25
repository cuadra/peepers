# Peepers

A hyper-minimal image viewer for Wayland, driven by vim motions.
The image is the interface: no title bar, toolbar or border. Every overlay
appears on a keystroke and fades away again.

Built in Rust with [iced 0.14](https://iced.rs), from the *Still* design comp.
A slim header shows the brand on the left and the `?` shortcut for every
key binding on the right; it hides in fullscreen (`f`) or with
`show_header = false`.

```
peepers photo.jpg          # opens the whole folder at photo.jpg
peepers ~/Pictures/2026    # a directory
peepers a.png b.png c.png  # exactly these files
fd -e jpg | peepers -      # paths on stdin
```

Launched from a terminal, `peepers` detaches straight away (new session, stdio
on `/dev/null`), so your shell or Yazi stays usable while the image sits in
the next tile. Use `-f/--foreground` to keep it attached.

## Motions

Put a count in front of any motion: `3l`, `5+`, `12G`. While you type, the
pending count or operator waits in the accent badge at the top left.

| Group | Keys | Does |
|---|---|---|
| Pan | `H` `J` `K` `L` (or arrow keys) | Nudge left / down / up / right 90px |
| | `b` `w` | Half a screen left / right |
| | `{` `}` | Half a screen up / down |
| Edges | `0` `$` | Left / right edge |
| | `gg` / `G` | Top / bottom edge |
| Zoom | `j` `k` (or `+` `-`) | In / out one step: 100 · 150 · 200 · 300 · 400 · 600 · 800% |
| | `3j` | Three steps at once |
| | `Esc` | Reset to fit (centred, 100%) |
| Remember | `ma` … `mz` | Save this view as mark *a* |
| | `'a` | Return to view *a* |
| | `''` | Previous view |
| Overlays | `i` | Image information |
| | `?` | Motion sheet |
| | `Esc` | Close an open overlay first; also cancels a pending count |
| Files | `h` `l` (or `N` `n`, `Backspace` `Space`, `PgUp` `PgDn`) | Previous / next image |
| | `5G` or `5gg` | Go to image 5 |
| | `Home` `End` | First / last image |
| | `f` | Toggle fullscreen |
| | `q` | Quit |

Mouse (optional): drag to pan, scroll to zoom around the cursor.

Troubleshooting keys: `PEEPERS_DEBUG=1 peepers -f photo.jpg` prints every key event the window receives.

## Configuration

`~/.config/peepers/config.toml`. Every key is optional; see
[`extras/config.toml`](extras/config.toml).

```toml
accent    = "#CFAE74"   # comp options: #9FB8D0 #D9A08B #F3EFE7
pan_step  = 90
prefetch  = 2           # neighbours decoded ahead, each side
cache_mb  = 768
upscale   = false
show_header = true      # brand + `?` shortcut bar
show_hint = true        # launch hint, header-less mode only

[keys]                  # rebind any key; "none" unbinds
"x" = "quit"
"J" = "next"
```

## Desktop integration

**Yazi.** Merge [`extras/yazi.toml`](extras/yazi.toml):

```toml
[opener]
peepers = [{ run = 'peepers "$@"', orphan = true, desc = "Peepers", for = "unix" }]

[open]
prepend_rules = [{ mime = "image/*", use = ["peepers", "open"] }]
```

**Hyprland.** The Wayland `app_id` (X11 `WM_CLASS`) is `peepers`. Source
[`extras/hyprland.conf`](extras/hyprland.conf) for borderless, shadowless
tiles, with optional floating rules.

**Default viewer.** The package installs `peepers.desktop` with image MIME types:

```sh
xdg-mime default peepers.desktop image/jpeg image/png image/webp image/gif
```

## Build & install

```sh
cargo build --release            # → target/release/peepers
makepkg -si                      # Arch, using the bundled PKGBUILD
```

The `PKGBUILD` builds from a `v$pkgver` tag on GitHub. Push a tag, then
`updpkgsums` and `makepkg --printsrcinfo > .SRCINFO` before publishing to the AUR.

Rendering uses wgpu (Vulkan/GL). Without a GPU driver iced falls back to its
CPU renderer automatically; force it with `ICED_BACKEND=tiny-skia`.

## How it works

```
 main thread (iced event loop)            worker threads (≤4)
 ─────────────────────────────            ───────────────────
 keys → motion engine → target view       read file → decode → EXIF orientation
 view animation  .38s cubic-bezier        → downscale (fast_image_resize, SIMD)
 overlay fades   .22s ease                → screen copy + thumbnail + metadata
 scheduler: current (Full) first,  ───▶   ───────────────────────────────┐
   then ±1, ±2 … (Preview)                                                │
 cache (byte budget, distance-LRU) ◀── Loaded ◀───────────────────────────┘
   └─ image::allocate → GPU upload off the critical path
      bundle swaps in only once every texture is resident
```

- **Zero-latency switching.** Neighbours are decoded *and uploaded to the GPU*
  before you press `n`, so the next image draws on the very next frame.
- **No memory bloat.** Prefetched neighbours are decoded at screen resolution,
  about one screenful of RGBA each. Only the image you're on is kept at full
  resolution, capped at 16384px on the long edge / 160 MP. The cache evicts the
  farthest, then least-recently-used, files once it's over budget.
- **No shimmer.** GPU textures have no mipmaps, so while the full bitmap would be
  minified the view draws a screen-sized copy. Once one image pixel covers 3+
  screen pixels it switches to nearest-neighbour sampling, so pixels show crisp.
- **Never blocks.** Decoding, resizing, EXIF parsing and file I/O all happen
  on worker threads. The UI thread only does bookkeeping.
- **The view math** (`src/motion.rs`) is the comp's, unit-tested: fit with 40px
  margins, pan clamped to `max(0, (z·FW − VW)/2 + 40)`, zoom keeps the centre,
  and the minimap viewport uses the same fractions.

```
src/
  main.rs     CLI, detach, window setup (app_id, no decorations)
  app.rs      state, decode scheduler, cache, GPU uploads, event loop
  motion.rs   vim key-sequence engine + view geometry (pure, tested)
  decode.rs   decoding, tiers, resizing, EXIF / ICC metadata
  ui.rs       the comp's layers: image, minimap, badge, hint, info, help
  theme.rs    design tokens, bundled fonts
  config.rs   config.toml + keymap
  files.rs    file lists, natural sort
```

## Notes on the comp translation

- **Backdrop blur.** iced can't blur what's behind a panel. The info panel and
  motion sheet use slightly more opaque fills (0.86 / 0.94 instead of 0.74 /
  0.92) to keep the same legibility.
- **Letter-spacing.** iced text has no tracking, so the comp's `0.16em` mono
  labels are spaced glyph by glyph.
- **Motion sheet keys.** The comp's 84px key column overlapped the labels in
  rows with 4 keys (`h j k l`). The column now grows to fit its keys.
- **Additions** the comp didn't specify: a FILES group on the motion sheet
  (`n`/`N`, `5G`, `q`), a brief `3 / 48` position badge when switching files
  (same badge as the count buffer, in muted grey), and the hint auto-fading
  after 4s.
- Fonts (Instrument Sans, Instrument Serif, JetBrains Mono) are embedded in
  the binary under the SIL OFL; see `assets/fonts/`.

## Limitations

- Formats: JPEG, PNG/APNG, GIF, WebP, BMP, TIFF, ICO, PNM, QOI, TGA, HDR, EXR.
  No AVIF, HEIC, JPEG XL or camera RAW yet. Animated GIF/WebP show the first frame.
- Startup includes GPU initialisation (typically 100–200ms on real hardware),
  so it won't quite match imv's few-millisecond cold start. Image switching is
  instant once neighbours are prefetched.
- Colour management: ICC profiles are reported in the info panel but not
  applied. Pixels are shown as sRGB.

MIT © Jose Cuadra
