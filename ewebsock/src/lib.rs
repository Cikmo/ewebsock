//! A [`WebSocket`](https://en.wikipedia.org/wiki/WebSocket) client library that can be compiled to both native and the web (WASM).
//!
//! Usage:
//! ``` no_run
//! let options = ewebsock::Options::default();
//! let (mut sender, receiver) = ewebsock::connect("ws://example.com", options).unwrap();
//! sender.send(ewebsock::WsMessage::Text("Hello!".into()));
//! while let Some(event) = receiver.try_recv() {
//!     println!("Received {:?}", event);
//! }
//! ```
//!
//! ## Feature flags
#![doc = document_features::document_features!()]
//!

#![warn(missing_docs)] // let's keep ewebsock well-documented

use std::ops::ControlFlow;


mod native_tungstenite_tokio;
pub use native_tungstenite_tokio::*;

mod tungstenite_common;

// ----------------------------------------------------------------------------

/// A web-socket message.
#[derive(Clone, Debug)]
pub enum WsMessage {
    /// Binary message.
    Binary(Vec<u8>),

    /// Text message.
    Text(String),

    /// Incoming message of unknown type.
    /// You cannot send these.
    Unknown(String),

    /// Only for native.
    Ping(Vec<u8>),

    /// Only for native.
    Pong(Vec<u8>),
}

/// Something happening with the connection.
#[derive(Clone, Debug)]
pub enum WsEvent {
    /// The connection has been established, and you can start sending messages.
    Opened,

    /// A message has been received.
    Message(WsMessage),

    /// An error occurred.
    Error(String),

    /// The connection has been closed.
    Closed,
}



/// An error.
pub type Error = String;

/// Short for `Result<T, ewebsock::Error>`.
pub type Result<T> = std::result::Result<T, Error>;

pub(crate) type EventHandler = Box<dyn Send + Fn(WsEvent) -> ControlFlow<()>>;

/// Options for a connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// The maximum size of a single incoming message frame, in bytes.
    ///
    /// The primary reason for setting this to something other than [`usize::MAX`] is
    /// to prevent a malicious server from eating up all your RAM.
    ///
    /// Ignored on Web.
    pub max_incoming_frame_size: usize,

    /// Delay blocking in ms - default 10ms
    pub delay_blocking: std::time::Duration,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            max_incoming_frame_size: 64 * 1024 * 1024,
            delay_blocking: std::time::Duration::from_millis(10),
        }
    }
}

/// Connect to the given URL, and return a sender and receiver.
///
/// If `on_event` returns [`ControlFlow::Break`], the connection will be closed
/// without calling `on_event` again.
///
/// This is a wrapper around [`ws_connect`].
///
/// # Errors
/// * On native: failure to spawn a thread.
/// * On web: failure to use `WebSocket` API.
///
/// See also the [`connect_with_wakeup`] function,
/// and the more advanced [`ws_connect`].
pub fn connect(url: impl Into<String>, options: Options) -> Result<(WsSender, WsReceiver)> {
    let (ws_receiver, on_event) = WsReceiver::new();
    let ws_sender = ws_connect(url.into(), options, on_event)?;
    Ok((ws_sender, ws_receiver))
}

/// Like [`connect`], but will call the given wake-up function on each incoming event.
///
/// This allows you to wake up the UI thread, for instance.
///
/// If `on_event` returns [`ControlFlow::Break`], the connection will be closed
/// without calling `on_event` again.
///
/// This is a wrapper around [`ws_connect`].
///
/// # Errors
/// * On native: failure to spawn a thread.
/// * On web: failure to use `WebSocket` API.
///
/// Note that you have to wait for [`WsEvent::Opened`] before sending messages.
pub fn connect_with_wakeup(
    url: impl Into<String>,
    options: Options,
    wake_up: impl Fn() + Send + Sync + 'static,
) -> Result<(WsSender, WsReceiver)> {
    let (receiver, on_event) = WsReceiver::new_with_callback(wake_up);
    let sender = ws_connect(url.into(), options, on_event)?;
    Ok((sender, receiver))
}

/// Connect and call the given event handler on each received event.
///
/// If `on_event` returns [`ControlFlow::Break`], the connection will be closed
/// without calling `on_event` again.
///
/// See [`crate::connect`] for a more high-level version.
///
/// # Errors
/// * On native: failure to spawn a thread.
/// * On web: failure to use `WebSocket` API.
pub fn ws_connect(url: String, options: Options, on_event: EventHandler) -> Result<WsSender> {
    ws_connect_impl(url, options, on_event)
}

/// Connect and call the given event handler on each received event.
///
/// This is like [`ws_connect`], but it doesn't return a [`WsSender`],
/// so it can only receive messages, not send them.
///
/// This can be slightly more efficient when you don't need to send messages.
///
/// If `on_event` returns [`ControlFlow::Break`], the connection will be closed
/// without calling `on_event` again.
///
/// # Errors
/// * On native: failure to spawn receiver thread.
/// * On web: failure to use `WebSocket` API.
pub fn ws_receive(url: String, options: Options, on_event: EventHandler) -> Result<()> {
    ws_receive_impl(url, options, on_event)
}
