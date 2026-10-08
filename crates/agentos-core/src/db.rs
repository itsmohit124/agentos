//! Derived SQLite store and migration runner.
//!
//! The database is derived state: it can always be deleted and rebuilt from the project.
//! Migrations are an ordered, append-only list; each is applied exactly once inside its own
//! transaction and recorded in `schema_migrations`.

use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction};

/// One schema migration.
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    /// Version number; versions start at 1 and increase by exactly 1.
    pub version: u32,
    /// Short stable name, recorded alongside the version.
    pub name: &'static str,
    /// SQL executed to apply the migration.
    pub sql: &'static str,
}

/// All migrations known to this build, in order. Append only; never edit a released entry.
pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "baseline",
    sql: "CREATE TABLE meta (key TEXT PRIMARY KEY NOT NULL, value TEXT NOT NULL) STRICT;",
}];

/// Latest schema version known to this build.
pub fn latest_version() -> u32 {
    latest_of(MIGRATIONS)
}

fn latest_of(migrations: &[Migration]) -> u32 {
    migrations.last().map_or(0, |m| m.version)
}

const CREATE_MIGRATIONS_TABLE: &str = "\
CREATE TABLE IF NOT EXISTS schema_migrations (
    version    INTEGER PRIMARY KEY NOT NULL,
    name       TEXT NOT NULL,
    applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))
) STRICT;";

/// Error returned by database operations.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// The database file does not exist.
    #[error("database {} does not exist", .0.display())]
    NotFound(PathBuf),
    /// The database was written by a newer AgentOS.
    #[error("database schema version {found} is newer than supported version {supported}")]
    TooNew {
        /// Version recorded in the database.
        found: u32,
        /// Latest version this build supports.
        supported: u32,
    },
    /// A recorded migration does not match the migration of the same version in this build.
    #[error("migration {version} is recorded as {recorded:?} but this build expects {expected:?}")]
    Mismatch {
        /// Migration version.
        version: u32,
        /// Name recorded in the database.
        recorded: String,
        /// Name expected by this build.
        expected: &'static str,
    },
    /// Applying a migration failed; the migration was rolled back.
    #[error("migration {version} ({name}) failed: {source}")]
    MigrationFailed {
        /// Migration version.
        version: u32,
        /// Migration name.
        name: &'static str,
        /// Underlying SQLite error.
        #[source]
        source: rusqlite::Error,
    },
    /// Any other SQLite error.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

/// Result of running migrations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationReport {
    /// Versions applied by this run, in order (empty if already up to date).
    pub applied: Vec<u32>,
    /// Schema version after the run.
    pub current_version: u32,
}

/// Read-only status of a database file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DbStatus {
    /// Schema version recorded in the database (0 if never migrated).
    pub current_version: u32,
    /// Latest version this build supports.
    pub latest_version: u32,
}

impl DbStatus {
    /// Number of migrations that [`Database::open`] would apply.
    pub fn pending(&self) -> u32 {
        self.latest_version.saturating_sub(self.current_version)
    }
}

/// An open, fully migrated AgentOS database.
#[derive(Debug)]
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Opens (creating if necessary) the database at `path` and applies pending migrations.
    pub fn open(path: &Path) -> Result<(Self, MigrationReport), DbError> {
        Self::open_with(path, MIGRATIONS)
    }

    /// Opens a private in-memory database with all migrations applied.
    pub fn open_in_memory() -> Result<(Self, MigrationReport), DbError> {
        let conn = Connection::open_in_memory()?;
        Self::init(conn, MIGRATIONS)
    }

    fn open_with(
        path: &Path,
        migrations: &[Migration],
    ) -> Result<(Self, MigrationReport), DbError> {
        let conn = Connection::open(path)?;
        Self::init(conn, migrations)
    }

    fn init(
        mut conn: Connection,
        migrations: &[Migration],
    ) -> Result<(Self, MigrationReport), DbError> {
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.pragma_update(None, "foreign_keys", true)?;
        let report = migrate(&mut conn, migrations)?;
        Ok((Self { conn }, report))
    }

    /// Current schema version.
    pub fn schema_version(&self) -> Result<u32, DbError> {
        Ok(recorded_version(&self.conn)?)
    }

    /// Underlying connection, for subsystem stores built on top of this database.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }
}

/// Inspects the database at `path` without creating or modifying it.
pub fn inspect(path: &Path) -> Result<DbStatus, DbError> {
    if !path.is_file() {
        return Err(DbError::NotFound(path.to_path_buf()));
    }
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    let has_table: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations')",
        [],
        |row| row.get(0),
    )?;
    let current_version = if has_table {
        recorded_version(&conn)?
    } else {
        0
    };
    let status = DbStatus {
        current_version,
        latest_version: latest_version(),
    };
    if status.current_version > status.latest_version {
        return Err(DbError::TooNew {
            found: status.current_version,
            supported: status.latest_version,
        });
    }
    Ok(status)
}

fn recorded_version(conn: &Connection) -> rusqlite::Result<u32> {
    conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )
}

