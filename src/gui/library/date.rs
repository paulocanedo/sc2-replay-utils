//! Utilitários de data para o filtro `DateRange`.
//!
//! Todo o cálculo civil ↔ dias-desde-epoch segue o algoritmo de
//! Howard Hinnant. No Windows usamos `GetLocalTime` para obter a data
//! local do sistema; em outras plataformas derivamos a partir de
//! `SystemTime::now()` em UTC (sem ajuste de fuso — razoável enquanto
//! o usuário roda a app na máquina onde os replays foram jogados).

use super::filter::DateRange;

pub(super) fn today_str() -> String {
    #[cfg(target_os = "windows")]
    {
        use std::mem::MaybeUninit;
        unsafe {
            let mut st = MaybeUninit::<winapi_local::SYSTEMTIME>::uninit();
            winapi_local::GetLocalTime(st.as_mut_ptr());
            let st = st.assume_init();
            format!("{:04}-{:02}-{:02}", st.w_year, st.w_month, st.w_day)
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let days = now / 86400;
        let (y, m, d) = civil_from_days(days as i64);
        format!("{y:04}-{m:02}-{d:02}")
    }
}

#[cfg(target_os = "windows")]
mod winapi_local {
    #[repr(C)]
    #[allow(dead_code)]
    pub struct SYSTEMTIME {
        pub w_year: u16,
        pub w_month: u16,
        pub w_day_of_week: u16,
        pub w_day: u16,
        pub w_hour: u16,
        pub w_minute: u16,
        pub w_second: u16,
        pub w_milliseconds: u16,
    }
    unsafe extern "system" {
        pub fn GetLocalTime(lp: *mut SYSTEMTIME);
    }
}

/// Days since epoch → (year, month, day). Algorithm from Howard Hinnant.
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

/// Day of week (0=Monday .. 6=Sunday) from (y, m, d).
fn day_of_week(y: i32, m: u32, d: u32) -> u32 {
    // Tomohiko Sakamoto's algorithm
    let t = [0i32, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let y = if m < 3 { y - 1 } else { y };
    let dow = (y + y / 4 - y / 100 + y / 400 + t[(m - 1) as usize] + d as i32) % 7;
    // Sakamoto: 0=Sunday. Convert to 0=Monday.
    ((dow + 6) % 7) as u32
}

fn parse_date(dt: &str) -> Option<(i32, u32, u32)> {
    if dt.len() < 10 {
        return None;
    }
    let y: i32 = dt[..4].parse().ok()?;
    let m: u32 = dt[5..7].parse().ok()?;
    let d: u32 = dt[8..10].parse().ok()?;
    Some((y, m, d))
}

pub(super) fn matches_date_range(datetime: &str, range: DateRange, today: &str) -> bool {
    match range {
        DateRange::All => true,
        DateRange::Today => datetime.starts_with(today),
        DateRange::ThisWeek => {
            let Some((ty, tm, td)) = parse_date(today) else { return true; };
            let Some((ry, rm, rd)) = parse_date(datetime) else { return false; };
            let today_dow = day_of_week(ty, tm, td);
            // Monday of this week
            let today_days = days_from_civil(ty, tm, td);
            let week_start = today_days - today_dow as i64;
            let replay_days = days_from_civil(ry, rm, rd);
            replay_days >= week_start && replay_days <= today_days
        }
        DateRange::ThisMonth => {
            if today.len() < 7 {
                return true;
            }
            datetime.starts_with(&today[..7])
        }
    }
}

fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = y as i64 - if m <= 2 { 1 } else { 0 };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = (y - era * 400) as u32;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i64 - 719468
}
