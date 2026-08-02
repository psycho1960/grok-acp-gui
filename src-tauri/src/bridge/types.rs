//! Shared scalar types crossing the bridge (IDs, timestamps, display paths).

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ID newtypes
// ---------------------------------------------------------------------------

macro_rules! id_newtype {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(raw: impl Into<String>) -> Self {
                Self(raw.into())
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

id_newtype!(TaskId);
id_newtype!(SessionId);
id_newtype!(ProjectId);
id_newtype!(CorrelationId);

// ---------------------------------------------------------------------------
// Path display wrapper
// ---------------------------------------------------------------------------

/// A path that has been validated to stay within an approved root.
/// The `display` field is safe to show in the UI; the internal `path`
/// is only used by the backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayPath {
    /// Human-readable path (relative or scrubbed) safe for the Renderer.
    pub display: String,
    /// Opaque token the Renderer passes back verbatim when referencing
    /// this path. The Renderer must never inspect or construct this field.
    #[serde(skip_serializing)]
    pub internal: String,
}

// ---------------------------------------------------------------------------
// Timestamp
// ---------------------------------------------------------------------------

/// ISO-8601 UTC timestamp string.
pub fn utc_now() -> String {
    // Simple RFC 3339 / ISO-8601 formatter without chrono dependency.
    use std::time::SystemTime;
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    // Decompose into date components (civil time)
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let h = time_secs / 3600;
    let m = (time_secs % 3600) / 60;
    let s = time_secs % 60;

    // Days since 1970-01-01 → year/month/day (simplified; correct for 1970–2099)
    let (y, mo, d) = civil_from_days(days as i64);

    format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mo, d, h, m, s)
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // Algorithm from Howard Hinnant
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_now_produces_iso8601() {
        let ts = utc_now();
        assert!(ts.ends_with("Z"));
        assert!(ts.contains("T"));
        assert_eq!(ts.len(), 20); // "YYYY-MM-DDTHH:MM:SSZ"
    }

    #[test]
    fn task_id_serde_round_trip() {
        let id = TaskId::new("task-42");
        let json = serde_json::to_string(&id).unwrap();
        let parsed: TaskId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, parsed);
    }
}
