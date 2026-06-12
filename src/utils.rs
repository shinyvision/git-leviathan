use std::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn configure_background_command(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    command
}

pub fn truncate(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}

pub fn split_path(path: &str) -> (&str, &str) {
    if let Some(idx) = path.rfind('/') {
        (&path[..=idx], &path[idx + 1..])
    } else {
        ("", path)
    }
}

pub fn initials(name: &str) -> String {
    name.split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .map(|c| c.to_uppercase().next().unwrap_or(c))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::truncate;

    #[test]
    fn truncate_preserves_utf8_boundaries() {
        assert_eq!(truncate("main", 10), "main");
        assert_eq!(truncate("mañana", 4), "maña");
        assert_eq!(truncate("über/feature", 5), "über/");
    }
}
