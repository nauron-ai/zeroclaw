use anyhow::{Context, Result};
use chrono::Utc;
use postgres::{Client, NoTls};
use std::time::Duration;

const POSTGRES_CONNECT_TIMEOUT_CAP_SECS: u64 = 300;

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
    db_url: String,
    connect_timeout_secs: Option<u64>,
    tls_mode: bool,
    qualified_table: String,
}

impl SyncStateStore {
    pub fn new(
        db_url: &str,
        schema: &str,
        connect_timeout_secs: Option<u64>,
        tls_mode: bool,
    ) -> Result<Self> {
        validate_identifier(schema, "storage schema")?;
        let qualified_table = format!("{}.\"memories_qdrant_sync\"", quote_identifier(schema));
        let store = Self {
            db_url: db_url.to_string(),
            connect_timeout_secs,
            tls_mode,
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
                    content_hash = EXCLUDED.content_hash
                "
            );
            client.execute(&stmt, &[&key, &op, &now, &content_hash])?;
            Ok(())
        })
        .await
    }

    pub async fn mark_synced(&self, key: &str) -> Result<()> {
        let key = key.to_string();
        let table = self.qualified_table.clone();
        self.run_db_task(move |client| {
            let now = Utc::now();
            let stmt = format!(
                "UPDATE {table}
                 SET status='synced', last_error=NULL, last_synced_at=$2, updated_at=$2
                 WHERE key=$1"
            );
            client.execute(&stmt, &[&key, &now])?;
            Ok(())
        })
        .await
    }

    pub async fn mark_failed(&self, key: &str, error: &str) -> Result<()> {
        let key = key.to_string();
        let error = error.to_string();
        let table = self.qualified_table.clone();
        self.run_db_task(move |client| {
            let now = Utc::now();
            let stmt = format!(
                "UPDATE {table}
                 SET status='failed', last_error=$2, attempt_count=attempt_count+1, last_attempt_at=$3, updated_at=$3
                 WHERE key=$1"
            );
            client.execute(&stmt, &[&key, &error, &now])?;
            Ok(())
        })
        .await
    }

    fn init_schema(&self) -> Result<()> {
        let table = self.qualified_table.clone();
        self.run_db_task_sync(move |client| {
            client.batch_execute(&format!(
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
            ))?;
            Ok(())
        })
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
        F: FnOnce(&mut Client) -> Result<T>,
    {
        let mut client = connect_client(&self.db_url, self.connect_timeout_secs, self.tls_mode)?;
        task(&mut client)
    }
}

fn connect_client(
    db_url: &str,
    connect_timeout_secs: Option<u64>,
    tls_mode: bool,
) -> Result<Client> {
    let mut config: postgres::Config = db_url
        .parse()
        .context("invalid PostgreSQL connection URL")?;
    if let Some(timeout_secs) = connect_timeout_secs {
        config.connect_timeout(Duration::from_secs(
            timeout_secs.min(POSTGRES_CONNECT_TIMEOUT_CAP_SECS),
        ));
    }

    if tls_mode {
        let mut tls_config = rustls::ClientConfig::builder()
            .with_root_certificates(rustls::RootCertStore::empty())
            .with_no_client_auth();
        tls_config
            .dangerous()
            .set_certificate_verifier(std::sync::Arc::new(super::tls::NoCertVerifier));
        let tls = tokio_postgres_rustls::MakeRustlsConnect::new(tls_config);
        config
            .connect(tls)
            .context("failed to connect PostgreSQL sync state (TLS)")
    } else {
        config
            .connect(NoTls)
            .context("failed to connect PostgreSQL sync state")
    }
}

fn validate_identifier(value: &str, field_name: &str) -> Result<()> {
    if value.is_empty() {
        anyhow::bail!("{field_name} must not be empty");
    }
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        anyhow::bail!("{field_name} must not be empty");
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        anyhow::bail!("{field_name} must start with an ASCII letter or underscore; got '{value}'");
    }
    if !chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
        anyhow::bail!(
            "{field_name} can only contain ASCII letters, numbers, and underscores; got '{value}'"
        );
    }
    Ok(())
}

fn quote_identifier(value: &str) -> String {
    format!("\"{value}\"")
}
