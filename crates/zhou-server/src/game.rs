//! 服务器的游戏逻辑，不管网络：收到一条客户端消息，返回要发给哪些连接的哪些消息。
//!
//! 回合和文明系列的同时回合一样：在线的玩家都点了「结束回合」就结算；也可以设定每回合的时限，
//! 到时间不管有没有人点都结算。不在线的玩家不会拖住回合，他的城靠城墙和守军自动防守。
//! 每个玩家只收到自己看得到的那一份（战争迷雾），不靠客户端隐藏。

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use zhou_core::state::GameState;
use zhou_core::{Command, Config, Id, PlayerReport, TurnReport, apply, end_turn, player_report};

/// 一条连接的编号，由网络层分配。
pub type ConnId = u64;

/// 客户端发来的消息。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientMsg {
    /// 以某个人物的身份登录（暂时没有密码）
    Login { player: Id },
    /// 下指令。`req` 是客户端自己编的号，回复里原样带回
    Command {
        #[serde(default)]
        req: u64,
        command: Box<Command>,
    },
    /// 要一份眼下的情况
    View,
    /// 点了「结束回合」；`ready` 为 false 是取消
    EndTurn {
        #[serde(default = "yes")]
        ready: bool,
    },
}

fn yes() -> bool {
    true
}

/// 发给客户端的消息。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerMsg {
    /// 登录成功：眼下的情况，上一回合结算的报告（如果有），以及全部数值配置（名称、兵种、物品类别……）。
    /// 配置只在登录时发一次
    Welcome {
        player: Id,
        config: Box<Config>,
        view: Box<PlayerReport>,
        last_turn: Option<Box<PlayerReport>>,
    },
    /// 指令的结果
    Result {
        req: u64,
        ok: bool,
        /// 指令新建出来的东西的 id（新军队、新战争……）
        ids: Vec<Id>,
        error: Option<String>,
    },
    /// 回合中情况有变（有人下了指令）：最新的情况
    View {
        view: Box<PlayerReport>,
    },
    /// 回合结算完：这一回合的报告，之后是新回合
    TurnEnded {
        report: Box<PlayerReport>,
    },
    /// 谁点了结束回合、还在等谁
    Waiting {
        ready: Vec<Id>,
        waiting: Vec<Id>,
    },
    Error {
        message: String,
    },
}

/// 要发出去的消息：发给哪条连接。
pub type Outbox = Vec<(ConnId, ServerMsg)>;

pub struct Game {
    pub state: GameState,
    pub config: Config,
    /// 每回合结算后存档到这里
    pub save: Option<PathBuf>,
    /// 连接 → 登录的人物，还没登录为 None
    conns: BTreeMap<ConnId, Option<Id>>,
    /// 点了结束回合的人
    ready: BTreeSet<Id>,
    /// 上一回合的全局报告，给后登录的人看
    last: Option<TurnReport>,
}

impl Game {
    pub fn new(state: GameState, config: Config, save: Option<PathBuf>) -> Self {
        Self {
            state,
            config,
            save,
            conns: BTreeMap::new(),
            ready: BTreeSet::new(),
            last: None,
        }
    }

    pub fn connect(&mut self, conn: ConnId) {
        self.conns.insert(conn, None);
    }

    /// 断线：他点过的结束回合不算了；剩下在线的人都点了，就结算。
    pub fn disconnect(&mut self, conn: ConnId) -> Outbox {
        let Some(Some(player)) = self.conns.remove(&conn) else {
            return Vec::new();
        };
        if !self.online().contains(&player) {
            self.ready.remove(&player);
        }
        self.check_ready()
    }

