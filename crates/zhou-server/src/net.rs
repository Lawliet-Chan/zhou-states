//! 网络层：WebSocket 连接收发 JSON 文本。所有连接的消息排队交给一个游戏任务依次处理，
//! 游戏状态只有它一个人碰。

use std::collections::BTreeMap;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::game::{ClientMsg, ConnId, Game, Outbox, ServerMsg};

enum Incoming {
    Connected(ConnId, mpsc::UnboundedSender<String>),
    Msg(ConnId, String),
    Disconnected(ConnId),
}

/// 在 `listener` 上接受连接，跑到进程结束。`turn_seconds` 不为 0 时，每回合到时间就结算。
pub async fn serve(listener: TcpListener, game: Game, turn_seconds: u64) {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(run_game(game, rx, turn_seconds));
    let mut next: ConnId = 1;
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        let conn = next;
        next += 1;
        tokio::spawn(connection(stream, conn, tx.clone()));
    }
}

async fn connection(stream: TcpStream, conn: ConnId, game: mpsc::UnboundedSender<Incoming>) {
    let Ok(ws) = tokio_tungstenite::accept_async(stream).await else {
        return;
    };
    let (mut sink, mut source) = ws.split();
    let (out_tx, mut out_rx) = mpsc::unbounded_channel::<String>();
    if game.send(Incoming::Connected(conn, out_tx)).is_err() {
        return;
    }
    let writer = tokio::spawn(async move {
        while let Some(text) = out_rx.recv().await {
            if sink.send(Message::text(text)).await.is_err() {
                break;
            }
        }
    });
    while let Some(Ok(msg)) = source.next().await {
        match msg {
            Message::Text(text) => {
                if game.send(Incoming::Msg(conn, text.to_string())).is_err() {
                    break;
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }
    let _ = game.send(Incoming::Disconnected(conn));
    writer.abort();
}

async fn run_game(mut game: Game, mut rx: mpsc::UnboundedReceiver<Incoming>, turn_seconds: u64) {
    let mut senders: BTreeMap<ConnId, mpsc::UnboundedSender<String>> = BTreeMap::new();
    let period = Duration::from_secs(turn_seconds.max(1));
    let mut timer = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
    loop {
        let out: Outbox = tokio::select! {
            incoming = rx.recv() => match incoming {
                None => return,
                Some(Incoming::Connected(conn, tx)) => {
                    senders.insert(conn, tx);
                    game.connect(conn);
                    Vec::new()
                }
                Some(Incoming::Disconnected(conn)) => {
                    senders.remove(&conn);
                    game.disconnect(conn)
                }
                Some(Incoming::Msg(conn, text)) => match serde_json::from_str::<ClientMsg>(&text) {
                    Ok(msg) => game.handle(conn, msg),
                    Err(e) => vec![(conn, ServerMsg::Error { message: format!("看不懂的消息：{e}") })],
                },
            },
            _ = timer.tick(), if turn_seconds > 0 => {
                game.end_turn()
            }
        };
        if out
            .iter()
            .any(|(_, m)| matches!(m, ServerMsg::TurnEnded { .. }))
        {
            timer.reset();
        }
        for (conn, msg) in out {
            if let Some(tx) = senders.get(&conn) {
                let text = serde_json::to_string(&msg).expect("messages serialize");
                let _ = tx.send(text);
            }
        }
    }
}
