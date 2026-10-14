//! SQLite mirror of `.agentos/memory/*.json`.
//!
//! Opening the store reloads the table from those files. Deleting `agentos.db` does not
//! delete the files. The next open rebuilds the rows. Knowledge `scan` does not touch them.

use std::fs;
use std::path::{Path, PathBuf};

use agentos_context::MemoryCandidate;
use agentos_core::db::Database;
use agentos_core::state::{DB_FILE, STATE_DIR};
use agentos_knowledge::{Provenance, ProvenanceKind};
use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};

use crate::error::MemoryError;
use crate::model::{
    FingerprintIndex, MemoryLink, MemoryRecord, NewMemory, normalize_status, parse_type,
};
use crate::redact::redact;

const MIGRATION: &str = "\
CREATE TABLE memory_records (
    id TEXT PRIMARY KEY NOT NULL,
    memory_type TEXT NOT NULL,
    name TEXT NOT NULL,
    content TEXT NOT NULL,
    payload TEXT NOT NULL,
    status TEXT NOT NULL,
    freshness TEXT NOT NULL,
    provenance_kind TEXT NOT NULL,
    provenance_source TEXT NOT NULL,
    provenance_location TEXT NOT NULL,
    extractor TEXT NOT NULL,
    extractor_version TEXT NOT NULL,
    method TEXT NOT NULL,
    confidence_millis INTEGER NOT NULL,
    fingerprint TEXT NOT NULL,
    task TEXT NOT NULL,
    run_id TEXT NOT NULL,
    outcome TEXT NOT NULL,
    links TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    pattern INTEGER NOT NULL,
    intent_kind TEXT NOT NULL
) STRICT;
";

/// Project memory for one root.
pub struct MemoryStore {
    root: PathBuf,
    db: Database,
}

impl MemoryStore {
    /// Opens `<root>/.agentos/agentos.db` and reloads memory rows from `.agentos/memory/`.
    pub fn open(root: &Path) -> Result<Self, MemoryError> {
        let dir = root.join(STATE_DIR);
        fs::create_dir_all(&dir)?;
        fs::create_dir_all(dir.join("memory"))?;
        let (db, _) = Database::open(&dir.join(DB_FILE))?;
        migrate(db.connection())?;
        let store = Self {
            root: root.to_path_buf(),
            db,
        };
        store.reindex()?;
        Ok(store)
    }

    /// Directory of memory files.
    pub fn memory_dir(&self) -> PathBuf {
        self.root.join(STATE_DIR).join("memory")
    }

    /// Inserts a record. A forgotten id with the same identity is not restored.
    pub fn add(&self, draft: NewMemory) -> Result<MemoryRecord, MemoryError> {
        self.require_clock(&draft.recorded_at)?;
        let memory_type = parse_type(&draft.memory_type)?.to_owned();
        if draft.pattern && memory_type != "procedural" {
            return Err(MemoryError::Type(
                "a context pattern must use type procedural".to_owned(),
            ));
        }
        for link in &draft.links {
            link.validate()?;
        }
        let name = redact(draft.name.trim());
        let content = redact(&draft.content);
        let task = redact(&draft.task);
        let outcome = redact(&draft.outcome);
        if name.is_empty() || content.trim().is_empty() {
            return Err(MemoryError::Empty);
        }
        let mut payload = BTreeMap::new();
        for (key, value) in draft.payload {
            payload.insert(key, redact(&value));
        }
        let mut links = draft.links;
        sort_links(&mut links);
        let id = memory_id(&memory_type, &content, &task, &draft.run_id);
        if let Some(existing) = self.get(&id)?
            && existing.status == "forgotten"
        {
            return Ok(existing);
        }
        let status = if draft.confirm {
            "confirmed"
        } else {
            "proposed"
        };
        let mut created_at = draft.recorded_at.clone();
        let mut status = status.to_owned();
        if let Some(existing) = self.get(&id)? {
            created_at = existing.created_at;
            if existing.status == "confirmed" && status == "proposed" {
                status = "confirmed".to_owned();
            }
        }
        let source = if draft.run_id.is_empty() {
            "memory-add".to_owned()
        } else {
            format!("run:{}", draft.run_id)
        };
        let record = MemoryRecord {
            id,
            memory_type,
            name,
            content: content.clone(),
            payload,
            status,
            freshness: "fresh".to_owned(),
            provenance: Provenance {
                kind: draft.provenance_kind,
                source,
                location: String::new(),
                extractor: "agentos-memory".to_owned(),
                extractor_version: "1".to_owned(),
                method: draft.method,
            },
            confidence_millis: draft.confidence_millis,
            fingerprint: content_fingerprint(&content),
            task,
            run_id: draft.run_id,
            outcome,
            links,
            created_at,
            updated_at: draft.recorded_at,
            pattern: draft.pattern,
            intent_kind: draft.intent_kind,
        };
        self.persist(&record)?;
        Ok(record)
    }

