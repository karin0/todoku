mod bot;
mod db;

use anyhow::{Result, anyhow};
use bot::{Bot, BotCommand, BotCommandScope, Update};
use html_escape::encode_text;
use rusqlite::Connection;
use std::env;
use std::fmt::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

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
            for (id, task, date) in todos {
                writeln!(
                    text,
                    "<a href=\"https://t.me/{}?start=done_{id}\">• [{id}] {}\t\t</a> \
                    (<tg-time unix=\"{date}\" format=\"dT\">{date}</tg-time>, \
                    <tg-time unix=\"{date}\" format=\"r\">{date}</tg-time>)",
                    self.username,
                    encode_text(&task),
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
            let text = format!(
                "Unauthorized update:\n<pre>{}</pre>",
                encode_text(&format!("{update:#?}"))
            );
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
        self.bot.delete_message(chat_id, message.id)?;
        Ok(())
    }

    fn new() -> Result<Self> {
        let bot = {
            let token = env::var("TELEGRAM_BOT_TOKEN")?;
            let base_url = env::var("TELEGRAM_API_BASE_URL");
            let base_url = base_url.as_deref().unwrap_or("https://api.telegram.org");
            Bot::new(&token, base_url)
        };
        let chat_id: i64 = env::var("ALLOWED_CHAT_ID")?.parse()?;

        // Fetch bot username for deep linking
        let username = bot
            .get_me()?
            .username
            .ok_or_else(|| anyhow!("No username"))?;

        let db_path = env::var("DB_PATH");
        let db_path = db_path.as_deref().unwrap_or("todo.db");
        let conn = Connection::open(db_path)?;
        db::init_db(&conn)?;

        // Register commands for the allowed chat
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

        Ok(Self {
            conn,
            bot,
            chat_id,
            username,
        })
    }

    fn init(&self) -> Result<Option<i64>> {
        let updates = self.bot.get_updates(Some(-1), 0)?;
        let username = &self.username;
        let (total_todo, total_done) = db::stat(&self.conn)?;

        let offset;
        let info = if let Some(last) = updates.last() {
            let last_id: i64 = last.update_id;
            let discarded = updates.len();
            for update in updates {
                eprintln!("Discarded update: {update:#?}");
            }
            offset = Some(last_id + 1);
            format!(
                "@{username} initialized: {total_todo} tasks, {total_done} done, {discarded} discarded, update_id={last_id}"
            )
        } else {
            offset = None;
            format!("@{username} initialized: {total_todo} tasks, {total_done} done")
        };

        println!("{info}");
        let msg = self.bot.send_message(self.chat_id, &info, None, None)?;
        self.refresh_panel(Some(msg.id))?;
        Ok(offset)
    }
}

#[cfg(unix)]
fn shutdown_all() {
    use std::fs;
    use std::os::{fd::RawFd, raw::c_int};

    unsafe extern "C" {
        fn shutdown(socket: RawFd, how: c_int) -> c_int;
    }

    match fs::read_dir("/proc/self/fd") {
        Ok(entries) => {
            for entry in entries.flatten() {
                if let Some(fd_str) = entry.file_name().to_str()
                    && let Ok(fd) = fd_str.parse::<RawFd>()
                    && unsafe { shutdown(fd, 2) } == 0
                {
                    println!("Shutdown fd: {fd}");
                }
            }
        }
        Err(e) => {
            eprintln!("Failed to read procfs: {e}");
        }
    }
}

static RUNNING: AtomicBool = AtomicBool::new(true);

fn main() -> Result<()> {
    ctrlc::set_handler(move || {
        if !RUNNING.swap(false, Ordering::Relaxed) {
            eprintln!("Exiting...");
            std::process::exit(1);
        }
        eprintln!("Exiting gracefully...");
        #[cfg(unix)]
        shutdown_all();
    })?;

    let app = App::new()?;
    let mut offset = app.init()?;
    let mut retry_delay = Duration::from_secs(2);
    let max_delay = Duration::from_mins(5);

    while RUNNING.load(Ordering::Relaxed) {
        match app.bot.get_updates(offset, 30) {
            Ok(updates) => {
                retry_delay = Duration::from_secs(2);
                if let Some(last) = updates.last() {
                    offset = Some(last.update_id + 1);
                    for update in updates {
                        if let Err(e) = app.handle_update(&update) {
                            eprintln!("Error handling update: {update:#?}: {e}");
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("Error during polling: {e}. Retrying in {retry_delay:?}...");
                if !RUNNING.load(Ordering::Relaxed) {
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
