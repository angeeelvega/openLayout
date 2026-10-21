use super::model::DeskMode;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Pause,
    Resume,
    Toggle,
    Overview,
    Mode(DeskMode),
    ProfileSave(String),
    ProfileApply(String),
    FocusNextDisplay,
}

pub fn parse_command(raw: &str) -> Option<Command> {
    let parts: Vec<&str> = raw.split_whitespace().collect();
    let head = parts.first()?.to_ascii_lowercase();
    match head.as_str() {
        "pause" => Some(Command::Pause),
        "resume" => Some(Command::Resume),
        "toggle" => Some(Command::Toggle),
        "overview" => Some(Command::Overview),
        "focus-next-display" => Some(Command::FocusNextDisplay),
        "mode" => match parts.get(1)?.to_ascii_lowercase().as_str() {
            "unified" => Some(Command::Mode(DeskMode::Unified)),
            "independent" => Some(Command::Mode(DeskMode::Independent)),
            _ => None,
        },
        "profile" => {
            let verb = parts.get(1)?.to_ascii_lowercase();
            let name = parts.get(2..)?.join(" ");
            if name.is_empty() {
                return None;
            }
            match verb.as_str() {
                "save" => Some(Command::ProfileSave(name)),
                "apply" => Some(Command::ProfileApply(name)),
                _ => None,
            }
        }
        _ => None,
    }
}

pub fn parse_url(url: &str) -> Option<Command> {
    let rest = url.strip_prefix("openlayout://")?;
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    let host = host.trim_end_matches('/');
    let segments: Vec<&str> = path.split('/').filter(|part| !part.is_empty()).collect();
    match host {
        "toggle" => Some(Command::Toggle),
        "pause" => Some(Command::Pause),
        "resume" => Some(Command::Resume),
        "overview" => Some(Command::Overview),
        "focus-next-display" => Some(Command::FocusNextDisplay),
        "mode" => match segments.first()?.to_ascii_lowercase().as_str() {
            "unified" => Some(Command::Mode(DeskMode::Unified)),
            "independent" => Some(Command::Mode(DeskMode::Independent)),
            _ => None,
        },
        "profile" if segments.len() >= 2 => {
            let name = decode(&segments[1..].join("/"));
            if name.is_empty() {
                return None;
            }
            match segments[0].to_ascii_lowercase().as_str() {
                "save" => Some(Command::ProfileSave(name)),
                "apply" => Some(Command::ProfileApply(name)),
                _ => None,
            }
        }
        _ => None,
    }
}

fn decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&value[index + 1..index + 3], 16) {
                out.push(byte);
                index += 3;
                continue;
            }
        }
        if bytes[index] == b'+' {
            out.push(b' ');
        } else {
            out.push(bytes[index]);
        }
        index += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands_and_urls() {
        assert_eq!(
            parse_command("profile save Deep work"),
            Some(Command::ProfileSave("Deep work".into()))
        );
        assert_eq!(
            parse_command("mode unified"),
            Some(Command::Mode(DeskMode::Unified))
        );
        assert_eq!(
            parse_url("openlayout://profile/apply/Deep%20work"),
            Some(Command::ProfileApply("Deep work".into()))
        );
    }
}
