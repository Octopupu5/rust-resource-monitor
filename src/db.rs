use crate::metrics::{MetricsSnapshot, RpcMetricsSnapshot};
use rusqlite::{params, Connection, DatabaseName};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tracing::{error, info, warn};

#[derive(Serialize)]
pub struct DbStats {
    pub total_records: usize,
    pub oldest_timestamp: Option<u64>,
    pub newest_timestamp: Option<u64>,
    pub database_size_bytes: u64,
}

pub struct MetricsDb {
    conn: Mutex<Connection>,
    path: PathBuf,
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
            path: path.to_path_buf(),
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

        let query = if limit.is_some() {
            "SELECT data FROM metrics WHERE timestamp_ms BETWEEN ?1 AND ?2 ORDER BY timestamp_ms DESC LIMIT ?3"
        } else {
            "SELECT data FROM metrics WHERE timestamp_ms BETWEEN ?1 AND ?2 ORDER BY timestamp_ms DESC"
        };

        let mut stmt = conn.prepare(query)?;
        let mut rows = if let Some(lim) = limit {
            stmt.query(params![from_ts as i64, to_ts as i64, lim as i64])?
        } else {
            stmt.query(params![from_ts as i64, to_ts as i64])?
        };

        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            let data: String = row.get(0)?;
            match serde_json::from_str(&data) {
                Ok(snapshot) => results.push(snapshot),
                Err(e) => warn!("Skipping corrupted row: {}", e),
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

        let (query, needs_since, needs_limit) = match (since_ts.is_some(), limit.is_some()) {
            (true, true) => (
                "SELECT data FROM metrics WHERE timestamp_ms >= ?1 ORDER BY timestamp_ms DESC LIMIT ?2",
                true, true,
            ),
            (true, false) => (
                "SELECT data FROM metrics WHERE timestamp_ms >= ?1 ORDER BY timestamp_ms DESC",
                true, false,
            ),
            (false, true) => (
                "SELECT data FROM metrics ORDER BY timestamp_ms DESC LIMIT ?1",
                false, true,
            ),
            (false, false) => (
                "SELECT data FROM metrics ORDER BY timestamp_ms DESC",
                false, false,
            ),
        };

        let mut stmt = conn.prepare(query)?;
        let mut rows = match (needs_since, needs_limit) {
            (true, true) => stmt.query(params![since_ts.unwrap() as i64, limit.unwrap() as i64])?,
            (true, false) => stmt.query(params![since_ts.unwrap() as i64])?,
            (false, true) => stmt.query(params![limit.unwrap() as i64])?,
            (false, false) => stmt.query([])?,
        };

        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            let data: String = row.get(0)?;
            match serde_json::from_str(&data) {
                Ok(snapshot) => results.push(snapshot),
                Err(e) => warn!("Skipping corrupted row: {}", e),
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

    pub fn get_stats(&self) -> Result<DbStats, rusqlite::Error> {
        let conn = self.conn.lock().unwrap();

        let total_records: usize = conn.query_row(
            "SELECT COUNT(*) FROM metrics",
            [],
            |row| row.get(0),
        )?;

        let oldest_timestamp: Option<u64> = conn
            .query_row("SELECT MIN(timestamp_ms) FROM metrics", [], |row| {
                row.get::<_, Option<i64>>(0)
            })?
            .map(|v| v as u64);

        let newest_timestamp: Option<u64> = conn
            .query_row("SELECT MAX(timestamp_ms) FROM metrics", [], |row| {
                row.get::<_, Option<i64>>(0)
            })?
            .map(|v| v as u64);

        let database_size_bytes = std::fs::metadata(&self.path)
            .map(|m| m.len())
            .unwrap_or(0);

        Ok(DbStats {
            total_records,
            oldest_timestamp,
            newest_timestamp,
            database_size_bytes,
        })
    }

    pub fn vacuum(&self) -> Result<(), rusqlite::Error> {
        let conn = self.conn.lock().unwrap();
        conn.execute("VACUUM", [])?;
        Ok(())
    }
}