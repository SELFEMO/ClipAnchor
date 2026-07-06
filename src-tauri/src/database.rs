use crate::{clipboard_service, models::{ClipItem, ClipKind, HistoryRecord}, paths::DataPaths};
use chrono::{Duration, Utc};
use rusqlite::{params, Connection};

fn conn(paths: &DataPaths) -> Result<Connection, String> {
    Connection::open(&paths.database).map_err(|error| error.to_string())
}

pub fn init(paths: &DataPaths) -> Result<(), String> {
    let db = conn(paths)?;
    db.execute_batch(
        "CREATE TABLE IF NOT EXISTS records (
            id TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            summary TEXT NOT NULL,
            text_content TEXT,
            image_path TEXT,
            file_paths TEXT NOT NULL,
            bytes INTEGER NOT NULL,
            created_at TEXT NOT NULL,
            content_hash TEXT NOT NULL DEFAULT '',
            is_pinned INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS records_created_at ON records(created_at);
        CREATE INDEX IF NOT EXISTS records_is_pinned ON records(is_pinned);
        CREATE INDEX IF NOT EXISTS records_content_hash ON records(kind, content_hash);"
    ).map_err(|error| error.to_string())?;
    ensure_columns(&db)?;
    backfill_content_hash(&db)?;
    Ok(())
}

fn ensure_columns(db: &Connection) -> Result<(), String> {
    let mut stmt = db.prepare("PRAGMA table_info(records)").map_err(|error| error.to_string())?;
    let columns = stmt.query_map([], |row| row.get::<_, String>(1)).map_err(|error| error.to_string())?;
    let mut has_pin_column = false;
    let mut has_hash_column = false;
    for column in columns {
        match column.map_err(|error| error.to_string())?.as_str() {
            "is_pinned" => has_pin_column = true,
            "content_hash" => has_hash_column = true,
            _ => {}
        }
    }
    if !has_pin_column {
        // 旧数据库需要无损迁移到收藏字段，否则用户升级后历史记录会因为缺列无法读取。
        // Existing databases need a non-destructive migration to the favorite column, otherwise history cannot be read after upgrading.
        db.execute("ALTER TABLE records ADD COLUMN is_pinned INTEGER NOT NULL DEFAULT 0", []).map_err(|error| error.to_string())?;
    }
    if !has_hash_column {
        // 内容哈希用于复制时后台去重；迁移旧库时给默认值，避免破坏已有历史结构。
        // Content hashes power background deduplication on copy; old databases receive a default value so existing history stays readable.
        db.execute("ALTER TABLE records ADD COLUMN content_hash TEXT NOT NULL DEFAULT ''", []).map_err(|error| error.to_string())?;
    }
    db.execute("CREATE INDEX IF NOT EXISTS records_content_hash ON records(kind, content_hash)", []).map_err(|error| error.to_string())?;
    Ok(())
}

fn kind_to_string(kind: &ClipKind) -> &'static str {
    match kind {
        ClipKind::Text => "text",
        ClipKind::Image => "image",
        ClipKind::File => "file",
        ClipKind::Mixed => "mixed",
    }
}

fn string_to_kind(value: String) -> ClipKind {
    match value.as_str() {
        "image" => ClipKind::Image,
        "file" => ClipKind::File,
        "mixed" => ClipKind::Mixed,
        _ => ClipKind::Text,
    }
}

