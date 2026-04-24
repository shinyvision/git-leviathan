use iced::{
    widget::{button, column, container, row, text, Space},
    Alignment, Border, Color, Element, Length, Padding, Shadow, Task, Theme, Vector,
};

use crate::{
    assets,
    message::{AppMessage, Message},
    screens::screen_trait::Screen,
    services::GitStatus,
    theme,
    widgets::shared::horizontal_space,
};

/// Screen-local action enum. `Recheck` triggers app-level work
/// (re-detect git, swap screen), so the screen's `update` turns it into
/// an `AppMessage`. `CopyCommand` / `ClearFlash` mutate screen-local state
/// and stay local.
#[derive(Debug, Clone)]
pub enum NoGitMessage {
    Recheck,
    CopyCommand(String),
    ClearFlash,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetOs {
    Linux,
    MacOs,
    Windows,
}

impl TargetOs {
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            TargetOs::Windows
        } else if cfg!(target_os = "macos") {
            TargetOs::MacOs
        } else {
            TargetOs::Linux
        }
    }
}

pub struct NoGitScreen {
    status: GitStatus,
    target_os: TargetOs,
    flashed_command: Option<String>,
}

struct InstallRow {
    label: &'static str,
    command: &'static str,
}

struct InstallGuide {
    intro: &'static str,
    rows: &'static [InstallRow],
    link_prefix: &'static str,
    link_url: &'static str,
}

const COMMAND_FONT_SIZE: f32 = 13.0;
const ROW_HEIGHT: f32 = 44.0;
const DISTRO_COL_WIDTH: f32 = 220.0;

impl NoGitScreen {
    pub fn new(status: GitStatus, target_os: TargetOs) -> Self {
        Self {
            status,
            target_os,
            flashed_command: None,
        }
    }

    pub fn set_status(&mut self, status: GitStatus) {
        self.status = status;
    }
}

impl Screen for NoGitScreen {
    type Message = NoGitMessage;

    fn update(&mut self, msg: NoGitMessage) -> Task<Message> {
        match msg {
            NoGitMessage::Recheck => Task::done(Message::App(AppMessage::GitRecheckRequested)),
            NoGitMessage::CopyCommand(cmd) => {
                self.flashed_command = Some(cmd.clone());
                let clipboard_task = iced::clipboard::write::<Message>(cmd);
                let clear_task = Task::perform(
                    async {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    },
                    |_| Message::no_git(NoGitMessage::ClearFlash),
                );
                Task::batch(vec![clipboard_task, clear_task])
            }
            NoGitMessage::ClearFlash => {
                self.flashed_command = None;
                Task::none()
            }
        }
    }

    fn view(&self) -> Element<'_, Message> {
        let guide = self.guide();

        let headline = text("Git is not installed")
            .size(28)
            .color(theme::TEXT_PRIMARY);

        let explanation = text(self.explanation())
            .size(theme::FONT_LG)
            .color(theme::TEXT_SECONDARY);

        let how_to_fix = text("How to fix")
            .size(theme::FONT_LG + 2.0)
            .color(theme::TEXT_PRIMARY);

        let intro = text(guide.intro)
            .size(theme::FONT_MD)
            .color(theme::TEXT_SECONDARY);

        let table = install_table(guide.rows, self.flashed_command.as_deref());

        let link_line = row![
            text(guide.link_prefix)
                .size(theme::FONT_MD)
                .color(theme::TEXT_SECONDARY),
            link_button(guide.link_url),
        ]
        .spacing(6)
        .align_y(Alignment::Center);

        let after_line = text("After install completes, click Recheck.")
            .size(theme::FONT_MD)
            .color(theme::TEXT_SECONDARY);

        let actions = row![
            horizontal_space().width(Length::Fill),
            secondary_button("Quit", Message::App(AppMessage::ShutdownRequested)),
            primary_button("Recheck", Message::no_git(NoGitMessage::Recheck)),
        ]
        .spacing(10)
        .align_y(Alignment::Center);

