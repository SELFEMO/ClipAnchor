use crate::models::{AppSettings, ClipItem, ClipKind};

pub(crate) fn should_filter_sensitive(settings: &AppSettings, item: &ClipItem) -> bool {
    match settings.privacy_filter_mode.as_str() {
        "off" => false,
        "light" => item_looks_sensitive_light(item),
        _ => item_looks_sensitive_light(item),
    }
}

fn item_looks_sensitive_light(item: &ClipItem) -> bool {
    match item.kind {
        ClipKind::Text => item.text_content.as_deref().map(text_looks_sensitive).unwrap_or(false),
        ClipKind::Image => false,
        ClipKind::File | ClipKind::Mixed => item.file_paths.iter().any(|path| file_path_looks_sensitive(path)),
    }
}

pub(crate) fn text_looks_sensitive(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return false;
    }
    if contains_private_key_block(trimmed) {
        return true;
    }

    let char_len = trimmed.chars().count();
    let line_count = trimmed.lines().count();
    if char_len > 280 || line_count > 6 {
        // 长文本最容易被隐私启发式误伤；这里只保留明确密钥赋值，避免普通文章、代码片段或日志被静默丢弃。
        // Long text is where privacy heuristics most often false-positive, so only explicit secret assignments are blocked instead of silently dropping prose, code, or logs.
        return trimmed.lines().any(line_has_explicit_secret_assignment);
    }

    let lower = trimmed.to_lowercase();
    if has_high_confidence_sensitive_marker(trimmed, &lower) || looks_like_long_secret(trimmed) {
        return true;
    }

    if looks_like_credit_card(trimmed)
        || looks_like_phone_number(trimmed)
        || looks_like_china_identity_number(trimmed)
        || looks_like_otp_code(trimmed)
    {
        return true;
    }
    false
}

fn contains_private_key_block(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.contains("-----begin") && lower.contains("private key")
}

fn has_high_confidence_sensitive_marker(text: &str, lower: &str) -> bool {
    let direct_markers = [
        "private key", "-----begin", "ssh-rsa", "bearer ", "authorization:", "set-cookie",
        "client_secret", "refresh_token", "api_key", "apikey", "access_key",
        "验证码", "校验码", "动态码",
    ];
    if direct_markers.iter().any(|marker| lower.contains(marker)) {
        return true;
    }

    text.lines().any(line_has_explicit_secret_assignment)
}

fn line_has_explicit_secret_assignment(line: &str) -> bool {
    let line = line.trim();
    if line.is_empty() || line.chars().count() > 260 {
        return false;
    }
    let lower = line.to_lowercase();
    let assignment_markers = [
        "password", "passwd", "pwd", "secret", "token", "密钥", "密码", "令牌", "私钥", "身份证", "银行卡",
    ];
    if !assignment_markers.iter().any(|marker| lower.contains(marker)) {
        return false;
    }
    let Some(separator) = line.find('=').or_else(|| line.find(':')).or_else(|| line.find('：')) else {
        return false;
    };
    let value = line[separator + 1..].trim().trim_matches(|ch: char| matches!(ch, '"' | '\'' | '`' | ',' | ';'));
    if value.is_empty() || value.contains(' ') || value.chars().count() < 4 {
        return false;
    }
    // 赋值语境下短密码也可能敏感；但仍要求是紧凑值，避免“Password: enter your password”这类说明文本被过滤。
    // Short passwords can be sensitive in assignment context, but the value must be compact so explanatory text is not filtered.
    value.chars().count() >= 8 || token_looks_like_secret(value)
}

fn looks_like_phone_number(text: &str) -> bool {
    let trimmed = text.trim();
    let char_len = trimmed.chars().count();
    let digits: String = trimmed.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if digits.len() == 11 && char_len <= 24 {
        let bytes = digits.as_bytes();
        if bytes.first() == Some(&b'1') && matches!(bytes.get(1), Some(b'3'..=b'9')) {
            return true;
        }
    }
    let lower = trimmed.to_lowercase();
    let phone_markers = ["phone", "mobile", "cell", "tel", "telephone", "whatsapp", "手机号", "手机", "电话", "联系方式"];
    if digits.len() >= 7 && digits.len() <= 15 && phone_markers.iter().any(|marker| lower.contains(marker)) {
        return true;
    }
    let has_phone_shape = trimmed.contains('+')
        || trimmed.matches('-').count() >= 1
        || trimmed.contains('(')
        || trimmed.contains(')');
    if char_len <= 34 && digits.len() >= 8 && digits.len() <= 15 && has_phone_shape {
        return true;
    }
    false
}

fn looks_like_otp_code(text: &str) -> bool {
    let trimmed = text.trim();
    if trimmed.chars().count() > 120 {
        return false;
    }
    let lower = trimmed.to_lowercase();
    let markers = ["otp", "2fa", "mfa", "verification", "验证码", "校验码", "动态码"];
    let digits: String = trimmed.chars().filter(|ch| ch.is_ascii_digit()).collect();
    digits.len() >= 4 && digits.len() <= 8 && markers.iter().any(|marker| lower.contains(marker))
}