    /// Moves `proposed` to `confirmed`. Does not write `canonical`.
    pub fn confirm(&self, id: &str, by: &str, at: &str) -> Result<MemoryRecord, MemoryError> {
        self.require_clock(at)?;
        let mut record = self.require(id)?;
        if record.status == "forgotten" {
            return Err(MemoryError::Forgotten(id.to_owned()));
        }
        if record.status != "confirmed" {
            record.status = "confirmed".to_owned();
            record.updated_at = at.to_owned();
            record.payload.insert("confirmed_by".to_owned(), redact(by));
            record
                .payload
                .insert("confirmed_at".to_owned(), at.to_owned());
            self.persist(&record)?;
        }
        debug_assert!(!record.is_canonical());
        Ok(record)
    }

    /// Marks a record forgotten. Retrieval will not return it.
    pub fn forget(&self, id: &str, at: &str) -> Result<MemoryRecord, MemoryError> {
        self.require_clock(at)?;
        let mut record = self.require(id)?;
        if record.status != "forgotten" {
            record.status = "forgotten".to_owned();
            record.updated_at = at.to_owned();
            self.persist(&record)?;
        }
        Ok(record)
    }

    /// Compares links with `index` and stores `stale` when a fingerprint differs or is missing.
    pub fn refresh(&self, index: &FingerprintIndex, at: &str) -> Result<(), MemoryError> {
        self.require_clock(at)?;
        for record in self.rows()? {
            let freshness = freshness_of(&record, index);
            if freshness == record.freshness {
                continue;
            }
            let mut record = record;
            record.freshness = freshness;
            record.updated_at = at.to_owned();
            self.persist(&record)?;
        }
        Ok(())
    }

    /// Deletes records that are stale after a refresh. Proposed and confirmed fresh records stay.
    pub fn prune_stale(
        &self,
        index: &FingerprintIndex,
        at: &str,
    ) -> Result<Vec<String>, MemoryError> {
        self.refresh(index, at)?;
        let mut removed = Vec::new();
        for record in self.rows()? {
            if record.freshness != "stale" {
                continue;
            }
            self.delete_record(&record.id)?;
            removed.push(record.id);
        }
        removed.sort();
        Ok(removed)
    }

    /// Every record, including forgotten ones, sorted by id.
    pub fn list(
        &self,
        index: &FingerprintIndex,
        at: &str,
    ) -> Result<Vec<MemoryRecord>, MemoryError> {
        self.refresh(index, at)?;
        self.rows()
    }

    /// One record, after staleness is refreshed.
    pub fn show(
        &self,
        id: &str,
        index: &FingerprintIndex,
        at: &str,
    ) -> Result<MemoryRecord, MemoryError> {
        self.refresh(index, at)?;
        self.require(id)
    }

    /// Non-forgotten records as context candidates.
    pub fn candidates(
        &self,
        index: &FingerprintIndex,
        at: &str,
    ) -> Result<Vec<MemoryCandidate>, MemoryError> {
        self.refresh(index, at)?;
        let mut out = Vec::new();
        for record in self.rows()? {
            if record.is_forgotten() {
                continue;
            }
            out.push(to_candidate(&record));
        }
        Ok(out)
    }

    /// Reads one row. Does not refresh staleness.
    pub fn get(&self, id: &str) -> Result<Option<MemoryRecord>, MemoryError> {
        let conn = self.db.connection();
        let mut stmt = conn.prepare(
            "SELECT id, memory_type, name, content, payload, status, freshness,
                    provenance_kind, provenance_source, provenance_location, extractor,
                    extractor_version, method, confidence_millis, fingerprint, task, run_id,
                    outcome, links, created_at, updated_at, pattern, intent_kind
             FROM memory_records WHERE id = ?1",
        )?;
        let row = stmt.query_row(params![id], map_row).optional()?;
        Ok(row.transpose()?.map(|mut record| {
            let _ = normalize_record(&mut record);
            record
        }))
    }

    fn require(&self, id: &str) -> Result<MemoryRecord, MemoryError> {
        self.get(id)?
            .ok_or_else(|| MemoryError::NotFound(id.to_owned()))
    }

