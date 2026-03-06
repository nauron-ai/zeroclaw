use super::super::postgres::{quote_identifier, validate_identifier, PostgresClientHolder};
use anyhow::{Context, Result};
use chrono::Utc;
use postgres::Client;
use std::sync::Arc;

const LAST_ERROR_MAX_CHARS: usize = 2048;

#[derive(Debug, Clone, Copy)]
pub enum SyncOp {
    Upsert,
    Delete,
}

impl SyncOp {
    fn as_str(self) -> &'static str {
        match self {
            Self::Upsert => "upsert",
            Self::Delete => "delete",
        }
    }
}

#[derive(Clone)]
pub struct SyncStateStore {
    client: Arc<PostgresClientHolder>,
    qualified_table: String,
}

impl SyncStateStore {
    pub fn new(client: Arc<PostgresClientHolder>, schema: &str) -> Result<Self> {
        validate_identifier(schema, "storage schema")?;
        let qualified_table = format!("{}.\"memories_qdrant_sync\"", quote_identifier(schema));
        let store = Self {
            client,
            qualified_table,
        };
        store.init_schema()?;
        Ok(store)
    }

    pub async fn set_pending(
        &self,
        key: &str,
        op: SyncOp,
        content_hash: Option<&str>,
    ) -> Result<()> {
        let key = key.to_string();
        let op = op.as_str().to_string();
        let content_hash = content_hash.map(str::to_string);
        let table = self.qualified_table.clone();
        self.run_db_task(move |client| {
            let now = Utc::now();
            let stmt = format!(
                "\
                INSERT INTO {table} (key, op, status, attempt_count, last_error, updated_at, last_attempt_at, last_synced_at, content_hash)
                VALUES ($1, $2, 'pending', 0, NULL, $3, NULL, NULL, $4)
                ON CONFLICT (key) DO UPDATE SET
                    op = EXCLUDED.op,
                    status = 'pending',
                    updated_at = EXCLUDED.updated_at,
                    content_hash = EXCLUDED.content_hash,
                    attempt_count = 0,
                    last_error = NULL,
                    last_attempt_at = NULL
                "
            );
            client.execute(&stmt, &[&key, &op, &now, &content_hash])?;
            Ok(())
        })
        .await
    }

    pub async fn mark_synced(
        &self,
        key: &str,
        expected_op: SyncOp,
        expected_content_hash: Option<&str>,
    ) -> Result<()> {
        let key = key.to_string();
        let expected_op = expected_op.as_str().to_string();
        let expected_content_hash = expected_content_hash.map(str::to_string);
        let table = self.qualified_table.clone();
        self.run_db_task(move |client| {
            let now = Utc::now();
            let affected = if let Some(content_hash) = expected_content_hash.as_deref() {
                let stmt = format!(
                    "UPDATE {table}
                     SET status='synced', last_error=NULL, last_synced_at=$2, updated_at=$2
                     WHERE key=$1
                       AND status='pending'
                       AND op=$3
                       AND content_hash=$4"
                );
                client.execute(&stmt, &[&key, &now, &expected_op, &content_hash])?
            } else {
                let stmt = format!(
                    "UPDATE {table}
                     SET status='synced', last_error=NULL, last_synced_at=$2, updated_at=$2
                     WHERE key=$1
                       AND status='pending'
                       AND op=$3
                       AND content_hash IS NULL"
                );
                client.execute(&stmt, &[&key, &now, &expected_op])?
            };
            if affected == 0 {
                anyhow::bail!("sync state changed concurrently for key '{key}' in {table}");
            }
            Ok(())
        })
        .await
    }

