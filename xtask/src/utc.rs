//! UTC instants as whole Unix seconds: the RFC 3339 forms xtask reads and writes, and the compact stamp of raw-file
//! names under `/private/` (`loadrec`, `nightly`). The calendar arithmetic is Howard Hinnant's `days_from_civil` and
//! `civil_from_days` over the proleptic Gregorian calendar; no time zone database is read, so a local time is written
//! with its offset (`2026-10-12T23:00:00+03:00`).

use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds since the Unix epoch of `t`; 0 for a time before it.
pub fn unix_secs(t: SystemTime) -> i64 {
    t.duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

/// Days since 1970-01-01 of the date `y-m-d` (proleptic Gregorian; `m` in 1..=12).
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = i64::from((m + 9) % 12);
    let doy = (153 * mp + 2) / 5 + i64::from(d) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The date `(y, m, d)` of a day count since 1970-01-01.
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn parts(secs: i64) -> (i64, u32, u32, i64, i64, i64) {
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let s = secs.rem_euclid(86_400);
    (y, m, d, s / 3_600, s / 60 % 60, s % 60)
}

/// `YYYY-MM-DDTHH:MM:SSZ`.
pub fn rfc3339(secs: i64) -> String {
    let (y, m, d, hh, mm, ss) = parts(secs);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// `YYYYMMDDTHHMMSSZ`, the stem of a raw file or directory name.
pub fn compact(secs: i64) -> String {
    let (y, m, d, hh, mm, ss) = parts(secs);
    format!("{y:04}{m:02}{d:02}T{hh:02}{mm:02}{ss:02}Z")
}

/// Whether `s` is a compact stamp ([`compact`]'s form).
pub fn is_compact(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 16
        && b[8] == b'T'
        && b[15] == b'Z'
        && b[..8].iter().chain(&b[9..15]).all(u8::is_ascii_digit)
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        _ => 28,
    }
}

/// Parses `YYYY-MM-DDTHH:MM[:SS]` followed by `Z` or an offset `±HH:MM` (RFC 3339 without fractional seconds; a time
/// without its offset is refused, since no time zone is assumed) into Unix seconds.
pub fn parse_rfc3339(s: &str) -> Result<i64, String> {
    let bad = |why: &str| {
        format!("'{s}' is not a date-time YYYY-MM-DDTHH:MM[:SS] with Z or an offset ±HH:MM ({why})")
    };
    let b = s.as_bytes();
    let num = |r: std::ops::Range<usize>| -> Result<u32, String> {
        let t = b.get(r).ok_or_else(|| bad("too short"))?;
        if t.iter().all(u8::is_ascii_digit) {
            Ok(t.iter().fold(0u32, |a, c| a * 10 + u32::from(c - b'0')))
        } else {
            Err(bad("a field is not decimal digits"))
        }
    };
    let sep = |i: usize, c: u8| -> Result<(), String> {
        if b.get(i) == Some(&c) {
            Ok(())
        } else {
            Err(bad(&format!("expected '{}' at byte {i}", c as char)))
        }
    };
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    sep(4, b'-')?;
    sep(7, b'-')?;
    sep(10, b'T')?;
    let (hh, mi) = (num(11..13)?, num(14..16)?);
    sep(13, b':')?;
    let (ss, rest) = if b.get(16) == Some(&b':') {
        (num(17..19)?, 19)
    } else {
        (0, 16)
    };
    if !(1..=12).contains(&mo) || d == 0 || d > days_in_month(i64::from(y), mo) {
        return Err(bad("no such date"));
    }
    if hh > 23 || mi > 59 || ss > 59 {
        return Err(bad("no such time"));
    }
    let offset = match &b[rest..] {
        b"Z" => 0,
        [sign @ (b'+' | b'-'), ..] if b.len() == rest + 6 => {
            sep(rest + 3, b':')?;
            let (oh, om) = (num(rest + 1..rest + 3)?, num(rest + 4..rest + 6)?);
            if oh > 23 || om > 59 {
                return Err(bad("no such offset"));
            }
            let o = i64::from(oh * 3_600 + om * 60);
            if *sign == b'+' { o } else { -o }
        }
        _ => return Err(bad("missing or malformed offset")),
    };
    let days = days_from_civil(i64::from(y), mo, d);
    Ok(days * 86_400 + i64::from(hh * 3_600 + mi * 60 + ss) - offset)
}

/// A duration in seconds as `XhYYm` (or `Ym` below an hour), for messages.
pub fn hm(secs: i64) -> String {
    let m = secs.max(0) / 60;
    if m >= 60 {
        format!("{}h{:02}m", m / 60, m % 60)
    } else {
        format!("{m}m")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn known_instants() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(compact(1_791_106_225), "20261004T093025Z");
        assert_eq!(rfc3339(1_791_106_225), "2026-10-04T09:30:25Z");
        assert_eq!(parse_rfc3339("2026-10-04T09:30:25Z"), Ok(1_791_106_225));
        assert_eq!(
            parse_rfc3339("2026-10-04T12:30:25+03:00"),
            Ok(1_791_106_225)
        );
        assert_eq!(
            parse_rfc3339("2026-10-04T05:00:25-04:30"),
            Ok(1_791_106_225)
        );
        assert_eq!(parse_rfc3339("2026-10-04T09:30Z"), Ok(1_791_106_200));
        assert_eq!(
            parse_rfc3339("2024-02-29T00:00:00Z").map(compact),
            Ok("20240229T000000Z".into())
        );
        assert_eq!(rfc3339(-1), "1969-12-31T23:59:59Z");
        assert!(is_compact("20261004T093025Z"));
        assert!(!is_compact("20261004T093025"));
        assert!(!is_compact("2026100xT093025Z"));
        assert_eq!(
            (hm(59), hm(3_600), hm(7_380), hm(-5)),
            ("0m".into(), "1h00m".into(), "2h03m".into(), "0m".into())
        );
    }

    #[test]
    fn malformed_date_times_are_refused() {
        for bad in [
            "",
            "2026-10-04",
            "2026-10-04T09:30:25",
            "2026-10-04 09:30:25Z",
            "2026-13-04T09:30:25Z",
            "2026-02-29T09:30:25Z",
            "2026-10-00T09:30:25Z",
            "2026-10-04T24:00:00Z",
            "2026-10-04T09:60:00Z",
            "2026-10-04T09:30:60Z",
            "2026-10-04T09:30:25+0300",
            "2026-10-04T09:30:25+24:00",
            "2026-10-04T09:30:25.5Z",
            "2026-10-04T09:30:25z",
            "2026-1x-04T09:30:25Z",
            "2026-10-04T09:30:25Zjunk",
        ] {
            assert!(parse_rfc3339(bad).is_err(), "{bad}");
        }
    }

    proptest! {
        #[test]
        fn formatting_and_parsing_round_trip(secs in -62_135_596_800i64..=253_402_300_799) {
            prop_assert_eq!(parse_rfc3339(&rfc3339(secs)), Ok(secs));
            let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
            prop_assert_eq!(days_from_civil(y, m, d), secs.div_euclid(86_400));
            prop_assert!(is_compact(&compact(secs)) || !(0..=9999).contains(&y));
        }
    }
}
