#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Variant {
    id: String,
    name: String,
    content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Prompt {
    id: String,
    title: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    category_id: String,
    #[serde(default)]
    variants: Vec<Variant>,
    #[serde(default)]
    favorite: bool,
    #[serde(default)]
    created_at: i64,
    #[serde(default)]
    updated_at: i64,
    #[serde(default)]
    use_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Category {
    id: String,
    name: String,
    #[serde(default)]
    system: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LibraryState {
    #[serde(default)]
    categories: Vec<Category>,
    #[serde(default)]
    prompts: Vec<Prompt>,
    #[serde(default)]
    seeded: bool,
}

struct AppDb {
    conn: Mutex<Connection>,
}

fn exe_dir() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    exe.parent()
        .map(|p| p.to_path_buf())
        .ok_or_else(|| "无法定位程序所在目录".to_string())
}

fn app_root() -> Result<PathBuf, String> {
    if let Ok(exe_parent) = exe_dir() {
        if fs::create_dir_all(exe_parent.join("data")).is_ok() {
            return Ok(exe_parent);
        }
    }
    let root = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("CueVault");
    fs::create_dir_all(root.join("data")).map_err(|e| format!("无法创建数据目录：{e}"))?;
    Ok(root)
}

fn db_path() -> Result<PathBuf, String> {
    Ok(app_root()?.join("data").join("prompt-library.db"))
}

fn webview_dir() -> Result<PathBuf, String> {
    let root = app_root()?;
    let new_dir = root.join("webview");
    let old_dir = root.join("data").join("webview");
    if !new_dir.exists() && old_dir.exists() && fs::rename(&old_dir, &new_dir).is_err() {
        return Ok(old_dir);
    }
    fs::create_dir_all(&new_dir).map_err(|e| format!("无法创建界面缓存目录：{e}"))?;
    Ok(new_dir)
}

fn init_db(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "
        PRAGMA foreign_keys = ON;
        PRAGMA busy_timeout = 5000;
        PRAGMA journal_mode = DELETE;
        CREATE TABLE IF NOT EXISTS categories (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            system INTEGER NOT NULL DEFAULT 0,
            sort_order INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS prompts (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            category_id TEXT NOT NULL DEFAULT '',
            favorite INTEGER NOT NULL DEFAULT 0,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            use_count INTEGER NOT NULL DEFAULT 0
        );
        CREATE TABLE IF NOT EXISTS variants (
            id TEXT PRIMARY KEY,
            prompt_id TEXT NOT NULL,
            name TEXT NOT NULL,
            content TEXT NOT NULL,
            sort_order INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_variants_prompt ON variants(prompt_id, sort_order);
        CREATE INDEX IF NOT EXISTS idx_prompts_category ON prompts(category_id);
        CREATE INDEX IF NOT EXISTS idx_prompts_updated ON prompts(updated_at DESC);
        ",
    )
    .map_err(|e| e.to_string())?;
    let has_tags = conn
        .prepare("PRAGMA table_info(prompts)")
        .map_err(|e| e.to_string())?
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?
        .iter()
        .any(|name| name == "tags_json");
    if !has_tags {
        conn.execute(
            "ALTER TABLE prompts ADD COLUMN tags_json TEXT NOT NULL DEFAULT '[]'",
            [],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn open_db() -> Result<Connection, String> {
    let conn = Connection::open(db_path()?).map_err(|e| format!("无法打开数据库：{e}"))?;
    init_db(&conn)?;
    Ok(conn)
}

fn read_state(conn: &Connection) -> Result<LibraryState, String> {
    let seeded: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'seeded'",
            [],
            |row| row.get(0),
        )
        .ok();

    let mut categories = Vec::new();
    let mut cat_stmt = conn
        .prepare("SELECT id, name, system FROM categories ORDER BY sort_order ASC")
        .map_err(|e| e.to_string())?;
    let cat_rows = cat_stmt
        .query_map([], |row| {
            Ok(Category {
                id: row.get(0)?,
                name: row.get(1)?,
                system: row.get::<_, i64>(2)? != 0,
            })
        })
        .map_err(|e| e.to_string())?;
    for row in cat_rows {
        categories.push(row.map_err(|e| e.to_string())?);
    }

    let mut prompts = Vec::new();
    let mut prompt_stmt = conn
        .prepare(
            "SELECT id, title, category_id, favorite, created_at, updated_at, use_count, tags_json
             FROM prompts ORDER BY updated_at DESC",
        )
        .map_err(|e| e.to_string())?;
    let prompt_rows = prompt_stmt
        .query_map([], |row| {
            Ok(Prompt {
                id: row.get(0)?,
                title: row.get(1)?,
                tags: serde_json::from_str(&row.get::<_, String>(7)?).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        7,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?,
                category_id: row.get(2)?,
                variants: Vec::new(),
                favorite: row.get::<_, i64>(3)? != 0,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
                use_count: row.get(6)?,
            })
        })
        .map_err(|e| e.to_string())?;
    for row in prompt_rows {
        prompts.push(row.map_err(|e| e.to_string())?);
    }

    let mut variant_map: std::collections::HashMap<String, Vec<Variant>> =
        std::collections::HashMap::new();
    let mut variant_stmt = conn
        .prepare("SELECT prompt_id, id, name, content FROM variants ORDER BY sort_order ASC")
        .map_err(|e| e.to_string())?;
    let var_rows = variant_stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                Variant {
                    id: row.get(1)?,
                    name: row.get(2)?,
                    content: row.get(3)?,
                },
            ))
        })
        .map_err(|e| e.to_string())?;
    for row in var_rows {
        let (prompt_id, variant) = row.map_err(|e| e.to_string())?;
        variant_map.entry(prompt_id).or_default().push(variant);
    }

    for prompt in &mut prompts {
        if let Some(vars) = variant_map.remove(&prompt.id) {
            prompt.variants = vars;
        }
    }

    Ok(LibraryState {
        categories,
        prompts,
        seeded: seeded.as_deref() == Some("1"),
    })
}

