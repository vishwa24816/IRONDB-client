// Prevent Tauri's build script from picking up the workspace root build.rs (none) — standard.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;

use commands::connect::DBConnect;
use commands::query::{db_cancel, DBQuery, DBQueryWithCancel};
use commands::secrets::{secret_delete, secret_store};
use commands::conns::{conn_delete, conn_list, conn_save};
use commands::objects::{db_objects, db_table_ddl};
use commands::transfer::{export_run, import_run};
use irondb_facade::AppFacade;
use tauri::Manager;

// Single smoke command proving the frontend-to-Rust bridge seam works.
#[tauri::command]
fn ping() -> String {
    "pong".into()
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            let dir = app
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join("IRONDB"));
            std::fs::create_dir_all(&dir).expect("app data dir");
            let facade = tauri::async_runtime::block_on(AppFacade::open(&dir.join("app.db")))
                .expect("open app store");
            app.manage(facade);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            DBConnect,
            DBQuery,
            DBQueryWithCancel,
            db_cancel,
            secret_store,
            secret_delete,
            db_objects,
            db_table_ddl,
            conn_list,
            conn_save,
            conn_delete,
            import_run,
            export_run,
        ])
        .run(tauri::generate_context!())
        .expect("tauri failed");
}
