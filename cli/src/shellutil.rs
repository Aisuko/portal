/// Single-quote a value for safe interpolation into a POSIX shell command
/// string. The SSH `exec` request is a single string handed to the remote
/// user's shell, so (unlike a local `Command` with an argv array) there is
/// no way to avoid building that string ourselves — this is the one
/// well-known safe way to do it: wrap in single quotes and escape any
/// embedded single quote as `'\''`.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('\'');
    for c in s.chars() {
        if c == '\'' {
            out.push_str("'\\''");
        } else {
            out.push(c);
        }
    }
    out.push('\'');
    out
}

#[cfg(test)]
mod tests {
    use super::quote;

    #[test]
    fn quotes_plain() {
        assert_eq!(quote("hello"), "'hello'");
    }

    #[test]
    fn escapes_single_quote() {
        assert_eq!(quote("it's"), "'it'\\''s'");
    }
}
