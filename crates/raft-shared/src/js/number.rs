//! raft_shared::js number helpers: ToNumber, Number::toString, toFixed, parseInt, Math (decisions.md D3).

use super::bigint::BigUint;
use super::convert::to_int32;
use super::error::JsError;
use super::string::{JsString, is_js_whitespace};

const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// ToIntegerOrInfinity: NaN and -0 become +0, otherwise truncate.
pub fn to_integer_or_infinity(x: f64) -> f64 {
    if x.is_nan() {
        return 0.0;
    }
    // Adding +0 turns -0 into +0.
    x.trunc() + 0.0
}

pub fn is_finite(x: f64) -> bool {
    x.is_finite()
}

/// Number.isInteger.
pub fn is_integer(x: f64) -> bool {
    x.is_finite() && x.trunc() == x
}

/// Number.isSafeInteger.
pub fn is_safe_integer(x: f64) -> bool {
    is_integer(x) && x.abs() <= MAX_SAFE_INTEGER
}

/// Math.round: halves round toward +Infinity, -0 kept for [-0.5, -0].
pub fn math_round(x: f64) -> f64 {
    if !x.is_finite() || x == 0.0 {
        return x;
    }
    if x > 0.0 && x < 0.5 {
        return 0.0;
    }
    if (-0.5..0.0).contains(&x) {
        return -0.0;
    }
    // At or above 2^52 every double is already an integer.
    if x.abs() >= 4_503_599_627_370_496.0 {
        return x;
    }
    let floor = x.floor();
    // x - floor is exact for |x| < 2^52.
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// Math.floor(a / b).
pub fn math_floor_div(a: f64, b: f64) -> f64 {
    (a / b).floor()
}

/// Number::toString(10).
pub fn number_to_string(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x == 0.0 {
        return "0".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    let mut out = String::new();
    if x < 0.0 {
        out.push('-');
    }
    // `{:e}` prints the shortest round-trip digits as d.ddde<exp>.
    let formatted = format!("{:e}", x.abs());
    let (mantissa, exp) = formatted
        .split_once('e')
        .unwrap_or((formatted.as_str(), "0"));
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let exp: i64 = exp.parse().unwrap_or(0);
    let k = i64::try_from(digits.len()).unwrap_or(i64::MAX);
    let n = exp + 1;
    if k <= n && n <= 21 {
        out.push_str(&digits);
        for _ in 0..(n - k) {
            out.push('0');
        }
    } else if 0 < n && n <= 21 {
        let split = usize::try_from(n).unwrap_or(0);
        out.push_str(&digits[..split]);
        out.push('.');
        out.push_str(&digits[split..]);
    } else if -6 < n && n <= 0 {
        out.push_str("0.");
        for _ in 0..(-n) {
            out.push('0');
        }
        out.push_str(&digits);
    } else {
        let e = n - 1;
        out.push_str(&digits[..1]);
        if k > 1 {
            out.push('.');
            out.push_str(&digits[1..]);
        }
        out.push('e');
        out.push(if e < 0 { '-' } else { '+' });
        out.push_str(&e.abs().to_string());
    }
    out
}

/// Splits a finite non-negative double into (m, e) with x = m * 2^e exactly.
fn decompose(x: f64) -> (u64, i32) {
    let bits = x.to_bits();
    let exp_bits = (bits >> 52) & 0x7FF;
    let frac = bits & ((1u64 << 52) - 1);
    // review: exp_bits is an 11-bit field, so it fits in i32 exactly.
    let exp_bits = i32::try_from(exp_bits).unwrap_or(0);
    if exp_bits == 0 {
        (frac, -1074)
    } else {
        (frac | (1u64 << 52), exp_bits - 1075)
    }
}

/// Number.prototype.toFixed(digits).
pub fn to_fixed(x: f64, digits: u32) -> Result<String, JsError> {
    if digits > 100 {
        return Err(JsError::range(
            "toFixed() digits argument must be between 0 and 100",
        ));
    }
    if x.is_nan() {
        return Ok("NaN".to_string());
    }
    if x.abs() >= 1e21 {
        return Ok(number_to_string(x));
    }
    let mut sign = "";
    let mut v = x;
    if v < 0.0 {
        sign = "-";
        v = -v;
    }
    // n = round(v * 10^digits), ties to the larger n, computed exactly.
    let (m, e) = decompose(v);
    let mut n = BigUint::from_u64(m);
    for _ in 0..digits {
        n.mul_small(10);
    }
    if e >= 0 {
        n.shl(e.unsigned_abs());
    } else {
        let shift = e.unsigned_abs();
        let round_up = n.bit(shift - 1);
        n.shr(shift);
        if round_up {
            n.add_small(1);
        }
    }
    let mut m_str = n.to_decimal();
    let f = usize::try_from(digits).unwrap_or(0);
    if f != 0 {
        if m_str.len() <= f {
            let zeros = "0".repeat(f + 1 - m_str.len());
            m_str = format!("{zeros}{m_str}");
        }
        let k = m_str.len();
        m_str = format!("{}.{}", &m_str[..k - f], &m_str[k - f..]);
    }
    Ok(format!("{sign}{m_str}"))
}

fn digit_value(u: u16) -> Option<u32> {
    let c = char::from_u32(u32::from(u))?;
    c.to_digit(36)
}

/// Exact integer value of digits in a power-of-two radix, correctly rounded.
fn power_of_two_radix_value(digits: &[u32], radix: u32) -> f64 {
    BigUint::from_radix_digits(digits, radix.trailing_zeros()).to_f64()
}

/// V8's accumulation for radices other than 10 and powers of two.
fn other_radix_value(digits: &[u32], radix: u32) -> f64 {
    const MAX_MULTIPLIER: u32 = u32::MAX / 36;
    // V8 skips leading zeros before splitting the digits into 32-bit parts.
    let first = digits.iter().position(|&d| d != 0).unwrap_or(digits.len());
    let digits = &digits[first..];
    let mut value = 0.0_f64;
    let mut i = 0;
    while i < digits.len() {
        let mut part: u32 = 0;
        let mut multiplier: u32 = 1;
        while i < digits.len() {
            let m = multiplier * radix;
            if m > MAX_MULTIPLIER {
                break;
            }
            part = part * radix + digits[i];
            multiplier = m;
            i += 1;
        }
        // review: V8 computes `result * multiplier + part`; its arm64 build (the Node 24.15.0
        // oracle) contracts that into one fused multiply-add, which mul_add reproduces. An x64
        // Node, which does not fuse, can differ in the last bit for long inputs.
        value = value.mul_add(f64::from(multiplier), f64::from(part));
    }
    value
}

fn decimal_digits_value(digits: &[u32]) -> f64 {
    let text: String = digits
        .iter()
        .filter_map(|&d| char::from_digit(d, 10))
        .collect();
    text.parse::<f64>().unwrap_or(f64::NAN)
}

fn radix_value(digits: &[u32], radix: u32) -> f64 {
    if radix == 10 {
        decimal_digits_value(digits)
    } else if radix.is_power_of_two() {
        power_of_two_radix_value(digits, radix)
    } else {
        other_radix_value(digits, radix)
    }
}

/// Global parseInt(string, radix). `radix` is the JS argument (a Number,
/// converted with ToInt32); None is an omitted or undefined radix.
pub fn parse_int(s: &JsString, radix: Option<f64>) -> f64 {
    let units = s.as_units();
    let mut i = units
        .iter()
        .position(|&u| !is_js_whitespace(u))
        .unwrap_or(units.len());
    let mut negative = false;
    if i < units.len() && (units[i] == u16::from(b'-') || units[i] == u16::from(b'+')) {
        negative = units[i] == u16::from(b'-');
        i += 1;
    }
    let mut r = to_int32(radix.unwrap_or(f64::NAN));
    let mut strip_prefix = true;
    if r != 0 {
        if !(2..=36).contains(&r) {
            return f64::NAN;
        }
        if r != 16 {
            strip_prefix = false;
        }
    } else {
        r = 10;
    }
    if strip_prefix
        && i + 1 < units.len()
        && units[i] == u16::from(b'0')
        && (units[i + 1] == u16::from(b'x') || units[i + 1] == u16::from(b'X'))
    {
        i += 2;
        r = 16;
    }
    let radix = r.unsigned_abs();
    let mut digits = Vec::new();
    while i < units.len() {
        match digit_value(units[i]) {
            Some(d) if d < radix => digits.push(d),
            _ => break,
        }
        i += 1;
    }
    if digits.is_empty() {
        return f64::NAN;
    }
    let value = radix_value(&digits, radix);
    if negative { -value } else { value }
}

/// Number(string): the StringNumericLiteral grammar.
pub fn to_number(s: &JsString) -> f64 {
    let units = s.as_units();
    let start = units
        .iter()
        .position(|&u| !is_js_whitespace(u))
        .unwrap_or(units.len());
    let end = units
        .iter()
        .rposition(|&u| !is_js_whitespace(u))
        .map_or(start, |i| i + 1);
    let body = &units[start..end.max(start)];
    if body.is_empty() {
        return 0.0;
    }
    // Everything valid below is ASCII.
    if body.iter().any(|&u| u > 0x7F) {
        return f64::NAN;
    }
    let text: String = body
        .iter()
        .filter_map(|&u| char::from_u32(u32::from(u)))
        .collect();
    let bytes = text.as_bytes();
    if bytes.len() >= 2 && bytes[0] == b'0' {
        let radix = match bytes[1] {
            b'x' | b'X' => Some(16),
            b'o' | b'O' => Some(8),
            b'b' | b'B' => Some(2),
            _ => None,
        };
        if let Some(radix) = radix {
            let rest = &bytes[2..];
            if rest.is_empty() {
                return f64::NAN;
            }
            let mut digits = Vec::with_capacity(rest.len());
            for &b in rest {
                match char::from(b).to_digit(radix) {
                    Some(d) => digits.push(d),
                    None => return f64::NAN,
                }
            }
            return power_of_two_radix_value(&digits, radix);
        }
    }
    match text.as_str() {
        "Infinity" | "+Infinity" => return f64::INFINITY,
        "-Infinity" => return f64::NEG_INFINITY,
        _ => {}
    }
    parse_decimal_literal(bytes).unwrap_or(f64::NAN)
}

/// StrDecimalLiteral with optional sign; None if the text does not match.
fn parse_decimal_literal(bytes: &[u8]) -> Option<f64> {
    let mut i = 0;
    let mut sign = "";
    if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
        if bytes[i] == b'-' {
            sign = "-";
        }
        i += 1;
    }
    let int_start = i;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    let int_digits = &bytes[int_start..i];
    let mut frac_digits: &[u8] = &[];
    if i < bytes.len() && bytes[i] == b'.' {
        i += 1;
        let frac_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        frac_digits = &bytes[frac_start..i];
    }
    if int_digits.is_empty() && frac_digits.is_empty() {
        return None;
    }
    let mut exponent = String::new();
    if i < bytes.len() && (bytes[i] == b'e' || bytes[i] == b'E') {
        i += 1;
        let mut exp_sign = "";
        if i < bytes.len() && (bytes[i] == b'+' || bytes[i] == b'-') {
            if bytes[i] == b'-' {
                exp_sign = "-";
            }
            i += 1;
        }
        let exp_start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1;
        }
        if exp_start == i {
            return None;
        }
        exponent = format!(
            "e{exp_sign}{}",
            std::str::from_utf8(&bytes[exp_start..i]).ok()?
        );
    }
    if i != bytes.len() {
        return None;
    }
    let int_text = if int_digits.is_empty() {
        "0"
    } else {
        std::str::from_utf8(int_digits).ok()?
    };
    let frac_text = if frac_digits.is_empty() {
        "0"
    } else {
        std::str::from_utf8(frac_digits).ok()?
    };
    format!("{sign}{int_text}.{frac_text}{exponent}")
        .parse::<f64>()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_int_other_radix_matches_v8_arm64() {
        let ones = JsString::from("1".repeat(25));
        assert_eq!(parse_int(&ones, Some(7.0)), 223_511_436_610_660_830_000.0);
        let padded = JsString::from(format!("0000000000{}", "1".repeat(25)));
        assert_eq!(parse_int(&padded, Some(7.0)), 223_511_436_610_660_830_000.0);
        let five = JsString::from("02324313404031122424440310302231040101004121441440234");
        assert_eq!(parse_int(&five, Some(5.0)), 1.206_752_615_513_974_2e36);
        assert_eq!(parse_int(&JsString::from("000"), Some(7.0)), 0.0);
    }

    #[test]
    fn long_power_of_two_radix_input_is_linear() {
        let hex = JsString::from("f".repeat(100_000));
        assert_eq!(parse_int(&hex, Some(16.0)), f64::INFINITY);
        assert_eq!(
            to_number(&JsString::from(format!("0x{}", "1".repeat(20)))),
            8.059_505_464_097_528e22
        );
    }
}
