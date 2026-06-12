use iced::{Color, Padding};

pub const BG_BASE: Color = Color {
    r: 0.090,
    g: 0.094,
    b: 0.133,
    a: 1.0,
}; // #171822
pub const BG_SIDEBAR: Color = Color {
    r: 0.071,
    g: 0.075,
    b: 0.110,
    a: 1.0,
}; // #12131c
pub const BG_PANEL: Color = Color {
    r: 0.098,
    g: 0.102,
    b: 0.145,
    a: 1.0,
}; // #191a25
pub const BG_HEADER: Color = Color {
    r: 0.063,
    g: 0.067,
    b: 0.098,
    a: 1.0,
}; // #101119
pub const BG_TOOLBAR: Color = Color {
    r: 0.055,
    g: 0.059,
    b: 0.086,
    a: 1.0,
}; // #0e0f16
pub const BG_SELECTED: Color = Color {
    r: 0.122,
    g: 0.196,
    b: 0.384,
    a: 1.0,
}; // #1f3262
pub const BG_HOVER: Color = Color {
    r: 0.125,
    g: 0.133,
    b: 0.184,
    a: 1.0,
};

pub const TEXT_PRIMARY: Color = Color {
    r: 0.882,
    g: 0.898,
    b: 0.957,
    a: 1.0,
}; // #e1e5f4
pub const TEXT_SECONDARY: Color = Color {
    r: 0.545,
    g: 0.565,
    b: 0.647,
    a: 1.0,
}; // #8b90a5
pub const TEXT_PATH: Color = Color {
    r: 0.455,
    g: 0.463,
    b: 0.510,
    a: 1.0,
}; // #747682
pub const TEXT_DIM: Color = Color {
    r: 0.345,
    g: 0.365,
    b: 0.431,
    a: 1.0,
}; // #585d6e
pub const TEXT_MUTED: Color = Color {
    r: 0.235,
    g: 0.251,
    b: 0.302,
    a: 1.0,
}; // #3c404d — darker than TEXT_DIM
pub const TEXT_ACTIVE_BRANCH: Color = Color {
    r: 0.376,
    g: 0.847,
    b: 0.494,
    a: 1.0,
}; // #60d87e

pub const BORDER: Color = Color {
    r: 0.141,
    g: 0.149,
    b: 0.208,
    a: 1.0,
}; // #242535
pub const DIVIDER: Color = Color {
    r: 0.102,
    g: 0.110,
    b: 0.157,
    a: 1.0,
}; // #1a1c28

pub const ACCENT_BLUE: Color = Color {
    r: 0.251,
    g: 0.600,
    b: 0.965,
    a: 1.0,
};
pub const ACCENT_GREEN: Color = Color {
    r: 0.294,
    g: 0.804,
    b: 0.424,
    a: 1.0,
};
pub const ACCENT_RED: Color = Color {
    r: 0.9,
    g: 0.3,
    b: 0.3,
    a: 1.0,
};

/// UI danger color (Iced palette `danger`, toast error strip). Intentionally a
/// distinct red from `ACCENT_RED`, which marks diff deletions.
pub const ACCENT_DANGER: Color = Color {
    r: 0.858,
    g: 0.243,
    b: 0.243,
    a: 1.0,
};

pub const ACCENT_ORANGE: Color = Color {
    r: 0.980,
    g: 0.639,
    b: 0.239,
    a: 1.0,
};

/// Keep in sync with the equivalents in `diff_view.rs`.
pub const ADDITION_BG: Color = Color {
    r: 0.05,
    g: 0.15,
    b: 0.08,
    a: 1.0,
};

pub const DELETION_BG: Color = Color {
    r: 0.18,
    g: 0.05,
    b: 0.05,
    a: 1.0,
};

pub const ADDITION_HIGHLIGHT_BG: Color = Color {
    r: 0.12,
    g: 0.38,
    b: 0.22,
    a: 1.0,
};

pub const DELETION_HIGHLIGHT_BG: Color = Color {
    r: 0.40,
    g: 0.15,
    b: 0.15,
    a: 1.0,
};

/// Lane color is determined by slot index; all segments in the same horizontal
/// lane share the same entry. Order: cyan, blue, purple, magenta, red, orange,
/// yellow, green.
pub const LANE_COLORS: [Color; 8] = [
    Color {
        r: 0.000,
        g: 0.878,
        b: 0.878,
        a: 1.0,
    }, // 0 – cyan
    Color {
        r: 0.200,
        g: 0.480,
        b: 1.000,
        a: 1.0,
    }, // 1 – blue
    Color {
        r: 0.580,
        g: 0.200,
        b: 1.000,
        a: 1.0,
    }, // 2 – purple
    Color {
        r: 0.920,
        g: 0.100,
        b: 0.920,
        a: 1.0,
    }, // 3 – magenta
    Color {
        r: 1.000,
        g: 0.200,
        b: 0.200,
        a: 1.0,
    }, // 4 – red
    Color {
        r: 1.000,
        g: 0.500,
        b: 0.100,
        a: 1.0,
    }, // 5 – orange
    Color {
        r: 0.950,
        g: 0.850,
        b: 0.050,
        a: 1.0,
    }, // 6 – yellow
    Color {
        r: 0.150,
        g: 0.850,
        b: 0.250,
        a: 1.0,
    }, // 7 – green
];

pub const LANE_WIDTH: f32 = 26.0;
pub const GRAPH_COL_GUTTER: f32 = 20.0;
pub const ROW_H: f32 = 34.0;
pub const AVATAR_RADIUS: f32 = 10.0;
pub const TURN_RADIUS: f32 = 15.0;

pub const SIDEBAR_WIDTH: u16 = 240;
pub const BRANCH_COL_WIDTH: u16 = 185;
pub const DETAIL_PANEL_WIDTH: u16 = 510;
pub const DETAIL_PANEL_HEIGHT: u16 = 320;
pub const PANE_SPLITTER_SIZE: f32 = 5.0;

pub const MIN_CENTER_WIDTH: f32 = 460.0;

pub const TAB_HEIGHT: u16 = 34;
pub const TOOLBAR_HEIGHT: u16 = 50;
pub const STATUS_BAR_HEIGHT: u16 = 20;

// Single source of truth for text_input vertical padding. Buttons that sit on
// the same row as an input (e.g. the "Browse" folder picker) should use
// `INPUT_HEIGHT` so the row aligns without tweaking per-callsite.
pub const INPUT_PADDING: Padding = Padding {
    top: 7.0,
    right: 8.0,
    bottom: 7.0,
    left: 8.0,
};
pub const INPUT_HEIGHT: f32 = 28.0;

pub const FONT_XS: f32 = 10.0;
pub const FONT_SM: f32 = 11.0;
pub const FONT_MD: f32 = 12.0;
pub const FONT_LG: f32 = 14.0;

pub const MONO: iced::Font = iced::Font::with_name("JetBrains Mono");

pub const FONT_DIFF: f32 = 13.0;
