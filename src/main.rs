mod db;

use anyhow::{Result, anyhow};
use html_escape::encode_text;
use jiff::Timestamp;
use jiff::tz::TimeZone;
use kuriero::{
    AnswerCallbackQuery, BotCommand, CallbackQuery, Client, CommandScope, Content, DeleteMessage,
    EditMessageText, GetMe, LinkPreviewOptions, Message, ParseMode, ReplyParameters, RichInput,
    SendMessage, SendRichMessage, SetMyCommands, Update,
};
use rusqlite::Connection;
use std::env;
use std::fmt::Write;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

struct App {
    conn: Connection,
    client: Client,
    chat_id: i64,
    /// `@` and the bot's username, which a client appends to a command sent in a group.
    mention: String,
    /// Where the fallback text of each task's time is written, for clients that show a
    /// date-time entity in a rich message as that text.
    tz: TimeZone,
}

impl App {
    fn render(&self, topic: Option<i64>) -> Result<String> {
        let todos = db::get_todos(&self.conn, topic)?;

        let mut text = format!("<h3>{} tasks</h3><p>", todos.len());
        if todos.is_empty() {
            text.push_str("All tasks completed! 🎉");
        }
        for (i, (id, task, date)) in todos.into_iter().enumerate() {
            if i > 0 {
                text.push_str("<br>");
            }
            let local = Timestamp::from_second(date)?
                .to_zoned(self.tz.clone())
                .strftime("%m-%d %H:%M");
            write!(
                text,
                "• <tg-button type=\"callback_data\" style=\"link\" data=\"done_{id}\">{}</tg-button> \
                · <tg-time unix=\"{date}\" format=\"r\">{local}</tg-time>",
                encode_text(&task),
            )?;
        }
        text.push_str("</p>");

        Ok(text)
    }

    fn send_panel(&self, topic: Option<i64>, reply_to_message_id: Option<i64>) -> Result<()> {
        let text = self.render(topic)?;
        let msg = self.client.send(&SendRichMessage {
            reply_parameters: replying(reply_to_message_id),
            ..SendRichMessage::new(self.chat_id, topic, RichInput::Html(&text))
        })?;
        let old_panel_id = db::get_panel_id(&self.conn, topic)?;
        db::set_panel_id(&self.conn, topic, msg.id)?;

        if let Some(msg_id) = old_panel_id
            && let Err(e) = self.client.send(&DeleteMessage {
                chat_id: self.chat_id,
                message_id: msg_id,
            })
        {
            eprintln!("Failed to delete old panel {msg_id}: {e}");
        }

        Ok(())
    }

    fn refresh_panel(&self, topic: Option<i64>, reply_to_message_id: Option<i64>) -> Result<()> {
        if let Some(msg_id) = db::get_panel_id(&self.conn, topic)? {
            let text = self.render(topic)?;
            let edit = EditMessageText {
                chat_id: self.chat_id,
                message_id: msg_id,
                content: Content::Rich(RichInput::Html(&text)),
                reply_markup: None,
            };
            match self.client.send(&edit) {
                Ok(_) => return Ok(()),
                Err(kuriero::Error::Rejected { description, .. })
                    if description.contains("message is not modified") =>
                {
                    return Ok(());
                }
                Err(e) => eprintln!("Edit failed: {e}. Sending a new panel instead."),
            }
        }
        self.send_panel(topic, reply_to_message_id)
    }

    fn send_usage(&self, topic: Option<i64>, reply_to_message_id: Option<i64>) -> Result<()> {
        self.say(SendMessage {
            reply_parameters: replying(reply_to_message_id),
            ..SendMessage::new(
                self.chat_id,
                topic,
                "Welcome to Todoku! 📝\n\n\
                    To add tasks to your todo list, simply type them here. \
                    You can send multiple tasks by separating them with newlines.\n\n\
                    Tap a task to complete it. Each topic keeps its own list.",
            )
        })
    }

    fn report_unauthorized(&self, chat_id: i64, update: &Update) -> Result<()> {
        eprintln!("Unauthorized update from {chat_id}:\n{update:#?}");
        let text = format!(
            "Unauthorized update:\n<pre>{}</pre>",
            encode_text(&format!("{update:#?}"))
        );
        self.say(SendMessage {
            parse_mode: Some(ParseMode::Html),
            ..SendMessage::new(self.chat_id, None, &text)
        })
    }

    fn handle_update(&self, update: &Update) -> Result<()> {
        if let Some(message) = &update.message {
            if message.chat.id == self.chat_id {
                self.handle_message(message)?;
            } else {
                self.report_unauthorized(message.chat.id, update)?;
            }
        }
        if let Some(query) = &update.callback_query {
            match &query.message {
                Some(message) if message.chat.id != self.chat_id => {
                    self.report_unauthorized(message.chat.id, update)?;
                }
                Some(_) => self.handle_callback(query)?,
                None => {}
            }
            self.client.send(&AnswerCallbackQuery {
                callback_query_id: &query.id,
            })?;
        }
        Ok(())
    }

