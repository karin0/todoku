use rusqlite::{Connection, OptionalExtension, Result};

pub fn init_db(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS Todo (
            id         INTEGER PRIMARY KEY NOT NULL,
            text       TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS Done (
            id         INTEGER PRIMARY KEY NOT NULL,
            todo_id    INTEGER NOT NULL,
            text       TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            done_at    INTEGER NOT NULL
        );
        CREATE TABLE IF NOT EXISTS Panel (
            id         INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
            message_id INTEGER NOT NULL
        );",
    )?;
    Ok(())
}

pub fn add_todo(conn: &Connection, text: &str, created_at: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO Todo (text, created_at) VALUES (?1, ?2);",
        rusqlite::params![text, created_at],
    )?;
    Ok(())
}

pub fn get_todos(conn: &Connection) -> Result<Vec<(i64, String, i64)>> {
    let mut stmt = conn.prepare("SELECT id, text, created_at FROM Todo ORDER BY id;")?;
    stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect()
}

pub fn get_todo(conn: &Connection, task_id: i64) -> Result<Option<String>> {
    conn.query_row("SELECT text FROM Todo WHERE id = ?1;", [task_id], |row| {
        row.get(0)
    })
    .optional()
}

pub fn delete_todo(conn: &Connection, task_id: i64, done_at: i64) -> Result<()> {
    let tx = conn.unchecked_transaction()?;

    let (text, created_at): (String, i64) = tx.query_row(
        "SELECT text, created_at FROM Todo WHERE id = ?1;",
        [task_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    tx.execute(
        "INSERT INTO Done (todo_id, text, created_at, done_at) VALUES (?1, ?2, ?3, ?4);",
        rusqlite::params![task_id, text, created_at, done_at],
    )?;
    tx.execute("DELETE FROM Todo WHERE id = ?1;", [task_id])?;

    tx.commit()?;
    Ok(())
}

pub fn get_panel_id(conn: &Connection) -> Result<Option<i64>> {
    conn.query_row("SELECT message_id FROM Panel WHERE id = 1;", [], |row| {
        row.get(0)
    })
    .optional()
}

pub fn set_panel_id(conn: &Connection, msg_id: i64) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO Panel (id, message_id) VALUES (1, ?1);",
        [msg_id],
    )?;
    Ok(())
}