fn validate_state(state: &LibraryState) -> Result<(), String> {
    let mut seen = HashSet::new();
    for category in &state.categories {
        let id = category.id.trim();
        if id.is_empty() || id != category.id {
            return Err("保存数据无效：存在空的分类编号".to_string());
        }
        if !seen.insert(format!("c:{id}")) {
            return Err(format!("保存数据无效：分类编号重复 {id}"));
        }
    }
    for prompt in &state.prompts {
        let id = prompt.id.trim();
        if id.is_empty() || id != prompt.id {
            return Err("保存数据无效：存在空的提示词编号".to_string());
        }
        if !seen.insert(format!("p:{id}")) {
            return Err(format!("保存数据无效：提示词编号重复 {id}"));
        }
        for variant in &prompt.variants {
            let id = variant.id.trim();
            if id.is_empty() || id != variant.id {
                return Err("保存数据无效：存在空的场景编号".to_string());
            }
            if !seen.insert(format!("v:{id}")) {
                return Err(format!("保存数据无效：场景编号重复 {id}"));
            }
        }
    }
    Ok(())
}

fn write_state(conn: &Connection, state: &LibraryState) -> Result<(), String> {
    validate_state(state)?;
    if state.categories.is_empty() && state.prompts.is_empty() && !state.seeded {
        return Err("保存数据无效：分类与提示词列表均为空".to_string());
    }
    let tx = conn.unchecked_transaction().map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM variants", [])
        .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM prompts", [])
        .map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM categories", [])
        .map_err(|e| e.to_string())?;

    for (index, category) in state.categories.iter().enumerate() {
        tx.execute(
            "INSERT INTO categories (id, name, system, sort_order) VALUES (?1, ?2, ?3, ?4)",
            params![
                category.id,
                category.name,
                if category.system { 1 } else { 0 },
                index as i64
            ],
        )
        .map_err(|e| e.to_string())?;
    }

    for prompt in &state.prompts {
        tx.execute(
            "INSERT INTO prompts (id, title, category_id, favorite, created_at, updated_at, use_count, tags_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                prompt.id,
                prompt.title,
                prompt.category_id,
                if prompt.favorite { 1 } else { 0 },
                prompt.created_at,
                prompt.updated_at,
                prompt.use_count,
                serde_json::to_string(&prompt.tags).map_err(|e| e.to_string())?
            ],
        )
        .map_err(|e| e.to_string())?;
        for (index, variant) in prompt.variants.iter().enumerate() {
            tx.execute(
                "INSERT INTO variants (id, prompt_id, name, content, sort_order) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![variant.id, prompt.id, variant.name, variant.content, index as i64],
            )
            .map_err(|e| e.to_string())?;
        }
    }

    tx.execute(
        "INSERT INTO settings (key, value) VALUES ('seeded', '1')
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())
}

