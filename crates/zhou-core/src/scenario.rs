//! 剧本：用 TOML 描述开局时的人物、城市、人群、士人和工事。
//! 剧本里用 key 互相引用，加载时换成 id。工事通过指令创建，和游戏中走同一套校验。

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

use crate::command::{Command, CommandError, execute};
use crate::config::Config;
use crate::inventory::Inventory;
use crate::state::{Building, Character, City, CityStatus, GameState, Id, Mode, Official, PopGroup, Quota, Task, Tile};

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
    #[error("开局工事「{key}」无效：{source}")]
    Assignment { key: String, source: CommandError },
}

#[derive(Debug, Deserialize)]
struct Scenario {
    #[serde(default)]
    characters: Vec<CharacterDef>,
    #[serde(default)]
    cities: Vec<CityDef>,
}

#[derive(Debug, Deserialize)]
struct CharacterDef {
    key: String,
    name: String,
    #[serde(default = "default_rank")]
    rank: String,
    #[serde(default)]
    reputation: f64,
    /// 私库
    #[serde(default)]
    private_store: Inventory,
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
    /// 地块种类 → 块数
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
    tax_rate: Option<f64>,
    /// 规定任务：每年要上缴给所有者的物资
    quota: Option<Inventory>,
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
    culture: f64,
    happiness: f64,
    /// 对管理者的忠诚度，不填为配置中的初始值
    loyalty: Option<f64>,
}

/// 开局即在任的士人，视为经验已满。
#[derive(Debug, Deserialize)]
struct OfficialDef {
    key: String,
    name: String,
    /// 出身的人群，决定他的技能槽
    from_group: String,
    loyalty: Option<f64>,
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
    let register = |keys: &mut BTreeMap<String, Id>, key: &str, id: Id| -> Result<(), ScenarioError> {
        match keys.insert(key.to_string(), id) {
            Some(_) => Err(ScenarioError::DuplicateKey(key.to_string())),
            None => Ok(()),
        }
    };
    let lookup = |keys: &BTreeMap<String, Id>, key: &str| -> Result<Id, ScenarioError> {
        keys.get(key).copied().ok_or_else(|| ScenarioError::UnknownKey(key.to_string()))
    };

    for def in &scenario.characters {
        let id = state.alloc_id();
        register(&mut keys, &def.key, id)?;
        if !config.ritual.ding_limit.contains_key(&def.rank) {
            return Err(ScenarioError::UnknownId { kind: "爵位", id: def.rank.clone() });
        }
        check_goods(&def.private_store, config)?;
        let mut character = Character::new(id, def.name.clone(), def.rank.clone(), def.reputation);
        character.private_store = def.private_store.clone();
        state.characters.insert(id, character);
    }

