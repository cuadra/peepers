//! Design tokens lifted from the comp.

use iced::font::{Family, Style, Weight};
use iced::{Color, Font};

pub const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a: 1.0 }
}
pub const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color { r: r as f32 / 255.0, g: g as f32 / 255.0, b: b as f32 / 255.0, a }
}

/// Shell background `#0B0B0A`.
pub const BG: Color = rgb(0x0B, 0x0B, 0x0A);
/// Primary text `#F3EFE7`.
pub const TEXT: Color = rgb(0xF3, 0xEF, 0xE7);
/// Secondary text `#D9D4CA`.
pub const TEXT_2: Color = rgb(0xD9, 0xD4, 0xCA);
/// Muted text `#B5AFA4`.
pub const MUTED: Color = rgb(0xB5, 0xAF, 0xA4);
/// Mono labels `#9C978D`.
pub const LABEL: Color = rgb(0x9C, 0x97, 0x8D);
/// Default accent `#CFAE74` (comp options: #9FB8D0, #D9A08B, #F3EFE7).
pub const ACCENT: Color = rgb(0xCF, 0xAE, 0x74);

/// Hairlines: rgba(243,239,231,α).
pub const fn line(a: f32) -> Color {
    rgba(243, 239, 231, a)
}

pub const SANS: Font = Font::with_name("Instrument Sans");
pub const SERIF: Font = Font::with_name("Instrument Serif");
pub const SERIF_ITALIC: Font = Font {
    family: Family::Name("Instrument Serif"),
    weight: Weight::Normal,
    style: Style::Italic,
    ..Font::DEFAULT
};
pub const MONO: Font = Font::with_name("JetBrains Mono");

pub const FONT_FILES: [&[u8]; 4] = [
    include_bytes!("../assets/fonts/InstrumentSans-Regular.ttf"),
    include_bytes!("../assets/fonts/InstrumentSerif-Regular.ttf"),
    include_bytes!("../assets/fonts/InstrumentSerif-Italic.ttf"),
    include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
];

/// Multiply a colour's alpha (used for overlay fades).
pub fn fade(c: Color, a: f32) -> Color {
    Color { a: c.a * a, ..c }
}
