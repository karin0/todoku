use anyhow::{Result, anyhow};
use serde::{Deserialize, Serialize};

use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::{fmt, io};
use ureq::unversioned::resolver::DefaultResolver;
use ureq::unversioned::transport::{
    Buffers, ConnectionDetails, Connector, Either, LazyBuffers, NextTimeout, Transport,
};

#[cfg(feature = "tls")]
use ureq::unversioned::transport::RustlsConnector;

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
    pub fn new(token: &str, base_url: &str, active_stream: Arc<Mutex<Option<TcpStream>>>) -> Self {
        let base_url = base_url.trim_end_matches('/');
        let api_url = format!("{base_url}/bot{token}");
        let config = ureq::config::Config::builder()
            .timeout_global(Some(std::time::Duration::from_secs(45)))
            .http_status_as_error(false)
            .build();

        let tcp_connector = InterruptibleTcpConnector::new(active_stream);

        #[cfg(feature = "tls")]
        let connector = ().chain(tcp_connector).chain(RustlsConnector::default());

        #[cfg(not(feature = "tls"))]
        let connector = ().chain(tcp_connector);

        let resolver = DefaultResolver::default();
        let client = ureq::Agent::with_parts(config, connector, resolver);

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
    ) -> Result<Message> {
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

#[derive(Clone, Default)]
pub struct InterruptibleTcpConnector {
    active_stream: Arc<Mutex<Option<TcpStream>>>,
}

impl fmt::Debug for InterruptibleTcpConnector {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InterruptibleTcpConnector").finish()
    }
}

impl InterruptibleTcpConnector {
    pub fn new(active_stream: Arc<Mutex<Option<TcpStream>>>) -> Self {
        Self { active_stream }
    }
}

impl<In: Transport> Connector<In> for InterruptibleTcpConnector {
    type Out = Either<In, InterruptibleTcpTransport>;

    fn connect(
        &self,
        details: &ConnectionDetails,
        chained: Option<In>,
    ) -> Result<Option<Self::Out>, ureq::Error> {
        if let Some(transport) = chained {
            return Ok(Some(Either::A(transport)));
        }

        let mut last_err = None;
        let mut stream = None;

        for addr in &details.addrs {
            let conn_res = if let Some(dur) = details.timeout.not_zero() {
                TcpStream::connect_timeout(addr, *dur)
            } else {
                TcpStream::connect(addr)
            };

            match conn_res {
                Ok(s) => {
                    stream = Some(s);
                    break;
                }
                Err(e) => last_err = Some(e),
            }
        }

        let Some(stream) = stream else {
            let err = last_err.unwrap_or_else(|| {
                io::Error::new(io::ErrorKind::AddrNotAvailable, "no addresses resolved")
            });
            return Err(ureq::Error::Io(err));
        };

        if details.config.no_delay() {
            stream.set_nodelay(true).map_err(ureq::Error::Io)?;
        }

        let clone = stream.try_clone().map_err(ureq::Error::Io)?;
        *self.active_stream.lock().unwrap() = Some(clone);

        let buffers = LazyBuffers::new(
            details.config.input_buffer_size(),
            details.config.output_buffer_size(),
        );
        Ok(Some(Either::B(InterruptibleTcpTransport::new(
            stream, buffers,
        ))))
    }
}

#[derive(Debug)]
pub struct InterruptibleTcpTransport {
    stream: TcpStream,
    buffers: LazyBuffers,
}

impl InterruptibleTcpTransport {
    pub fn new(stream: TcpStream, buffers: LazyBuffers) -> Self {
        Self { stream, buffers }
    }
}

impl Transport for InterruptibleTcpTransport {
    fn buffers(&mut self) -> &mut dyn Buffers {
        &mut self.buffers
    }

    fn transmit_output(&mut self, amount: usize, timeout: NextTimeout) -> Result<(), ureq::Error> {
        self.stream
            .set_write_timeout(timeout.not_zero().map(|d| *d))
            .map_err(ureq::Error::Io)?;
        let output = &self.buffers.output()[..amount];
        io::Write::write_all(&mut self.stream, output).map_err(|e| {
            if e.kind() == io::ErrorKind::TimedOut {
                ureq::Error::Timeout(timeout.reason)
            } else {
                ureq::Error::Io(e)
            }
        })
    }

    fn await_input(&mut self, timeout: NextTimeout) -> Result<bool, ureq::Error> {
        self.stream
            .set_read_timeout(timeout.not_zero().map(|d| *d))
            .map_err(ureq::Error::Io)?;
        let input = self.buffers.input_append_buf();
        let amount = io::Read::read(&mut self.stream, input).map_err(|e| {
            if e.kind() == io::ErrorKind::TimedOut {
                ureq::Error::Timeout(timeout.reason)
            } else {
                ureq::Error::Io(e)
            }
        })?;
        self.buffers.input_appended(amount);
        Ok(amount > 0)
    }

    fn is_open(&mut self) -> bool {
        self.stream.peer_addr().is_ok()
    }
}