fn read_setting(conn: &Connection, key: &str) -> Result<Option<String>, String> {
    match conn.query_row(
        "SELECT value FROM settings WHERE key = ?1",
        params![key],
        |row| row.get(0),
    ) {
        Ok(value) => Ok(Some(value)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(err) => Err(err.to_string()),
    }
}

fn write_setting(conn: &Connection, key: &str, value: &str) -> Result<(), String> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

fn lock_db(db: &AppDb) -> Result<std::sync::MutexGuard<'_, Connection>, String> {
    db.conn.lock().map_err(|_| "数据库忙".to_string())
}

#[tauri::command]
fn load_state(db: tauri::State<AppDb>) -> Result<LibraryState, String> {
    let conn = lock_db(&db)?;
    read_state(&conn)
}

#[tauri::command]
fn save_state(db: tauri::State<AppDb>, state: LibraryState) -> Result<(), String> {
    let conn = lock_db(&db)?;
    write_state(&conn, &state)
}

#[tauri::command]
fn get_setting(db: tauri::State<AppDb>, key: String) -> Result<Option<String>, String> {
    let conn = lock_db(&db)?;
    read_setting(&conn, &key)
}

#[tauri::command]
fn set_setting(db: tauri::State<AppDb>, key: String, value: String) -> Result<(), String> {
    let conn = lock_db(&db)?;
    write_setting(&conn, &key, &value)
}

#[tauri::command]
fn save_backup(json: String) -> Result<bool, String> {
    let Some(path) = rfd::FileDialog::new()
        .set_title("导出提示词")
        .set_file_name("CueVault-backup.json")
        .add_filter("JSON", &["json"])
        .save_file()
    else {
        return Ok(false);
    };
    fs::write(path, json).map_err(|e| format!("导出失败：{e}"))?;
    Ok(true)
}

#[cfg(target_os = "windows")]
#[allow(clippy::upper_case_acronyms)]
mod single_instance {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    type HANDLE = *mut std::ffi::c_void;
    type HWND = *mut std::ffi::c_void;
    type BOOL = i32;
    type DWORD = u32;
    type LPCWSTR = *const u16;

    const ERROR_ALREADY_EXISTS: DWORD = 183;
    const SW_RESTORE: i32 = 9;

    #[link(name = "user32")]
    extern "system" {
        fn FindWindowW(lpClassName: LPCWSTR, lpWindowName: LPCWSTR) -> HWND;
        fn SetForegroundWindow(hWnd: HWND) -> BOOL;
        fn ShowWindow(hWnd: HWND, nCmdShow: i32) -> BOOL;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateMutexW(
            lpMutexAttributes: *mut std::ffi::c_void,
            bInitialOwner: BOOL,
            lpName: LPCWSTR,
        ) -> HANDLE;
        fn GetLastError() -> DWORD;
        fn CloseHandle(hObject: HANDLE) -> BOOL;
    }

    pub struct SingleInstanceGuard {
        handle: HANDLE,
    }

    impl Drop for SingleInstanceGuard {
        fn drop(&mut self) {
            if !self.handle.is_null() {
                unsafe { CloseHandle(self.handle) };
            }
        }
    }

