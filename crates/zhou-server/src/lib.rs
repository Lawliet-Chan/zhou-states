//! 权威服务器：游戏状态只在服务器上，客户端（Godot）通过 WebSocket 收发 JSON。

pub mod game;
pub mod net;