        let content = column![
            headline,
            explanation,
            Space::new().height(8),
            how_to_fix,
            intro,
            table,
            link_line,
            after_line,
            Space::new().height(8),
            actions,
        ]
        .spacing(14);

        let card = container(content)
            .padding(Padding::from([32, 36]))
            .max_width(780)
            .style(|_: &Theme| container::Style {
                background: Some(theme::BG_PANEL.into()),
                border: Border {
                    color: theme::BORDER,
                    width: 1.0,
                    radius: 10.0.into(),
                },
                shadow: Shadow {
                    color: Color {
                        r: 0.0,
                        g: 0.0,
                        b: 0.0,
                        a: 0.45,
                    },
                    offset: Vector::new(0.0, 8.0),
                    blur_radius: 32.0,
                },
                ..container::Style::default()
            });

        container(card)
            .center(Length::Fill)
            .style(|_: &Theme| container::Style {
                background: Some(theme::BG_BASE.into()),
                ..container::Style::default()
            })
            .into()
    }
}

impl NoGitScreen {
    fn explanation(&self) -> &'static str {
        match self.status {
            GitStatus::MacOsCommandLineToolsMissing => {
                "Git Leviathan requires the `git` command-line tool. \
                 macOS ships a stub at /usr/bin/git that cannot run until \
                 the Command Line Tools are installed."
            }
            _ => {
                "Git Leviathan requires the `git` command-line tool to read \
                 and modify repositories, but it was not found on your system PATH."
            }
        }
    }

    fn guide(&self) -> InstallGuide {
        if matches!(self.status, GitStatus::MacOsCommandLineToolsMissing) {
            return MACOS_CLT_GUIDE;
        }
        match self.target_os {
            TargetOs::Windows => WINDOWS_GUIDE,
            TargetOs::MacOs => MACOS_GUIDE,
            TargetOs::Linux => LINUX_GUIDE,
        }
    }
}

const LINUX_GUIDE: InstallGuide = InstallGuide {
    intro: "Install Git via your distribution's package manager:",
    rows: &[
        InstallRow {
            label: "Debian / Ubuntu",
            command: "sudo apt install git",
        },
        InstallRow {
            label: "Fedora / RHEL",
            command: "sudo dnf install git",
        },
        InstallRow {
            label: "Arch",
            command: "sudo pacman -S git",
        },
    ],
    link_prefix: "Or see",
    link_url: "https://git-scm.com/download/linux",
};

const MACOS_GUIDE: InstallGuide = InstallGuide {
    intro: "Install Git using one of:",
    rows: &[
        InstallRow {
            label: "Homebrew",
            command: "brew install git",
        },
        InstallRow {
            label: "Command Line Tools",
            command: "xcode-select --install",
        },
    ],
    link_prefix: "Or download the installer from",
    link_url: "https://git-scm.com/download/mac",
};

const MACOS_CLT_GUIDE: InstallGuide = InstallGuide {
    intro: "Install the macOS Command Line Tools, or install Git separately:",
    rows: &[
        InstallRow {
            label: "Command Line Tools",
            command: "xcode-select --install",
        },
        InstallRow {
            label: "Homebrew",
            command: "brew install git",
        },
    ],
    link_prefix: "Or download the installer from",
    link_url: "https://git-scm.com/download/mac",
};

const WINDOWS_GUIDE: InstallGuide = InstallGuide {
    intro: "Install Git using one of:",
    rows: &[
        InstallRow {
            label: "winget",
            command: "winget install --id Git.Git -e",
        },
        InstallRow {
            label: "Chocolatey",
            command: "choco install git",
        },
    ],
    link_prefix: "Or download the installer from",
    link_url: "https://git-scm.com/download/win",
};

