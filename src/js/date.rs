//! Minimal `Date` support: js-yaml turns unquoted timestamps into Dates, and
//! they surface through JSON.stringify / template strings / yaml.dump.
//! The process time zone is assumed to be UTC (as in the container image).

const MS_PER_DAY: f64 = 86_400_000.0;

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `Date.UTC(year, month, day, h, min, s, ms)` with month overflow handling.
pub fn date_utc(year: f64, month: f64, day: f64, h: f64, min: f64, s: f64, ms: f64) -> f64 {
    let args = [year, month, day, h, min, s, ms];
    if args.iter().any(|v| !v.is_finite()) {
        return f64::NAN;
    }
    let ym = year + (month / 12.0).floor();
    let mn = month.rem_euclid(12.0);
    let days = days_from_civil(ym as i64, mn as i64 + 1, 1) as f64 + day - 1.0;
    let time = h * 3_600_000.0 + min * 60_000.0 + s * 1000.0 + ms;
    let tv = days * MS_PER_DAY + time;
    if tv.abs() > 8.64e15 { f64::NAN } else { tv }
}

struct Parts {
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    ms: i64,
    weekday: i64,
}

fn parts(tv: f64) -> Parts {
    let t = tv as i64;
    let days = t.div_euclid(86_400_000);
    let rem = t.rem_euclid(86_400_000);
    let (year, month, day) = civil_from_days(days);
    Parts {
        year,
        month,
        day,
        hour: rem / 3_600_000,
        minute: rem / 60_000 % 60,
        second: rem / 1000 % 60,
        ms: rem % 1000,
        weekday: (days + 4).rem_euclid(7),
    }
}

/// `Date.prototype.toISOString` (None for invalid dates).
pub fn to_iso_string(tv: f64) -> Option<String> {
    if !tv.is_finite() {
        return None;
    }
    let p = parts(tv);
    let year = if (0..=9999).contains(&p.year) {
        format!("{:04}", p.year)
    } else if p.year < 0 {
        format!("-{:06}", -p.year)
    } else {
        format!("+{:06}", p.year)
    };
    Some(format!("{}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z", year, p.month, p.day, p.hour, p.minute, p.second, p.ms))
}

const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// `Date.prototype.toString` in UTC.
pub fn date_to_string(tv: f64) -> String {
    if !tv.is_finite() {
        return "Invalid Date".into();
    }
    let p = parts(tv);
    let year = if p.year < 0 { format!("-{:06}", -p.year) } else { format!("{:04}", p.year) };
    format!(
        "{} {} {:02} {} {:02}:{:02}:{:02} GMT+0000 (Coordinated Universal Time)",
        WEEKDAYS[p.weekday as usize],
        MONTHS[(p.month - 1) as usize],
        p.day,
        year,
        p.hour,
        p.minute,
        p.second
    )
}

/// Current UTC year (`new Date().getFullYear()` in a UTC process).
pub fn current_year() -> i64 {
    let now =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0);
    parts(now).year
}
