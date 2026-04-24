pub fn truncate(s: &str, max: usize) -> &str {
    if s.chars().nth(max).is_none() {
        s
    } else {
        let end = s
            .char_indices()
            .nth(max)
            .map(|(idx, _)| idx)
            .unwrap_or(s.len());
        &s[..end]
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