fn install_table(rows: &'static [InstallRow], flashed: Option<&str>) -> Element<'static, Message> {
    let border_color = theme::BORDER;

    let header = row![
        table_cell(
            text("Distribution")
                .size(theme::FONT_MD)
                .color(theme::TEXT_PRIMARY)
                .into(),
            DISTRO_COL_WIDTH,
            Length::Shrink,
        ),
        vertical_divider(border_color),
        table_cell(
            text("Command")
                .size(theme::FONT_MD)
                .color(theme::TEXT_PRIMARY)
                .into(),
            0.0,
            Length::Fill,
        ),
    ]
    .height(Length::Fixed(36.0))
    .align_y(Alignment::Center);

    let header_row: Element<Message> = container(header)
        .style(move |_: &Theme| container::Style {
            background: Some(theme::BG_HEADER.into()),
            ..container::Style::default()
        })
        .into();

    let mut children: Vec<Element<Message>> = vec![header_row];

    for (i, r) in rows.iter().enumerate() {
        if i == 0 {
            children.push(horizontal_divider(border_color));
        }
        let is_flashed = flashed == Some(r.command);
        children.push(data_row(r, border_color, is_flashed));
        if i + 1 < rows.len() {
            children.push(horizontal_divider(border_color));
        }
    }

    container(column(children).spacing(0))
        .style(move |_: &Theme| container::Style {
            background: Some(theme::BG_PANEL.into()),
            border: Border {
                color: border_color,
                width: 1.0,
                radius: 6.0.into(),
            },
            ..container::Style::default()
        })
        .width(Length::Fill)
        .into()
}

fn data_row(r: &InstallRow, border_color: Color, flashed: bool) -> Element<'static, Message> {
    let label_el: Element<Message> = text(r.label)
        .size(theme::FONT_MD)
        .color(theme::TEXT_PRIMARY)
        .into();

    let command_el: Element<Message> = row![
        text(r.command)
            .size(COMMAND_FONT_SIZE)
            .font(iced::Font {
                family: iced::font::Family::Name("JetBrains Mono"),
                ..iced::Font::DEFAULT
            })
            .color(theme::TEXT_PRIMARY),
        horizontal_space().width(Length::Fill),
        copy_button(r.command.to_string(), flashed),
    ]
    .align_y(Alignment::Center)
    .spacing(8)
    .into();

    row![
        table_cell(label_el, DISTRO_COL_WIDTH, Length::Shrink),
        vertical_divider(border_color),
        table_cell(command_el, 0.0, Length::Fill),
    ]
    .height(Length::Fixed(ROW_HEIGHT))
    .align_y(Alignment::Center)
    .into()
}

fn table_cell(
    content: Element<'static, Message>,
    fixed_width: f32,
    flex_width: Length,
) -> Element<'static, Message> {
    let c = container(content).padding(Padding::from([8, 14]));
    let c = if fixed_width > 0.0 {
        c.width(Length::Fixed(fixed_width))
    } else {
        c.width(flex_width)
    };
    c.height(Length::Fill).align_y(Alignment::Center).into()
}

fn vertical_divider(color: Color) -> Element<'static, Message> {
    container(Space::new().width(Length::Fixed(1.0)))
        .width(Length::Fixed(1.0))
        .height(Length::Fill)
        .style(move |_: &Theme| container::Style {
            background: Some(color.into()),
            ..container::Style::default()
        })
        .into()
}

fn horizontal_divider(color: Color) -> Element<'static, Message> {
    container(Space::new().height(Length::Fixed(1.0)))
        .width(Length::Fill)
        .height(Length::Fixed(1.0))
        .style(move |_: &Theme| container::Style {
            background: Some(color.into()),
            ..container::Style::default()
        })
        .into()
}

