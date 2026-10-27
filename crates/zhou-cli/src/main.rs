//! 命令行工具：加载剧本，自动结算若干回合，打印每回合的报告。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use zhou_core::turn::{AssignmentReport, CellReport, CityReport};
use zhou_core::{
    Building, Config, Hex, Id, Inventory, Mode, Task, TurnReport, end_turn, load_scenario,
};

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
    #[arg(long, default_value_t = 12)]
    turns: u32,
    /// 以 JSON 输出全部报告，便于用其他工具分析
    #[arg(long)]
    json: bool,
    /// 只输出这个人物看得到的部分（战争迷雾），以 JSON 输出
    #[arg(long)]
    player: Option<Id>,
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

    if let Some(player) = args.player {
        let views: Vec<_> = (0..args.turns)
            .map(|_| {
                let report = end_turn(&mut state, &config);
                zhou_core::player_report(&state, &config, &report, player)
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&views).expect("reports serialize")
        );
        return ExitCode::SUCCESS;
    }
    let reports: Vec<TurnReport> = (0..args.turns)
        .map(|_| end_turn(&mut state, &config))
        .collect();
    if args.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&reports).expect("reports serialize")
        );
    } else {
        if let Some(first) = reports.first() {
            print_map(first, &config);
        }
        for report in &reports {
            print_report(report, &config);
        }
        if let Some(last) = reports.last().filter(|_| reports.len() > 1) {
            print_map(last, &config);
        }
    }
    ExitCode::SUCCESS
}

/// 左边画地形，右边画归属：城市用城名的第一个字，治所加方括号，野地为「·」。奇数行向右缩进半格。
fn print_map(report: &TurnReport, config: &Config) {
    let offset = |pos: Hex| pos.to_offset();
    let (Some(min_col), Some(max_col)) = (
        report.map.iter().map(|c| offset(c.pos).0).min(),
        report.map.iter().map(|c| offset(c.pos).0).max(),
    ) else {
        return;
    };
    let (min_row, max_row) = (
        report
            .map
            .iter()
            .map(|c| offset(c.pos).1)
            .min()
            .unwrap_or(0),
        report
            .map
            .iter()
            .map(|c| offset(c.pos).1)
            .max()
            .unwrap_or(0),
    );
    let cells: BTreeMap<(i32, i32), &CellReport> =
        report.map.iter().map(|c| (offset(c.pos), c)).collect();
    let mark = |city: Id| {
        report
            .cities
            .iter()
            .find(|c| c.id == city)
            .and_then(|c| c.name.chars().next())
            .unwrap_or('?')
    };
    let seats: Vec<Hex> = report.cities.iter().map(|c| c.seat).collect();
    println!("══ 地图 ══");
    for row in min_row..=max_row {
        let indent = if row & 1 == 1 { "  " } else { "" };
        let (mut terrain, mut owner) = (indent.to_string(), indent.to_string());
        for col in min_col..=max_col {
            match cells.get(&(col, row)) {
                Some(cell) => {
                    let symbol = config
                        .terrains
                        .get(&cell.terrain)
                        .map_or("?", |t| t.symbol.as_str());
                    terrain += &format!("{symbol}  ");
                    owner += &match cell.city {
                        Some(city) if seats.contains(&cell.pos) => format!("[{}]", mark(city)),
                        Some(city) => format!(" {} ", mark(city)),
                        None => " ·  ".into(),
                    };
                }
                None => {
                    terrain += "    ";
                    owner += "    ";
                }
            }
        }
        println!("  {terrain}    {owner}");
    }
    println!();
}

