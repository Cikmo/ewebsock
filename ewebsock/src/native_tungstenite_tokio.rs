use std::ops::ControlFlow;

use crate::{EventHandler, Options, Result, WsEvent, WsMessage};

/// This is how you send [`WsMessage`]s to the server.
///
/// When this is dropped, the connection is closed.
pub struct WsSender {
    tx: Option<tokio::sync::mpsc::Sender<WsMessage>>,
}

impl Drop for WsSender {
    fn drop(&mut self) {
        self.close();
    }
}

impl WsSender {
    /// Send a message.
    ///
    /// You have to wait for [`WsEvent::Opened`] before you can start sending messages.
    pub fn send(&mut self, msg: WsMessage) {
        if let Some(tx) = self.tx.clone() {
            tokio::spawn(async move { tx.send(msg).await });
        }
    }

    /// Close the connection.
    ///
    /// This is called automatically when the sender is dropped.
    pub fn close(&mut self) {
        if self.tx.is_some() {
            log::debug!("Closing WebSocket");
        }
        self.tx = None;
    }

    /// Forget about this sender without closing the connection.
    pub fn forget(mut self) {
        #[allow(clippy::mem_forget)] // intentional
        std::mem::forget(self.tx.take());
    }
}

/// Receiver for incoming [`WsEvent`]s.
pub struct WsReceiver {
    rx: tokio::sync::mpsc::UnboundedReceiver<WsEvent>,
}

impl WsReceiver {
    /// Returns a receiver and an event-handler that can be passed to `crate::ws_connect`.
    pub fn new() -> (Self, EventHandler) {
        Self::new_with_callback(|| {})
    }

    /// The given callback will be called on each new message.
    ///
    /// This can be used to wake up the UI thread.
    pub fn new_with_callback(wake_up: impl Fn() + Send + Sync + 'static) -> (Self, EventHandler) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let on_event = Box::new(move |event| {
            wake_up(); // wake up UI thread
            if tx.send(event).is_ok() {
                ControlFlow::Continue(())
            } else {
                ControlFlow::Break(())
            }
        });
        let ws_receiver = Self { rx };
        (ws_receiver, on_event)
    }

    /// Try receiving a new event without blocking.
    pub fn try_recv(&mut self) -> Option<WsEvent> {
        self.rx.try_recv().ok()
    }
}

async fn ws_connect_async(
    url: String,
    options: Options,
    outgoing_messages_stream: impl futures::Stream<Item = WsMessage>,
    on_event: EventHandler,
) {
    use futures::StreamExt as _;

    let config = tungstenite::protocol::WebSocketConfig::from(options);
    let disable_nagle = false; // God damn everyone who adds negations to the names of their variables
    let (ws_stream, _response) = match tokio_tungstenite::connect_async_with_config(
        url,
        Some(config),
        disable_nagle,
    )
    .await
    {
        Ok(result) => result,
        Err(err) => {
            on_event(WsEvent::Error(err.to_string()));
            return;
        }
    };

    log::info!("WebSocket handshake has been successfully completed");

    let control = on_event(WsEvent::Opened);
    if control.is_break() {
        log::warn!("ControlFlow::Break not implemented for the tungstenite tokio backend");
    }

    let (write, read) = ws_stream.split();

    let writer = outgoing_messages_stream
        .map(|ws_message| match ws_message {
            WsMessage::Text(text) => tungstenite::protocol::Message::Text(text),
            WsMessage::Binary(data) => tungstenite::protocol::Message::Binary(data),
            WsMessage::Ping(data) => tungstenite::protocol::Message::Ping(data),
            WsMessage::Pong(data) => tungstenite::protocol::Message::Pong(data),
            WsMessage::Unknown(_) => panic!("You cannot send WsMessage::Unknown"),
        })
        .map(Ok)
        .forward(write);

    let reader = read.for_each(move |event| {
        let control = match event {
            Ok(message) => match message {
                tungstenite::protocol::Message::Text(text) => {
                    on_event(WsEvent::Message(WsMessage::Text(text)))
                }
                tungstenite::protocol::Message::Binary(data) => {
                    on_event(WsEvent::Message(WsMessage::Binary(data)))
                }
                tungstenite::protocol::Message::Ping(data) => {
                    on_event(WsEvent::Message(WsMessage::Ping(data)))
                }
                tungstenite::protocol::Message::Pong(data) => {
                    on_event(WsEvent::Message(WsMessage::Pong(data)))
                }
                tungstenite::protocol::Message::Close(_) => on_event(WsEvent::Closed),
                tungstenite::protocol::Message::Frame(_) => ControlFlow::Continue(()),
            },
            Err(err) => on_event(WsEvent::Error(err.to_string())),
        };
        if control.is_break() {
            log::warn!("ControlFlow::Break not implemented for the tungstenite tokio backend");
        }
        async {}
    });

    futures_util::pin_mut!(reader, writer);
    futures_util::future::select(reader, writer).await;
}

#[allow(clippy::unnecessary_wraps)]
pub(crate) fn ws_connect_impl(
    url: String,
    options: Options,
    on_event: EventHandler,
) -> Result<WsSender> {
    Ok(ws_connect_native(url, options, on_event))
}

/// Like [`crate::ws_connect`], but cannot fail. Only available on native builds.
fn ws_connect_native(url: String, options: Options, on_event: EventHandler) -> WsSender {
    let (tx, mut rx) = tokio::sync::mpsc::channel(1000);

    let outgoing_messages_stream = async_stream::stream! {
        while let Some(item) = rx.recv().await {
            yield item;
        }
        log::debug!("WsSender dropped - closing connection.");
    };

    tokio::spawn(async move {
        ws_connect_async(url.clone(), options, outgoing_messages_stream, on_event).await;
        log::debug!("WS connection finished.");
    });
    WsSender { tx: Some(tx) }
}

pub(crate) fn ws_receive_impl(url: String, options: Options, on_event: EventHandler) -> Result<()> {
    ws_connect_impl(url, options, on_event).map(|sender| sender.forget())
}
