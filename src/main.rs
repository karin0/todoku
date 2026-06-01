mod bot;
mod db;

use anyhow::{Result, anyhow};
use bot::{Bot, BotCommand, BotCommandScope, Update};
use rusqlite::Connection;
use std::env;
use std::fmt::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

struct App {
    conn: Connection,
    bot: Bot,
    chat_id: i64,
    username: String,
}

impl App {
    fn render(&self) -> Result<String> {
        let todos = db::get_todos(&self.conn)?;

        // Build list with deep links
        let mut text = format!("<b>{} tasks:</b>\n", todos.len());
        if todos.is_empty() {
            text.push_str("All tasks completed! 🎉");
        } else {
            for (id, task, date) in &todos {
                writeln!(
                    text,
                    "<a href=\"https://t.me/{}?start=done_{id}\">• [{id}] {}\t\t</a> \
                    (<tg-time unix=\"{date}\" format=\"dT\">{date}</tg-time>, \
                    <tg-time unix=\"{date}\" format=\"r\">{date}</tg-time>)",
                    self.username,
                    escape(task)
                )?;
            }
        }

        Ok(text)
    }

    fn send_panel(&self, reply_to_message_id: Option<i64>) -> Result<()> {
        let text = self.render()?;
        let msg = self
            .bot
            .send_message(self.chat_id, &text, Some("HTML"), reply_to_message_id)?;
        let old_panel_id = db::get_panel_id(&self.conn)?;
        db::set_panel_id(&self.conn, msg.id)?;

        if let Some(msg_id) = old_panel_id
            && let Err(e) = self.bot.delete_message(self.chat_id, msg_id)
        {
            eprintln!("Failed to delete old panel {msg_id}: {e}");
        }

        Ok(())
    }

    fn refresh_panel(&self, reply_to_message_id: Option<i64>) -> Result<()> {
        let text = self.render()?;
        if let Some(msg_id) = db::get_panel_id(&self.conn)? {
            match self
                .bot
                .edit_message_text(self.chat_id, msg_id, &text, Some("HTML"))
            {
                Ok(_) => return Ok(()),
                Err(e) => {
                    if e.to_string()
                        .to_ascii_lowercase()
                        .contains("message is not modified")
                    {
                        return Ok(());
                    }
                    eprintln!("Edit failed: {e}. Sending a new message instead.");
                }
            }
        }
        let msg = self
            .bot
            .send_message(self.chat_id, &text, Some("HTML"), reply_to_message_id)?;
        db::set_panel_id(&self.conn, msg.id)?;

        Ok(())
    }

    fn send_usage(&self, reply_to_message_id: Option<i64>) -> Result<()> {
        self.bot.send_message(
            self.chat_id,
            "Welcome to Todoku! 📝\n\n\
                    To add tasks to your todo list, simply type them here. \
                    You can send multiple tasks by separating them with newlines.\n\n\
                    Each task will be shown with a link to complete/delete it.",
            None,
            reply_to_message_id,
        )?;
        Ok(())
    }

    fn handle_update(&self, update: &Update) -> Result<()> {
        let Some(message) = &update.message else {
            return Ok(());
        };

        let chat_id = message.chat.id;
        if chat_id != self.chat_id {
            eprintln!("Unauthorized update from {chat_id}:\n{update:#?}");
            let debug_text = format!("{update:#?}");
            let text = format!("Unauthorized update:\n<pre>{}</pre>", escape(&debug_text));
            self.bot
                .send_message(self.chat_id, &text, Some("HTML"), None)?;
            return Ok(());
        }

        let Some(text) = &message.text else {
            return Ok(());
        };

        let text = text.trim();
        if text.is_empty() {
            return Ok(());
        }

        let reply_to = Some(message.id);

        if let Some(task_id_str) = text.strip_prefix("/start done_") {
            if let Ok(task_id) = task_id_str.parse::<i64>()
                && let Some(task) = db::get_todo(&self.conn, task_id)?
            {
                println!("Completed task: {task_id}: {task}");
                db::delete_todo(&self.conn, task_id, message.date)?;
            } else {
                eprintln!("Bad task ID: {task_id_str}");
            }
        } else if text == "/start" {
            if db::has_todos(&self.conn)? {
                self.send_panel(reply_to)?;
            } else {
                self.send_usage(reply_to)?;
            }
            return Ok(());
        } else if text == "/help" {
            self.send_usage(reply_to)?;
            return Ok(());
        } else {
            // Regular message: split into lines and add each non-empty line as a todo
            for line in text.lines() {
                let line = line.trim();
                if !line.is_empty() {
                    println!("Adding todo: {line}");
                    db::add_todo(&self.conn, line, message.date)?;
                }
            }
        }

        self.refresh_panel(reply_to)?;
        self.bot.delete_message(self.chat_id, message.id)?;
        Ok(())
    }

