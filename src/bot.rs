use kuriero::{Client, Error, Sent, Update, User};
use serde::Serialize;
use serde::de::IgnoredAny;

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

#[derive(Serialize)]
struct InputRichMessage<'a> {
    html: &'a str,
}

#[derive(Serialize)]
struct ReplyParameters {
    message_id: i64,
    allow_sending_without_reply: bool,
}

impl ReplyParameters {
    fn to(message_id: Option<i64>) -> Option<Self> {
        message_id.map(|message_id| Self {
            message_id,
            allow_sending_without_reply: true,
        })
    }
}

pub struct Bot {
    client: Client,
}

impl Bot {
    pub fn new(token: &str, base_url: &str) -> Self {
        Self {
            client: Client::new(base_url, token),
        }
    }

    pub fn get_me(&self) -> Result<User, Error> {
        self.client.call("getMe", &())
    }

    pub fn set_commands(
        &self,
        commands: &[BotCommand<'_>],
        scope: Option<&BotCommandScope>,
    ) -> Result<(), Error> {
        #[derive(Serialize)]
        struct SetMyCommands<'a> {
            commands: &'a [BotCommand<'a>],
            #[serde(skip_serializing_if = "Option::is_none")]
            scope: Option<&'a BotCommandScope>,
        }

        let body = SetMyCommands { commands, scope };
        self.client
            .call::<IgnoredAny>("setMyCommands", &body)
            .map(drop)
    }

    pub fn get_updates(&self, offset: i64, timeout: u64) -> Result<Vec<Update>, Error> {
        self.client.get_updates(offset, timeout, false)
    }

    pub fn send_message(
        &self,
        chat_id: i64,
        topic: Option<i64>,
        text: &str,
        parse_mode: Option<&str>,
        reply_to_message_id: Option<i64>,
    ) -> Result<Sent, Error> {
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
            reply_parameters: ReplyParameters::to(reply_to_message_id),
        };
        self.client.call("sendMessage", &body)
    }

    pub fn send_rich_message(
        &self,
        chat_id: i64,
        topic: Option<i64>,
        html: &str,
        reply_to_message_id: Option<i64>,
    ) -> Result<Sent, Error> {
        #[derive(Serialize)]
        struct SendRichMessage<'a> {
            chat_id: i64,
            #[serde(skip_serializing_if = "Option::is_none")]
            message_thread_id: Option<i64>,
            rich_message: InputRichMessage<'a>,
            #[serde(skip_serializing_if = "Option::is_none")]
            reply_parameters: Option<ReplyParameters>,
        }

        let body = SendRichMessage {
            chat_id,
            message_thread_id: topic,
            rich_message: InputRichMessage { html },
            reply_parameters: ReplyParameters::to(reply_to_message_id),
        };
        self.client.call("sendRichMessage", &body)
    }

    pub fn edit_rich_message(
        &self,
        chat_id: i64,
        message_id: i64,
        html: &str,
    ) -> Result<(), Error> {
        #[derive(Serialize)]
        struct EditMessageText<'a> {
            chat_id: i64,
            message_id: i64,
            rich_message: InputRichMessage<'a>,
        }

        let body = EditMessageText {
            chat_id,
            message_id,
            rich_message: InputRichMessage { html },
        };
        self.client
            .call::<IgnoredAny>("editMessageText", &body)
            .map(drop)
    }

    pub fn answer_callback_query(&self, callback_query_id: &str) -> Result<(), Error> {
        #[derive(Serialize)]
        struct AnswerCallbackQuery<'a> {
            callback_query_id: &'a str,
        }

        let body = AnswerCallbackQuery { callback_query_id };
        self.client
            .call::<IgnoredAny>("answerCallbackQuery", &body)
            .map(drop)
    }

    pub fn delete_message(&self, chat_id: i64, message_id: i64) -> Result<(), Error> {
        #[derive(Serialize)]
        struct DeleteMessage {
            chat_id: i64,
            message_id: i64,
        }

        let body = DeleteMessage {
            chat_id,
            message_id,
        };
        self.client
            .call::<IgnoredAny>("deleteMessage", &body)
            .map(drop)
    }
}
