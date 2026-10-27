//! 启动服务器：有存档就读存档，没有就从剧本开局。

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use zhou_core::{Config, load_scenario};
use zhou_server::game::{Game, load};

#[derive(Parser)]
#[command(about = "周代列国的权威服务器，客户端用 WebSocket 连接")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:7878")]
    addr: String,
    #[arg(long, default_value = "config/balance.toml")]
    config: PathBuf,
    /// 没有存档时从这个剧本开局
    #[arg(long, default_value = "scenarios/demo.toml")]
    scenario: PathBuf,
    /// 存档文件：启动时有就读，每回合结算后写
    #[arg(long, default_value = "saves/world.json")]
    save: PathBuf,
    /// 每回合的时限（秒），到时间就结算；0 为只等在线的人都点结束回合
    #[arg(long, default_value_t = 0)]
    turn_seconds: u64,
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = Args::parse();
    let config = match Config::load(&args.config) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let state = if args.save.exists() {
        match load(&args.save) {
            Ok(state) => {
                println!("读取存档 {}，第 {} 回合", args.save.display(), state.turn);
                state
            }
            Err(e) => {
                eprintln!("存档读不了：{e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        match load_scenario(&args.scenario, &config) {
            Ok(state) => {
                println!("从剧本 {} 开局", args.scenario.display());
                state
            }
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        }
    };
    if let Some(dir) = args.save.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let listener = match tokio::net::TcpListener::bind(&args.addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("监听 {} 失败：{e}", args.addr);
            return ExitCode::FAILURE;
        }
    };
    println!("服务器在 ws://{} 等待连接", args.addr);
    for c in state.characters.values() {
        println!("  人物 {}：{}", c.id, c.name);
    }
    let game = Game::new(state, config, Some(args.save));
    zhou_server::net::serve(listener, game, args.turn_seconds).await;
    ExitCode::SUCCESS
}
