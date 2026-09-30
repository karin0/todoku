use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Clone)]
pub struct BotCommand<'a> {
    pub command: &'a str,
    pub description: &'a str,
}

#[derive(Debug, Serialize, Clone)]
#[serde(tag = "type")]
pub enum BotCommandScope {
    #[serde(rename = "chat")]
    Chat { chat_id: i64 },
}

#[derive(Debug, Deserialize, Clone)]
pub struct User {
    pub username: Option<String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Chat {
    pub id: i64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Message {
    #[serde(rename = "message_id")]
    pub id: i64,
    pub chat: Chat,
    pub text: Option<String>,
    pub date: i64,
    #[serde(rename = "message_thread_id")]
    thread_id: Option<i64>,
    #[serde(rename = "is_topic_message", default)]
    in_topic: bool,
}

impl Message {
    /// The topic the message is in. `message_thread_id` also numbers the reply threads
    /// of a group without topics, where nothing can be sent to one, so only a topic
    /// message's counts.
    pub fn topic(&self) -> Option<i64> {
        self.thread_id.filter(|_| self.in_topic)
    }
}

#[derive(Debug, Deserialize, Clone)]
pub struct Update {
    pub update_id: i64,
    pub message: Option<Message>,
}

#[derive(Debug, Deserialize)]
struct ApiResponse<T> {
    ok: bool,
    result: Option<T>,
    description: Option<String>,
}

pub struct Bot {
    api_url: String,
    client: ureq::Agent,
}

impl Bot {
    pub fn new(token: &str, base_url: &str) -> Self {
        let base_url = base_url.trim_end_matches('/');
        let api_url = format!("{base_url}/bot{token}");
        let config = ureq::config::Config::builder()
            .timeout_global(Some(std::time::Duration::from_secs(45)))
            .http_status_as_error(false)
            .build();
        let client = ureq::Agent::new_with_config(config);

        Self { api_url, client }
    }

    fn request<S: Serialize, R: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        body: &S,
    ) -> Result<R> {
        let url = format!("{}/{}", self.api_url, method);
        let mut response = self.client.post(&url).send_json(body)?;

        let api_resp: ApiResponse<R> = response.body_mut().read_json()?;

        if api_resp.ok {
            if let Some(res) = api_resp.result {
                Ok(res)
            } else {
                Err(anyhow!("API ok but missing result"))
            }
        } else {
            let desc = api_resp
                .description
                .unwrap_or_else(|| "Unknown API error".to_string());
            Err(anyhow!(desc))
        }
    }

    pub fn get_me(&self) -> Result<User> {
        self.request("getMe", &())
    }

    pub fn set_commands(
        &self,
        commands: &[BotCommand<'_>],
        scope: Option<&BotCommandScope>,
    ) -> Result<()> {
        #[derive(Serialize)]
        struct SetMyCommands<'a> {
            commands: &'a [BotCommand<'a>],
            #[serde(skip_serializing_if = "Option::is_none")]
            scope: Option<&'a BotCommandScope>,
        }

        let body = SetMyCommands { commands, scope };
        if self.request("setMyCommands", &body)? {
            Ok(())
        } else {
            Err(anyhow!("Unexpected `false` from setMyCommands"))
        }
    }

    pub fn get_updates(&self, offset: Option<i64>, timeout: i64) -> Result<Vec<Update>> {
        #[derive(Serialize)]
        struct GetUpdates {
            #[serde(skip_serializing_if = "Option::is_none")]
            offset: Option<i64>,
            timeout: i64,
            allowed_updates: &'static [&'static str],
        }

        let body = GetUpdates {
            offset,
            timeout,
            allowed_updates: &["message"],
        };
        self.request("getUpdates", &body)
    }

    pub fn send_message(
        &self,
        chat_id: i64,
        topic: Option<i64>,
        text: &str,
        parse_mode: Option<&str>,
        reply_to_message_id: Option<i64>,
    ) -> Result<Message> {
        #[derive(Serialize)]
        struct ReplyParameters {
            message_id: i64,
            allow_sending_without_reply: bool,
        }

        #[derive(Serialize)]
        struct SendMessage<'a> {
            chat_id: i64,
            #[serde(skip_serializing_if = "Option::is_none")]
            message_thread_id: Option<i64>,
            text: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            parse_mode: Option<&'a str>,
            disable_web_page_preview: bool,
            #[serde(skip_serializing_if = "Option::is_none")]
            reply_parameters: Option<ReplyParameters>,
        }

        let body = SendMessage {
            chat_id,
            message_thread_id: topic,
            text,
            parse_mode,
            disable_web_page_preview: true,
            reply_parameters: reply_to_message_id.map(|message_id| ReplyParameters {
                message_id,
                allow_sending_without_reply: true,
            }),
        };
        self.request("sendMessage", &body)
    }

    pub fn edit_message_text(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        parse_mode: Option<&str>,
    ) -> Result<Message> {
        #[derive(Serialize)]
        struct EditMessageText<'a> {
            chat_id: i64,
            message_id: i64,
            text: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            parse_mode: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            disable_web_page_preview: Option<bool>,
        }

        let body = EditMessageText {
            chat_id,
            message_id,
            text,
            parse_mode,
            disable_web_page_preview: Some(true),
        };
        self.request("editMessageText", &body)
    }

    pub fn delete_message(&self, chat_id: i64, message_id: i64) -> Result<()> {
        #[derive(Serialize)]
        struct DeleteMessage {
            chat_id: i64,
            message_id: i64,
        }

        let body = DeleteMessage {
            chat_id,
            message_id,
        };
        let result: bool = self.request("deleteMessage", &body)?;
        if result {
            Ok(())
        } else {
            Err(anyhow!("Unexpected `false` from deleteMessage"))
        }
    }
}
