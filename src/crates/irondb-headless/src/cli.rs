//! clap derive CLI mirroring Go internal/cli surface (query|export|connection|version).
//! Exit codes: 0 ok, 2 usage, 3 connection, 4 policy-denied, 5 execution, 6 cancelled.

use std::path::{Path, PathBuf};

use clap::{Parser, Subcommand};

pub const EXIT_OK: i32 = 0;
pub const EXIT_USAGE: i32 = 2;
pub const EXIT_CONNECTION: i32 = 3;
pub const EXIT_POLICY: i32 = 4;
pub const EXIT_EXECUTION: i32 = 5;

#[derive(Parser, Debug)]
#[command(name = "irondb-headless", version)]
pub struct Cli {
    #[arg(long, global = true)]
    pub data_root: Option<PathBuf>,
    #[command(subcommand)]
    pub cmd: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    Query(QueryArgs),
    Export(ExportArgs),
    Connection(ConnectionArgs),
    Serve(ServeArgs),
    Version,
}

#[derive(clap::Args, Debug)]
pub struct QueryArgs {
    #[arg(long)]
    pub conn: Option<String>,
    #[arg(long)]
    pub connection_file: Option<PathBuf>,
    #[arg(long)]
    pub database: Option<String>,
    #[arg(long)]
    pub sql: Option<String>,
    #[arg(long)]
    pub sql_file: Option<PathBuf>,
    #[arg(long, default_value = "jsonl")]
    pub format: String,
    #[arg(long, alias = "allow-mutating")]
    pub allow_write: bool,
    #[arg(long, default_value_t = 0)]
    pub query_timeout: i64,
}

#[derive(clap::Args, Debug)]
pub struct ExportArgs {
    #[arg(long)]
    pub conn: Option<String>,
    #[arg(long)]
    pub connection_file: Option<PathBuf>,
    #[arg(long)]
    pub database: Option<String>,
    #[arg(long)]
    pub sql: Option<String>,
    #[arg(long)]
    pub sql_file: Option<PathBuf>,
    #[arg(long)]
    pub output: Option<PathBuf>,
    #[arg(long, default_value = "csv")]
    pub format: String,
    #[arg(long)]
    pub force: bool,
    #[arg(long, default_value_t = 0)]
    pub query_timeout: i64,
}

#[derive(clap::Args, Debug)]
pub struct ConnectionArgs {
    #[command(subcommand)]
    pub cmd: ConnectionCmd,
}

#[derive(Subcommand, Debug)]
pub enum ConnectionCmd {
    List,
    Add { id: String },
    Import { file: PathBuf },
}

#[derive(clap::Args, Debug)]
pub struct ServeArgs {
    #[arg(long, default_value = "127.0.0.1:18789")]
    pub bind: String,
}

#[derive(Debug, serde::Deserialize)]
pub struct ConnectionFile {
    pub engine: Option<String>,
    pub dsn: Option<String>,
    pub id: Option<String>,
}

/// Validate --connection-file: regular file, no symlink, size cap 1MiB, unix 0600 owner check.
pub fn load_connection_file(path: &Path) -> Result<ConnectionFile, CliError> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| CliError::Usage("connection file not found".into()))?;
    if meta.file_type().is_symlink() {
        return Err(CliError::Usage("connection file must not be a symlink".into()));
    }
    if !meta.is_file() {
        return Err(CliError::Usage("connection file must be a regular file".into()));
    }
    if meta.len() > 1024 * 1024 {
        return Err(CliError::Usage("connection file too large".into()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.mode() & 0o077 != 0 {
            return Err(CliError::Usage("connection file must be mode 0600".into()));
        }
        if meta.uid() != unsafe { libc_uid() } {
            return Err(CliError::Usage("connection file owner mismatch".into()));
        }
    }
    let text = std::fs::read_to_string(path).map_err(|_| CliError::Usage("cannot read connection file".into()))?;
    serde_json::from_str(&text).map_err(|_| CliError::Usage("invalid connection file JSON".into()))
}

#[cfg(unix)]
unsafe fn libc_uid() -> u32 {
    unsafe extern "C" {
        fn getuid() -> u32;
    }
    unsafe { getuid() }
}

/// Default-deny write guard: mutating SQL requires --allow-write.
pub fn write_guard(sql: &str, allow_write: bool) -> Result<(), CliError> {
    if allow_write {
        return Ok(());
    }
    let head = sql.trim_start().to_ascii_uppercase();
    let readonly = head.starts_with("SELECT") || head.starts_with("WITH") || head.starts_with("EXPLAIN");
    if readonly {
        Ok(())
    } else {
        Err(CliError::PolicyDenied("write blocked: pass --allow-write".into()))
    }
}

pub fn resolve_sql(sql: &Option<String>, sql_file: &Option<PathBuf>, positional: &[String]) -> Result<String, CliError> {
    if let Some(s) = sql {
        return Ok(s.clone());
    }
    if let Some(f) = sql_file {
        return std::fs::read_to_string(f).map_err(|_| CliError::Usage("cannot read sql file".into()));
    }
    if !positional.is_empty() {
        return Ok(positional.join(" "));
    }
    Err(CliError::Usage("missing --sql|--sql-file".into()))
}

#[derive(Debug)]
pub enum CliError {
    Usage(String),
    Connection(String),
    PolicyDenied(String),
    Execution(String),
}

impl CliError {
    pub fn exit_code(&self) -> i32 {
        match self {
            CliError::Usage(_) => EXIT_USAGE,
            CliError::Connection(_) => EXIT_CONNECTION,
            CliError::PolicyDenied(_) => EXIT_POLICY,
            CliError::Execution(_) => EXIT_EXECUTION,
        }
    }
    /// Never include secrets: callers must pass pre-redacted messages.
    pub fn message(&self) -> &str {
        match self {
            CliError::Usage(m) | CliError::Connection(m) | CliError::PolicyDenied(m) | CliError::Execution(m) => m,
        }
    }
}
