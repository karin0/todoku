use serde::{Deserialize, Serialize};

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
    ) -> Result<R, Box<dyn std::error::Error>> {
        let url = format!("{}/{}", self.api_url, method);
        let mut response = self.client.post(&url).send_json(body)?;

        let api_resp: ApiResponse<R> = response.body_mut().read_json()?;

        if api_resp.ok {
            if let Some(res) = api_resp.result {
                Ok(res)
            } else {
                Err("API ok but missing result".into())
            }
        } else {
            let desc = api_resp
                .description
                .unwrap_or_else(|| "Unknown API error".to_string());
            Err(desc.into())
        }
    }

    pub fn get_me(&self) -> Result<User, Box<dyn std::error::Error>> {
        self.request("getMe", &())
    }

    pub fn get_updates(
        &self,
        offset: Option<i64>,
        timeout: i64,
    ) -> Result<Vec<Update>, Box<dyn std::error::Error>> {
        #[derive(Serialize)]
        struct GetUpdates {
            #[serde(skip_serializing_if = "Option::is_none")]
            offset: Option<i64>,
            timeout: i64,
        }

        let body = GetUpdates { offset, timeout };
        self.request("getUpdates", &body)
    }

    pub fn send_message(
        &self,
        chat_id: i64,
        text: &str,
        parse_mode: Option<&str>,
        reply_to_message_id: Option<i64>,
    ) -> Result<Message, Box<dyn std::error::Error>> {
        #[derive(Serialize)]
        struct ReplyParameters {
            message_id: i64,
            allow_sending_without_reply: bool,
        }

        #[derive(Serialize)]
        struct SendMessage<'a> {
            chat_id: i64,
            text: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            parse_mode: Option<&'a str>,
            #[serde(skip_serializing_if = "Option::is_none")]
            disable_web_page_preview: Option<bool>,
            #[serde(skip_serializing_if = "Option::is_none")]
            reply_parameters: Option<ReplyParameters>,
        }

        let reply_parameters = reply_to_message_id.map(|id| ReplyParameters {
            message_id: id,
            allow_sending_without_reply: true,
        });

        let body = SendMessage {
            chat_id,
            text,
            parse_mode,
            disable_web_page_preview: Some(true),
            reply_parameters,
        };
        self.request("sendMessage", &body)
    }

    #[cfg(feature = "edit_mode")]
    pub fn edit_message_text(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        parse_mode: Option<&str>,
    ) -> Result<Message, Box<dyn std::error::Error>> {
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

    pub fn delete_message(
        &self,
        chat_id: i64,
        message_id: i64,
    ) -> Result<(), Box<dyn std::error::Error>> {
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
            Err("Unexpected `false` from deleteMessage".into())
        }
    }
}
