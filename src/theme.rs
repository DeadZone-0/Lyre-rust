use ratatui::style::Color;

#[derive(Clone, Copy)]
pub struct Theme {
    pub name: &'static str,
    pub primary: Color,
    pub secondary: Color,
    pub accent: Color,
    pub dimmed: Color,
    pub text: Color,
}

pub const THEMES: [Theme; 4] = [
    Theme {
        name: "Terminal",
        primary: Color::Cyan,
        secondary: Color::Yellow,
        accent: Color::Magenta,
        dimmed: Color::DarkGray,
        text: Color::Reset,
    },
    Theme {
        name: "Cmus",
        primary: Color::Green,
        secondary: Color::Cyan,
        accent: Color::White,
        dimmed: Color::DarkGray,
        text: Color::Reset,
    },
    Theme {
        name: "Nord",
        primary: Color::Rgb(136, 192, 208),
        secondary: Color::Rgb(129, 161, 193),
        accent: Color::Rgb(235, 203, 139),
        dimmed: Color::Rgb(76, 86, 106),
        text: Color::Rgb(229, 233, 240),
    },
    Theme {
        name: "Amber",
        primary: Color::Rgb(255, 176, 0),
        secondary: Color::Rgb(255, 218, 128),
        accent: Color::Rgb(255, 245, 200),
        dimmed: Color::Rgb(90, 60, 0),
        text: Color::Rgb(255, 200, 80),
    },
];

pub fn blend(c1: Color, c2: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    match (c1, c2) {
        (Color::Rgb(r1, g1, b1), Color::Rgb(r2, g2, b2)) => Color::Rgb(
            (r1 as f32 * (1.0 - t) + r2 as f32 * t) as u8,
            (g1 as f32 * (1.0 - t) + g2 as f32 * t) as u8,
            (b1 as f32 * (1.0 - t) + b2 as f32 * t) as u8,
        ),
        _ => if t > 0.35 { c2 } else { c1 },
    }
}
