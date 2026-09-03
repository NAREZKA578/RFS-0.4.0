//! Лимиты чата/имён из конфига.
//!
//! Донор: `RFS-0.3/rfs-server/.../mod.rs:1762-1778` (санитайз) +
//! `config.rs:101-103` (поля) + обработчик `mod.rs:569-570`.
//!
//! Ошибка донора (№223 CHAT-LIMIT-1): обработчик чата хардкодил 32/200,
//! конфиг читался только в connect-пути. Здесь лимиты — параметр.

/// Лимиты из серверного конфига (зеркало `ServerConfig` донора).
#[derive(Debug, Clone, Copy)]
pub struct ChatLimits {
    pub max_name_len: usize,
    pub max_chat_len: usize,
}

impl Default for ChatLimits {
    fn default() -> Self {
        Self {
            max_name_len: 32,
            max_chat_len: 200,
        }
    }
}

fn is_format_char(c: char) -> bool {
    matches!(c,
        '\u{202A}'..='\u{202E}'
        | '\u{2066}'..='\u{2069}'
        | '\u{200B}' | '\u{200C}' | '\u{200D}'
        | '\u{FEFF}' | '\u{2060}'
        | '\u{2061}'..='\u{2065}')
}

fn sanitize(s: &str, max_len: usize) -> String {
    s.chars()
        .filter(|c| !c.is_control() && !is_format_char(*c))
        .take(max_len)
        .collect::<String>()
        .trim()
        .to_string()
}

/// FIX №223: лимиты только из конфига.
pub fn sanitize_chat_msg(name: &str, msg: &str, limits: &ChatLimits) -> (String, String) {
    (
        sanitize(name, limits.max_name_len),
        sanitize(msg, limits.max_chat_len),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_limits_apply_regression_223() {
        let lim = ChatLimits {
            max_name_len: 4,
            max_chat_len: 5,
        };
        let (n, m) = sanitize_chat_msg("123456", "abcdef", &lim);
        assert_eq!(n, "1234");
        assert_eq!(m, "abcde");
    }

    #[test]
    fn control_and_bidi_stripped() {
        let lim = ChatLimits::default();
        let (_, m) = sanitize_chat_msg("ok", "a\u{202Eb}\u{0}c", &lim);
        assert_eq!(m, "abc");
    }
}
