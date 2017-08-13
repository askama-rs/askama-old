use serde::Serialize;
use serde_json;

/// Serialize to JSON
///
/// ## Errors
///
/// This will panic if `S`'s implementation of `Serialize` decides to fail,
/// or if `T` contains a map with non-string keys.
pub fn json<S: Serialize>(s: &S) -> String {
	serde_json::to_string(s).expect("json filter could not serialize input")
}

/// Serialize to pretty JSON
///
/// ## Errors
///
/// This will panic if `S`'s implementation of `Serialize` decides to fail,
/// or if `T` contains a map with non-string keys.
pub fn json_pretty<S: Serialize>(s: &S) -> String {
	serde_json::to_string_pretty(s).expect("json filter could not serialize input")
}


#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_json() {
        assert_eq!(json(&true), "true");
        assert_eq!(json(&"foo"), r#""foo""#);
        assert_eq!(json(&vec!["foo", "bar"]), r#"["foo","bar"]"#);
    }

    #[test]
    fn test_json_pretty() {
        assert_eq!(json_pretty(&true), "true");
        assert_eq!(json_pretty(&"foo"), r#""foo""#);
        assert_eq!(json_pretty(&vec!["foo", "bar"]),
r#"[
  "foo",
  "bar"
]"#);
    }
}