fn print_report(report: &TurnReport, config: &Config) {
    println!(
        "══ 第 {} 年 {} · {} 月 ══",
        report.year, report.season, report.month
    );
    let name_of = |id: Option<Id>| {
        id.and_then(|id| report.characters.iter().find(|c| c.id == id))
            .map_or("无", |c| c.name.as_str())
    };
    let country_of = |id: Option<Id>| {
        id.and_then(|id| report.countries.iter().find(|c| c.id == id))
            .map_or("无国籍", |c| c.name.as_str())
    };
    for city in &report.cities {
        print_city(city, config, &name_of, &country_of);
    }
    for war in &report.wars {
        let side = |p: &zhou_core::Party| match p {
            zhou_core::Party::Country { id } => format!("{}国", country_of(Some(*id))),
            zhou_core::Party::Person { id } => name_of(Some(*id)).to_string(),
        };
        let mut line = format!(
            "  战争 {}：{} 攻 {}",
            war.id,
            side(&war.attacker),
            side(&war.defender)
        );
        if !war.justified {
            line += "（无理由）";
        }
        if war.halted {
            line += "（国君已制止）";
        }
        println!("{line}");
    }
    for army in &report.armies {
        let city = report.cities.iter().find(|c| c.id == army.city);
        let squads: Vec<String> = army
            .squads
            .iter()
            .map(|s| {
                let unit = config
                    .units
                    .get(&s.unit)
                    .map_or(s.unit.as_str(), |u| u.name.as_str());
                let elite = if s.elite { "精兵·" } else { "" };
                let mut text = format!(
                    "{elite}{unit} {} 人（装备 {:.0}%）",
                    s.count,
                    s.equipped * 100.0
                );
                if s.gear.iter().next().is_some() {
                    text += &format!("[{}]", format_inventory(&s.gear, config));
                }
                text
            })
            .collect();
        let mut march = army.dest.map_or(String::new(), |d| format!("，正开往 {d}"));
        if army.on_campaign {
            march += &match &army.supply {
                Some(l) if !l.path.is_empty() => {
                    let from = report.cities.iter().find(|c| c.id == l.from);
                    format!(
                        "，粮道自{}长 {} 格、辎重 {} 人",
                        from.map_or("?", |c| c.name.as_str()),
                        l.path.len(),
                        l.convoy
                    )
                }
                Some(_) => "，粮道不通".to_string(),
                None => "，没有粮道".to_string(),
            };
        }
        if army.hunger > 0 {
            march += &format!(
                "，断粮 {} 回合、战斗力剩 {:.0}%",
                army.hunger,
                army.strength * 100.0
            );
        }
        println!(
            "  军队 {}（{}）统帅 {} 在 {}{}：{}",
            army.id,
            city.map_or("?", |c| c.name.as_str()),
            name_of(Some(army.commander)),
            army.pos,
            march,
            squads.join("；")
        );
    }
    for c in &report.characters {
        let city_name = |id: Option<Id>| {
            id.and_then(|id| report.cities.iter().find(|c| c.id == id))
                .map_or("无处可待", |c| c.name.as_str())
        };
        let mut line = format!(
            "  人物 {}（{} · {}）在{}  风评 {:.0}",
            c.name,
            country_of(c.nationality),
            c.rank,
            city_name(c.location),
            c.reputation
        );
        if c.private_store.iter().next().is_some() {
            line += &format!("  私库：{}", format_inventory(&c.private_store, config));
        }
        for (id, trust) in &c.trust {
            line += &format!("  对{}的信任 {trust:.0}", name_of(Some(*id)));
        }
        println!("{line}");
    }
    for event in &report.events {
        println!("  · {}", event.text);
    }
    println!();
}