    pub async fn mark_failed(
        &self,
        key: &str,
        error: &str,
        expected_op: SyncOp,
        expected_content_hash: Option<&str>,
    ) -> Result<()> {
        let key = key.to_string();
        let error = sanitize_error_for_storage(error);
        let expected_op = expected_op.as_str().to_string();
        let expected_content_hash = expected_content_hash.map(str::to_string);
        let table = self.qualified_table.clone();
        self.run_db_task(move |client| {
            let now = Utc::now();
            let affected = if let Some(content_hash) = expected_content_hash.as_deref() {
                let stmt = format!(
                    "UPDATE {table}
                     SET status='failed', last_error=$2, attempt_count=attempt_count+1, last_attempt_at=$3, updated_at=$3
                     WHERE key=$1
                       AND status='pending'
                       AND op=$4
                       AND content_hash=$5"
                );
                client.execute(&stmt, &[&key, &error, &now, &expected_op, &content_hash])?
            } else {
                let stmt = format!(
                    "UPDATE {table}
                     SET status='failed', last_error=$2, attempt_count=attempt_count+1, last_attempt_at=$3, updated_at=$3
                     WHERE key=$1
                       AND status='pending'
                       AND op=$4
                       AND content_hash IS NULL"
                );
                client.execute(&stmt, &[&key, &error, &now, &expected_op])?
            };
            if affected == 0 {
                anyhow::bail!("sync state changed concurrently for key '{key}' in {table}");
            }
            Ok(())
        })
        .await
    }

    pub async fn is_pending_upsert_hash(&self, key: &str, expected_hash: &str) -> Result<bool> {
        let key = key.to_string();
        let expected_hash = expected_hash.to_string();
        let table = self.qualified_table.clone();
        self.run_db_task(move |client| {
            let stmt = format!("SELECT op, status, content_hash FROM {table} WHERE key=$1");
            let row = client.query_opt(&stmt, &[&key])?;
            let Some(row) = row else {
                return Ok(false);
            };
            let op: String = row.get(0);
            let status: String = row.get(1);
            let hash: Option<String> = row.get(2);
            Ok(op == SyncOp::Upsert.as_str()
                && status == "pending"
                && hash.as_deref() == Some(expected_hash.as_str()))
        })
        .await
    }

    fn init_schema(&self) -> Result<()> {
        let table = self.qualified_table.clone();
        let store = self.clone();
        let init_handle = std::thread::Builder::new()
            .name("postgres-qdrant-sync-init".to_string())
            .spawn(move || {
                store.run_db_task_sync(move |client| {
                    let lock_key = format!("{table}:init");
                    client.query("SELECT pg_advisory_lock(hashtext($1))", &[&lock_key])?;

                    let init_result = client.batch_execute(&format!(
                        "\
                        CREATE TABLE IF NOT EXISTS {table} (
                            key TEXT PRIMARY KEY,
                            op TEXT NOT NULL,
                            status TEXT NOT NULL,
                            attempt_count INTEGER NOT NULL DEFAULT 0,
                            last_error TEXT,
                            updated_at TIMESTAMPTZ NOT NULL,
                            last_attempt_at TIMESTAMPTZ,
                            last_synced_at TIMESTAMPTZ,
                            content_hash TEXT
                        );
                        CREATE INDEX IF NOT EXISTS idx_memories_qdrant_sync_status_updated ON {table}(status, updated_at DESC);
                        CREATE INDEX IF NOT EXISTS idx_memories_qdrant_sync_op_status ON {table}(op, status);
                        CREATE INDEX IF NOT EXISTS idx_memories_qdrant_sync_last_attempt ON {table}(last_attempt_at);
                        "
                    ));

                    let _ = client.query("SELECT pg_advisory_unlock(hashtext($1))", &[&lock_key]);
                    init_result?;
                    Ok(())
                })
            })
            .context("failed to spawn postgres-qdrant-sync-init thread")?;

        init_handle
            .join()
            .map_err(|_| anyhow::anyhow!("postgres-qdrant-sync-init thread panicked"))?
    }

    async fn run_db_task<T, F>(&self, task: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Client) -> Result<T> + Send + 'static,
    {
        let store = self.clone();
        tokio::task::spawn_blocking(move || store.run_db_task_sync(task))
            .await
            .context("failed to join sync state task")?
    }

    fn run_db_task_sync<T, F>(&self, task: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Client) -> Result<T> + Send + 'static,
    {
        self.client.with_client(task)
    }
}

fn sanitize_error_for_storage(error: &str) -> String {
    let mut out = String::with_capacity(error.len().min(LAST_ERROR_MAX_CHARS));
    let mut count = 0usize;
    for ch in error.chars() {
        if count >= LAST_ERROR_MAX_CHARS {
            break;
        }
        if ch.is_control() {
            out.push(' ');
        } else {
            out.push(ch);
        }
        count += 1;
    }
    out.trim().to_string()
}
