//! raft_shared::js errors: `JsError` (a thrown JS built-in error) and `String(err)` (decisions.md D3, D4).

use std::fmt;

/// A JS built-in error: `name` is the JS `err.name`, `message` is `err.message`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JsError {
    pub name: String,
    pub message: String,
}

impl JsError {
    fn with_name(name: &str, message: impl Into<String>) -> Self {
        JsError {
            name: name.to_string(),
            message: message.into(),
        }
    }

    pub fn syntax(message: impl Into<String>) -> Self {
        Self::with_name("SyntaxError", message)
    }

    pub fn range(message: impl Into<String>) -> Self {
        Self::with_name("RangeError", message)
    }

    pub fn uri(message: impl Into<String>) -> Self {
        Self::with_name("URIError", message)
    }

    pub fn type_error(message: impl Into<String>) -> Self {
        Self::with_name("TypeError", message)
    }
}

/// An error with a JS `name` (mapping-guide §4.2): `String(err)` renders as
/// `"{js_name}: {message}"`, where the message is the `Display` text. Every
/// error type that reaches a translated `String(err)` implements this.
pub trait JsErrorName: std::error::Error {
    fn js_name(&self) -> &str;
}

impl JsErrorName for JsError {
    fn js_name(&self) -> &str {
        &self.name
    }
}

impl fmt::Display for JsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for JsError {}

impl serde::ser::Error for JsError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        JsError::type_error(msg.to_string())
    }
}

impl serde::de::Error for JsError {
    fn custom<T: fmt::Display>(msg: T) -> Self {
        JsError::type_error(msg.to_string())
    }
}

/// `String(err)` / `${err}` (Error.prototype.toString).
pub fn error_to_string<E: JsErrorName + ?Sized>(err: &E) -> String {
    error_to_string_parts(err.js_name(), &err.to_string())
}

/// Error.prototype.toString over a name and message: an empty name gives the
/// message alone, an empty message gives the name alone.
pub fn error_to_string_parts(name: &str, message: &str) -> String {
    if name.is_empty() {
        message.to_string()
    } else if message.is_empty() {
        name.to_string()
    } else {
        format!("{name}: {message}")
    }
}
