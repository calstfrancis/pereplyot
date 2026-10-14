//! Earlier versions of a file as timestamped copies (`<dir>/2026-10-14T15-30-12Z.json`, UTC),
//! for mistakes undo can no longer reach. Keeps the newest [`KEEP_RECENT`] plus one a day.

use std::fs;
use std::path::{Path, PathBuf};

use crate::fsutil;

pub const KEEP_RECENT: usize = 20;
pub const KEEP_DAYS: i64 = 90;
/// How long after one copy an ordinary save takes another.
pub const MIN_GAP_SECS: i64 = 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub path: PathBuf,
    /// Seconds since the Unix epoch.
    pub time: i64,
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn name_for(time: i64) -> String {
    let dt = glib::DateTime::from_unix_utc(time).expect("time in range");
    format!(
        "{:04}-{:02}-{:02}T{:02}-{:02}-{:02}Z.json",
        dt.year(),
        dt.month(),
        dt.day_of_month(),
        dt.hour(),
        dt.minute(),
        dt.second()
    )
}

fn time_of(name: &str) -> Option<i64> {
    let s = name.strip_suffix("Z.json")?;
    let (date, time) = s.split_once('T')?;
    let d: Vec<i32> = date
        .split('-')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    let t: Vec<i32> = time
        .split('-')
        .map(|p| p.parse().ok())
        .collect::<Option<_>>()?;
    if d.len() != 3 || t.len() != 3 {
        return None;
    }
    let dt = glib::DateTime::new(
        &glib::TimeZone::utc(),
        d[0],
        d[1],
        d[2],
        t[0],
        t[1],
        t[2] as f64,
    )
    .ok()?;
    Some(dt.to_unix())
}

/// The versions kept in `dir`, newest first.
pub fn list(dir: &Path) -> Vec<Version> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Version> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name();
            let time = time_of(name.to_str()?)?;
            Some(Version {
                path: e.path(),
                time,
            })
        })
        .collect();
    out.sort_by_key(|v| std::cmp::Reverse(v.time));
    out
}

/// Whether an ordinary save should take a copy now: none yet, or the newest is an hour old.
pub fn due(dir: &Path, now: i64) -> bool {
    list(dir)
        .first()
        .map_or(true, |v| now - v.time >= MIN_GAP_SECS)
}

/// Keep `text` as the version at `now`, unless it is the same as the newest one kept. Thins
/// the folder afterwards.
pub fn keep(dir: &Path, text: &str, now: i64) -> std::io::Result<()> {
    let existing = list(dir);
    if let Some(newest) = existing.first() {
        if fs::read_to_string(&newest.path).is_ok_and(|t| t == text) {
            return Ok(());
        }
    }
    fsutil::write_atomic(&dir.join(name_for(now)), text.as_bytes())?;
    thin(dir, now);
    Ok(())
}

fn thin(dir: &Path, now: i64) {
    let all = list(dir);
    let mut days_seen = std::collections::HashSet::new();
    for (i, v) in all.iter().enumerate() {
        let day = v.time.div_euclid(86_400);
        let first_of_day = days_seen.insert(day);
        let recent = i < KEEP_RECENT;
        let daily = first_of_day && now - v.time <= KEEP_DAYS * 86_400;
        if !recent && !daily {
            let _ = fs::remove_file(&v.path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("pereplyot-versions-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn names_round_trip() {
        let t = 1_760_450_012;
        assert_eq!(time_of(&name_for(t)), Some(t));
        assert_eq!(time_of("notes.json"), None);
    }

    #[test]
    fn identical_text_is_not_kept_twice() {
        let d = dir("dedup");
        keep(&d, "a", 1000).unwrap();
        keep(&d, "a", 5000).unwrap();
        keep(&d, "b", 9000).unwrap();
        let v = list(&d);
        assert_eq!(v.len(), 2);
        assert_eq!(fs::read_to_string(&v[0].path).unwrap(), "b");
    }

    #[test]
    fn due_after_an_hour() {
        let d = dir("due");
        assert!(due(&d, 0));
        keep(&d, "a", 1_000_000).unwrap();
        assert!(!due(&d, 1_000_000 + MIN_GAP_SECS - 1));
        assert!(due(&d, 1_000_000 + MIN_GAP_SECS));
    }

    #[test]
    fn thinning_keeps_recent_and_one_a_day() {
        let d = dir("thin");
        let day = 86_400;
        let start = 1_700_000_000 - 1_700_000_000 % day;
        // Thirty days, four copies a day.
        let mut t = start;
        for i in 0..120 {
            t = start + (i / 4) * day + (i % 4) * 3600;
            keep(&d, &format!("{i}"), t).unwrap();
        }
        let v = list(&d);
        // Twenty most recent, plus the newest of each earlier day (30 days, the last five of
        // which are already covered by the twenty).
        assert_eq!(v.len(), 20 + 25);
        assert_eq!(fs::read_to_string(&v[0].path).unwrap(), "119");
        // Long past the daily window, only the twenty most recent remain.
        keep(&d, "late", t + 200 * day).unwrap();
        assert_eq!(list(&d).len(), KEEP_RECENT);
    }
}
