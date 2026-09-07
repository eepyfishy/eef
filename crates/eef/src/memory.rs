use indexmap::IndexMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

use crate::config::Config;

#[derive(Clone)]
pub struct IdentityMemory {
    config: Config,
}

impl IdentityMemory {
    pub fn new(config: Config) -> Self {
        Self { config }
    }
    pub fn name(&self) -> String {
        self.config.string("identity.name", "EEF")
    }
    pub fn version(&self) -> String {
        self.config.string("identity.version", crate::VERSION)
    }
    pub fn behavior_rules(&self) -> Vec<String> {
        self.config.strings("behavior_rules")
    }
    pub fn summary(&self) -> Value {
        json!({
            "name": self.name(),
            "creator": self.config.string("identity.creator", ""),
            "version": self.version(),
            "description": self.config.string("identity.description", ""),
        })
    }
}

pub struct MutableMemory {
    connection: Mutex<Connection>,
    max_conversation: usize,
}

impl MutableMemory {
    pub fn open(path: impl AsRef<Path>, max_conversation: usize) -> Result<Arc<Self>> {
        let connection = Connection::open(path)?;
        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS facts (
                id INTEGER PRIMARY KEY AUTOINCREMENT, key TEXT NOT NULL UNIQUE,
                value_json TEXT NOT NULL, confidence REAL NOT NULL DEFAULT 0.5,
                source TEXT NOT NULL DEFAULT 'user', created_at REAL NOT NULL, updated_at REAL NOT NULL
            );
            CREATE TABLE IF NOT EXISTS notes (
                id INTEGER PRIMARY KEY AUTOINCREMENT, note TEXT NOT NULL, created_at REAL NOT NULL
            );
            CREATE TABLE IF NOT EXISTS conversation (
                id INTEGER PRIMARY KEY AUTOINCREMENT, ts REAL NOT NULL, role TEXT NOT NULL, content TEXT NOT NULL
            );"
        )?;
        // Compatible additive migration: retain existing conversation rows.
        let has_context = connection
            .prepare("PRAGMA table_info(conversation)")?
            .query_map([], |row| row.get::<_, String>(1))?
            .collect::<rusqlite::Result<Vec<_>>>()?
            .iter()
            .any(|name| name == "request_context");
        if !has_context {
            connection.execute(
                "ALTER TABLE conversation ADD COLUMN request_context TEXT",
                [],
            )?;
        }
        Ok(Arc::new(Self {
            connection: Mutex::new(connection),
            max_conversation,
        }))
    }

    pub fn add_fact(&self, key: &str, value: &Value, confidence: f64, source: &str) -> Result<()> {
        let now = now();
        self.connection.lock().expect("memory lock").execute(
            "INSERT INTO facts (key,value_json,confidence,source,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?5)
             ON CONFLICT(key) DO UPDATE SET value_json=excluded.value_json,confidence=excluded.confidence,source=excluded.source,updated_at=excluded.updated_at",
            params![key, serde_json::to_string(value)?, confidence.clamp(0.0, 1.0), source, now],
        )?;
        Ok(())
    }

    pub fn get_fact(&self, key: &str) -> Result<Option<Value>> {
        let connection = self.connection.lock().expect("memory lock");
        connection.query_row(
            "SELECT key,value_json,confidence,source,created_at,updated_at FROM facts WHERE key=?1", [key], fact_row,
        ).optional().map_err(Into::into)
    }

    pub fn list_facts(&self, limit: usize) -> Result<Vec<Value>> {
        let connection = self.connection.lock().expect("memory lock");
        let mut statement = connection.prepare("SELECT key,value_json,confidence,source,created_at,updated_at FROM facts ORDER BY updated_at DESC LIMIT ?1")?;
        Ok(statement
            .query_map([limit as i64], fact_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn add_note(&self, note: &str) -> Result<()> {
        self.connection.lock().expect("memory lock").execute(
            "INSERT INTO notes(note,created_at) VALUES (?1,?2)",
            params![note, now()],
        )?;
        Ok(())
    }

    pub fn list_notes(&self, limit: usize) -> Result<Vec<Value>> {
        let connection = self.connection.lock().expect("memory lock");
        let mut statement = connection
            .prepare("SELECT id,note,created_at FROM notes ORDER BY created_at DESC LIMIT ?1")?;
        Ok(statement.query_map([limit as i64], |row| Ok(json!({"id": row.get::<_, i64>(0)?, "note": row.get::<_, String>(1)?, "created_at": row.get::<_, f64>(2)?})))?
            .collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn add_conversation(&self, role: &str, content: &str) -> Result<()> {
        self.add_conversation_with_context(role, content, None)
    }

    pub fn add_conversation_with_context(
        &self,
        role: &str,
        content: &str,
        context: Option<&eefn::context::RequestContext>,
    ) -> Result<()> {
        let mut connection = self.connection.lock().expect("memory lock");
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO conversation(ts,role,content,request_context) VALUES (?1,?2,?3,?4)",
            params![
                now(),
                role,
                content,
                context.map(serde_json::to_string).transpose()?
            ],
        )?;
        transaction.execute(
            "DELETE FROM conversation WHERE id IN (SELECT id FROM conversation ORDER BY id DESC LIMIT -1 OFFSET ?1)",
            [self.max_conversation as i64],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn recent_conversation(&self, limit: usize) -> Result<Vec<Value>> {
        let connection = self.connection.lock().expect("memory lock");
        let mut statement = connection.prepare(
            "SELECT ts,role,content,request_context FROM conversation ORDER BY id DESC LIMIT ?1",
        )?;
        let mut rows = statement.query_map([limit as i64], |row| Ok(json!({"ts": row.get::<_, f64>(0)?, "role": row.get::<_, String>(1)?, "content": row.get::<_, String>(2)?, "request_context":row.get::<_, Option<String>>(3)?.and_then(|s|serde_json::from_str::<Value>(&s).ok())})))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.reverse();
        Ok(rows)
    }

    pub fn reset(&self) -> Result<()> {
        self.connection
            .lock()
            .expect("memory lock")
            .execute_batch("DELETE FROM facts; DELETE FROM notes; DELETE FROM conversation;")?;
        Ok(())
    }
}

fn fact_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Value> {
    let raw: String = row.get(1)?;
    let value = serde_json::from_str(&raw).unwrap_or(Value::Null);
    Ok(json!({
        "key": row.get::<_, String>(0)?, "value": value, "confidence": row.get::<_, f64>(2)?,
        "source": row.get::<_, String>(3)?, "created_at": row.get::<_, f64>(4)?, "updated_at": row.get::<_, f64>(5)?,
    }))
}

fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

pub struct WorkingMemory {
    values: IndexMap<String, Value>,
    max_items: usize,
}

impl WorkingMemory {
    pub fn new(max_items: usize) -> Self {
        Self {
            values: IndexMap::new(),
            max_items,
        }
    }
    pub fn set(&mut self, key: impl Into<String>, value: Value) {
        let key = key.into();
        if self.values.len() >= self.max_items && !self.values.contains_key(&key) {
            self.values.shift_remove_index(0);
        }
        self.values.insert(key, value);
    }
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.values.get(key)
    }
    pub fn clear(&mut self) {
        self.values.clear()
    }
    pub fn snapshot(&self) -> Value {
        serde_json::to_value(&self.values).unwrap_or_else(|_| json!({}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_history_migrates_and_origin_survives_reopen_with_bounded_retention() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("memory.db");
        let legacy = Connection::open(&path).unwrap();
        legacy.execute_batch("CREATE TABLE conversation (id INTEGER PRIMARY KEY AUTOINCREMENT, ts REAL NOT NULL, role TEXT NOT NULL, content TEXT NOT NULL); INSERT INTO conversation(ts,role,content) VALUES (1,'user','legacy');").unwrap();
        drop(legacy);
        let context = eefn::context::RequestContext::new(
            "request".into(),
            "origin".into(),
            vec!["Home".into()],
        )
        .unwrap();
        let memory = MutableMemory::open(&path, 2).unwrap();
        assert_eq!(
            memory.recent_conversation(10).unwrap()[0]["content"],
            "legacy"
        );
        memory
            .add_conversation_with_context("user", "new", Some(&context))
            .unwrap();
        drop(memory);
        let memory = MutableMemory::open(&path, 2).unwrap();
        let history = memory.recent_conversation(10).unwrap();
        assert!(history[0]["request_context"].is_null());
        assert_eq!(history[1]["request_context"], json!(context));
        memory
            .add_conversation_with_context("assistant", "reply", Some(&context))
            .unwrap();
        let history = memory.recent_conversation(10).unwrap();
        assert_eq!(history.len(), 2);
        assert_eq!(history[0]["content"], "new");
        memory.reset().unwrap();
        assert!(memory.recent_conversation(10).unwrap().is_empty());
    }

    #[test]
    fn mutable_reset_does_not_touch_identity() {
        let dir = tempfile::tempdir().unwrap();
        let memory = MutableMemory::open(dir.path().join("memory.db"), 2).unwrap();
        memory
            .add_fact("key", &json!({"a": 1}), 0.8, "test")
            .unwrap();
        assert_eq!(memory.get_fact("key").unwrap().unwrap()["value"]["a"], 1);
        memory.reset().unwrap();
        assert!(memory.get_fact("key").unwrap().is_none());
    }
}
