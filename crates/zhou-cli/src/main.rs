//! 命令行工具：加载剧本，自动结算若干回合，打印每回合的报告。

use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use zhou_core::turn::CityReport;
use zhou_core::{Config, Inventory, Mode, Task, TurnReport, end_turn, load_scenario};

#[derive(Parser)]
#[command(about = "结算剧本若干回合并打印报告")]
struct Args {
    /// 剧本文件
    #[arg(long, default_value = "scenarios/demo.toml")]
    scenario: PathBuf,
    /// 数值配置文件
    #[arg(long, default_value = "config/balance.toml")]
    config: PathBuf,
    /// 结算的回合数
    #[arg(long, default_value_t = 8)]
    turns: u32,
    /// 以 JSON 输出全部报告，便于用其他工具分析
    #[arg(long)]
    json: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let config = match Config::load(&args.config) {
        Ok(config) => config,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let mut state = match load_scenario(&args.scenario, &config) {
        Ok(state) => state,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };

    let reports: Vec<TurnReport> = (0..args.turns).map(|_| end_turn(&mut state, &config)).collect();
    if args.json {
        println!("{}", serde_json::to_string_pretty(&reports).expect("reports serialize"));
    } else {
        for report in &reports {
            print_report(report, &config);
        }
    }
    ExitCode::SUCCESS
}

fn print_report(report: &TurnReport, config: &Config) {
    println!("══ 第 {} 年 {} ══", report.year, report.season);
    for city in &report.cities {
        print_city(city, config);
    }
    for c in &report.characters {
        println!("  人物 {}（{}）风评 {:.0}", c.name, c.rank, c.reputation);
    }
    for event in &report.events {
        println!("  · {event}");
    }
    println!();
}

fn print_city(city: &CityReport, config: &Config) {
    let buildings: Vec<String> = city
        .buildings
        .iter()
        .map(|(id, n)| format!("{}×{n}", config.buildings.get(id).map_or(id.as_str(), |b| b.name.as_str())))
        .collect();
    println!(
        "  【{}】人口 {}  士人 {}（空闲 {}）  建筑 {}",
        city.name,
        city.population,
        city.officials,
        city.idle_officials,
        if buildings.is_empty() { "无".into() } else { buildings.join(" ") }
    );
    println!("    府库：{}", format_inventory(&city.treasury, config));
    println!("    民间：{}", format_inventory(&city.private_wealth, config));
    for g in &city.groups {
        println!(
            "    {:<8} {:>4} 人  受管 {:>3}  愉悦 {:>4.0}  文化 {:>4.0}",
            g.name, g.size, g.managed, g.happiness, g.culture
        );
    }
    for a in &city.assignments {
        println!("    工事：{} 主事，{}，{} 人", a.shi, describe_task(&a.task, a.progress, config), a.workers);
    }
}

fn describe_task(task: &Task, progress: f64, config: &Config) -> String {
    match task {
        Task::Produce { recipe, mode } => {
            let mode = match mode {
                Mode::Guan => "官营",
                Mode::Min => "民营",
            };
            format!("{mode}{}", config.recipes.get(recipe).map_or(recipe.as_str(), |r| r.name.as_str()))
        }
        Task::Construct { building } => match config.buildings.get(building) {
            Some(b) => format!("营建{}（{:.0}/{:.0}）", b.name, progress, b.labor),
            None => format!("营建{building}"),
        },
        Task::CollectTax => "收税".into(),
        Task::Sacrifice => "祭祀".into(),
    }
}

fn format_inventory(inventory: &Inventory, config: &Config) -> String {
    let items: Vec<String> = inventory
        .iter()
        .filter(|(_, amount)| **amount >= 0.05)
        .map(|(good, amount)| format!("{} {:.1}", config.good_name(good), amount))
        .collect();
    if items.is_empty() { "空".into() } else { items.join(" · ") }
}
