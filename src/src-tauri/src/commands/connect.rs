//! Frozen `DBConnect` bridge command — thin wrapper over `AppFacade::connect`.
//! Failures serialize inside the QueryResult envelope (success:false); never
//! `Result::Err` to the frontend, never panic, DSN sanitized.

use irondb_contracts::QueryResult;
use irondb_facade::{sanitize_dsn, SharedFacade};

fn failure(message: impl Into<String>) -> QueryResult {
    QueryResult {
        success: false,
        message: message.into(),
        ..Default::default()
    }
}

/// Testable core: same logic the command runs, minus the Tauri `State` wrapper.
pub async fn db_connect_inner(
    facade: &SharedFacade,
    id: &str,
    engine: &str,
    dsn: &str,
) -> QueryResult {
    match facade.connect(id, engine, dsn).await {
        Ok(()) => QueryResult {
            success: true,
            ..Default::default()
        },
        Err(e) => {
            let msg = format!(
                "connect failed for engine {}: {}",
                sanitize_dsn(engine),
                sanitize_dsn(&e.to_string())
            );
            failure(msg)
        }
    }
}

// Frozen-name Tauri entry point: delegates to [`db_connect_inner`].
// `Result<_, String>` shape is a Tauri macro requirement for `State` params;
// the `Err` arm is unreachable (failures stay inside the envelope).
#[allow(non_snake_case)]
#[tauri::command]
pub async fn DBConnect(
    facade: tauri::State<'_, SharedFacade>,
    id: String,
    engine: String,
    dsn: String,
) -> Result<QueryResult, String> {
    Ok(db_connect_inner(&facade, &id, &engine, &dsn).await)
}