fn looks_like_china_identity_number(text: &str) -> bool {
    let compact: String = text.chars().filter(|ch| ch.is_ascii_alphanumeric()).collect();
    if compact.len() != 18 {
        return false;
    }
    let upper = compact.to_ascii_uppercase();
    let chars: Vec<char> = upper.chars().collect();
    if !chars.iter().take(17).all(|ch| ch.is_ascii_digit()) || !(chars[17].is_ascii_digit() || chars[17] == 'X') {
        return false;
    }
    let year = chars[6..10].iter().collect::<String>().parse::<u32>().unwrap_or(0);
    let month = chars[10..12].iter().collect::<String>().parse::<u32>().unwrap_or(0);
    let day = chars[12..14].iter().collect::<String>().parse::<u32>().unwrap_or(0);
    (1900..=2099).contains(&year) && (1..=12).contains(&month) && (1..=31).contains(&day)
}

fn file_path_looks_sensitive(path: &str) -> bool {
    let lower = path.to_lowercase();
    ["password", "secret", "token", "key", "credential", "private", "密码", "密钥", "凭证", "私密"]
        .iter()
        .any(|marker| lower.contains(marker))
}

fn looks_like_credit_card(text: &str) -> bool {
    let trimmed = text.trim();
    let lower = trimmed.to_lowercase();
    let card_markers = ["card", "credit", "银行卡", "信用卡", "卡号"];
    if trimmed.chars().count() > 80 && !card_markers.iter().any(|marker| lower.contains(marker)) {
        return false;
    }
    let digits: String = trimmed.chars().filter(|ch| ch.is_ascii_digit()).collect();
    if digits.len() < 13 || digits.len() > 19 {
        return false;
    }
    let mut sum = 0u32;
    let mut double_digit = false;
    for ch in digits.chars().rev() {
        let mut value = ch.to_digit(10).unwrap_or(0);
        if double_digit {
            value *= 2;
            if value > 9 { value -= 9; }
        }
        sum += value;
        double_digit = !double_digit;
    }
    sum % 10 == 0
}

fn looks_like_long_secret(text: &str) -> bool {
    text.split(|ch: char| ch.is_whitespace() || matches!(ch, '"' | '\'' | '`' | '<' | '>' | ',' | ';'))
        .any(token_looks_like_secret)
}

fn token_looks_like_secret(raw: &str) -> bool {
    let token = raw.trim_matches(|ch: char| matches!(ch, '(' | ')' | '[' | ']' | '{' | '}' | ',' | ';' | '.'));
    let len = token.len();
    if !(32..=240).contains(&len) || token.contains('\\') || token.contains('/') || token.contains(':') {
        return false;
    }
    let lower = token.to_ascii_lowercase();
    let known_prefixes = ["sk-", "ghp_", "gho_", "ghs_", "github_pat_", "xoxb-", "xoxp-", "akia"];
    if known_prefixes.iter().any(|prefix| lower.starts_with(prefix)) {
        return true;
    }
    if token.matches('.').count() == 2 {
        let jwt_like = token.split('.').all(|part| part.len() >= 8 && part.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'));
        if jwt_like {
            return true;
        }
    }
    if !token.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')) {
        return false;
    }
    let has_upper = token.chars().any(|ch| ch.is_ascii_uppercase());
    let has_lower = token.chars().any(|ch| ch.is_ascii_lowercase());
    let has_digit = token.chars().any(|ch| ch.is_ascii_digit());
    let separator_count = token.chars().filter(|ch| matches!(ch, '_' | '-' | '.')).count();
    let alnum_count = token.chars().filter(|ch| ch.is_ascii_alphanumeric()).count();
    // 长密钥必须是单个高熵 token，而不是把整段文本去掉空格后拼成的长串，否则普通说明文字会被误过滤。
    // A long secret must be one high-entropy token rather than a whole paragraph with spaces removed; otherwise normal prose is filtered by mistake.
    alnum_count >= 32 && has_upper && has_lower && has_digit && separator_count <= 8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_private_keys_and_api_tokens() {
        assert!(text_looks_sensitive("-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAK\n-----END RSA PRIVATE KEY-----"));
        assert!(text_looks_sensitive("ghp_abcdefghijklmnopqrstuvwxyz0123456789ABCD"));
        assert!(text_looks_sensitive("password=SuperSecretValue"));
        assert!(text_looks_sensitive("4111111111111111"));
    }

    #[test]
    fn keeps_ordinary_prose_and_code() {
        assert!(!text_looks_sensitive("Meeting notes for Tuesday"));
        assert!(!text_looks_sensitive("fn main() { println!(\"hello\"); }"));
        assert!(!text_looks_sensitive("Password: enter your password here"));
        let long = "This is a long article about security that mentions the word token in passing but does not assign a secret value. ".repeat(8);
        assert!(!text_looks_sensitive(&long));
    }

    #[test]
    fn light_filter_ignores_images_and_checks_file_names() {
        let image = ClipItem {
            id: "1".into(),
            kind: ClipKind::Image,
            summary: "image".into(),
            text_content: None,
            image_path: Some("shot.png".into()),
            file_paths: Vec::new(),
            bytes: 10,
            created_at: String::new(),
            content_hash: String::new(),
            is_pinned: false,
        };
        let mut settings = AppSettings::default();
        settings.privacy_filter_mode = "light".into();
        assert!(!should_filter_sensitive(&settings, &image));

        let file = ClipItem {
            kind: ClipKind::File,
            file_paths: vec!["C:/secrets/password.txt".into()],
            ..image.clone()
        };
        assert!(should_filter_sensitive(&settings, &file));
    }
}