    fn require_clock(&self, at: &str) -> Result<(), MemoryError> {
        if at.is_empty() {
            Err(MemoryError::Clock)
        } else {
            Ok(())
        }
    }

    fn rows(&self) -> Result<Vec<MemoryRecord>, MemoryError> {
        let conn = self.db.connection();
        let mut stmt = conn.prepare(
            "SELECT id, memory_type, name, content, payload, status, freshness,
                    provenance_kind, provenance_source, provenance_location, extractor,
                    extractor_version, method, confidence_millis, fingerprint, task, run_id,
                    outcome, links, created_at, updated_at, pattern, intent_kind
             FROM memory_records ORDER BY id",
        )?;
        let mapped = stmt.query_map([], map_row)?;
        let mut records = Vec::new();
        for row in mapped {
            let mut record = row??;
            let _ = normalize_record(&mut record);
            records.push(record);
        }
        Ok(records)
    }

    fn persist(&self, record: &MemoryRecord) -> Result<(), MemoryError> {
        debug_assert!(!record.is_canonical());
        debug_assert_ne!(record.status, "canonical");
        write_file(&self.memory_dir(), record)?;
        upsert(self.db.connection(), record)?;
        Ok(())
    }

    fn delete_record(&self, id: &str) -> Result<(), MemoryError> {
        let path = self.memory_dir().join(format!("{id}.json"));
        if path.is_file() {
            fs::remove_file(path)?;
        }
        self.db
            .connection()
            .execute("DELETE FROM memory_records WHERE id = ?1", params![id])?;
        Ok(())
    }

    fn reindex(&self) -> Result<(), MemoryError> {
        let mut records = read_files(&self.memory_dir())?;
        for record in &mut records {
            if normalize_record(record) {
                write_file(&self.memory_dir(), record)?;
            }
        }
        let conn = self.db.connection();
        let tx = conn.unchecked_transaction()?;
        tx.execute("DELETE FROM memory_records", [])?;
        for record in &records {
            upsert(&tx, record)?;
        }
        tx.commit()?;
        Ok(())
    }
}

fn content_fingerprint(content: &str) -> String {
    hex_bytes(&Sha256::digest(content.as_bytes()))
}

/// SHA-256 prefix. The same type, content, task, and run id always yield the same id.
pub fn memory_id(memory_type: &str, content: &str, task: &str, run_id: &str) -> String {
    let digest = Sha256::digest(format!("{memory_type}\n{content}\n{task}\n{run_id}").as_bytes());
    let hex = hex_bytes(&digest);
    format!("memory.{memory_type}.{}", &hex[..16])
}

fn hex_bytes(digest: &[u8]) -> String {
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

fn freshness_of(record: &MemoryRecord, index: &FingerprintIndex) -> String {
    if record.links.is_empty() {
        return record.freshness.clone();
    }
    let stale = record.links.iter().any(|link| {
        index
            .values
            .get(&link.id)
            .is_none_or(|fingerprint| fingerprint != &link.fingerprint)
    });
    if stale { "stale" } else { "fresh" }.to_owned()
}

fn to_candidate(record: &MemoryRecord) -> MemoryCandidate {
    MemoryCandidate {
        id: record.id.clone(),
        memory_type: record.memory_type.clone(),
        name: record.name.clone(),
        status: record.status.clone(),
        content: record.content.clone(),
        source: record.provenance.source.clone(),
        task: record.task.clone(),
        outcome: record.outcome.clone(),
        links: record.links.iter().map(|link| link.id.clone()).collect(),
        stale: record.freshness == "stale",
        pattern: record.pattern,
        intent_kind: record.intent_kind.clone(),
    }
}

fn normalize_record(record: &mut MemoryRecord) -> bool {
    let (status, freshness, changed) = normalize_status(&record.status, &record.freshness);
    record.status = status;
    record.freshness = freshness;
    changed
}

fn sort_links(links: &mut [MemoryLink]) {
    links.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then(left.id.cmp(&right.id))
            .then(left.fingerprint.cmp(&right.fingerprint))
    });
}

fn write_file(dir: &Path, record: &MemoryRecord) -> Result<(), MemoryError> {
    fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.json", record.id));
    let temporary = dir.join(format!("{}.json.tmp", record.id));
    let mut body = serde_json::to_string_pretty(record)?;
    body.push('\n');
    fs::write(&temporary, body)?;
    fs::rename(&temporary, &path)?;
    Ok(())
}

