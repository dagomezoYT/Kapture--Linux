//! Minimal JSON writer for metrics sidecars. Avoids a serde dependency in M0.

pub fn escape(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control() => {
                out.push_str("\\u");
                let code = c as u32;
                out.push_str(&format!("{code:04x}"));
            }
            c => out.push(c),
        }
    }
    out
}

pub fn string_value(input: &str) -> String {
    format!("\"{}\"", escape(input))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_quotes() {
        assert_eq!(escape("a\"b"), "a\\\"b");
        assert_eq!(string_value("ok"), "\"ok\"");
    }
}