    fn to_wide(s: &str) -> Vec<u16> {
        OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn focus_existing_window() {
        let win_title = to_wide("CueVault · 提示词库");
        for _ in 0..50 {
            let hwnd = unsafe { FindWindowW(std::ptr::null(), win_title.as_ptr()) };
            if !hwnd.is_null() {
                unsafe {
                    ShowWindow(hwnd, SW_RESTORE);
                    SetForegroundWindow(hwnd);
                }
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    pub fn acquire_or_focus() -> Result<Option<SingleInstanceGuard>, String> {
        let mutex_name = to_wide("Local\\CueVault_SingleInstance_Mutex_v1");
        let handle = unsafe { CreateMutexW(std::ptr::null_mut(), 1, mutex_name.as_ptr()) };
        if handle.is_null() {
            return Err("无法创建单实例锁，CueVault 已退出。".to_string());
        }
        let err = unsafe { GetLastError() };
        if err == ERROR_ALREADY_EXISTS {
            focus_existing_window();
            unsafe { CloseHandle(handle) };
            return Ok(None);
        }
        Ok(Some(SingleInstanceGuard { handle }))
    }
}

fn show_startup_error(message: &str) {
    rfd::MessageDialog::new()
        .set_title("CueVault · 提示词库")
        .set_description(message)
        .set_level(rfd::MessageLevel::Error)
        .show();
}

fn main() {
    #[cfg(target_os = "windows")]
    let _single_instance_guard = match single_instance::acquire_or_focus() {
        Ok(Some(guard)) => guard,
        Ok(None) => return,
        Err(message) => {
            show_startup_error(&message);
            return;
        }
    };

    let conn = match open_db() {
        Ok(conn) => conn,
        Err(message) => {
            show_startup_error(&message);
            return;
        }
    };
    if let Err(err) = tauri::Builder::default()
        .manage(AppDb {
            conn: Mutex::new(conn),
        })
        .invoke_handler(tauri::generate_handler![
            load_state,
            save_state,
            get_setting,
            set_setting,
            save_backup
        ])
        .setup(|app| {
            let webview_data = webview_dir()?;
            tauri::WebviewWindowBuilder::new(
                app,
                "main",
                tauri::WebviewUrl::App("index.html".into()),
            )
            .title("CueVault · 提示词库")
            .inner_size(1180.0, 800.0)
            .min_inner_size(760.0, 520.0)
            .resizable(true)
            .center()
            .decorations(true)
            .visible(true)
            .data_directory(webview_data)
            .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
    {
        show_startup_error(&format!("CueVault 窗口启动失败：{err}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_roundtrip_keeps_order_and_text() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let state = LibraryState {
            categories: vec![
                Category {
                    id: "all".into(),
                    name: "全部".into(),
                    system: true,
                },
                Category {
                    id: "writing".into(),
                    name: "写作".into(),
                    system: false,
                },
            ],
            prompts: vec![Prompt {
                id: "p1".into(),
                title: "标题".into(),
                tags: vec!["写作".into(), "校对".into(), "润色".into()],
                category_id: "writing".into(),
                variants: vec![
                    Variant {
                        id: "v1".into(),
                        name: "默认".into(),
                        content: "第一段，含中文".into(),
                    },
                    Variant {
                        id: "v2".into(),
                        name: "补充".into(),
                        content: "第二段".into(),
                    },
                ],
                favorite: true,
                created_at: 10,
                updated_at: 20,
                use_count: 3,
            }],
            seeded: false,
        };
        write_state(&conn, &state).unwrap();
        let loaded = read_state(&conn).unwrap();
        assert!(loaded.seeded);
        assert_eq!(loaded.categories[0].id, "all");
        assert!(loaded.categories[0].system);
        assert!(!loaded.categories[1].system);
        assert_eq!(loaded.prompts.len(), 1);
        assert_eq!(loaded.prompts[0].tags, state.prompts[0].tags);
        assert!(loaded.prompts[0].favorite);
        assert_eq!(loaded.prompts[0].variants[1].content, "第二段");
        write_setting(&conn, "theme", "dark").unwrap();
        assert_eq!(
            read_setting(&conn, "theme").unwrap().as_deref(),
            Some("dark")
        );
        let cleared = LibraryState {
            categories: vec![Category {
                id: "all".into(),
                name: "全部".into(),
                system: true,
            }],
            prompts: vec![],
            seeded: true,
        };
        write_state(&conn, &cleared).unwrap();
        let again = read_state(&conn).unwrap();
        assert!(again.seeded);
        let empty_unseeded = LibraryState {
            categories: vec![],
            prompts: vec![],
            seeded: false,
        };
        assert!(write_state(&conn, &empty_unseeded).is_err());
    }

    fn sample_state() -> LibraryState {
        LibraryState {
            categories: vec![
                Category {
                    id: "all".into(),
                    name: "全部".into(),
                    system: true,
                },
                Category {
                    id: "writing".into(),
                    name: "写作".into(),
                    system: false,
                },
            ],
            prompts: vec![Prompt {
                id: "p1".into(),
                title: "标题".into(),
                tags: vec![],
                category_id: "writing".into(),
                variants: vec![
                    Variant {
                        id: "v1".into(),
                        name: "默认".into(),
                        content: "第一段".into(),
                    },
                    Variant {
                        id: "v2".into(),
                        name: "补充".into(),
                        content: "第二段".into(),
                    },
                ],
                favorite: true,
                created_at: 10,
                updated_at: 20,
                use_count: 3,
            }],
            seeded: false,
        }
    }

    #[test]
    fn duplicate_id_keeps_the_saved_library() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let state = sample_state();
        write_state(&conn, &state).unwrap();
        let mut duplicated_prompt = state.clone();
        duplicated_prompt.prompts.push(state.prompts[0].clone());
        assert!(write_state(&conn, &duplicated_prompt).is_err());
        let mut duplicated_variant = state.clone();
        duplicated_variant.prompts[0].variants[1].id = "v1".into();
        assert!(write_state(&conn, &duplicated_variant).is_err());
        let mut blank_id = state.clone();
        blank_id.categories[1].id = "  ".into();
        assert!(write_state(&conn, &blank_id).is_err());
        let loaded = read_state(&conn).unwrap();
        assert_eq!(loaded.prompts.len(), 1);
        assert_eq!(loaded.prompts[0].title, "标题");
        assert_eq!(loaded.prompts[0].variants.len(), 2);
        assert_eq!(loaded.categories[1].name, "写作");
    }

    #[test]
    fn missing_seed_flag_still_returns_existing_rows() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        write_state(&conn, &sample_state()).unwrap();
        conn.execute("DELETE FROM settings", []).unwrap();
        let loaded = read_state(&conn).unwrap();
        assert!(!loaded.seeded);
        assert_eq!(loaded.prompts.len(), 1);
        assert_eq!(loaded.prompts[0].variants[1].content, "第二段");
    }

    #[test]
    fn legacy_database_adds_tags_without_changing_content() {
        let conn = Connection::open_in_memory().unwrap();
        init_db(&conn).unwrap();
        let state = sample_state();
        write_state(&conn, &state).unwrap();
        conn.execute("ALTER TABLE prompts DROP COLUMN tags_json", [])
            .unwrap();
        init_db(&conn).unwrap();
        init_db(&conn).unwrap();
        let loaded = read_state(&conn).unwrap();
        assert!(loaded.prompts[0].tags.is_empty());
        assert_eq!(
            loaded.prompts[0].variants[0].content,
            state.prompts[0].variants[0].content
        );
        assert_eq!(loaded.prompts[0].id, state.prompts[0].id);
        let old_json = serde_json::to_value(&state).unwrap();
        let mut old_json = old_json;
        old_json["prompts"][0]
            .as_object_mut()
            .unwrap()
            .remove("tags");
        let old_state: LibraryState = serde_json::from_value(old_json).unwrap();
        assert!(old_state.prompts[0].tags.is_empty());
    }

    #[test]
    fn file_database_uses_delete_journal() {
        let dir = std::env::temp_dir().join(format!(
            "cuevault-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("prompt-library.db");
        let conn = Connection::open(&path).unwrap();
        init_db(&conn).unwrap();
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .unwrap();
        drop(conn);
        assert_eq!(mode.to_lowercase(), "delete");
        assert!(!path.with_extension("db-wal").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