    fn init(&self) -> Result<Option<i64>> {
        let offset;
        let discarded;

        if let Ok(updates) = self.bot.get_updates(Some(-1), 0)
            && let Some(last) = updates.last()
        {
            let last_id: i64 = last.update_id;
            discarded = updates.len();
            for update in updates {
                eprintln!("Discarded update: {update:#?}");
            }
            offset = Some(last_id + 1);
            println!("Discarded accumulated updates up to id {last_id}");
        } else {
            discarded = 0;
            offset = None;
        }

        let text = if discarded > 0 {
            format!(
                "{} initialized! Discarded {discarded} updates.",
                self.username
            )
        } else {
            format!("{} initialized!", self.username)
        };

        let msg = self.bot.send_message(self.chat_id, &text, None, None)?;
        self.refresh_panel(Some(msg.id))?;
        Ok(offset)
    }
}

fn main() -> Result<()> {
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();
    let active_stream = Arc::new(Mutex::new(None::<std::net::TcpStream>));
    let active_stream_handler = active_stream.clone();
    ctrlc::set_handler(move || {
        if !r.swap(false, Ordering::Relaxed) {
            eprintln!("Exiting...");
            std::process::exit(1);
        }
        eprintln!("Exiting gracefully...");
        let mut guard = active_stream_handler.lock().unwrap();
        if let Some(stream) = guard.take()
            && let Err(e) = stream.shutdown(std::net::Shutdown::Both)
        {
            eprintln!("Failed to shutdown stream: {e}");
        }
    })?;

    let token = env::var("TELEGRAM_BOT_TOKEN")?;
    let chat_id: i64 = env::var("ALLOWED_CHAT_ID")?.parse()?;
    let conn = Connection::open("todo.db")?;
    db::init_db(&conn)?;

    let bot = {
        let api_base = env::var("TELEGRAM_API_BASE_URL")
            .unwrap_or_else(|_| "https://api.telegram.org".to_string());

        Bot::new(&token, &api_base, active_stream)
    };

    // Fetch bot username for deep linking
    let username = bot
        .get_me()?
        .username
        .ok_or_else(|| anyhow!("No username"))?;
    println!("Bot initialized: @{username}");

    // Register commands with Telegram (scoped specifically to the allowed chat)
    bot.set_commands(
        &[
            BotCommand {
                command: "start",
                description: "Start!",
            },
            BotCommand {
                command: "help",
                description: "Show usage",
            },
        ],
        Some(&BotCommandScope::Chat { chat_id }),
    )?;

    let app = App {
        conn,
        bot,
        chat_id,
        username,
    };

    let mut offset = app.init()?;
    let mut retry_delay = Duration::from_secs(2);
    let max_delay = Duration::from_mins(5);

    while running.load(Ordering::Relaxed) {
        match app.bot.get_updates(offset, 30) {
            Ok(updates) => {
                retry_delay = Duration::from_secs(2);
                for update in updates {
                    offset = Some(update.update_id + 1);
                    if let Err(e) = app.handle_update(&update) {
                        eprintln!("Error handling update: {e}");
                    }
                }
            }
            Err(e) => {
                eprintln!("Error during polling: {e}. Retrying in {retry_delay:?}...");
                if !running.load(Ordering::Relaxed) {
                    break;
                }
                std::thread::sleep(retry_delay);
                retry_delay = max_delay.min(retry_delay * 2);
            }
        }
    }
    println!("Shutting down.");
    Ok(())
}
