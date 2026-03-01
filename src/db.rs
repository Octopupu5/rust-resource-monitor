use crate::metrics::{MetricsSnapshot, RpcMetricsSnapshot};
use rusqlite::{params, Connection, DatabaseName};
use std::path::Path;
use std::sync::Mutex;
use tracing::{error, info};

pub struct MetricsDb {
    conn: Mutex<Connection>,
}

impl MetricsDb {
    pub fn new(path: &Path) -> Result<Self, rusqlite::Error> {
        let conn = Connection::open(path)?;

        conn.pragma_update(Some(DatabaseName::Main), "journal_mode", "WAL")?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS metrics (
                timestamp_ms INTEGER PRIMARY KEY,
                data TEXT NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_timestamp ON metrics(timestamp_ms)",
            [],
        )?;

        info!("Database initialized at {}", path.display());

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn insert(&self, snapshot: &MetricsSnapshot) -> Result<(), rusqlite::Error> {
        let data = match serde_json::to_string(&snapshot.to_rpc_format()) {
            Ok(json) => json,
            Err(e) => {
                error!("Failed to serialize snapshot: {}", e);
                return Err(rusqlite::Error::ToSqlConversionFailure(Box::new(e)));
            }
        };

        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO metrics (timestamp_ms, data) VALUES (?1, ?2)",
            params![snapshot.timestamp_ms as i64, data],
        )?;

        Ok(())
    }

    pub fn get_latest(&self) -> Result<Option<RpcMetricsSnapshot>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        let mut stmt =
            conn.prepare("SELECT data FROM metrics ORDER BY timestamp_ms DESC LIMIT 1")?;

        let mut rows = stmt.query([])?;

        if let Some(row) = rows.next()? {
            let data: String = row.get(0)?;
            match serde_json::from_str(&data) {
                Ok(snapshot) => return Ok(Some(snapshot)),
                Err(e) => {
                    error!("Failed to deserialize snapshot: {}", e);
                    return Ok(None);
                }
            }
        }

        Ok(None)
    }

    pub fn get_range(
        &self,
        from_ts: u64,
        to_ts: u64,
        limit: Option<usize>,
    ) -> Result<Vec<RpcMetricsSnapshot>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        let query = if let Some(limit) = limit {
            format!(
                "SELECT data FROM metrics WHERE timestamp_ms BETWEEN ?1 AND ?2 ORDER BY timestamp_ms DESC LIMIT {}",
                limit
            )
        } else {
            "SELECT data FROM metrics WHERE timestamp_ms BETWEEN ?1 AND ?2 ORDER BY timestamp_ms DESC".to_string()
        };

        let mut stmt = conn.prepare(&query)?;
        let mut rows = stmt.query(params![from_ts as i64, to_ts as i64])?;

        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            let data: String = row.get(0)?;
            if let Ok(snapshot) = serde_json::from_str(&data) {
                results.push(snapshot);
            }
        }

        Ok(results)
    }

    pub fn get_history(
        &self,
        limit: Option<usize>,
        since_ts: Option<u64>,
    ) -> Result<Vec<RpcMetricsSnapshot>, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        let query = match (since_ts, limit) {
            (Some(since), Some(lim)) => {
                format!(
                    "SELECT data FROM metrics WHERE timestamp_ms >= {} ORDER BY timestamp_ms DESC LIMIT {}",
                    since, lim
                )
            }
            (Some(since), None) => {
                format!(
                    "SELECT data FROM metrics WHERE timestamp_ms >= {} ORDER BY timestamp_ms DESC",
                    since
                )
            }
            (None, Some(lim)) => {
                format!(
                    "SELECT data FROM metrics ORDER BY timestamp_ms DESC LIMIT {}",
                    lim
                )
            }
            (None, None) => "SELECT data FROM metrics ORDER BY timestamp_ms DESC".to_string(),
        };

        let mut stmt = conn.prepare(&query)?;
        let mut rows = stmt.query([])?;

        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            let data: String = row.get(0)?;
            if let Ok(snapshot) = serde_json::from_str(&data) {
                results.push(snapshot);
            }
        }

        Ok(results)
    }

    pub fn cleanup_old(&self, keep_hours: u64) -> Result<usize, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        let cutoff =
            (chrono::Utc::now().timestamp_millis() as u64 - keep_hours * 3600 * 1000) as i64;

        let deleted = conn.execute(
            "DELETE FROM metrics WHERE timestamp_ms < ?1",
            params![cutoff],
        )?;

        if deleted > 0 {
            info!("Cleaned up {} old records from database", deleted);
        }

        Ok(deleted)
    }

    pub fn vacuum(&self) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute("VACUUM", [])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_db_operations() {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let db = MetricsDb::new(&db_path).unwrap();

        let snapshot = crate::metrics::MetricsSnapshot {
            timestamp_ms: 1000,
            cpu: crate::metrics::CpuMetrics {
                total_usage_pct: 10.0,
                per_core_usage_pct: vec![10.0, 20.0],
                load_avg_1: 0.1,
                load_avg_5: 0.2,
                load_avg_15: 0.3,
                temperature_celsius: Some(50.0),
            },
            memory: crate::metrics::MemoryMetrics {
                total_bytes: 100,
                used_bytes: 50,
                available_bytes: 50,
                swap_total_bytes: 4096,
                swap_used_bytes: 1024,
            },
            network: crate::metrics::NetworkMetrics {
                rx_bytes_total: 1000,
                tx_bytes_total: 2000,
                rx_bytes_per_sec: 10.0,
                tx_bytes_per_sec: 20.0,
            },
            disk: crate::metrics::DiskMetrics {
                total_bytes: 500_000_000_000,
                available_bytes: 200_000_000_000,
                used_pct: 60.0,
            },
            battery: None,
            gpu: None,
        };

        db.insert(&snapshot).unwrap();

        let latest = db.get_latest().unwrap().unwrap();
        assert_eq!(latest.timestamp_ms, 1000);

        let range = db.get_range(500, 1500, None).unwrap();
        assert_eq!(range.len(), 1);

        // Получаем history
        let history = db.get_history(Some(10), None).unwrap();
        assert_eq!(history.len(), 1);
    }
}
