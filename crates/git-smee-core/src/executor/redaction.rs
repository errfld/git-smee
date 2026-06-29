pub(super) fn redact_command(command: &str) -> String {
    let tokens = tokenize_command(command);
    let executable_index = tokens
        .iter()
        .position(|token| !is_inline_env_assignment(token));
    let mut redacted = executable_index
        .and_then(|index| tokens.get(index))
        .cloned()
        .unwrap_or_else(|| "<redacted>".to_string());
    if redacted.chars().count() > 80 {
        redacted = redacted.chars().take(77).collect();
        redacted.push_str("...");
    }
    if let Some(index) = executable_index
        && tokens.len() > index + 1
    {
        redacted.push_str(" <args redacted>");
    }
    redacted
}

fn is_inline_env_assignment(token: &str) -> bool {
    let Some((key, _)) = token.split_once('=') else {
        return false;
    };
    is_valid_env_var_name(key)
}

fn is_valid_env_var_name(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !matches!(first, 'A'..='Z' | 'a'..='z' | '_') {
        return false;
    }
    chars.all(|ch| matches!(ch, 'A'..='Z' | 'a'..='z' | '0'..='9' | '_'))
}

fn tokenize_command(command: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_single_quotes = false;
    let mut in_double_quotes = false;
    let mut chars = command.chars().peekable();

    while let Some(ch) = chars.next() {
        match ch {
            '\\' if !in_single_quotes => {
                current.push(ch);
                if let Some(next) = chars.peek().copied()
                    && (next.is_whitespace() || matches!(next, '\\' | '\'' | '"'))
                {
                    current.push(chars.next().expect("peeked char should exist"));
                }
            }
            '\'' if !in_double_quotes => {
                in_single_quotes = !in_single_quotes;
            }
            '"' if !in_single_quotes => {
                in_double_quotes = !in_double_quotes;
            }
            ch if ch.is_whitespace() && !in_single_quotes && !in_double_quotes => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }

    if !current.is_empty() {
        tokens.push(current);
    }

    tokens
}
