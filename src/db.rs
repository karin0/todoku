use rusqlite::{Connection, OptionalExtension, Result};

/// Telegram numbers a topic by the message that opened it, so 0 names the part of the
/// chat outside every topic.
fn key(topic: Option<i64>) -> i64 {
    topic.unwrap_or(0)
}

fn topic_of(key: i64) -> Option<i64> {
    (key != 0).then_some(key)
}

pub fn init_db(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS Todo (
            id         INTEGER PRIMARY KEY NOT NULL,
            text       TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            topic      INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS Done (
            id         INTEGER PRIMARY KEY NOT NULL,
            todo_id    INTEGER NOT NULL,
            text       TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            done_at    INTEGER NOT NULL,
            topic      INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS Panel (
            topic      INTEGER PRIMARY KEY NOT NULL,
            message_id INTEGER NOT NULL
        );",
    )?;
    Ok(())
}

pub fn add_todo(conn: &Connection, text: &str, created_at: i64, topic: Option<i64>) -> Result<()> {
    conn.execute(
        "INSERT INTO Todo (text, created_at, topic) VALUES (?1, ?2, ?3);",
        rusqlite::params![text, created_at, key(topic)],
    )?;
    Ok(())
}

pub fn has_todos(conn: &Connection, topic: Option<i64>) -> Result<bool> {
    conn.query_one(
        "SELECT EXISTS (SELECT 1 FROM Todo WHERE topic = ?1);",
        [key(topic)],
        |row| row.get(0),
    )
}

pub fn get_todos(conn: &Connection, topic: Option<i64>) -> Result<Vec<(i64, String, i64)>> {
    let mut stmt =
        conn.prepare("SELECT id, text, created_at FROM Todo WHERE topic = ?1 ORDER BY id;")?;
    stmt.query_map([key(topic)], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?))
    })?
    .collect()
}

pub fn stat(conn: &Connection) -> Result<(i64, i64)> {
    let total_todo = conn.query_one("SELECT COUNT(*) FROM Todo;", [], |row| row.get(0))?;
    let total_done = conn.query_one("SELECT COUNT(*) FROM Done;", [], |row| row.get(0))?;
    Ok((total_todo, total_done))
}

/// Moves a task to `Done`, returning its text and topic, or `None` for a task that is
/// already gone.
pub fn delete_todo(
    conn: &Connection,
    task_id: i64,
    done_at: i64,
) -> Result<Option<(String, Option<i64>)>> {
    let tx = conn.unchecked_transaction()?;
    let Some((text, created_at, topic)): Option<(String, i64, i64)> = tx
        .query_one(
            "DELETE FROM Todo WHERE id = ?1 RETURNING text, created_at, topic;",
            [task_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?
    else {
        return Ok(None);
    };
    tx.execute(
        "INSERT INTO Done (todo_id, text, created_at, done_at, topic) VALUES (?1, ?2, ?3, ?4, ?5);",
        rusqlite::params![task_id, text, created_at, done_at, topic],
    )?;
    tx.commit()?;
    Ok(Some((text, topic_of(topic))))
}

pub fn get_panel_topics(conn: &Connection) -> Result<Vec<Option<i64>>> {
    let mut stmt = conn.prepare("SELECT topic FROM Panel ORDER BY topic;")?;
    stmt.query_map([], |row| row.get(0).map(topic_of))?
        .collect()
}

pub fn get_panel_id(conn: &Connection, topic: Option<i64>) -> Result<Option<i64>> {
    conn.query_one(
        "SELECT message_id FROM Panel WHERE topic = ?1;",
        [key(topic)],
        |row| row.get(0),
    )
    .optional()
}

pub fn set_panel_id(conn: &Connection, topic: Option<i64>, msg_id: i64) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO Panel (topic, message_id) VALUES (?1, ?2);",
        [key(topic), msg_id],
    )?;
    Ok(())
}
