//! Local-time formatting for the Unix seconds the server hands out. The
//! browser has the teacher's clock; the non-wasm build (server render) prints
//! UTC, which no one sees because timestamps arrive after hydration.

/// "HH:MM" in the browser's local time.
#[cfg(target_arch = "wasm32")]
pub fn local_time(secs: i64) -> String {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(secs as f64 * 1000.0));
    format!("{:02}:{:02}", date.get_hours(), date.get_minutes())
}

/// "DD/MM/YYYY HH:MM" in the browser's local time.
#[cfg(target_arch = "wasm32")]
pub fn local_date_time(secs: i64) -> String {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_f64(secs as f64 * 1000.0));
    format!(
        "{:02}/{:02}/{} {:02}:{:02}",
        date.get_date(),
        date.get_month() + 1,
        date.get_full_year(),
        date.get_hours(),
        date.get_minutes()
    )
}

#[cfg(not(target_arch = "wasm32"))]
pub fn local_time(secs: i64) -> String {
    let (_, _, _, hour, minute) = civil_utc(secs);
    format!("{hour:02}:{minute:02}")
}

#[cfg(not(target_arch = "wasm32"))]
pub fn local_date_time(secs: i64) -> String {
    let (year, month, day, hour, minute) = civil_utc(secs);
    format!("{day:02}/{month:02}/{year} {hour:02}:{minute:02}")
}

/// Year, month, day, hour, minute of a Unix time in UTC (Howard Hinnant's
/// days-to-civil algorithm).
#[cfg(not(target_arch = "wasm32"))]
fn civil_utc(secs: i64) -> (i64, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (
        year,
        month,
        day,
        (rem / 3_600) as u32,
        ((rem % 3_600) / 60) as u32,
    )
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn utc_fallback_formats_known_instants() {
        assert_eq!(local_date_time(0), "01/01/1970 00:00");
        assert_eq!(local_date_time(951_782_400), "29/02/2000 00:00");
        assert_eq!(local_time(1_790_000_000), "14:13");
        assert_eq!(local_date_time(1_790_000_000), "21/09/2026 14:13");
    }
}