fn backfill_content_hash(db: &Connection) -> Result<(), String> {
    let mut stmt = db.prepare(
        "SELECT id, kind, text_content, image_path, file_paths FROM records WHERE content_hash = ''"
    ).map_err(|error| error.to_string())?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, String>(4)?,
        ))
    }).map_err(|error| error.to_string())?;

    let mut pending = Vec::new();
    for row in rows {
        let (id, kind, text_content, image_path, file_paths_raw) = row.map_err(|error| error.to_string())?;
        if let Some(content_hash) = hash_existing_record(&kind, text_content, image_path, &file_paths_raw) {
            pending.push((id, content_hash));
        }
    }
    for (id, content_hash) in pending {
        // 回填旧记录的哈希后，后续复制同内容时才能删除旧项并只保留最新项。
        // Backfilled hashes let later copies remove older duplicates and keep only the newest item.
        db.execute("UPDATE records SET content_hash = ?1 WHERE id = ?2", params![content_hash, id]).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn hash_existing_record(kind: &str, text_content: Option<String>, image_path: Option<String>, file_paths_raw: &str) -> Option<String> {
    match kind {
        "image" => image_path
            .and_then(|path| image::open(path).ok())
            .map(|image| clipboard_service::content_hash_for_bytes("image", &image.to_rgba8().into_raw())),
        "file" => serde_json::from_str::<Vec<String>>(file_paths_raw)
            .ok()
            .map(|paths| clipboard_service::content_hash_for_paths(&paths)),
        _ => text_content.map(|text| clipboard_service::content_hash_for_bytes("text", text.as_bytes())),
    }
}

pub fn insert(paths: &DataPaths, item: &ClipItem) -> Result<(), String> {
    let db = conn(paths)?;
    ensure_columns(&db)?;
    backfill_content_hash(&db)?;
    let file_paths = serde_json::to_string(&item.file_paths).map_err(|error| error.to_string())?;
    let inherited_pinned = remove_duplicate_records(&db, item)?;
    let is_pinned = item.is_pinned || inherited_pinned;

    // 写入前先删除同内容旧记录，是为了让历史记录按“最新复制”排序且不会堆积重复项。
    // Older records with the same content are removed before insert so history reflects the latest copy without duplicate clutter.
    db.execute(
        "INSERT OR REPLACE INTO records (id, kind, summary, text_content, image_path, file_paths, bytes, created_at, content_hash, is_pinned)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![item.id, kind_to_string(&item.kind), item.summary, item.text_content, item.image_path, file_paths, item.bytes, item.created_at, item.content_hash, is_pinned as i32]
    ).map_err(|error| error.to_string())?;
    Ok(())
}

pub fn upsert_text(paths: &DataPaths, item: &ClipItem) -> Result<HistoryRecord, String> {
    insert(paths, item)?;
    get(paths, &item.id)?.ok_or_else(|| "Record not found after save".to_string())
}

pub fn update_text(paths: &DataPaths, id: &str, text: &str) -> Result<HistoryRecord, String> {
    let existing = get(paths, id)?.ok_or_else(|| "Record not found".to_string())?;
    if existing.kind != ClipKind::Text {
        return Err("Only text records can be edited".into());
    }
    let summary: String = text.chars().take(200).collect();
    let item = ClipItem {
        id: existing.id,
        kind: ClipKind::Text,
        summary,
        text_content: Some(text.to_string()),
        image_path: None,
        file_paths: Vec::new(),
        bytes: text.as_bytes().len() as i64,
        created_at: chrono::Utc::now().to_rfc3339(),
        content_hash: clipboard_service::content_hash_for_bytes("text", text.as_bytes()),
        is_pinned: existing.is_pinned,
    };
    // 编辑文本时复用去重写入路径，是为了让“编辑后内容重复”也遵守只保留最新记录的规则。
    // Text edits reuse the deduplicating write path so edited duplicates still keep only the newest record.
    upsert_text(paths, &item)
}

fn remove_duplicate_records(db: &Connection, item: &ClipItem) -> Result<bool, String> {
    if item.content_hash.is_empty() {
        return Ok(false);
    }
    let mut stmt = db.prepare(
        "SELECT id, is_pinned FROM records WHERE kind = ?1 AND content_hash = ?2 AND id <> ?3"
    ).map_err(|error| error.to_string())?;
    let duplicates = stmt.query_map(params![kind_to_string(&item.kind), item.content_hash, item.id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i32>(1)? != 0))
    }).map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())?;
    if duplicates.is_empty() {
        return Ok(false);
    }
    let inherited_pinned = duplicates.iter().any(|(_, pinned)| *pinned);
    for (id, _) in duplicates {
        db.execute("DELETE FROM records WHERE id = ?1", params![id]).map_err(|error| error.to_string())?;
    }
    // 如果旧重复项是收藏项，将收藏状态转移到最新记录，既满足去重，又不会让用户的重要内容“消失”。
    // If an older duplicate was favorited, the favorite state moves to the newest record, satisfying deduplication without losing important saved content.
    Ok(inherited_pinned)
}

