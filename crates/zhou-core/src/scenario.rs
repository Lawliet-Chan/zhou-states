//! 剧本：用 TOML 描述开局时的人物、城市、人群、士人和工事。
//! 剧本里用 key 互相引用，加载时换成 id。工事通过指令创建，和游戏中走同一套校验。
//!
//! 地图有两种写法：
//! - 手画：`[map]` 里用地形的符号一行一行画出地图，再用城市的 mark 画一层归属；
//! - 不画：城市只写各种地块的块数，加载时各城相隔很远，地块围着治所由近及远排开。

use crate::fx::Fx;
use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

use crate::command::{Command, CommandError, execute};
use crate::config::Config;
use crate::inventory::Inventory;
use crate::map::{Hex, Map};
use crate::state::{
    Building, Character, City, CityStatus, Country, GameState, Id, Mode, Official, PopGroup, Quota,
    Task, Tile, Tribute,
};

#[derive(Debug, Error)]
pub enum ScenarioError {
    #[error("读取剧本失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("剧本格式错误：{0}")]
    Parse(#[from] toml::de::Error),
    #[error("剧本引用了不存在的 key「{0}」")]
    UnknownKey(String),
    #[error("剧本中的 key「{0}」重复")]
    DuplicateKey(String),
    #[error("剧本引用了不存在的{kind}「{id}」")]
    UnknownId { kind: &'static str, id: String },
    #[error("没有地块放得下开局的建筑「{0}」")]
    NoRoomFor(String),
    #[error("开局工事「{key}」写法有误：{msg}")]
    BadTask { key: String, msg: String },
    #[error("地图有误：{0}")]
    BadMap(String),
    #[error("开局工事「{key}」无效：{source}")]
    Assignment { key: String, source: CommandError },
}

#[derive(Debug, Deserialize)]
struct Scenario {
    map: Option<MapDef>,
    #[serde(default)]
    countries: Vec<CountryDef>,
    #[serde(default)]
    characters: Vec<CharacterDef>,
    #[serde(default)]
    cities: Vec<CityDef>,
}

/// 手画的地图。每行用空格分隔各格，奇数行向右缩进半格；「·」表示这里没有格子。
#[derive(Debug, Deserialize)]
struct MapDef {
    /// 每格写地形的 symbol
    rows: Vec<String>,
    /// 和 rows 一一对应，每格写所属城市的 mark，「·」为野地
    #[serde(default)]
    owners: Vec<String>,
}

const EMPTY: &str = "·";

/// 不画地图时，相邻两城的治所相隔的列数
const AUTO_SPACING: i32 = 20;

#[derive(Debug, Deserialize)]
struct CountryDef {
    key: String,
    name: String,
    /// 国君，自动拥有这个国籍
    ruler: String,
    /// 都城
    capital: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CharacterDef {
    key: String,
    name: String,
    #[serde(default = "default_rank")]
    rank: String,
    #[serde(default)]
    reputation: Fx,
    /// 私库
    #[serde(default)]
    private_store: Inventory,
    /// 国籍：国家的 key，不填为无国籍
    nationality: Option<String>,
}

fn default_rank() -> String {
    "none".into()
}

#[derive(Debug, Deserialize)]
struct CityDef {
    key: String,
    name: String,
    owner: Option<String>,
    manager: Option<String>,
    #[serde(default = "default_status")]
    status: CityStatus,
    totem: String,
    /// 在手画地图上标记归属用的字，不填为城名的第一个字
    mark: Option<String>,
    /// 治所所在的格子 [列, 行]，不填为这座城的第一块地
    seat: Option<[i32; 2]>,
    /// 不画地图时用：地块种类 → 块数
    #[serde(default)]
    tiles: BTreeMap<String, u32>,
    /// 官营建筑 → 座数
    #[serde(default)]
    guan_buildings: BTreeMap<String, u32>,
    /// 民营建筑 → 座数
    #[serde(default)]
    min_buildings: BTreeMap<String, u32>,
    #[serde(default)]
    treasury: Inventory,
    /// 民间财富
    #[serde(default)]
    private_wealth: Inventory,
    tax_rate: Option<Fx>,
    /// 规定任务：每年要上缴给所有者的物资
    quota: Option<Inventory>,
    /// 封君：把这座城封给所有者的人
    liege: Option<String>,
    /// 封君手动指定的每年纳贡，不填为按比例自动纳贡
    tribute: Option<Inventory>,
    #[serde(default)]
    groups: Vec<GroupDef>,
    #[serde(default)]
    officials: Vec<OfficialDef>,
    #[serde(default)]
    assignments: Vec<AssignmentDef>,
}

fn default_status() -> CityStatus {
    CityStatus::Ruled
}

#[derive(Debug, Deserialize)]
struct GroupDef {
    key: String,
    name: String,
    size: u32,
    /// 不填则只有初始技能
    #[serde(default)]
    skills: Vec<String>,
    /// 不填则使用城市的文化图腾
    totem: Option<String>,
    #[serde(default)]
    culture: Fx,
    happiness: Fx,
    /// 对管理者的忠诚度，不填为配置中的初始值
    loyalty: Option<Fx>,
    /// 对封君的敌视
    #[serde(default)]
    hostility: Fx,
}

/// 开局即在任的士人，视为经验已满。
#[derive(Debug, Deserialize)]
struct OfficialDef {
    key: String,
    name: String,
    /// 出身的人群，决定他的技能槽
    from_group: String,
    loyalty: Option<Fx>,
}

#[derive(Debug, Deserialize)]
struct AssignmentDef {
    key: String,
    shi: String,
    /// 和 Task 相同；需要地块的工事用 `on = "地块种类"` 代替 tile，加载时换成该种地块中第一块还有空位的
    task: toml::Table,
    #[serde(default)]
    workers: BTreeMap<String, u32>,
}

pub fn load_scenario(path: impl AsRef<Path>, config: &Config) -> Result<GameState, ScenarioError> {
    from_toml(&std::fs::read_to_string(path)?, config)
}

pub fn from_toml(text: &str, config: &Config) -> Result<GameState, ScenarioError> {
    let scenario: Scenario = toml::from_str(text)?;
    let mut state = GameState::default();
    let mut keys: BTreeMap<String, Id> = BTreeMap::new();
    let register =
        |keys: &mut BTreeMap<String, Id>, key: &str, id: Id| -> Result<(), ScenarioError> {
            match keys.insert(key.to_string(), id) {
                Some(_) => Err(ScenarioError::DuplicateKey(key.to_string())),
                None => Ok(()),
            }
        };
    let lookup = |keys: &BTreeMap<String, Id>, key: &str| -> Result<Id, ScenarioError> {
        keys.get(key)
            .copied()
            .ok_or_else(|| ScenarioError::UnknownKey(key.to_string()))
    };

    // 每座城的地块位置：手画地图时从归属层读出，否则围着治所排开
    let mut land: Vec<Vec<(Hex, String)>> = Vec::new();
    match &scenario.map {
        Some(map) => {
            state.map = parse_map(map, config)?;
            let marks: Vec<String> = scenario
                .cities
                .iter()
                .map(|c| {
                    c.mark
                        .clone()
                        .unwrap_or_else(|| c.name.chars().take(1).collect())
                })
                .collect();
            land = vec![Vec::new(); scenario.cities.len()];
            for (row, line) in map.owners.iter().enumerate() {
                for (col, token) in line.split_whitespace().enumerate() {
                    if token == EMPTY {
                        continue;
                    }
                    let pos = Hex::from_offset(col as i32, row as i32);
                    let city = marks.iter().position(|m| m == token).ok_or_else(|| {
                        ScenarioError::BadMap(format!(
                            "归属层第 {row} 行的「{token}」不是哪座城的 mark"
                        ))
                    })?;
                    let cell = state.map.get(pos).ok_or_else(|| {
                        ScenarioError::BadMap(format!("归属层 {pos} 在地形层里没有格子"))
                    })?;
                    land[city].push((pos, cell.terrain.clone()));
                }
            }
            for (def, cells) in scenario.cities.iter().zip(&land) {
                if !def.tiles.is_empty() {
                    return Err(ScenarioError::BadMap(format!(
                        "画了地图时，城市「{}」不能再写 tiles",
                        def.name
                    )));
                }
                if cells.is_empty() {
                    return Err(ScenarioError::BadMap(format!(
                        "城市「{}」在地图上没有地",
                        def.name
                    )));
                }
            }
        }
        None => {
            for (i, def) in scenario.cities.iter().enumerate() {
                let seat = Hex::from_offset(i as i32 * AUTO_SPACING, 0);
                let terrains = def
                    .tiles
                    .iter()
                    .flat_map(|(t, n)| std::iter::repeat_n(t.clone(), *n as usize));
                let cells: Vec<(Hex, String)> = seat.spiral().zip(terrains).collect();
                for (pos, terrain) in &cells {
                    if !config.terrains.contains_key(terrain) {
                        return Err(ScenarioError::UnknownId {
                            kind: "地块",
                            id: terrain.clone(),
                        });
                    }
                    state.map.insert(*pos, terrain.clone());
                }
                land.push(cells);
            }
        }
    }

    for (def, cells) in scenario.cities.iter().zip(&land) {
        if let Some((pos, terrain)) = cells.iter().find(|(_, t)| !config.terrains[t].passable()) {
            let name = &config.terrains[terrain].name;
            return Err(ScenarioError::BadMap(format!(
                "城市「{}」的 {pos} 是{name}，过不去的地形不能属于城市",
                def.name
            )));
        }
    }

    let mut country_ids = Vec::new();
    for def in &scenario.countries {
        let id = state.alloc_id();
        register(&mut keys, &def.key, id)?;
        country_ids.push(id);
    }

    for def in &scenario.characters {
        let id = state.alloc_id();
        register(&mut keys, &def.key, id)?;
        if !config.ritual.ding_limit.contains_key(&def.rank) {
            return Err(ScenarioError::UnknownId {
                kind: "爵位",
                id: def.rank.clone(),
            });
        }
        check_goods(&def.private_store, config)?;
        let mut character = Character::new(id, def.name.clone(), def.rank.clone(), def.reputation);
        character.private_store = def.private_store.clone();
        if let Some(country) = &def.nationality {
            let country = lookup(&keys, country)?;
            if !country_ids.contains(&country) {
                return Err(ScenarioError::UnknownId {
                    kind: "国家",
                    id: def.nationality.clone().unwrap_or_default(),
                });
            }
            character.nationality = Some(country);
        }
        state.characters.insert(id, character);
    }

    for (def, id) in scenario.countries.iter().zip(&country_ids) {
        let ruler = lookup(&keys, &def.ruler)?;
        let character = state
            .characters
            .get_mut(&ruler)
            .ok_or_else(|| ScenarioError::UnknownKey(def.ruler.clone()))?;
        character.nationality = Some(*id);
        state.countries.insert(
            *id,
            Country {
                id: *id,
                name: def.name.clone(),
                ruler,
                capital: None,
                national_war: false,
            },
        );
    }

    for (def, cells) in scenario.cities.iter().zip(land) {
        let city_id = state.alloc_id();
        register(&mut keys, &def.key, city_id)?;
        let seat = match def.seat {
            Some([col, row]) => Hex::from_offset(col, row),
            None => cells.first().map_or(Hex::new(0, 0), |(pos, _)| *pos),
        };
        if !cells.iter().any(|(pos, _)| *pos == seat) {
            return Err(ScenarioError::BadMap(format!(
                "城市「{}」的治所 {seat} 不是它自己的地",
                def.name
            )));
        }
        let mut tiles = BTreeMap::new();
        for (pos, terrain) in cells {
            let id = state.alloc_id();
            tiles.insert(id, Tile::new(id, pos, terrain));
        }
        // 开局的建筑依次放到第一块还有空位、允许建造的地块上
        let buildings = [
            (Mode::Guan, &def.guan_buildings),
            (Mode::Min, &def.min_buildings),
        ];
        for (mode, building, count) in buildings
            .iter()
            .flat_map(|(m, b)| b.iter().map(move |(id, n)| (*m, id, *n)))
        {
            let Some(allowed) = config.buildings.get(building).map(|b| &b.terrains) else {
                return Err(ScenarioError::UnknownId {
                    kind: "建筑",
                    id: building.clone(),
                });
            };
            for _ in 0..count {
                let tile = tiles
                    .values_mut()
                    .filter(|t| allowed.is_empty() || allowed.contains(&t.terrain))
                    .find(|t| {
                        (t.buildings.len() as u32) < config.terrains[&t.terrain].building_slots
                    })
                    .ok_or_else(|| ScenarioError::NoRoomFor(building.clone()))?;
                tile.buildings.push(Building {
                    kind: building.clone(),
                    mode,
                });
            }
        }
        check_goods(&def.treasury, config)?;
        if let Some(quota) = &def.quota {
            check_goods(quota, config)?;
        }
        if let Some(tribute) = &def.tribute {
            check_goods(tribute, config)?;
        }
        check_goods(&def.private_wealth, config)?;

        let mut groups = BTreeMap::new();
        for g in &def.groups {
            let id = state.alloc_id();
            register(&mut keys, &g.key, id)?;
            let skills = if g.skills.is_empty() {
                vec![config.population.initial_skill.clone()]
            } else {
                g.skills.clone()
            };
            if skills.len() > config.population.max_skills {
                return Err(ScenarioError::UnknownId {
                    kind: "技能槽（超过上限）",
                    id: g.key.clone(),
                });
            }
            for skill in &skills {
                if !config.skills.contains_key(skill) {
                    return Err(ScenarioError::UnknownId {
                        kind: "技能",
                        id: skill.clone(),
                    });
                }
            }
            groups.insert(
                id,
                PopGroup {
                    id,
                    name: g.name.clone(),
                    size: g.size,
                    skills,
                    totem: g.totem.clone().unwrap_or_else(|| def.totem.clone()),
                    culture: g.culture,
                    happiness: g.happiness,
                    loyalty: g.loyalty.unwrap_or(config.loyalty.initial),
                    growth_carry: Fx::ZERO,
                    hostility: g.hostility,
                },
            );
        }

        let mut officials = BTreeMap::new();
        for o in &def.officials {
            let id = state.alloc_id();
            register(&mut keys, &o.key, id)?;
            let origin = lookup(&keys, &o.from_group)?;
            let group = groups
                .get(&origin)
                .ok_or_else(|| ScenarioError::UnknownKey(o.from_group.clone()))?;
            let mut skills = group.skills.clone();
            if !skills.contains(&config.shi.skill) {
                skills.push(config.shi.skill.clone());
            }
            officials.insert(
                id,
                Official {
                    id,
                    name: o.name.clone(),
                    skills,
                    loyalty: o.loyalty.unwrap_or(config.shi.initial_loyalty),
                    service_turns: config.shi.ramp_turns,
                    origin_group: origin,
                },
            );
        }

        let owner = def.owner.as_deref().map(|k| lookup(&keys, k)).transpose()?;
        let manager = def
            .manager
            .as_deref()
            .map(|k| lookup(&keys, k))
            .transpose()?;
        state.cities.insert(
            city_id,
            City {
                id: city_id,
                name: def.name.clone(),
                owner,
                manager,
                status: def.status,
                totem: def.totem.clone(),
                seat,
                tiles,
                treasury: def.treasury.clone(),
                private_wealth: def.private_wealth.clone(),
                tax_rate: def.tax_rate.unwrap_or(config.economy.default_tax_rate),
                groups,
                officials,
                assignments: BTreeMap::new(),
                learning: BTreeMap::new(),
                quota: def.quota.clone().map(|goods| Quota {
                    goods,
                    delivered: Inventory::default(),
                }),
                manager_offer: None,
                sown: BTreeMap::new(),
                liege: def.liege.as_deref().map(|k| lookup(&keys, k)).transpose()?,
                fief_offer: None,
                tribute: def.tribute.clone().map(|goods| Tribute {
                    goods,
                    ..Tribute::default()
                }),
                tribute_shortfall_years: 0,
                income: Inventory::default(),
                last_income: None,
                passage: BTreeMap::new(),
                wall_damage: Fx::ZERO,
                center_damage: Fx::ZERO,
                wall_attacked: false,
                plundered: false,
                occupier: None,
            },
        );

        for a in &def.assignments {
            let shi = lookup(&keys, &a.shi)?;
            let workers = a
                .workers
                .iter()
                .map(|(k, n)| Ok((lookup(&keys, k)?, *n)))
                .collect::<Result<BTreeMap<_, _>, ScenarioError>>()?;
            let task = resolve_task(&a.task, &state.cities[&city_id], &state.map, config).map_err(
                |msg| ScenarioError::BadTask {
                    key: a.key.clone(),
                    msg,
                },
            )?;
            let command = Command::StartAssignment {
                city: city_id,
                shi,
                task,
                workers,
            };
            let created = execute(&mut state, config, &command).map_err(|source| {
                ScenarioError::Assignment {
                    key: a.key.clone(),
                    source,
                }
            })?;
            register(&mut keys, &a.key, created[0])?;
        }
    }
    for (def, id) in scenario.countries.iter().zip(&country_ids) {
        if let Some(capital) = &def.capital {
            let city = lookup(&keys, capital)?;
            if !state.cities.contains_key(&city) {
                return Err(ScenarioError::UnknownId {
                    kind: "城市",
                    id: capital.clone(),
                });
            }
            state
                .countries
                .get_mut(id)
                .expect("country inserted above")
                .capital = Some(city);
        }
    }
    state.settle_locations();
    Ok(state)
}

fn parse_map(def: &MapDef, config: &Config) -> Result<Map, ScenarioError> {
    let mut map = Map::default();
    for (row, line) in def.rows.iter().enumerate() {
        for (col, token) in line.split_whitespace().enumerate() {
            if token == EMPTY {
                continue;
            }
            let terrain = config
                .terrains
                .iter()
                .find(|(_, t)| t.symbol == token)
                .map(|(id, _)| id.clone())
                .ok_or_else(|| {
                    ScenarioError::BadMap(format!("第 {row} 行的「{token}」不是哪种地形的 symbol"))
                })?;
            map.insert(Hex::from_offset(col as i32, row as i32), terrain);
        }
    }
    Ok(map)
}

fn resolve_task(
    raw: &toml::Table,
    city: &City,
    map: &Map,
    config: &Config,
) -> Result<Task, String> {
    let mut raw = raw.clone();
    // 开垦用 at = [列, 行] 指定格子，地形从地图上读
    if let Some(at) = raw.remove("at") {
        let pos = at
            .as_array()
            .and_then(|a| Some((a.first()?.as_integer()?, a.get(1)?.as_integer()?)))
            .map(|(col, row)| Hex::from_offset(col as i32, row as i32))
            .ok_or("at 必须是 [列, 行]")?;
        let cell = map.get(pos).ok_or_else(|| format!("地图上没有 {pos}"))?;
        let mut table = toml::Table::new();
        table.insert("q".into(), toml::Value::Integer(pos.q.into()));
        table.insert("r".into(), toml::Value::Integer(pos.r.into()));
        raw.insert("at".into(), toml::Value::Table(table));
        raw.entry("terrain")
            .or_insert_with(|| toml::Value::String(cell.terrain.clone()));
    }
    if let Some(on) = raw.remove("on") {
        let terrain = on.as_str().ok_or("on 必须是地块种类")?;
        let is_construct = raw.get("kind").and_then(|k| k.as_str()) == Some("construct");
        let tile = city
            .tiles
            .values()
            .find(|t| {
                t.terrain == terrain
                    && (!is_construct || city.free_building_slots(t.id, config) > 0)
            })
            .ok_or_else(|| format!("没有可用的「{terrain}」地块"))?;
        raw.insert("tile".into(), toml::Value::Integer(tile.id as i64));
    }
    Task::deserialize(raw).map_err(|e| e.to_string())
}

fn check_goods(inventory: &Inventory, config: &Config) -> Result<(), ScenarioError> {
    for good in inventory.0.keys() {
        if !config.goods.contains_key(good) {
            return Err(ScenarioError::UnknownId {
                kind: "物品",
                id: good.clone(),
            });
        }
    }
    Ok(())
}
