//! 时间戳:记录里是 RFC 3339(`2026-11-22T12:15:22.726Z`),台账里存 Unix 毫秒。
//! vg-core 不引入 chrono,只处理这一种格式。

/// 解析 `YYYY-MM-DDTHH:MM:SS[.fff](Z|±HH:MM)`;认不出返回 None
pub fn parse_rfc3339_ms(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || !matches!(b[10], b'T' | b' ') || b[13] != b':' || b[16] != b':' {
        return None;
    }
    let num = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let (h, mi, sec) = (num(11..13)?, num(14..16)?, num(17..19)?);
    let mut i = 19;
    let mut ms = 0i64;
    if b.get(i) == Some(&b'.') {
        i += 1;
        let start = i;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        let frac = &s[start..i];
        let digits: String = frac.chars().chain(std::iter::repeat('0')).take(3).collect();
        ms = digits.parse().ok()?;
    }
    let offset_min = match b.get(i) {
        None | Some(b'Z') | Some(b'z') => 0,
        Some(b'+') | Some(b'-') => {
            let sign = if b[i] == b'-' { -1 } else { 1 };
            let oh = num(i + 1..i + 3)?;
            let om = num(i + 4..i + 6).unwrap_or(0);
            sign * (oh * 60 + om)
        }
        _ => return None,
    };
    let days = days_from_civil(y, mo, d);
    Some(((days * 86_400 + h * 3600 + mi * 60 + sec - offset_min * 60) * 1000) + ms)
}

/// 公历日期 → 自 1970-01-01 起的天数(Howard Hinnant 的算法)
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 天数 → (年, 月, 日)
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 毫秒时间戳所在的 UTC 月份,台账按它分文件:"2026-11"
pub fn month_key(ms: i64) -> String {
    let (y, m, _) = civil_from_days(ms.div_euclid(86_400_000));
    format!("{y:04}-{m:02}")
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn system_time_ms(t: std::time::SystemTime) -> i64 {
    t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_timestamps() {
        assert_eq!(parse_rfc3339_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339_ms("1970-01-01T00:00:01.5Z"), Some(1500));
        assert_eq!(parse_rfc3339_ms("2026-11-22T12:15:22.726Z"), Some(1_795_349_722_726));
        assert_eq!(parse_rfc3339_ms("2026-11-22T20:15:22.726+08:00"), Some(1_795_349_722_726));
        assert_eq!(parse_rfc3339_ms("nope"), None);
    }

    #[test]
    fn round_trips_civil_dates() {
        for days in [-1, 0, 59, 365, 20_000, 20_414] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m as i64, d as i64), days);
        }
        assert_eq!(month_key(1_795_349_722_726), "2026-11");
    }
}
