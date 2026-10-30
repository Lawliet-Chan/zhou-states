use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;
use zhou_core::{Command, Config, load_scenario};
use zhou_server::game::{ClientMsg, Game, ServerMsg};

const KING: zhou_core::Id = 2; // 周王，有成周
const WEI: zhou_core::Id = 5; // 卫伯，有卫
const CHENGZHOU: zhou_core::Id = 53;
const WEI_CITY: zhou_core::Id = 41;

fn game() -> Game {
    let config = Config::load("../../config/balance.toml").unwrap();
    let state = load_scenario("../../scenarios/demo.toml", &config).unwrap();
    Game::new(state, config, None)
}

fn json(msg: &ServerMsg) -> Value {
    serde_json::to_value(msg).unwrap()
}

#[test]
fn players_log_in_command_and_end_turns_together() {
    let mut g = game();
    g.connect(1);
    g.connect(2);

    // 没登录不能下指令
    let out = g.handle(1, ClientMsg::View);
    assert_eq!(json(&out[0].1)["type"], "error");

    // 登录：只看得到自己的城的内情
    let out = g.handle(1, ClientMsg::Login { player: KING });
    let welcome = json(&out[0].1);
    assert_eq!(welcome["type"], "welcome");
    assert_eq!(welcome["config"]["categories"]["ritual"], "礼器");
    assert!(welcome["config"]["units"]["cavalry"]["name"].is_string());
    let cities = welcome["view"]["cities"].as_array().unwrap();
    assert_eq!(cities.len(), 1);
    assert_eq!(cities[0]["id"], CHENGZHOU);
    g.handle(2, ClientMsg::Login { player: WEI });

    // 指令失败：只回给下指令的人
    let out = g.handle(
        1,
        ClientMsg::Command {
            req: 7,
            command: Box::new(Command::DisbandArmy { army: 999 }),
        },
    );
    assert_eq!(out.len(), 1);
    let result = json(&out[0].1);
    assert_eq!(
        (result["ok"].clone(), result["req"].clone()),
        (json!(false), json!(7))
    );
    assert!(result["error"].as_str().unwrap().contains("999"));

    // 别人的城不能管
    let out = g.handle(
        1,
        ClientMsg::Command {
            req: 8,
            command: Box::new(Command::SetTaxRate {
                city: WEI_CITY,
                rate: zhou_core::Fx::milli(300),
            }),
        },
    );
    assert_eq!(json(&out[0].1)["ok"], false);

    // 指令成功：回复结果，在线的人都收到最新情况
    let out = g.handle(
        1,
        ClientMsg::Command {
            req: 9,
            command: Box::new(Command::SetTaxRate {
                city: CHENGZHOU,
                rate: zhou_core::Fx::milli(300),
            }),
        },
    );
    assert_eq!(json(&out[0].1)["ok"], true);
    let views: Vec<_> = out[1..]
        .iter()
        .map(|(c, m)| (*c, json(m)["type"].clone()))
        .collect();
    assert_eq!(views, vec![(1, json!("view")), (2, json!("view"))]);

    // 结束回合：要等在线的人都点了
    let out = g.handle(1, ClientMsg::EndTurn { ready: true });
    let waiting = json(&out[0].1);
    assert_eq!(waiting["type"], "waiting");
    assert_eq!(waiting["waiting"], json!([WEI]));
    assert_eq!(g.state.turn, 0);
    let out = g.handle(2, ClientMsg::EndTurn { ready: true });
    assert_eq!(g.state.turn, 1);
    assert_eq!(out.len(), 2);
    let report = json(&out[0].1);
    assert_eq!(report["type"], "turn_ended");
    assert_eq!(report["report"]["me"], KING);

    // 不在线的人不会拖住回合：卫伯断线后，周王一点就结算
    g.handle(2, ClientMsg::EndTurn { ready: true });
    let out = g.disconnect(2);
    assert!(out.iter().all(|(c, _)| *c == 1));
    g.handle(1, ClientMsg::EndTurn { ready: true });
    assert_eq!(g.state.turn, 2);

    // 后登录的人能看到上一回合的报告
    g.connect(3);
    let out = g.handle(3, ClientMsg::Login { player: WEI });
    assert_eq!(json(&out[0].1)["last_turn"]["turn"], 1);
}

#[test]
fn saves_round_trip() {
    let mut g = game();
    g.end_turn();
    let dir = std::env::temp_dir().join(format!("zhou-save-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("world.json");
    zhou_server::game::save(&g.state, &path).unwrap();
    let loaded = zhou_server::game::load(&path).unwrap();
    assert_eq!(loaded.turn, 1);
    assert_eq!(
        serde_json::to_value(&loaded).unwrap(),
        serde_json::to_value(&g.state).unwrap()
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn clients_talk_json_over_websocket() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(zhou_server::net::serve(listener, game(), 0));
    let (mut ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}"))
        .await
        .unwrap();
    ws.send(Message::text("乱码")).await.unwrap();
    let msg = recv(&mut ws).await;
    assert_eq!(msg["type"], "error");

    ws.send(Message::text(
        json!({"type": "login", "player": KING}).to_string(),
    ))
    .await
    .unwrap();
    assert_eq!(recv(&mut ws).await["type"], "welcome");
    assert_eq!(recv(&mut ws).await["type"], "waiting");

    // 数值一律是整数（千分之一为单位），发小数看不懂
    let command = json!({
        "type": "command",
        "req": 1,
        "command": {"type": "set_tax_rate", "city": CHENGZHOU, "rate": 0.25}
    });
    ws.send(Message::text(command.to_string())).await.unwrap();
    assert_eq!(recv(&mut ws).await["type"], "error");

    let command = json!({
        "type": "command",
        "req": 1,
        "command": {"type": "set_tax_rate", "city": CHENGZHOU, "rate": 250}
    });
    ws.send(Message::text(command.to_string())).await.unwrap();
    let result = recv(&mut ws).await;
    assert_eq!(
        (result["type"].clone(), result["ok"].clone()),
        (json!("result"), json!(true))
    );
    assert_eq!(recv(&mut ws).await["type"], "view");

    ws.send(Message::text(json!({"type": "end_turn"}).to_string()))
        .await
        .unwrap();
    let ended = recv(&mut ws).await;
    assert_eq!(ended["type"], "turn_ended");
    assert_eq!(ended["report"]["turn"], 0);
}

async fn recv<S>(ws: &mut S) -> Value
where
    S: StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin,
{
    loop {
        if let Message::Text(t) = ws.next().await.unwrap().unwrap() {
            return serde_json::from_str(&t).unwrap();
        }
    }
}
