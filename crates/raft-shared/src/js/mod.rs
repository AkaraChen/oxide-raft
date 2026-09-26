//! raft_shared::js: JavaScript semantics helpers and the JS value model (decisions.md D3).

mod bigint;
mod bridge;
mod convert;
mod error;
mod json;
mod number;
mod object;
mod string;
mod value;

pub use bridge::{from_value, to_value};
pub use convert::{
    f64_to_i32_exact, f64_to_i64_exact, f64_to_u32_exact, f64_to_u64_exact, f64_to_usize_exact,
    i64_to_f64, i64_to_f64_exact, to_int32, to_uint32, to_usize_index, u64_to_f64,
    u64_to_f64_exact, usize_to_f64,
};
pub use error::{JsError, JsErrorName, error_to_string, error_to_string_parts};
pub use json::{json_parse, json_parse_js, json_stringify, json_stringify_pretty, response_text};
pub use number::{
    is_finite, is_integer, is_safe_integer, math_floor_div, math_round, number_to_string,
    parse_int, to_fixed, to_integer_or_infinity, to_number,
};
pub use object::{Object, object_assign};
pub use string::{
    JsString, MAX_STRING_LENGTH, collapse_js_whitespace, encode_uri_component, pad_end, pad_start,
    sort_default, split, string_replace_all, string_replace_first, trim, trim_end, trim_start,
    utf16_len, utf16_slice,
};
pub use value::{Value, is_nullish, is_truthy, less_than, to_display_string, to_number_value};

#[cfg(test)]
#[path = "golden_tests.rs"]
mod golden_tests;
