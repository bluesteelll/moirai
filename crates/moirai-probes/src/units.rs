//! Sample units, the byte-quantity syntax of the guard's command line, and the display forms the aggregates use
//! ([MP §1.3], [MP §8.2], [MP §7.4]).

/// The unit of an arm's samples ([MP §1.3]).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum Unit {
    /// Nanoseconds of the monotonic clock ([MP §4.4]).
    Ns,
    /// Bytes of memory or disk.
    Bytes,
    /// Events (flushes, opens, …).
    Count,
}

impl Unit {
    /// The record spelling ([MP §7.1]).
    pub const fn as_str(self) -> &'static str {
        match self {
            Unit::Ns => "ns",
            Unit::Bytes => "bytes",
            Unit::Count => "count",
        }
    }

    /// The inverse of [`Unit::as_str`].
    pub fn parse(s: &str) -> Option<Unit> {
        match s {
            "ns" => Some(Unit::Ns),
            "bytes" => Some(Unit::Bytes),
            "count" => Some(Unit::Count),
            _ => None,
        }
    }
}

/// 10³ bytes ([MP §1.3]).
pub const KB: u64 = 1_000;
/// 10⁶ bytes.
pub const MB: u64 = 1_000_000;
/// 10⁹ bytes.
pub const GB: u64 = 1_000_000_000;
/// 10¹² bytes.
pub const TB: u64 = 1_000_000_000_000;
/// 2¹⁰ bytes.
pub const KIB: u64 = 1 << 10;
/// 2²⁰ bytes.
pub const MIB: u64 = 1 << 20;
/// 2³⁰ bytes.
pub const GIB: u64 = 1 << 30;
/// 2⁴⁰ bytes.
pub const TIB: u64 = 1 << 40;

/// One millisecond in nanoseconds.
pub const MS: u64 = 1_000_000;
/// One second in nanoseconds.
pub const SEC: u64 = 1_000_000_000;

/// The multiplier of a byte-unit suffix ([MP §8.2]); the empty suffix and `B` are bytes.
fn suffix_multiplier(s: &str) -> Option<u64> {
    Some(match s {
        "" | "B" => 1,
        "KB" => KB,
        "MB" => MB,
        "GB" => GB,
        "TB" => TB,
        "KiB" => KIB,
        "MiB" => MIB,
        "GiB" => GIB,
        "TiB" => TIB,
        _ => return None,
    })
}

/// Parses a byte quantity ([MP §8.2]): decimal digits, an optional `.` and fraction digits, and an optional unit
/// (`B`, `KB`, `MB`, `GB`, `TB`, `KiB`, `MiB`, `GiB`, `TiB`). The value must be a whole number of bytes that fits
/// in 64 bits. `"25GB"` is 25,000,000,000; `"1.5GB"` is 1,500,000,000; `"0.5B"` is refused.
pub fn parse_bytes(text: &str) -> Result<u64, String> {
    let bad = |why: &str| format!("'{text}' is not a byte quantity: {why}");
    let split = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let (number, suffix) = text.split_at(split);
    let mult = suffix_multiplier(suffix).ok_or_else(|| bad("unknown unit"))?;
    let (int, frac) = match number.split_once('.') {
        Some((i, f)) => (i, f),
        None => (number, ""),
    };
    if int.is_empty()
        || (number.contains('.') && frac.is_empty())
        || !frac.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(bad("expected digits"));
    }
    if frac.len() > 18 {
        return Err(bad("too many fraction digits"));
    }
    let digits = |s: &str| -> Option<u128> {
        s.bytes().try_fold(0u128, |acc, b| {
            acc.checked_mul(10)?.checked_add(u128::from(b - b'0'))
        })
    };
    let i = digits(int).ok_or_else(|| bad("too large"))?;
    let f = digits(frac).ok_or_else(|| bad("too large"))?;
    let scale = 10u128.pow(frac.len() as u32);
    let mantissa = i
        .checked_mul(scale)
        .and_then(|v| v.checked_add(f))
        .ok_or_else(|| bad("too large"))?;
    let scaled = mantissa
        .checked_mul(u128::from(mult))
        .ok_or_else(|| bad("too large"))?;
    if scaled % scale != 0 {
        return Err(bad("not a whole number of bytes"));
    }
    u64::try_from(scaled / scale).map_err(|_| bad("too large"))
}

/// `value / divisor` in thousandths, rounded half up.
fn milli_of(value: u64, divisor: u64) -> u128 {
    (u128::from(value) * 1000 + u128::from(divisor) / 2) / u128::from(divisor)
}

/// Thousandths as `"<int>.<3 digits>"`.
fn three_decimals(milli: u128) -> String {
    format!("{}.{:03}", milli / 1000, milli % 1000)
}