    pub fn handle(&mut self, conn: ConnId, msg: ClientMsg) -> Outbox {
        let player = self.conns.get(&conn).copied().flatten();
        match (msg, player) {
            (ClientMsg::Login { player }, _) => {
                if !self.state.characters.contains_key(&player) {
                    return error(conn, format!("人物 {player} 不存在"));
                }
                self.conns.insert(conn, Some(player));
                let view = self.view(player);
                let last_turn = self
                    .last
                    .as_ref()
                    .map(|r| Box::new(player_report(&self.state, &self.config, r, player)));
                let mut out = vec![(
                    conn,
                    ServerMsg::Welcome {
                        player,
                        config: Box::new(self.config.clone()),
                        view,
                        last_turn,
                    },
                )];
                out.extend(self.waiting_to(&[conn]));
                out
            }
            (_, None) => error(conn, "请先登录".into()),
            (ClientMsg::View, Some(player)) => vec![(
                conn,
                ServerMsg::View {
                    view: self.view(player),
                },
            )],
            (ClientMsg::Command { req, command }, Some(player)) => {
                match apply(&mut self.state, &self.config, player, &command) {
                    Ok(ids) => {
                        let mut out = vec![(
                            conn,
                            ServerMsg::Result {
                                req,
                                ok: true,
                                ids,
                                error: None,
                            },
                        )];
                        // 实时推送：每个在线的人收到自己看得到的最新情况
                        out.extend(self.push_views());
                        out
                    }
                    Err(e) => vec![(
                        conn,
                        ServerMsg::Result {
                            req,
                            ok: false,
                            ids: Vec::new(),
                            error: Some(e.to_string()),
                        },
                    )],
                }
            }
            (ClientMsg::EndTurn { ready }, Some(player)) => {
                if ready {
                    self.ready.insert(player);
                } else {
                    self.ready.remove(&player);
                }
                self.check_ready()
            }
        }
    }

    /// 在线的人都点了结束回合就结算，否则告诉大家还在等谁。
    fn check_ready(&mut self) -> Outbox {
        let online = self.online();
        if !online.is_empty() && online.iter().all(|p| self.ready.contains(p)) {
            return self.end_turn();
        }
        let conns: Vec<ConnId> = self.logged_in().map(|(c, _)| c).collect();
        self.waiting_to(&conns)
    }

    fn waiting_to(&self, conns: &[ConnId]) -> Outbox {
        let online = self.online();
        let ready: Vec<Id> = online.intersection(&self.ready).copied().collect();
        let waiting: Vec<Id> = online.difference(&self.ready).copied().collect();
        conns
            .iter()
            .map(|c| {
                (
                    *c,
                    ServerMsg::Waiting {
                        ready: ready.clone(),
                        waiting: waiting.clone(),
                    },
                )
            })
            .collect()
    }

    /// 结算回合：存档，每个在线的人收到自己那份报告。
    pub fn end_turn(&mut self) -> Outbox {
        let report = end_turn(&mut self.state, &self.config);
        self.ready.clear();
        if let Some(path) = &self.save
            && let Err(e) = save(&self.state, path)
        {
            eprintln!("存档失败：{e}");
        }
        let out = self
            .logged_in()
            .map(|(conn, player)| {
                (
                    conn,
                    ServerMsg::TurnEnded {
                        report: Box::new(player_report(&self.state, &self.config, &report, player)),
                    },
                )
            })
            .collect();
        self.last = Some(report);
        out
    }

    fn view(&self, player: Id) -> Box<PlayerReport> {
        let snapshot = zhou_core::turn::snapshot(&self.state, &self.config);
        Box::new(player_report(&self.state, &self.config, &snapshot, player))
    }

    fn push_views(&self) -> Outbox {
        let snapshot = zhou_core::turn::snapshot(&self.state, &self.config);
        self.logged_in()
            .map(|(conn, player)| {
                (
                    conn,
                    ServerMsg::View {
                        view: Box::new(player_report(&self.state, &self.config, &snapshot, player)),
                    },
                )
            })
            .collect()
    }

    fn logged_in(&self) -> impl Iterator<Item = (ConnId, Id)> + '_ {
        self.conns.iter().filter_map(|(c, p)| p.map(|p| (*c, p)))
    }

    /// 在线的人。
    fn online(&self) -> BTreeSet<Id> {
        self.logged_in().map(|(_, p)| p).collect()
    }
}

fn error(conn: ConnId, message: String) -> Outbox {
    vec![(conn, ServerMsg::Error { message })]
}

/// 存档：整个游戏状态写成 JSON，先写临时文件再改名，免得写到一半断电坏了存档。
pub fn save(state: &GameState, path: &std::path::Path) -> std::io::Result<()> {
    let json = serde_json::to_string(state).map_err(std::io::Error::other)?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(tmp, path)
}

pub fn load(path: &std::path::Path) -> std::io::Result<GameState> {
    let json = std::fs::read_to_string(path)?;
    serde_json::from_str(&json).map_err(std::io::Error::other)
}