    fn handle_callback(&self, query: &CallbackQuery) -> Result<()> {
        let data = query.data.as_deref().unwrap_or_default();
        if let Some(task_id) = data.strip_prefix("done_").and_then(|id| id.parse().ok())
            && let Some((task, topic)) =
                db::delete_todo(&self.conn, task_id, Timestamp::now().as_second())?
        {
            println!("Completed task: {task_id}: {task}");
            self.refresh_panel(topic, None)?;
        } else {
            eprintln!("Bad callback data: {data}");
        }
        Ok(())
    }

    fn handle_message(&self, message: &Message) -> Result<()> {
        let Some(text) = &message.text else {
            return Ok(());
        };

        let text = text.trim();
        if text.is_empty() {
            return Ok(());
        }

        let topic = message.topic();
        let reply_to = Some(message.id);
        let command = if text.starts_with('/') {
            text.strip_suffix(self.mention.as_str()).unwrap_or(text)
        } else {
            text
        };

        match command {
            "/start" => {
                if db::has_todos(&self.conn, topic)? {
                    self.send_panel(topic, reply_to)?;
                } else {
                    self.send_usage(topic, reply_to)?;
                }
                return Ok(());
            }
            "/help" => {
                self.send_usage(topic, reply_to)?;
                return Ok(());
            }
            _ => {
                // Regular message: split into lines and add each non-empty line as a todo
                for line in text.lines() {
                    let line = line.trim();
                    if !line.is_empty() {
                        println!("Adding todo in topic {topic:?}: {line}");
                        db::add_todo(&self.conn, line, message.date, topic)?;
                    }
                }
            }
        }

        self.refresh_panel(topic, reply_to)?;
        self.client.send(&DeleteMessage {
            chat_id: self.chat_id,
            message_id: message.id,
        })?;
        Ok(())
    }

    fn say(&self, message: SendMessage<'_>) -> Result<()> {
        self.client.send(&SendMessage {
            link_preview_options: Some(LinkPreviewOptions { is_disabled: true }),
            ..message
        })?;
        Ok(())
    }

    fn new() -> Result<Self> {
        let client = {
            let token = env::var("TELEGRAM_BOT_TOKEN")?;
            let base_url = env::var("TELEGRAM_API_BASE_URL");
            let base_url = base_url.as_deref().unwrap_or("https://api.telegram.org");
            Client::new(base_url, &token)
        };
        let chat_id: i64 = env::var("ALLOWED_CHAT_ID")?.parse()?;

        let username = client
            .send(&GetMe)?
            .username
            .ok_or_else(|| anyhow!("No username"))?;

        let db_path = env::var("DB_PATH");
        let db_path = db_path.as_deref().unwrap_or("todo.db");
        let conn = Connection::open(db_path)?;
        db::init_db(&conn)?;

        // Register commands for the allowed chat
        client.send(&SetMyCommands {
            commands: &[
                BotCommand {
                    command: "start",
                    description: "Start!",
                },
                BotCommand {
                    command: "help",
                    description: "Show usage",
                },
            ],
            scope: CommandScope::Chat { chat_id },
        })?;

        Ok(Self {
            conn,
            client,
            chat_id,
            mention: format!("@{username}"),
            tz: TimeZone::system(),
        })
    }

    fn init(&self) -> Result<i64> {
        let updates = self.client.get_updates(-1, 0, false)?;
        let mention = &self.mention;
        let (total_todo, total_done) = db::stat(&self.conn)?;

        let offset;
        let info = if let Some(last) = updates.last() {
            let last_id: i64 = last.id;
            let discarded = updates.len();
            for update in updates {
                eprintln!("Discarded update: {update:#?}");
            }
            offset = last_id + 1;
            format!(
                "{mention} initialized: {total_todo} tasks, {total_done} done, {discarded} discarded, update_id={last_id}"
            )
        } else {
            offset = 0;
            format!("{mention} initialized: {total_todo} tasks, {total_done} done")
        };

        println!("{info}");
        self.say(SendMessage::new(self.chat_id, None, &info))?;
        for topic in db::get_panel_topics(&self.conn)? {
            self.refresh_panel(topic, None)?;
        }
        Ok(offset)
    }
}

fn replying(message_id: Option<i64>) -> Option<ReplyParameters> {
    message_id.map(|message_id| ReplyParameters {
        message_id,
        allow_sending_without_reply: true,
    })
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
                    && fd > 2
                    && unsafe { shutdown(fd, 2) } == 0
                {
                    // Skip stdio to avoid breaking systemd journal sockets
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
            eprintln!("Aborting.");
            std::process::exit(1);
        }
        eprintln!("Exiting...");
        #[cfg(unix)]
        shutdown_all();
    })?;

    let app = App::new()?;
    let mut offset = app.init()?;
    let mut retry_delay = Duration::from_secs(2);
    let max_delay = Duration::from_mins(5);

    while RUNNING.load(Ordering::Relaxed) {
        match app.client.get_updates(offset, 30, false) {
            Ok(updates) => {
                retry_delay = Duration::from_secs(2);
                if let Some(last) = updates.last() {
                    offset = last.id + 1;
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
