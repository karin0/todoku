use rusqlite::{Connection, OptionalExtension, Result};

pub fn init_db(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS Todo (
            id    INTEGER PRIMARY KEY NOT NULL,
            text  TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS Panel (
            id         INTEGER PRIMARY KEY DEFAULT 1 CHECK (id = 1),
            message_id INTEGER NOT NULL
        );",
    )?;
    Ok(())
}

pub fn add_todo(conn: &Connection, text: &str) -> Result<()> {
    conn.execute("INSERT INTO Todo (text) VALUES (?1);", [text])?;
    Ok(())
}

pub fn get_todos(conn: &Connection) -> Result<Vec<(i64, String)>> {
    let mut stmt = conn.prepare("SELECT id, text FROM Todo ORDER BY id;")?;
    stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect()
}

pub fn get_todo(conn: &Connection, task_id: i64) -> Result<Option<String>> {
    conn.query_row("SELECT text FROM Todo WHERE id = ?1;", [task_id], |row| {
        row.get(0)
    })
    .optional()
}

pub fn delete_todo(conn: &Connection, task_id: i64) -> Result<()> {
    conn.execute("DELETE FROM Todo WHERE id = ?1;", [task_id])?;
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
