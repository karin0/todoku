mod bot;
mod db;

use bot::{Bot, Update};
use rusqlite::Connection;
use std::env;
use std::fmt::Write;
use std::time::Duration;

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn refresh_panel(
    conn: &Connection,
    bot: &Bot,
    chat_id: i64,
    username: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let todos = db::get_todos(conn)?;

    // Build list with deep links
    let mut text = format!("<b>{} tasks:</b>\n", todos.len());
    if todos.is_empty() {
        text.push_str("All tasks completed! 🎉");
    } else {
        for (id, task) in &todos {
            let escaped = escape(task);
            writeln!(
                text,
                "• <a href=\"https://t.me/{username}?start=done_{id}\">[{id}] {escaped}</a>"
            )?;
        }
    }

    if let Some(msg_id) = db::get_panel_id(conn)? {
        // Try to edit the existing message
        match bot.edit_message_text(chat_id, msg_id, &text, Some("HTML")) {
            Ok(_) => return Ok(()),
            Err(e) => {
                if e.to_string()
                    .to_ascii_lowercase()
                    .contains("message is not modified")
                {
                    return Ok(());
                }
                // Send new message if edit fails (deleted, expired, etc.)
                eprintln!("Edit failed: {e}. Sending a new message instead.");
            }
        }
    }

    let msg = bot.send_message(chat_id, &text, Some("HTML"))?;
    db::set_panel_id(conn, msg.id)?;

    Ok(())
}

fn handle_update(
    conn: &Connection,
    bot: &Bot,
    update: &Update,
    username: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(message) = &update.message else {
        return Ok(());
    };
    let Some(text) = &message.text else {
        return Ok(());
    };

    let text = text.trim();
    if text.is_empty() {
        return Ok(());
    }

    let chat_id = message.chat.id;

    if let Some(task_id_str) = text.strip_prefix("/start done_") {
        if let Ok(task_id) = task_id_str.parse::<i64>()
            && let Some(task) = db::get_todo(conn, task_id)?
        {
            println!("Completed task: {task_id}: {task}");
            db::delete_todo(conn, task_id)?;
        } else {
            eprintln!("Bad task ID: {task_id_str}");
        }
    } else if text == "/start" {
        bot.send_message(
            chat_id,
            "Welcome to Todoku! 📝\n\n\
             To add tasks to your todo list, simply type them here. \
             You can send multiple tasks by separating them with newlines.\n\n\
             Each task will be shown with a link to complete/delete it.",
            None,
        )?;
        return Ok(());
    } else {
        // Regular message: split into lines and add each non-empty line as a todo
        for line in text.lines() {
            let line = line.trim();
            if !line.is_empty() {
                println!("Adding todo: {line}");
                db::add_todo(conn, line)?;
            }
        }
    }

    refresh_panel(conn, bot, chat_id, username)?;
    if let Err(e) = bot.delete_message(chat_id, message.id) {
        eprintln!("Warning: Failed to delete user's message: {e}");
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = env::var("TELEGRAM_BOT_TOKEN")
        .map_err(|_| "TELEGRAM_BOT_TOKEN environment variable not set")?;

    // Connect to SQLite DB
    let conn = Connection::open("todo.db")?;

    // Initialize DB schemas
    db::init_db(&conn)?;

    let api_base = env::var("TELEGRAM_API_BASE_URL")
        .unwrap_or_else(|_| "https://api.telegram.org".to_string());

    let bot = Bot::new(&token, &api_base);

    // Fetch bot username for deep linking
    let me = bot.get_me()?;
    let username = me.username.ok_or("Bot does not have a username set")?;
    println!("Bot initialized as @{username}");

    let mut offset: Option<i64> = None;
    let mut retry_delay = Duration::from_secs(2);
    let max_delay = Duration::from_mins(5);

    loop {
        match bot.get_updates(offset, 30) {
            Ok(updates) => {
                retry_delay = Duration::from_secs(2);
                for update in updates {
                    offset = Some(update.update_id + 1);
                    if let Err(e) = handle_update(&conn, &bot, &update, &username) {
                        eprintln!("Error handling update: {e}");
                    }
                }
            }
            Err(e) => {
                eprintln!("Error during polling: {e}. Retrying in {retry_delay:?}...");
                std::thread::sleep(retry_delay);
                retry_delay = max_delay.min(retry_delay * 2);
            }
        }
    }
}
