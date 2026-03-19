/// Core preprocessing utilities for the pipeline.
///
/// The implementation focuses on simple, deterministic cleaning so later
/// stages always see the same newline layout and no stray control characters.
pub fn normalize_text(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut last_was_cr = false;

    for ch in input.chars() {
        if last_was_cr {
            if ch == '\n' {
                last_was_cr = false;
                continue;
            }
            last_was_cr = false;
        }

        match ch {
            '\r' => {
                output.push('\n');
                last_was_cr = true;
            }
            '\n' | '\u{2028}' | '\u{2029}' => {
                output.push('\n');
            }
            '\t' => {
                output.push(' ');
            }
            c if c.is_control() => {
                continue;
            }
            c => output.push(c),
        }
    }

    output
}

#[cfg(test)]
mod tests {
    use super::normalize_text;

    #[test]
    fn normalize_removes_control_characters() {
        let raw = "line\x00\x1f\nsecond";
        assert_eq!(normalize_text(raw), "line\nsecond");
    }

    #[test]
    fn normalize_composes_newlines() {
        let raw = "a\r\nb\r\nc\n";
        assert_eq!(normalize_text(raw), "a\nb\nc\n");
    }
}