fn copy_button(command: String, flashed: bool) -> Element<'static, Message> {
    let icon_color = if flashed {
        Color::WHITE
    } else {
        theme::TEXT_SECONDARY
    };
    button(container(assets::icon(assets::COPY, 16.0, icon_color)).padding(2))
        .style(move |_: &Theme, status: button::Status| {
            let bg = if flashed {
                Some(theme::ACCENT_GREEN.into())
            } else {
                match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Some(theme::BG_HOVER.into())
                    }
                    _ => None,
                }
            };
            button::Style {
                background: bg,
                text_color: icon_color,
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 4.0.into(),
                },
                shadow: Default::default(),
                snap: false,
            }
        })
        .padding(6)
        .on_press(Message::no_git(NoGitMessage::CopyCommand(command)))
        .into()
}

fn link_button(url: &'static str) -> Element<'static, Message> {
    let content = row![
        text(url).size(theme::FONT_MD).color(theme::ACCENT_BLUE),
        assets::icon(assets::EXTERNAL_LINK, 13.0, theme::ACCENT_BLUE),
    ]
    .spacing(6)
    .align_y(Alignment::Center);

    button(content)
        .style(|_: &Theme, status: button::Status| {
            let bg = match status {
                button::Status::Hovered | button::Status::Pressed => Some(theme::BG_HOVER.into()),
                _ => None,
            };
            button::Style {
                background: bg,
                text_color: theme::ACCENT_BLUE,
                border: Border {
                    color: Color::TRANSPARENT,
                    width: 0.0,
                    radius: 4.0.into(),
                },
                shadow: Default::default(),
                snap: false,
            }
        })
        .padding(Padding::from([2, 6]))
        .on_press(Message::App(AppMessage::OpenUrl(url.to_string())))
        .into()
}

fn primary_button(label: &'static str, msg: Message) -> Element<'static, Message> {
    button(
        container(text(label).size(theme::FONT_LG).color(Color::WHITE))
            .padding(Padding::from([0, 4])),
    )
    .style(|_: &Theme, status: button::Status| {
        let bg = match status {
            button::Status::Hovered => Color {
                r: theme::ACCENT_BLUE.r * 1.10,
                g: theme::ACCENT_BLUE.g * 1.10,
                b: (theme::ACCENT_BLUE.b * 1.05).min(1.0),
                a: 1.0,
            },
            button::Status::Pressed => Color {
                r: theme::ACCENT_BLUE.r * 0.90,
                g: theme::ACCENT_BLUE.g * 0.90,
                b: theme::ACCENT_BLUE.b * 0.90,
                a: 1.0,
            },
            _ => theme::ACCENT_BLUE,
        };
        button::Style {
            background: Some(bg.into()),
            text_color: Color::WHITE,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 6.0.into(),
            },
            shadow: Default::default(),
            snap: false,
        }
    })
    .padding(Padding::from([10, 22]))
    .on_press(msg)
    .into()
}

fn secondary_button(label: &'static str, msg: Message) -> Element<'static, Message> {
    let neutral_bg = Color {
        r: 0.96,
        g: 0.97,
        b: 0.98,
        a: 1.0,
    };
    let hover_bg = Color {
        r: 0.88,
        g: 0.89,
        b: 0.91,
        a: 1.0,
    };
    let press_bg = Color {
        r: 0.80,
        g: 0.81,
        b: 0.83,
        a: 1.0,
    };
    let dark_text = Color {
        r: 0.12,
        g: 0.13,
        b: 0.16,
        a: 1.0,
    };

    button(
        container(text(label).size(theme::FONT_LG).color(dark_text)).padding(Padding::from([0, 4])),
    )
    .style(move |_: &Theme, status: button::Status| {
        let bg = match status {
            button::Status::Hovered => hover_bg,
            button::Status::Pressed => press_bg,
            _ => neutral_bg,
        };
        button::Style {
            background: Some(bg.into()),
            text_color: dark_text,
            border: Border {
                color: Color::TRANSPARENT,
                width: 0.0,
                radius: 6.0.into(),
            },
            shadow: Default::default(),
            snap: false,
        }
    })
    .padding(Padding::from([10, 22]))
    .on_press(msg)
    .into()
}
