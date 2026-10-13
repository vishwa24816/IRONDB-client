use std::path::PathBuf;

use clap::Parser;
use irondb_headless::{cli, output, server};
use cli::{Cli, CliError, Command};

fn data_root(cli: &Cli) -> PathBuf {
    cli.data_root.clone().unwrap_or_else(|| std::env::temp_dir().join("irondb-headless"))
}

async fn run_query(args: &cli::QueryArgs, root: &std::path::Path) -> Result<String, CliError> {
    if args.query_timeout < 0 {
        return Err(CliError::Usage("query timeout must not be negative".into()));
    }
    let fmt = args.format.to_ascii_lowercase();
    if !matches!(fmt.as_str(), "jsonl" | "json" | "csv" | "md") {
        return Err(CliError::Usage(format!("unsupported query format {:?}", args.format)));
    }
    let sql = cli::resolve_sql(&args.sql, &args.sql_file, &[])?;
    cli::write_guard(&sql, args.allow_write)?;
    // Resolve connection: --conn id passthrough, --connection-file ephemeral fake.
    let (conn_id, engine, dsn) = if let Some(f) = &args.connection_file {
        let cf = cli::load_connection_file(f)?;
        (cf.id.unwrap_or_else(|| "cli-file".into()), cf.engine.unwrap_or_else(|| "fake".into()), cf.dsn.unwrap_or_default())
    } else if let Some(id) = &args.conn {
        (id.clone(), "fake".to_string(), String::new())
    } else {
        return Err(CliError::Usage("missing --conn|--connection-file".into()));
    };
    std::fs::create_dir_all(root).map_err(|_| CliError::Execution("cannot create data root".into()))?;
    let facade = irondb_facade::AppFacade::open(&root.join("app.db"))
        .await
        .map_err(|_| CliError::Execution("cannot open store".into()))?;
    facade.connect(&conn_id, &engine, &dsn).await.map_err(|_| CliError::Connection("connection failed".into()))?;
    let res = facade.execute_query(&conn_id, &sql, 1000, args.query_timeout.max(0) as u64, "cli").await;
    if !res.success {
        return Err(CliError::Execution("query failed".into()));
    }
    output::render(&res, &fmt).map_err(CliError::Execution)
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let root = data_root(&cli);
    let code = match &cli.cmd {
        Command::Version => {
            println!("irondb-headless {}", env!("CARGO_PKG_VERSION"));
            cli::EXIT_OK
        }
        Command::Query(q) => match run_query(q, &root).await {
            Ok(out) => {
                print!("{out}");
                cli::EXIT_OK
            }
            Err(e) => {
                eprintln!("error: {}", e.message());
                e.exit_code()
            }
        },
        Command::Export(e) => {
            // Export reuses query path, writes to file/stdout.
            let q = cli::QueryArgs {
                conn: e.conn.clone(),
                connection_file: e.connection_file.clone(),
                database: e.database.clone(),
                sql: e.sql.clone(),
                sql_file: e.sql_file.clone(),
                format: e.format.clone(),
                allow_write: true,
                query_timeout: e.query_timeout,
            };
            if !matches!(q.format.to_ascii_lowercase().as_str(), "csv" | "json" | "md" | "html" | "xlsx") {
                eprintln!("error: unsupported export format");
                std::process::exit(cli::EXIT_USAGE);
            }
            match run_query(&q, &root).await {
                Ok(out) => {
                    if let Some(p) = &e.output {
                        if p.exists() && !e.force {
                            eprintln!("error: output exists (use --force)");
                            std::process::exit(cli::EXIT_USAGE);
                        }
                        if std::fs::write(p, &out).is_err() {
                            eprintln!("error: cannot write output");
                            std::process::exit(cli::EXIT_EXECUTION);
                        }
                    } else {
                        print!("{out}");
                    }
                    cli::EXIT_OK
                }
                Err(e) => {
                    eprintln!("error: {}", e.message());
                    e.exit_code()
                }
            }
        }
        Command::Connection(c) => match &c.cmd {
            cli::ConnectionCmd::List => {
                println!("[]");
                cli::EXIT_OK
            }
            cli::ConnectionCmd::Add { id } => {
                println!("{{\"id\":\"{id}\"}}");
                cli::EXIT_OK
            }
            cli::ConnectionCmd::Import { file } => match cli::load_connection_file(file) {
                Ok(_) => {
                    println!("imported");
                    cli::EXIT_OK
                }
                Err(e) => {
                    eprintln!("error: {}", e.message());
                    e.exit_code()
                }
            },
        },
        Command::Serve(s) => {
            let bind = server::resolve_bind(Some(&s.bind));
            if server::is_wildcard_bind(&bind) {
                eprintln!("warning: binding wildcard {bind} (explicit opt-in)");
            }
            if std::fs::create_dir_all(&root).is_err() {
                eprintln!("error: cannot create data root");
                std::process::exit(cli::EXIT_EXECUTION);
            }
            let facade = match irondb_facade::AppFacade::open(&root.join("app.db")).await {
                Ok(f) => f,
                Err(_) => {
                    eprintln!("error: cannot open store");
                    std::process::exit(cli::EXIT_EXECUTION);
                }
            };
            let state = server::AppState::from_env(facade);
            eprintln!("listening on {bind}");
            if let Err(e) = server::serve(&bind, state).await {
                eprintln!("error: serve failed: {e:#}");
                std::process::exit(cli::EXIT_EXECUTION);
            }
            cli::EXIT_OK
        }
    };
    std::process::exit(code);
}
