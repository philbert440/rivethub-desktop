use std::sync::Arc;

use futures::{SinkExt, StreamExt};
use rustls::ClientConfig;
use tokio::sync::mpsc;
use tokio_tungstenite::{Connector, connect_async_tls_with_config, tungstenite::Message};
use wire::gateway::term::{TermClientFrame, TermServerFrame};

use crate::GatewayError;

const BUFFER: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermOutput {
    Started { columns: u16, rows: u16 },
    Bytes(Vec<u8>),
    Owner { mine: bool },
    Exited { code: Option<i32> },
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermInput {
    Bytes(Vec<u8>),
    Resize { columns: u16, rows: u16 },
    Claim { columns: u16, rows: u16 },
}

#[derive(Debug)]
pub struct TerminalStream {
    pub output: mpsc::Receiver<TermOutput>,
    pub input: mpsc::Sender<TermInput>,
}

fn frame(input: TermInput) -> Message {
    let control = match input {
        TermInput::Bytes(bytes) => return Message::Binary(bytes.into()),
        TermInput::Resize { columns, rows } => TermClientFrame::Resize { cols: columns, rows },
        TermInput::Claim { columns, rows } => TermClientFrame::Claim {
            cols: Some(columns),
            rows: Some(rows),
        },
    };
    Message::Text(serde_json::to_string(&control).unwrap_or_default().into())
}

fn output(text: &str) -> Option<TermOutput> {
    match serde_json::from_str::<TermServerFrame>(text).ok()? {
        TermServerFrame::Hello { cols, rows, .. } => Some(TermOutput::Started { columns: cols, rows }),
        TermServerFrame::Owner { self_owner, .. } => Some(TermOutput::Owner { mine: self_owner }),
        TermServerFrame::Exit { code, .. } => Some(TermOutput::Exited { code }),
    }
}

pub async fn attach(
    base_url: &str,
    tls: Arc<ClientConfig>,
    pty: &str,
) -> Result<TerminalStream, GatewayError> {
    let ws_base = base_url
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1);
    let url = format!("{ws_base}/term?id={pty}");
    let (socket, _) = connect_async_tls_with_config(url, None, false, Some(Connector::Rustls(tls))).await?;
    let (mut sink, mut source) = socket.split();
    let (output_tx, output_rx) = mpsc::channel(BUFFER);
    let (input, mut input_rx) = mpsc::channel(BUFFER);
    tokio::spawn(async move {
        loop {
            tokio::select! {
                incoming = source.next() => {
                    let event = match incoming {
                        Some(Ok(Message::Binary(bytes))) => TermOutput::Bytes(bytes.to_vec()),
                        Some(Ok(Message::Text(text))) => match output(&text) {
                            Some(event) => event,
                            None => continue,
                        },
                        Some(Ok(_)) => continue,
                        Some(Err(_)) | None => {
                            let _ = output_tx.send(TermOutput::Closed).await;
                            break;
                        }
                    };
                    if output_tx.send(event).await.is_err() {
                        break;
                    }
                }
                outgoing = input_rx.recv() => {
                    let Some(outgoing) = outgoing else { break };
                    if sink.send(frame(outgoing)).await.is_err() {
                        let _ = output_tx.send(TermOutput::Closed).await;
                        break;
                    }
                }
            }
        }
    });
    Ok(TerminalStream {
        output: output_rx,
        input,
    })
}