fn read_files(dir: &Path) -> Result<Vec<MemoryRecord>, MemoryError> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut names: Vec<PathBuf> = fs::read_dir(dir)?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .collect();
    names.sort();
    let mut records = Vec::new();
    for path in names {
        let text = fs::read_to_string(&path)?;
        let record: MemoryRecord =
            serde_json::from_str(&text).map_err(|err| MemoryError::File {
                path: path.clone(),
                message: err.to_string(),
            })?;
        records.push(record);
    }
    records.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(records)
}

fn migrate(conn: &Connection) -> Result<(), MemoryError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS memory_migrations (
            version INTEGER PRIMARY KEY NOT NULL,
            name TEXT NOT NULL
        ) STRICT;",
    )?;
    let version: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM memory_migrations",
        [],
        |row| row.get(0),
    )?;
    if version < 1 {
        conn.execute_batch(MIGRATION)?;
        conn.execute(
            "INSERT INTO memory_migrations (version, name) VALUES (1, 'memory')",
            [],
        )?;
    }
    Ok(())
}

fn upsert(conn: &Connection, record: &MemoryRecord) -> Result<(), MemoryError> {
    let payload = serde_json::to_string(&record.payload)?;
    let links = serde_json::to_string(&record.links)?;
    conn.execute(
        "INSERT INTO memory_records (
            id, memory_type, name, content, payload, status, freshness,
            provenance_kind, provenance_source, provenance_location, extractor,
            extractor_version, method, confidence_millis, fingerprint, task, run_id,
            outcome, links, created_at, updated_at, pattern, intent_kind
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16,
            ?17, ?18, ?19, ?20, ?21, ?22, ?23
         )
         ON CONFLICT(id) DO UPDATE SET
            memory_type = excluded.memory_type,
            name = excluded.name,
            content = excluded.content,
            payload = excluded.payload,
            status = excluded.status,
            freshness = excluded.freshness,
            provenance_kind = excluded.provenance_kind,
            provenance_source = excluded.provenance_source,
            provenance_location = excluded.provenance_location,
            extractor = excluded.extractor,
            extractor_version = excluded.extractor_version,
            method = excluded.method,
            confidence_millis = excluded.confidence_millis,
            fingerprint = excluded.fingerprint,
            task = excluded.task,
            run_id = excluded.run_id,
            outcome = excluded.outcome,
            links = excluded.links,
            created_at = excluded.created_at,
            updated_at = excluded.updated_at,
            pattern = excluded.pattern,
            intent_kind = excluded.intent_kind",
        params![
            record.id,
            record.memory_type,
            record.name,
            record.content,
            payload,
            record.status,
            record.freshness,
            record.provenance.kind.as_str(),
            record.provenance.source,
            record.provenance.location,
            record.provenance.extractor,
            record.provenance.extractor_version,
            record.provenance.method,
            record.confidence_millis,
            record.fingerprint,
            record.task,
            record.run_id,
            record.outcome,
            links,
            record.created_at,
            record.updated_at,
            i64::from(record.pattern),
            record.intent_kind,
        ],
    )?;
    Ok(())
}

fn map_row(row: &rusqlite::Row<'_>) -> Result<Result<MemoryRecord, MemoryError>, rusqlite::Error> {
    let payload: String = row.get(4)?;
    let links: String = row.get(18)?;
    let kind: String = row.get(7)?;
    Ok(record_from_row(row, &payload, &links, &kind))
}

fn record_from_row(
    row: &rusqlite::Row<'_>,
    payload: &str,
    links: &str,
    kind: &str,
) -> Result<MemoryRecord, MemoryError> {
    let provenance_kind = ProvenanceKind::parse(kind).unwrap_or(ProvenanceKind::Generated);
    Ok(MemoryRecord {
        id: row.get(0)?,
        memory_type: row.get(1)?,
        name: row.get(2)?,
        content: row.get(3)?,
        payload: serde_json::from_str(payload)?,
        status: row.get(5)?,
        freshness: row.get(6)?,
        provenance: Provenance {
            kind: provenance_kind,
            source: row.get(8)?,
            location: row.get(9)?,
            extractor: row.get(10)?,
            extractor_version: row.get(11)?,
            method: row.get(12)?,
        },
        confidence_millis: row.get(13)?,
        fingerprint: row.get(14)?,
        task: row.get(15)?,
        run_id: row.get(16)?,
        outcome: row.get(17)?,
        links: serde_json::from_str(links)?,
        created_at: row.get(19)?,
        updated_at: row.get(20)?,
        pattern: row.get::<_, i64>(21)? != 0,
        intent_kind: row.get(22)?,
    })
}

use std::collections::BTreeMap;
