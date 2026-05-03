/// Strip the `WIP on <branch>:` / `On <branch>:` prefix from a stash message
/// and return only the first line of whatever remains.
pub fn stash_display_name(message: &str) -> String {
    let first_line = message.lines().next().unwrap_or("").trim();
    let without_prefix = strip_stash_prefix(first_line);
    without_prefix.trim().to_string()
}

fn strip_stash_prefix(line: &str) -> &str {
    for prefix in ["WIP on ", "On "] {
        if let Some(rest) = line.strip_prefix(prefix) {
            if let Some(colon_idx) = rest.find(':') {
                return rest[colon_idx + 1..].trim_start();
            }
            return rest;
        }
    }
    line
}
