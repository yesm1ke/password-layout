//! Reading Hyprland's flat JSON replies, ported from `jsonIntegers`,
//! `jsonStrings` and `valuesOf` in `src/compositor.cpp`.
//!
//! The documents are Hyprland's replies, not general JSON: the functions must
//! give the C++ answers on any input, so they work on bytes and never slice a
//! `&str` by byte index (an index may fall inside a UTF-8 character).

/// First `needle` in `haystack` at or after `from`, or `None`, like C++
/// `std::string_view::find(needle, from)`: a `from` at the end of the input
/// finds nothing (a match may not start before `from` again).
fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if needle.is_empty() {
        return Some(from.min(haystack.len()));
    }
    if from >= haystack.len() {
        return None;
    }
    let start = from.min(haystack.len().saturating_sub(needle.len()));
    haystack[start..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|at| at + start)
}

/// Where the value of each `"key":` starts, in order of appearance.
/// After each match only spaces are skipped (not tabs or newlines), and the
/// next search starts at that position.
fn values_of(json: &[u8], key: &str) -> Vec<usize> {
    let mut needle = String::with_capacity(key.len() + 2);
    needle.push('"');
    needle.push_str(key);
    needle.push_str("\":");
    let needle = needle.into_bytes();
    let mut starts = Vec::new();
    let mut from = 0;
    while let Some(matched) = find(json, &needle, from) {
        let mut at = matched + needle.len();
        while at < json.len() && json[at] == b' ' {
            at += 1;
        }
        starts.push(at);
        from = at;
    }
    starts
}

/// The integer values of each `"key":`: like C++ `std::from_chars(long)`, an
/// optional `-` (no `+`, no leading spaces or tabs), then the longest run of
/// ASCII digits; a value with no digits, or one that overflows `i64`, is
/// skipped. `"pid": 12abc` gives 12; `"pid": 1.5` gives 1.
pub fn json_integers(json: &str, key: &str) -> Vec<i64> {
    let bytes = json.as_bytes();
    let mut values = Vec::new();
    for start in values_of(bytes, key) {
        let mut at = start;
        if at < bytes.len() && bytes[at] == b'-' {
            at += 1;
        }
        let digits_from = at;
        while at < bytes.len() && bytes[at].is_ascii_digit() {
            at += 1;
        }
        if at == digits_from {
            continue;
        }
        // The bytes are ASCII: one `-` at most, then digits.
        if let Some(value) = std::str::from_utf8(&bytes[start..at])
            .ok()
            .and_then(|text| text.parse::<i64>().ok())
        {
            values.push(value);
        }
    }
    values
}

/// The string values of each `"key":`: the value must start with `"`; bytes
/// are collected until the next unescaped `"`. A `\` followed by a byte gives
/// that byte (no real unescaping: `\n` gives `n`); a `\` as the last byte is
/// kept; an unterminated string runs to the end and is still returned.
pub fn json_strings(json: &str, key: &str) -> Vec<String> {
    let bytes = json.as_bytes();
    let mut values = Vec::new();
    for at in values_of(bytes, key) {
        if at >= bytes.len() || bytes[at] != b'"' {
            continue;
        }
        let mut value = Vec::new();
        let mut at = at + 1;
        while at < bytes.len() && bytes[at] != b'"' {
            if bytes[at] == b'\\' && at + 1 < bytes.len() {
                at += 1;
            }
            value.push(bytes[at]);
            at += 1;
        }
        values.push(String::from_utf8_lossy(&value).into_owned());
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integers_copy_the_cpp_semantics() {
        assert_eq!(json_integers(r#"{"pid": -0}"#, "pid"), vec![0]);
        assert_eq!(json_integers(r#"{"pid": 12abc}"#, "pid"), vec![12]);
        assert_eq!(json_integers("{\"pid\":\t7}", "pid"), Vec::<i64>::new());
    }

    #[test]
    fn a_key_at_the_end_of_the_input_does_not_loop() {
        // The match ends at the end of the input: the next search starts
        // there and must find nothing (as C++ does), not re-find the match.
        assert_eq!(json_integers("{\"a\": 1, \"b\":", "b"), Vec::<i64>::new());
        assert_eq!(json_strings("{\"a\": 1, \"b\":", "b"), Vec::<String>::new());
        assert_eq!(json_integers("{\"a\": 1, \"b\":", "a"), vec![1]);
    }

    #[test]
    fn backslash_is_not_unescaped() {
        assert_eq!(json_strings(r#"{"s": "a\\b"}"#, "s"), vec!["a\\b"]);
    }
}
