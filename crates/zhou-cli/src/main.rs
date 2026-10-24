//! 命令行工具：加载剧本，自动结算若干回合，打印每回合的报告。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use zhou_core::turn::{AssignmentReport, CityReport};
use zhou_core::{Building, Config, Id, Inventory, Mode, Task, TurnReport, end_turn, load_scenario};

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
    let name_of = |id: Option<Id>| {
        id.and_then(|id| report.characters.iter().find(|c| c.id == id)).map_or("无", |c| c.name.as_str())
    };
    for city in &report.cities {
        print_city(city, config, &name_of);
    }
    for c in &report.characters {
        let mut line = format!("  人物 {}（{}）风评 {:.0}", c.name, c.rank, c.reputation);
        if c.private_store.iter().next().is_some() {
            line += &format!("  私库：{}", format_inventory(&c.private_store, config));
        }
        for (id, trust) in &c.trust {
            line += &format!("  对{}的信任 {trust:.0}", name_of(Some(*id)));
        }
        println!("{line}");
    }
    for event in &report.events {
        println!("  · {event}");
    }
    println!();
}

fn print_city<'a>(city: &CityReport, config: &Config, name_of: &dyn Fn(Option<Id>) -> &'a str) {
    let building_name = |b: &Building| {
        let mode = if config.is_workplace(&b.kind) { mode_name(b.mode) } else { "" };
        format!("{mode}{}", config.buildings.get(&b.kind).map_or(b.kind.as_str(), |d| d.name.as_str()))
    };
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    for b in city.tiles.iter().flat_map(|t| &t.buildings) {
        *counts.entry(building_name(b)).or_insert(0) += 1;
    }
    let buildings: Vec<String> = counts.iter().map(|(name, n)| format!("{name}×{n}")).collect();
    println!(
        "  【{}】人口 {}/{}  士人 {}（空闲 {}）  建筑 {}",
        city.name,
        city.population,
        city.housing,
        city.officials,
        city.idle_officials,
        if buildings.is_empty() { "无".into() } else { buildings.join(" ") }
    );
    let mut tiles: BTreeMap<String, u32> = BTreeMap::new();
    for t in &city.tiles {
        let mut name = config.terrains.get(&t.terrain).map_or(t.terrain.clone(), |d| d.name.clone());
        for (id, level) in &t.improvements {
            name += &format!("（{}{level}级）", config.improvements.get(id).map_or(id.as_str(), |i| i.name.as_str()));
        }
        if !t.buildings.is_empty() {
            let buildings: Vec<String> = t.buildings.iter().map(building_name).collect();
            name += &format!("[{}]", buildings.join("、"));
        }
        *tiles.entry(name).or_insert(0) += 1;
    }
    let tiles: Vec<String> = tiles.iter().map(|(name, n)| format!("{name}×{n}")).collect();
    let mut rule = format!("    所有者 {}  管理者 {}", name_of(city.owner), name_of(city.manager));
    if city.manager_offer.is_some() {
        rule += &format!("（已任命 {}，待答复）", name_of(city.manager_offer));
    }
    if let Some(quota) = &city.quota {
        let progress: Vec<String> = quota
            .goods
            .iter()
            .map(|(g, n)| format!("{} {:.1}/{:.0}", config.good_name(g), quota.delivered.get(g), n))
            .collect();
        rule += &format!("  规定任务：{}", progress.join(" · "));
    }
    println!("{rule}");
    println!("    地块：{}", tiles.join(" "));
    let jobs: Vec<String> = city
        .jobs
        .iter()
        .filter(|j| j.capacity > 0 || j.turned_away > 0)
        .map(|j| {
            let mut s = format!("{} {}/{}", config.pool_name(&j.pool), j.used, j.capacity);
            if j.turned_away > 0 {
                s += &format!("（{} 人无处劳作）", j.turned_away);
            }
            s
        })
        .collect();
    println!("    劳作：{}", jobs.join(" · "));
    println!("    府库：{}", format_inventory(&city.treasury, config));
    println!("    民间：{}", format_inventory(&city.private_wealth, config));
    for g in city.groups.iter().filter(|g| g.size > 0) {
        let skills: Vec<&str> = g.skills.iter().map(|s| config.skill_name(s)).collect();
        println!(
            "    {:<8} {:>4} 人  受管 {:>3}  愉悦 {:>4.0}  文化 {:>4.0}  忠诚 {:>4.0}  {}",
            g.name,
            g.size,
            g.managed,
            g.happiness,
            g.culture,
            g.loyalty,
            skills.join("、")
        );
    }
    for a in &city.assignments {
        println!("    工事：{} 主事，{}，{} 人", a.shi, describe_task(a, city, config), a.workers);
    }
    for l in &city.learning {
        let group = city.groups.iter().find(|g| g.id == l.group).map_or("", |g| g.name.as_str());
        println!("    学技能：{group} {} 人学{}（已学 {} 回合）", l.count, config.skill_name(&l.skill), l.turns);
    }
}

fn describe_task(a: &AssignmentReport, city: &CityReport, config: &Config) -> String {
    let progress = a.progress;
    let terrain_of = |tile: Id| {
        city.tiles
            .iter()
            .find(|t| t.id == tile)
            .and_then(|t| config.terrains.get(&t.terrain))
            .map_or("", |t| t.name.as_str())
    };
    match &a.task {
        Task::Produce { recipe, mode } => {
            format!("{}{}", mode_name(*mode), config.recipes.get(recipe).map_or(recipe.as_str(), |r| r.name.as_str()))
        }
        Task::Construct { building, tile, mode } => match config.buildings.get(building) {
            Some(b) => {
                let mode = if config.is_workplace(building) { mode_name(*mode) } else { "" };
                format!("在{}营建{mode}{}（{:.0}/{:.0}）", terrain_of(*tile), b.name, progress, b.labor)
            }
            None => format!("营建{building}"),
        },
        Task::Reclaim { terrain } => match config.terrains.get(terrain) {
            Some(t) => format!("开垦{}（{:.0}/{:.0}）", t.name, progress, t.reclaim_labor),
            None => format!("开垦{terrain}"),
        },
        Task::Improve { tile, improvement } => {
            let terrain = terrain_of(*tile);
            match config.improvements.get(improvement) {
                Some(i) => format!("{terrain}{}（{:.0}/{:.0}）", i.name, progress, i.labor),
                None => format!("{terrain}{improvement}"),
            }
        }
        Task::CollectTax => "收税".into(),
        Task::Sacrifice => "祭祀".into(),
    }
}

fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Guan => "官营",
        Mode::Min => "民营",
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
