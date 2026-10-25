//! peepers — a hyper-minimal, vim-motion image viewer for Wayland.

mod app;
mod config;
mod decode;
mod files;
mod motion;
mod theme;
mod ui;

use clap::Parser;
use std::io::{BufRead, IsTerminal};
use std::path::PathBuf;

/// The image is the interface.
#[derive(Parser, Debug)]
#[command(name = "peepers", version, about = "A vim-motion image viewer for Wayland", long_about = None)]
struct Cli {
    /// Image files or directories. One file opens its whole folder at that
    /// file; `-` reads newline-separated paths from stdin.
    paths: Vec<PathBuf>,

    /// Stay attached to the terminal instead of detaching.
    #[arg(short, long)]
    foreground: bool,

    /// Config file (default: $XDG_CONFIG_HOME/peepers/config.toml).
    #[arg(short, long, value_name = "FILE")]
    config: Option<PathBuf>,

    /// Start in fullscreen.
    #[arg(short = 'F', long)]
    fullscreen: bool,

    /// Initial window size, e.g. 1440x900 (tiling WMs override this).
    #[arg(long, value_name = "WxH", default_value = "1440x900")]
    size: String,

    /// Keys to replay once the image is shown, space-separated (e.g. "4 + 2 l").
    #[arg(long, hide = true, allow_hyphen_values = true)]
    keys: Option<String>,
}

const DETACHED: &str = "PEEPERS_DETACHED";

fn main() -> iced::Result {
    let cli = Cli::parse();

    // Read stdin paths before detaching (the child gets /dev/null).
    let mut paths = cli.paths.clone();
    if paths.iter().any(|p| p.as_os_str() == "-") {
        paths.retain(|p| p.as_os_str() != "-");
        for l in std::io::stdin().lock().lines().map_while(Result::ok) {
            let l = l.trim();
            if !l.is_empty() {
                paths.push(PathBuf::from(l));
            }
        }
    }

    // Launched from a terminal (Yazi, a shell): detach so the terminal is
    // free immediately and the window outlives it.
    if !cli.foreground && std::env::var_os(DETACHED).is_none() && std::io::stderr().is_terminal()
        && detach(&paths, &cli) {
            return Ok(());
        }

    let config = config::load(cli.config.clone());
    let (files, start) = files::collect(&paths);
    let (w, h) = parse_size(&cli.size).unwrap_or((1440.0, 900.0));

    let opts = std::cell::RefCell::new(Some(app::Options {
        files,
        start,
        config,
        keys: cli.keys.as_deref().map(|k| k.split_whitespace().map(String::from).collect()).unwrap_or_default(),
    }));

    let mut window = iced::window::Settings {
        size: iced::Size::new(w, h),
        decorations: false,
        transparent: false,
        min_size: Some(iced::Size::new(160.0, 120.0)),
        ..Default::default()
    };
    #[cfg(target_os = "linux")]
    {
        // Wayland app_id / X11 WM_CLASS — what Hyprland window rules match.
        window.platform_specific.application_id = "peepers".into();
    }
    if cli.fullscreen {
        window.fullscreen = true;
    }

    let mut a = iced::application(
        move || opts.borrow_mut().take().map(app::App::new).expect("boot once"),
        app::App::update,
        ui::view,
    )
    .title(app::App::title)
    .subscription(app::App::subscription)
    .theme(|_: &app::App| iced::Theme::Dark)
    .style(|_, _| iced::theme::Style { background_color: theme::BG, text_color: theme::TEXT })
    .default_font(theme::SANS)
    .antialiasing(true)
    .window(window);
    for f in theme::FONT_FILES {
        a = a.font(f);
    }
    a.run()
}

fn parse_size(s: &str) -> Option<(f32, f32)> {
    let (w, h) = s.split_once(['x', 'X'])?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

/// Re-exec ourselves in a new session with stdio on /dev/null, then exit.
fn detach(paths: &[PathBuf], cli: &Cli) -> bool {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let Ok(exe) = std::env::current_exe() else { return false };
    let mut cmd = Command::new(exe);
    cmd.env(DETACHED, "1").arg("--foreground");
    if let Some(c) = &cli.config {
        cmd.arg("--config").arg(c);
    }
    if cli.fullscreen {
        cmd.arg("--fullscreen");
    }
    cmd.arg("--size").arg(&cli.size);
    // Resolve relative paths against our cwd before handing them over.
    cmd.arg("--").args(paths.iter().map(|p| std::fs::canonicalize(p).unwrap_or_else(|_| p.clone())));
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // SAFETY: setsid is async-signal-safe and runs in the forked child.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    cmd.spawn().is_ok()
}
