//! 剧本：用 TOML 描述开局时的人物、城市、人群、士人和工事。
//! 剧本里用 key 互相引用，加载时换成 id。工事通过指令创建，和游戏中走同一套校验。

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::Deserialize;
use thiserror::Error;

use crate::command::{Command, CommandError, execute};
use crate::config::Config;
use crate::inventory::Inventory;
use crate::state::{Character, City, CityStatus, GameState, Id, Official, PopGroup, Task};

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
    #[serde(default)]
    resources: BTreeSet<String>,
    #[serde(default)]
    buildings: BTreeMap<String, u32>,
    #[serde(default)]
    treasury: Inventory,
    /// 民间财富
    #[serde(default)]
    private_wealth: Inventory,
    tax_rate: Option<f64>,
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
    skills: Vec<String>,
    /// 不填则使用城市的文化图腾
    totem: Option<String>,
    #[serde(default)]
    culture: f64,
    happiness: f64,
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
    task: Task,
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
        state.characters.insert(
            id,
            Character { id, name: def.name.clone(), rank: def.rank.clone(), reputation: def.reputation },
        );
    }

    for def in &scenario.cities {
        let city_id = state.alloc_id();
        register(&mut keys, &def.key, city_id)?;
        for building in def.buildings.keys() {
            if !config.buildings.contains_key(building) {
                return Err(ScenarioError::UnknownId { kind: "建筑", id: building.clone() });
            }
        }
        check_goods(&def.treasury, config)?;
        check_goods(&def.private_wealth, config)?;

        let mut groups = BTreeMap::new();
        for g in &def.groups {
            let id = state.alloc_id();
            register(&mut keys, &g.key, id)?;
            for skill in &g.skills {
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
                    skills: g.skills.clone(),
                    totem: g.totem.clone().unwrap_or_else(|| def.totem.clone()),
                    culture: g.culture,
                    happiness: g.happiness,
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
                resources: def.resources.clone(),
                buildings: def.buildings.clone(),
                treasury: def.treasury.clone(),
                private_wealth: def.private_wealth.clone(),
                tax_rate: def.tax_rate.unwrap_or(config.economy.default_tax_rate),
                groups,
                officials,
                assignments: BTreeMap::new(),
            },
        );

        for a in &def.assignments {
            let shi = lookup(&keys, &a.shi)?;
            let workers = a
                .workers
                .iter()
                .map(|(k, n)| Ok((lookup(&keys, k)?, *n)))
                .collect::<Result<BTreeMap<_, _>, ScenarioError>>()?;
            let command = Command::StartAssignment { city: city_id, shi, task: a.task.clone(), workers };
            let created = execute(&mut state, config, &command)
                .map_err(|source| ScenarioError::Assignment { key: a.key.clone(), source })?;
            register(&mut keys, &a.key, created[0])?;
        }
    }
    Ok(state)
}

fn check_goods(inventory: &Inventory, config: &Config) -> Result<(), ScenarioError> {
    for good in inventory.0.keys() {
        if !config.goods.contains_key(good) {
            return Err(ScenarioError::UnknownId { kind: "物品", id: good.clone() });
        }
    }
    Ok(())
}