pub fn list(paths: &DataPaths, query: &str, kind: &str, limit: u32) -> Result<Vec<HistoryRecord>, String> {
    let db = conn(paths)?;
    ensure_columns(&db)?;
    backfill_content_hash(&db)?;
    let pattern = format!("%{}%", query);
    let limit_clause = if limit == 0 { String::new() } else { format!(" LIMIT {}", limit) };

    // 历史上限由设置控制，0 表示无限制；SQL 片段只来自数字设置，避免用户输入影响查询结构。
    // The history limit is settings-driven and 0 means unlimited; the SQL fragment is numeric-only so user input cannot affect query structure.
    let sql = if kind == "favorite" {
        format!("SELECT id, kind, summary, text_content, image_path, file_paths, bytes, created_at, content_hash, is_pinned FROM records WHERE summary LIKE ?1 AND is_pinned = 1 ORDER BY created_at DESC{}", limit_clause)
    } else if kind == "all" {
        format!("SELECT id, kind, summary, text_content, image_path, file_paths, bytes, created_at, content_hash, is_pinned FROM records WHERE summary LIKE ?1 ORDER BY created_at DESC{}", limit_clause)
    } else {
        format!("SELECT id, kind, summary, text_content, image_path, file_paths, bytes, created_at, content_hash, is_pinned FROM records WHERE summary LIKE ?1 AND kind = ?2 ORDER BY created_at DESC{}", limit_clause)
    };

    let mut stmt = db.prepare(&sql).map_err(|error| error.to_string())?;
    if kind == "favorite" || kind == "all" {
        let rows = stmt.query_map(params![pattern], row_to_record).map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())
    } else {
        let rows = stmt.query_map(params![pattern, kind], row_to_record).map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())
    }
}

pub fn get(paths: &DataPaths, id: &str) -> Result<Option<HistoryRecord>, String> {
    let db = conn(paths)?;
    ensure_columns(&db)?;
    backfill_content_hash(&db)?;
    let mut stmt = db.prepare(
        "SELECT id, kind, summary, text_content, image_path, file_paths, bytes, created_at, content_hash, is_pinned FROM records WHERE id = ?1"
    ).map_err(|error| error.to_string())?;
    let mut rows = stmt.query(params![id]).map_err(|error| error.to_string())?;
    if let Some(row) = rows.next().map_err(|error| error.to_string())? {
        row_to_record(row).map(Some).map_err(|error| error.to_string())
    } else {
        Ok(None)
    }
}

fn row_to_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<HistoryRecord> {
    let raw_kind: String = row.get(1)?;
    let raw_paths: String = row.get(5)?;
    let file_paths = serde_json::from_str(&raw_paths).unwrap_or_default();
    let pin_flag: i32 = row.get(9)?;
    Ok(HistoryRecord {
        id: row.get(0)?,
        kind: string_to_kind(raw_kind),
        summary: row.get(2)?,
        text_content: row.get(3)?,
        image_path: row.get(4)?,
        file_paths,
        bytes: row.get(6)?,
        created_at: row.get(7)?,
        content_hash: row.get(8)?,
        is_pinned: pin_flag != 0,
    })
}

pub fn set_pinned(paths: &DataPaths, id: &str, pinned: bool) -> Result<HistoryRecord, String> {
    let db = conn(paths)?;
    ensure_columns(&db)?;
    db.execute("UPDATE records SET is_pinned = ?1 WHERE id = ?2", params![pinned as i32, id]).map_err(|error| error.to_string())?;
    get(paths, id)?.ok_or_else(|| "Record not found".to_string())
}