/// `value / divisor` with three decimals, rounded half up, as `"<int>.<3 digits>"`.
fn thousandths(value: u64, divisor: u64) -> String {
    three_decimals(milli_of(value, divisor))
}

/// A duration for an aggregate ([MP §7.4]): whole nanoseconds below 1 µs, otherwise three decimals of µs, ms or s.
/// A value whose rounding reaches 1000 of a unit is written in the next unit (999,999,999 ns is `1.000 s`, never
/// `1000.000 ms`).
pub fn format_ns(ns: u64) -> String {
    if ns < 1_000 {
        return format!("{ns} ns");
    }
    for (divisor, unit) in [(1_000, "µs"), (MS, "ms")] {
        let milli = milli_of(ns, divisor);
        if milli < 1_000_000 {
            return format!("{} {unit}", three_decimals(milli));
        }
    }
    format!("{} s", thousandths(ns, SEC))
}

/// A value in its unit for an aggregate: durations by [`format_ns`], bytes as exact integers with `B`, counts as
/// integers.
pub fn format_value(unit: Unit, value: u64) -> String {
    match unit {
        Unit::Ns => format_ns(value),
        Unit::Bytes => format!("{value} B"),
        Unit::Count => value.to_string(),
    }
}

/// A byte quantity in GB with three decimals, for the guard's refusal texts.
pub fn format_gb(bytes: u64) -> String {
    format!("{} GB", thousandths(bytes, GB))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn units_round_trip() {
        for u in [Unit::Ns, Unit::Bytes, Unit::Count] {
            assert_eq!(Unit::parse(u.as_str()), Some(u));
        }
        assert_eq!(Unit::parse("ms"), None);
    }

    #[test]
    fn byte_quantities() {
        assert_eq!(parse_bytes("25GB"), Ok(25 * GB));
        assert_eq!(parse_bytes("1.5GB"), Ok(1_500_000_000));
        assert_eq!(parse_bytes("30GiB"), Ok(30 * GIB));
        assert_eq!(parse_bytes("1500000000"), Ok(1_500_000_000));
        assert_eq!(parse_bytes("4KiB"), Ok(4096));
        assert_eq!(parse_bytes("0.5KB"), Ok(500));
        assert_eq!(parse_bytes("7B"), Ok(7));
        assert_eq!(parse_bytes("0"), Ok(0));
        assert_eq!(parse_bytes("18446744073709551615"), Ok(u64::MAX));
        for bad in [
            "",
            "GB",
            "1.",
            ".5GB",
            "1.2.3",
            "0.5B",
            "0.5",
            "1 GB",
            "1gb",
            "-1",
            "18446744073709551616",
            "20000000TB",
            "1.0000000000000000001GB",
        ] {
            assert!(parse_bytes(bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn display_forms() {
        assert_eq!(format_ns(999), "999 ns");
        assert_eq!(format_ns(1_000), "1.000 µs");
        assert_eq!(format_ns(1_234_567), "1.235 ms");
        assert_eq!(format_ns(49_999_999), "50.000 ms");
        assert_eq!(format_ns(2 * SEC + 1), "2.000 s");
        // Rounding never writes 1000 of a unit: it moves to the next one.
        assert_eq!(format_ns(999_999), "999.999 µs");
        assert_eq!(format_ns(MS), "1.000 ms");
        assert_eq!(format_ns(999_999_499), "999.999 ms");
        assert_eq!(format_ns(999_999_500), "1.000 s");
        assert_eq!(format_ns(999_999_999), "1.000 s");
        assert_eq!(format_ns(u64::MAX), "18446744073.710 s");
        assert_eq!(format_value(Unit::Bytes, 4_000_000), "4000000 B");
        assert_eq!(format_value(Unit::Count, 3), "3");
        assert_eq!(format_gb(1_499_999_999), "1.500 GB");
        assert_eq!(format_gb(25 * GB), "25.000 GB");
    }

    proptest! {
        #[test]
        fn whole_bytes_round_trip(v in any::<u64>()) {
            prop_assert_eq!(parse_bytes(&v.to_string()), Ok(v));
        }

        #[test]
        fn sub_second_units_stay_below_a_thousand(ns in any::<u64>()) {
            let s = format_ns(ns);
            let (number, unit) = s.split_once(' ').unwrap();
            let whole: u64 = number.split('.').next().unwrap().parse().unwrap();
            prop_assert!(unit == "s" || whole < 1000, "{}", s);
        }

        #[test]
        fn decimal_units_scale(v in 0u64..=18_000_000, frac in 0u64..1000) {
            let text = format!("{v}.{frac:03}GB");
            prop_assert_eq!(parse_bytes(&text), Ok(v * GB + frac * MB));
        }
    }
}