fn migrate(conn: &mut Connection, migrations: &[Migration]) -> Result<MigrationReport, DbError> {
    debug_assert!(
        migrations
            .iter()
            .enumerate()
            .all(|(i, m)| m.version as usize == i + 1),
        "migration versions must be 1..=n in order"
    );
    conn.execute_batch(CREATE_MIGRATIONS_TABLE)?;

    let current = recorded_version(conn)?;
    let supported = latest_of(migrations);
    if current > supported {
        return Err(DbError::TooNew {
            found: current,
            supported,
        });
    }
    for m in migrations.iter().filter(|m| m.version <= current) {
        let recorded: Option<String> = conn
            .query_row(
                "SELECT name FROM schema_migrations WHERE version = ?1",
                [m.version],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(recorded) = recorded.filter(|r| r != m.name) {
            return Err(DbError::Mismatch {
                version: m.version,
                recorded,
                expected: m.name,
            });
        }
    }

    let mut applied = Vec::new();
    for m in migrations.iter().filter(|m| m.version > current) {
        let tx = conn.transaction()?;
        apply(&tx, m).map_err(|source| DbError::MigrationFailed {
            version: m.version,
            name: m.name,
            source,
        })?;
        tx.commit()?;
        tracing::debug!(version = m.version, name = m.name, "applied migration");
        applied.push(m.version);
    }
    Ok(MigrationReport {
        applied,
        current_version: recorded_version(conn)?,
    })
}

fn apply(tx: &Transaction<'_>, m: &Migration) -> rusqlite::Result<()> {
    tx.execute_batch(m.sql)?;
    tx.execute(
        "INSERT INTO schema_migrations (version, name) VALUES (?1, ?2)",
        (m.version, m.name),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO: &[Migration] = &[
        Migration {
            version: 1,
            name: "one",
            sql: "CREATE TABLE a (x INTEGER);",
        },
        Migration {
            version: 2,
            name: "two",
            sql: "CREATE TABLE b (y INTEGER);",
        },
    ];

    fn table_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [name],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn production_migrations_are_well_formed() {
        for (i, m) in MIGRATIONS.iter().enumerate() {
            assert_eq!(m.version as usize, i + 1);
            assert!(!m.name.is_empty());
        }
        assert_eq!(latest_version(), MIGRATIONS.len() as u32);
    }

    #[test]
    fn fresh_db_applies_all_migrations() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("t.db");
        let (db, report) = Database::open(&path).unwrap();
        assert_eq!(report.applied, vec![1]);
        assert_eq!(report.current_version, latest_version());
        assert_eq!(db.schema_version().unwrap(), latest_version());
        assert!(table_exists(db.connection(), "meta"));
        assert!(table_exists(db.connection(), "schema_migrations"));
    }

    #[test]
    fn reopen_applies_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("t.db");
        drop(Database::open(&path).unwrap());
        let (db, report) = Database::open(&path).unwrap();
        assert!(report.applied.is_empty());
        assert_eq!(db.schema_version().unwrap(), latest_version());
    }

    #[test]
    fn upgrade_applies_only_new_migrations() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("t.db");
        let (_, report) = Database::open_with(&path, &TWO[..1]).unwrap();
        assert_eq!(report.applied, vec![1]);
        let (db, report) = Database::open_with(&path, TWO).unwrap();
        assert_eq!(report.applied, vec![2]);
        assert!(table_exists(db.connection(), "b"));
    }

    #[test]
    fn newer_db_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("t.db");
        drop(Database::open_with(&path, TWO).unwrap());
        let err = Database::open_with(&path, &TWO[..1]).unwrap_err();
        assert!(matches!(
            err,
            DbError::TooNew {
                found: 2,
                supported: 1
            }
        ));
    }

    #[test]
    fn renamed_migration_is_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("t.db");
        drop(Database::open_with(&path, &TWO[..1]).unwrap());
        let renamed = [Migration {
            name: "renamed",
            ..TWO[0]
        }];
        let err = Database::open_with(&path, &renamed).unwrap_err();
        assert!(matches!(err, DbError::Mismatch { version: 1, .. }));
    }

    #[test]
    fn failing_migration_rolls_back() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("t.db");
        let broken = [
            TWO[0],
            Migration {
                version: 2,
                name: "broken",
                sql: "CREATE TABLE c (z INTEGER); THIS IS NOT SQL;",
            },
        ];
        let err = Database::open_with(&path, &broken).unwrap_err();
        assert!(matches!(err, DbError::MigrationFailed { version: 2, .. }));

        let status = inspect(&path).unwrap();
        assert_eq!(status.current_version, 1);
        let conn = Connection::open(&path).unwrap();
        assert!(!table_exists(&conn, "c"));
    }

    #[test]
    fn inspect_is_read_only_and_reports_status() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("t.db");
        assert!(matches!(inspect(&path), Err(DbError::NotFound(_))));
        assert!(!path.exists());

        drop(Database::open(&path).unwrap());
        let status = inspect(&path).unwrap();
        assert_eq!(status.current_version, latest_version());
        assert_eq!(status.pending(), 0);
    }

    #[test]
    fn inspect_unmigrated_db_reports_pending() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("t.db");
        Connection::open(&path)
            .unwrap()
            .execute_batch("CREATE TABLE unrelated (x INTEGER);")
            .unwrap();
        let status = inspect(&path).unwrap();
        assert_eq!(status.current_version, 0);
        assert_eq!(status.pending(), latest_version());
    }

    #[test]
    fn inspect_rejects_non_database_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("t.db");
        std::fs::write(&path, b"this is not sqlite").unwrap();
        assert!(matches!(inspect(&path), Err(DbError::Sqlite(_))));
    }

    #[test]
    fn in_memory_db_is_migrated() {
        let (db, report) = Database::open_in_memory().unwrap();
        assert_eq!(report.current_version, latest_version());
        assert_eq!(db.schema_version().unwrap(), latest_version());
    }
}