pub fn delete(paths: &DataPaths, ids: &[String]) -> Result<Vec<HistoryRecord>, String> {
    let db = conn(paths)?;
    ensure_columns(&db)?;
    let mut deleted = Vec::new();
    for id in ids {
        if let Some(record) = get(paths, id)? {
            if record.is_pinned {
                continue;
            }
            db.execute("DELETE FROM records WHERE id = ?1 AND is_pinned = 0", params![id]).map_err(|error| error.to_string())?;
            deleted.push(record);
        }
    }
    Ok(deleted)
}

pub fn delete_force(paths: &DataPaths, ids: &[String]) -> Result<Vec<HistoryRecord>, String> {
    let db = conn(paths)?;
    ensure_columns(&db)?;
    let mut deleted = Vec::new();
    for id in ids {
        if let Some(record) = get(paths, id)? {
            db.execute("DELETE FROM records WHERE id = ?1", params![id]).map_err(|error| error.to_string())?;
            deleted.push(record);
        }
    }
    Ok(deleted)
}

pub fn clear(paths: &DataPaths, preserve_pinned: bool) -> Result<Vec<HistoryRecord>, String> {
    let db = conn(paths)?;
    ensure_columns(&db)?;
    let select_sql = if preserve_pinned {
        "SELECT id, kind, summary, text_content, image_path, file_paths, bytes, created_at, content_hash, is_pinned FROM records WHERE is_pinned = 0 ORDER BY created_at DESC"
    } else {
        "SELECT id, kind, summary, text_content, image_path, file_paths, bytes, created_at, content_hash, is_pinned FROM records ORDER BY created_at DESC"
    };
    let mut stmt = db.prepare(select_sql).map_err(|error| error.to_string())?;
    let deleted = stmt.query_map([], row_to_record).map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())?;
    if preserve_pinned {
        // 默认清空只移除非收藏项，避免一次误操作破坏用户长期保存的重要片段。
        // The default clear removes only non-favorites so one mistake cannot destroy long-lived saved snippets.
        db.execute("DELETE FROM records WHERE is_pinned = 0", []).map_err(|error| error.to_string())?;
    } else {
        db.execute("DELETE FROM records", []).map_err(|error| error.to_string())?;
    }
    Ok(deleted)
}

pub fn delete_older_than(paths: &DataPaths, days: u32, preserve_pinned: bool) -> Result<Vec<HistoryRecord>, String> {
    let db = conn(paths)?;
    ensure_columns(&db)?;
    let cutoff = (Utc::now() - Duration::days(i64::from(days))).to_rfc3339();
    let select_sql = if preserve_pinned {
        "SELECT id, kind, summary, text_content, image_path, file_paths, bytes, created_at, content_hash, is_pinned FROM records WHERE created_at < ?1 AND is_pinned = 0 ORDER BY created_at DESC"
    } else {
        "SELECT id, kind, summary, text_content, image_path, file_paths, bytes, created_at, content_hash, is_pinned FROM records WHERE created_at < ?1 ORDER BY created_at DESC"
    };
    let mut stmt = db.prepare(select_sql).map_err(|error| error.to_string())?;
    let deleted = stmt.query_map(params![cutoff], row_to_record).map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())?;
    if preserve_pinned {
        // 按天数清理默认保留收藏项，是为了让自动化维护空间时不误删用户明确保存的重要内容。
        // Day-based cleanup keeps favorites by default so storage maintenance cannot remove content the user explicitly protected.
        db.execute("DELETE FROM records WHERE created_at < ?1 AND is_pinned = 0", params![cutoff]).map_err(|error| error.to_string())?;
    } else {
        db.execute("DELETE FROM records WHERE created_at < ?1", params![cutoff]).map_err(|error| error.to_string())?;
    }
    Ok(deleted)
}
