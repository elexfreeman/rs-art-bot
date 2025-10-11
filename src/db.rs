use anyhow::Result;
use tokio_rusqlite::Connection;

#[derive(Clone)]
pub struct Db {
    conn: Connection,
}

impl Db {
/// Открывает (или создаёт) базу SQLite по пути `path` и гарантирует наличие схемы.
    pub async fn open(path: &str) -> Result<Self> {
        let conn = Connection::open(path).await?;
        let db = Self { conn };
        db.init().await?;
        Ok(db)
    }

/// Инициализирует схему БД (идемпотентно):
/// - `config` — ключ/значение, хранит `channel_id`;
/// - `posts`  — лог опубликованных сообщений;
/// - `files`  — реестр обработанных файлов по хэшу SHA-256.
    async fn init(&self) -> Result<()> {
        self.conn
            .call(|conn| {
                conn.execute_batch(
                    r#"
                    PRAGMA journal_mode = WAL;
                    CREATE TABLE IF NOT EXISTS config (
                        key TEXT PRIMARY KEY,
                        value TEXT
                    );
                    CREATE TABLE IF NOT EXISTS posts (
                        id INTEGER PRIMARY KEY AUTOINCREMENT,
                        channel_id INTEGER NOT NULL,
                        message_id INTEGER,
                        file_id TEXT,
                        caption TEXT,
                        created_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
                    );
                    CREATE TABLE IF NOT EXISTS files (
                        hash TEXT PRIMARY KEY,
                        path TEXT,
                        created_at INTEGER NOT NULL DEFAULT (strftime('%s','now'))
                    );
                    "#,
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

/// Возвращает сохранённый `channel_id` из таблицы `config`.
/// Если ещё не задан — вернёт `Ok(None)`.
    pub async fn get_channel_id(&self) -> Result<Option<i64>> {
        let val: Option<String> = self
            .conn
            .call(|conn| {
                let mut stmt = conn.prepare("SELECT value FROM config WHERE key = 'channel_id'")?;
                let mut rows = stmt.query([])?;
                if let Some(row) = rows.next()? {
                    let v: String = row.get(0)?;
                    Ok(Some(v))
                } else {
                    Ok(None)
                }
            })
            .await?;

        Ok(match val {
            Some(s) => s.parse::<i64>().ok(),
            None => None,
        })
    }

/// Обновляет или добавляет `channel_id` в таблицу `config`.
    pub async fn set_channel_id(&self, id: i64) -> Result<()> {
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO config(key, value) VALUES('channel_id', ?1) \
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    [id.to_string()],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

/// Добавляет запись о публикации в таблицу `posts` (для аудита и отладки).
    pub async fn log_post(&self, channel_id: i64, message_id: Option<i64>, file_id: Option<String>, caption: Option<String>) -> Result<()> {
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO posts(channel_id, message_id, file_id, caption) VALUES(?1, ?2, ?3, ?4)",
                    rusqlite::params![channel_id, message_id, file_id, caption],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }

/// Проверяет наличие хэша файла в таблице `files`.
/// Нужна, чтобы пропускать повторную публикацию одного и того же файла.
    pub async fn has_file_hash(&self, hash: &str) -> Result<bool> {
        let h = hash.to_string();
        let exists: bool = self
            .conn
            .call(move |conn| {
                let mut stmt = conn.prepare("SELECT 1 FROM files WHERE hash = ?1 LIMIT 1")?;
                let mut rows = stmt.query([h])?;
                Ok(rows.next()?.is_some())
            })
            .await?;
        Ok(exists)
    }

/// Вставляет хэш файла (и путь) в таблицу `files`.
/// Операция идемпотентна благодаря `INSERT OR IGNORE`.
    pub async fn insert_file_hash(&self, hash: &str, path: &str) -> Result<()> {
        let h = hash.to_string();
        let p = path.to_string();
        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT OR IGNORE INTO files(hash, path) VALUES(?1, ?2)",
                    rusqlite::params![h, p],
                )?;
                Ok(())
            })
            .await?;
        Ok(())
    }
}