    for def in &scenario.cities {
        let city_id = state.alloc_id();
        register(&mut keys, &def.key, city_id)?;
        let mut tiles = BTreeMap::new();
        for (terrain, count) in &def.tiles {
            if !config.terrains.contains_key(terrain) {
                return Err(ScenarioError::UnknownId { kind: "地块", id: terrain.clone() });
            }
            for _ in 0..*count {
                let id = state.alloc_id();
                tiles.insert(id, Tile::new(id, terrain.clone()));
            }
        }
        // 开局的建筑依次放到第一块还有空位、允许建造的地块上
        let buildings = [(Mode::Guan, &def.guan_buildings), (Mode::Min, &def.min_buildings)];
        for (mode, building, count) in buildings.iter().flat_map(|(m, b)| b.iter().map(move |(id, n)| (*m, id, *n))) {
            let Some(allowed) = config.buildings.get(building).map(|b| &b.terrains) else {
                return Err(ScenarioError::UnknownId { kind: "建筑", id: building.clone() });
            };
            for _ in 0..count {
                let tile = tiles
                    .values_mut()
                    .filter(|t| allowed.is_empty() || allowed.contains(&t.terrain))
                    .find(|t| (t.buildings.len() as u32) < config.terrains[&t.terrain].building_slots)
                    .ok_or_else(|| ScenarioError::NoRoomFor(building.clone()))?;
                tile.buildings.push(Building { kind: building.clone(), mode });
            }
        }
        check_goods(&def.treasury, config)?;
        if let Some(quota) = &def.quota {
            check_goods(quota, config)?;
        }
        check_goods(&def.private_wealth, config)?;

        let mut groups = BTreeMap::new();
        for g in &def.groups {
            let id = state.alloc_id();
            register(&mut keys, &g.key, id)?;
            let skills = if g.skills.is_empty() { vec![config.population.initial_skill.clone()] } else { g.skills.clone() };
            if skills.len() > config.population.max_skills {
                return Err(ScenarioError::UnknownId { kind: "技能槽（超过上限）", id: g.key.clone() });
            }
            for skill in &skills {
                if !config.skills.contains_key(skill) {
                    return Err(ScenarioError::UnknownId { kind: "技能", id: skill.clone() });
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
                    growth_carry: 0.0,
                },
            );
        }

        let mut officials = BTreeMap::new();
        for o in &def.officials {
            let id = state.alloc_id();
            register(&mut keys, &o.key, id)?;
            let origin = lookup(&keys, &o.from_group)?;
            let group = groups.get(&origin).ok_or_else(|| ScenarioError::UnknownKey(o.from_group.clone()))?;
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
        let manager = def.manager.as_deref().map(|k| lookup(&keys, k)).transpose()?;
        state.cities.insert(
            city_id,
            City {
                id: city_id,
                name: def.name.clone(),
                owner,
                manager,
                status: def.status,
                totem: def.totem.clone(),
                tiles,
                treasury: def.treasury.clone(),
                private_wealth: def.private_wealth.clone(),
                tax_rate: def.tax_rate.unwrap_or(config.economy.default_tax_rate),
                groups,
                officials,
                assignments: BTreeMap::new(),
                learning: BTreeMap::new(),
                quota: def.quota.clone().map(|goods| Quota { goods, delivered: Inventory::default() }),
                manager_offer: None,
            },
        );

        for a in &def.assignments {
            let shi = lookup(&keys, &a.shi)?;
            let workers = a
                .workers
                .iter()
                .map(|(k, n)| Ok((lookup(&keys, k)?, *n)))
                .collect::<Result<BTreeMap<_, _>, ScenarioError>>()?;
            let task = resolve_task(&a.task, &state.cities[&city_id], config)
                .map_err(|msg| ScenarioError::BadTask { key: a.key.clone(), msg })?;
            let command = Command::StartAssignment { city: city_id, shi, task, workers };
            let created = execute(&mut state, config, &command)
                .map_err(|source| ScenarioError::Assignment { key: a.key.clone(), source })?;
            register(&mut keys, &a.key, created[0])?;
        }
    }
    Ok(state)
}

fn resolve_task(raw: &toml::Table, city: &City, config: &Config) -> Result<Task, String> {
    let mut raw = raw.clone();
    if let Some(on) = raw.remove("on") {
        let terrain = on.as_str().ok_or("on 必须是地块种类")?;
        let is_construct = raw.get("kind").and_then(|k| k.as_str()) == Some("construct");
        let tile = city
            .tiles
            .values()
            .find(|t| t.terrain == terrain && (!is_construct || city.free_building_slots(t.id, config) > 0))
            .ok_or_else(|| format!("没有可用的「{terrain}」地块"))?;
        raw.insert("tile".into(), toml::Value::Integer(tile.id as i64));
    }
    Task::deserialize(raw).map_err(|e| e.to_string())
}

fn check_goods(inventory: &Inventory, config: &Config) -> Result<(), ScenarioError> {
    for good in inventory.0.keys() {
        if !config.goods.contains_key(good) {
            return Err(ScenarioError::UnknownId { kind: "物品", id: good.clone() });
        }
    }
    Ok(())
}