fn print_city<'a>(
    city: &CityReport,
    config: &Config,
    name_of: &dyn Fn(Option<Id>) -> &'a str,
    country_of: &dyn Fn(Option<Id>) -> &'a str,
) {
    let building_name = |b: &Building| {
        let mode = if config.is_workplace(&b.kind) {
            mode_name(b.mode)
        } else {
            ""
        };
        format!(
            "{mode}{}",
            config
                .buildings
                .get(&b.kind)
                .map_or(b.kind.as_str(), |d| d.name.as_str())
        )
    };
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    for b in city.tiles.iter().flat_map(|t| &t.buildings) {
        *counts.entry(building_name(b)).or_insert(0) += 1;
    }
    let buildings: Vec<String> = counts
        .iter()
        .map(|(name, n)| format!("{name}×{n}"))
        .collect();
    let mut defense = match city.wall {
        Some(hp) if hp < config.siege.wall_hp => {
            format!("  城墙 {hp:.0}/{:.0}", config.siege.wall_hp)
        }
        _ => String::new(),
    };
    if city.center < config.siege.center_hp {
        defense += &format!("  城中心 {:.0}/{:.0}", city.center, config.siege.center_hp);
    }
    if city.occupied {
        defense += "  被攻占";
    }
    println!(
        "  【{}】{}{}{}  人口 {}/{}  士人 {}（空闲 {}）  建筑 {}",
        city.name,
        country_of(city.country),
        if city.neighbor_bonus > 0.0 {
            format!("  邻城增益 +{:.0}%", city.neighbor_bonus * 100.0)
        } else {
            String::new()
        },
        defense,
        city.population,
        city.housing,
        city.officials,
        city.idle_officials,
        if buildings.is_empty() {
            "无".into()
        } else {
            buildings.join(" ")
        }
    );
    let mut tiles: BTreeMap<String, u32> = BTreeMap::new();
    for t in &city.tiles {
        let mut name = config
            .terrains
            .get(&t.terrain)
            .map_or(t.terrain.clone(), |d| d.name.clone());
        for (id, level) in &t.improvements {
            name += &format!(
                "（{}{level}级）",
                config
                    .improvements
                    .get(id)
                    .map_or(id.as_str(), |i| i.name.as_str())
            );
        }
        if !t.buildings.is_empty() {
            let buildings: Vec<String> = t.buildings.iter().map(building_name).collect();
            name += &format!("[{}]", buildings.join("、"));
        }
        *tiles.entry(name).or_insert(0) += 1;
    }
    let tiles: Vec<String> = tiles
        .iter()
        .map(|(name, n)| format!("{name}×{n}"))
        .collect();
    let mut rule = format!(
        "    所有者 {}  管理者 {}",
        name_of(city.owner),
        name_of(city.manager)
    );
    if city.fief_offer.is_some() {
        rule += &format!("（已封授 {}，待答复）", name_of(city.fief_offer));
    }
    if city.liege.is_some() {
        rule += &format!("  封君 {}", name_of(city.liege));
        rule += &match &city.tribute {
            None => format!(
                "  纳贡：按例纳府库产出的 {:.0}%",
                config.fief.auto_rate * 100.0
            ),
            Some(t) => {
                let progress: Vec<String> = t
                    .goods
                    .iter()
                    .map(|(g, n)| {
                        format!("{} {:.1}/{:.0}", config.good_name(g), t.delivered.get(g), n)
                    })
                    .collect();
                let note = match (city.tribute_excessive, t.refused) {
                    (_, true) => "（过火，已拒绝）",
                    (true, false) => "（过火）",
                    _ => "",
                };
                format!("  纳贡：{}{note}", progress.join(" · "))
            }
        };
        if !city.grievances.is_empty() {
            let reasons: Vec<String> = city.grievances.iter().map(|g| g.describe()).collect();
            rule += &format!("  可征讨：{}", reasons.join("、"));
        }
    }
    if !city.passage.is_empty() {
        let holders: Vec<String> = city
            .passage
            .iter()
            .map(|(id, left)| format!("{}（还剩 {left} 回合）", name_of(Some(*id))))
            .collect();
        rule += &format!("  通行权：{}", holders.join("、"));
    }
    if city.manager_offer.is_some() {
        rule += &format!("（已任命 {}，待答复）", name_of(city.manager_offer));
    }
    if let Some(quota) = &city.quota {
        let progress: Vec<String> = quota
            .goods
            .iter()
            .map(|(g, n)| {
                format!(
                    "{} {:.1}/{:.0}",
                    config.good_name(g),
                    quota.delivered.get(g),
                    n
                )
            })
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
    println!(
        "    民间：{}",
        format_inventory(&city.private_wealth, config)
    );
    for g in city.groups.iter().filter(|g| g.size > 0) {
        let skills: Vec<&str> = g.skills.iter().map(|s| config.skill_name(s)).collect();
        let hostility = if city.liege.is_some() {
            format!("  敌视 {:>4.0}", g.hostility)
        } else {
            String::new()
        };
        println!(
            "    {:<8} {:>4} 人  受管 {:>3}  愉悦 {:>4.0}  文化 {:>4.0}  忠诚 {:>4.0}{hostility}  {}",
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
        println!(
            "    工事：{} 主事，{}，{} 人",
            a.shi,
            describe_task(a, city, config),
            a.workers
        );
    }
    for l in &city.learning {
        let group = city
            .groups
            .iter()
            .find(|g| g.id == l.group)
            .map_or("", |g| g.name.as_str());
        println!(
            "    学技能：{group} {} 人学{}（已学 {} 回合）",
            l.count,
            config.skill_name(&l.skill),
            l.turns
        );
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
            format!(
                "{}{}",
                mode_name(*mode),
                config
                    .recipes
                    .get(recipe)
                    .map_or(recipe.as_str(), |r| r.name.as_str())
            )
        }
        Task::Construct {
            building,
            tile,
            mode,
        } => match config.buildings.get(building) {
            Some(b) => {
                let mode = if config.is_workplace(building) {
                    mode_name(*mode)
                } else {
                    ""
                };
                format!(
                    "在{}营建{mode}{}（{:.0}/{:.0}）",
                    terrain_of(*tile),
                    b.name,
                    progress,
                    b.labor
                )
            }
            None => format!("营建{building}"),
        },
        Task::Reclaim { at, terrain } => match config.terrains.get(terrain) {
            Some(t) => format!(
                "开垦 {at} 的{}（{:.0}/{:.0}）",
                t.name, progress, t.reclaim_labor
            ),
            None => format!("开垦 {at} 的{terrain}"),
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
        Task::RepairWall => "修城墙".into(),
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
    if items.is_empty() {
        "空".into()
    } else {
        items.join(" · ")
    }
}
