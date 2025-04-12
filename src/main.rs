// hide console window on Windows in release
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use argh::FromArgs;
use notify::{
    Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
    event::{ModifyKind, RenameMode},
};
use rusqlite::{Connection, params};
use tokio::{
    sync::mpsc,
    time::{Duration, interval},
};
use uuid::Uuid;

#[derive(FromArgs)]
/// A file change monitoring tool
struct Args {
    /// root directory to watch
    #[argh(positional)]
    root_dir: String,

    /// sqlite database path
    #[argh(positional)]
    db_path: String,
}

struct EventRecord {
    id: String,
    timestamp: i64,
    change_type: String,
    path: String,
    file_name: String,
}

fn init_db(db_path: &str) -> rusqlite::Result<()> {
    let conn = Connection::open(db_path)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
        CREATE TABLE IF NOT EXISTS file_events (
            id TEXT PRIMARY KEY,
            timestamp INTEGER NOT NULL,
            change_type TEXT NOT NULL,
            path TEXT NOT NULL,
            file_name TEXT NOT NULL
        );",
    )?;
    Ok(())
}

fn process_event(event: Event) -> Vec<EventRecord> {
    let change_type = match event.kind {
        EventKind::Create(_) => "create",
        EventKind::Modify(kind) => {
            match kind {
                // ignore metadata update
                ModifyKind::Metadata(_) => return vec![],
                ModifyKind::Name(RenameMode::From) => "rename_from",
                ModifyKind::Name(RenameMode::To) => "rename_to",
                ModifyKind::Name(RenameMode::Any) => "renamed",
                _ => "modify",
            }
        }
        EventKind::Remove(_) => "remove",
        _ => return vec![],
    };

    event
        .paths
        .into_iter()
        .filter_map(|path| {
            let file_name = path.file_name()?.to_str()?;
            Some(EventRecord {
                id: Uuid::new_v4().to_string(),
                timestamp: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64,
                change_type: change_type.to_string(),
                path: path.to_str()?.to_string(),
                file_name: file_name.to_string(),
            })
        })
        .collect()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Args = argh::from_env();
    init_db(&args.db_path)?;

    let (tx, mut rx) = mpsc::channel(100);
    let db_path = args.db_path.clone();

    // File watcher thread
    let root_dir = args.root_dir.clone();
    tokio::task::spawn_blocking(move || {
        let (sync_tx, sync_rx) = std::sync::mpsc::channel();
        let mut watcher = RecommendedWatcher::new(sync_tx, Config::default()).unwrap();

        watcher
            .watch(Path::new(&root_dir), RecursiveMode::Recursive)
            .unwrap();

        for event in sync_rx.iter().flatten() {
            let tx = tx.clone();
            tokio::task::spawn(async move {
                let events = process_event(event);
                for record in events {
                    tx.send(record).await.unwrap();
                }
            });
        }
    });

    // Database writer task
    tokio::spawn(async move {
        let mut buffer = Vec::with_capacity(100);
        let mut interval = interval(Duration::from_secs(2));

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    if !buffer.is_empty() {
                        let tmp = buffer;
                        buffer = Vec::new();
                        flush_records(tmp, &db_path).await;
                    }
                }
                Some(record) = rx.recv() => {
                    buffer.push(record);
                    if buffer.len() >= 100 {
                        let tmp = buffer;
                        buffer = Vec::new();
                        flush_records(tmp, &db_path).await;
                        buffer.clear();
                    }
                }
            }
        }
    })
    .await?;

    Ok(())
}

async fn flush_records(records: Vec<EventRecord>, db_path: &str) {
    let db_path = db_path.to_string();

    tokio::task::spawn_blocking(move || {
        let conn = Connection::open(&db_path).unwrap();
        let tx = conn.unchecked_transaction().unwrap();

        for record in records {
            tx.execute(
                "INSERT INTO file_events (id, timestamp, change_type, path, file_name) 
                VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    record.id,
                    record.timestamp,
                    record.change_type,
                    record.path,
                    record.file_name
                ],
            )
            .unwrap();
        }

        tx.commit().unwrap();
    })
    .await
    .unwrap();
}
