//! 指令：玩家和 AI 的所有操作都通过这里，先校验，再执行。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::Config;
use crate::state::{Assignment, City, GameState, Id, Official, Task};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Command {
    /// 从未被管理的人口中任用士人。没有「士」技能的人先习得「士」，当回合即可任用；
    /// 技能槽已满时需要指定丢弃的旧技能。
    AppointShi {
        city: Id,
        group: Id,
        count: u32,
        #[serde(default)]
        drop_skill: Option<String>,
    },
    /// 罢免士人，他回到原来的人群
    DismissShi { city: Id, shi: Id },
    /// 派士人主事一项工事
    StartAssignment {
        city: Id,
        shi: Id,
        task: Task,
        #[serde(default)]
        workers: BTreeMap<Id, u32>,
    },
    CancelAssignment { city: Id, assignment: Id },
    SetTaxRate { city: Id, rate: f64 },
}

impl Command {
    pub fn city(&self) -> Id {
        match self {
            Command::AppointShi { city, .. }
            | Command::DismissShi { city, .. }
            | Command::StartAssignment { city, .. }
            | Command::CancelAssignment { city, .. }
            | Command::SetTaxRate { city, .. } => *city,
        }
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum CommandError {
    #[error("城市 {0} 不存在")]
    NoSuchCity(Id),
    #[error("你不是城市 {0} 的管理者，城中的士人不听你的")]
    NotManager(Id),
    #[error("人群 {0} 不存在")]
    NoSuchGroup(Id),
    #[error("士人 {0} 不存在")]
    NoSuchOfficial(Id),
    #[error("工事 {0} 不存在")]
    NoSuchAssignment(Id),
    #[error("配方「{0}」不存在")]
    NoSuchRecipe(String),
    #[error("建筑「{0}」不存在")]
    NoSuchBuilding(String),
    #[error("人数必须大于 0")]
    ZeroCount,
    #[error("人群 {group} 只有 {available} 人未被管理，不足 {requested} 人")]
    NotEnoughUnmanaged { group: Id, available: u32, requested: u32 },
    #[error("技能槽已满，需要先指定丢弃一项旧技能")]
    SkillSlotsFull,
    #[error("人群没有要丢弃的技能「{0}」")]
    NoSuchSkillToDrop(String),
    #[error("人群 {group} 没有「{skill}」技能")]
    MissingSkill { group: Id, skill: String },
    #[error("士人 {0} 正在主事其他工事")]
    OfficialBusy(Id),
    #[error("投入 {requested} 人，超出士人的管理上限 {capacity} 人")]
    OverCapacity { capacity: u32, requested: u32 },
    #[error("城市缺少自然资源「{0}」")]
    MissingResource(String),
    #[error("城市缺少设施「{0}」")]
    MissingFacility(String),
    #[error("府库材料不足")]
    MissingMaterials,
    #[error("人群 {group} 只有 {available} 人尚未被收税覆盖，不足 {requested} 人")]
    NotEnoughUntaxed { group: Id, available: u32, requested: u32 },
    #[error("祭祀不需要投入人口")]
    SacrificeTakesNoWorkers,
    #[error("府库中没有礼器，无法祭祀")]
    NoVessel,
    #[error("税率必须在 0 到 {0} 之间")]
    InvalidTaxRate(f64),
}

/// 人物 `actor` 发出一条指令：先检查权限，再执行。返回新创建的实体 id（任用的士人或新开的工事）。
pub fn apply(state: &mut GameState, config: &Config, actor: Id, command: &Command) -> Result<Vec<Id>, CommandError> {
    let city_id = command.city();
    let city = state.cities.get(&city_id).ok_or(CommandError::NoSuchCity(city_id))?;
    // 现有的指令都是管理城内事务，只有管理者能发
    if city.manager != Some(actor) {
        return Err(CommandError::NotManager(city_id));
    }
    execute(state, config, command)
}

/// 不检查权限直接执行，只用于剧本加载这类开局设置。
pub(crate) fn execute(state: &mut GameState, config: &Config, command: &Command) -> Result<Vec<Id>, CommandError> {
    match command {
        Command::AppointShi { city, group, count, drop_skill } => {
            appoint_shi(state, config, *city, *group, *count, drop_skill.as_deref())
        }
        Command::DismissShi { city, shi } => {
            let city = city_mut(state, *city)?;
            let official = city.officials.get(shi).ok_or(CommandError::NoSuchOfficial(*shi))?;
            if city.assignment_of(*shi).is_some() {
                return Err(CommandError::OfficialBusy(*shi));
            }
            let origin = official.origin_group;
            city.officials.remove(shi);
            if let Some(group) = city.groups.get_mut(&origin) {
                group.size += 1;
            }
            Ok(vec![])
        }
        Command::StartAssignment { city, shi, task, workers } => {
            start_assignment(state, config, *city, *shi, task, workers)
        }
        Command::CancelAssignment { city, assignment } => {
            let city = city_mut(state, *city)?;
            city.assignments
                .remove(assignment)
                .ok_or(CommandError::NoSuchAssignment(*assignment))?;
            Ok(vec![])
        }
        Command::SetTaxRate { city, rate } => {
            let max = config.economy.max_tax_rate;
            if !(0.0..=max).contains(rate) {
                return Err(CommandError::InvalidTaxRate(max));
            }
            city_mut(state, *city)?.tax_rate = *rate;
            Ok(vec![])
        }
    }
}

fn city_mut(state: &mut GameState, city: Id) -> Result<&mut City, CommandError> {
    state.cities.get_mut(&city).ok_or(CommandError::NoSuchCity(city))
}

fn appoint_shi(
    state: &mut GameState,
    config: &Config,
    city_id: Id,
    group_id: Id,
    count: u32,
    drop_skill: Option<&str>,
) -> Result<Vec<Id>, CommandError> {
    if count == 0 {
        return Err(CommandError::ZeroCount);
    }
    let city = state.cities.get(&city_id).ok_or(CommandError::NoSuchCity(city_id))?;
    let group = city.groups.get(&group_id).ok_or(CommandError::NoSuchGroup(group_id))?;
    let available = city.unmanaged(group_id);
    if count > available {
        return Err(CommandError::NotEnoughUnmanaged { group: group_id, available, requested: count });
    }

    // 被选中的人习得「士」：只有他们改变技能槽，人群里的其他人不受影响
    let shi_skill = &config.shi.skill;
    let mut skills = group.skills.clone();
    if !group.has_skill(shi_skill) {
        if skills.len() >= config.population.max_skills {
            let drop = drop_skill.ok_or(CommandError::SkillSlotsFull)?;
            let index = skills
                .iter()
                .position(|s| s == drop)
                .ok_or_else(|| CommandError::NoSuchSkillToDrop(drop.to_string()))?;
            skills.remove(index);
        }
        skills.push(shi_skill.clone());
    }
    let group_name = group.name.clone();

    let ids: Vec<Id> = (0..count).map(|_| state.alloc_id()).collect();
    let city = state.cities.get_mut(&city_id).expect("city checked above");
    city.groups.get_mut(&group_id).expect("group checked above").size -= count;
    for id in &ids {
        city.officials.insert(
            *id,
            Official {
                id: *id,
                name: format!("{group_name}之士·{id}"),
                skills: skills.clone(),
                loyalty: config.shi.initial_loyalty,
                service_turns: 0,
                origin_group: group_id,
            },
        );
    }
    Ok(ids)
}

fn start_assignment(
    state: &mut GameState,
    config: &Config,
    city_id: Id,
    shi: Id,
    task: &Task,
    workers: &BTreeMap<Id, u32>,
) -> Result<Vec<Id>, CommandError> {
    let city = state.cities.get(&city_id).ok_or(CommandError::NoSuchCity(city_id))?;
    let official = city.officials.get(&shi).ok_or(CommandError::NoSuchOfficial(shi))?;
    if city.assignment_of(shi).is_some() {
        return Err(CommandError::OfficialBusy(shi));
    }
    for (group, count) in workers {
        if !city.groups.contains_key(group) {
            return Err(CommandError::NoSuchGroup(*group));
        }
        if *count == 0 {
            return Err(CommandError::ZeroCount);
        }
    }
    let requested: u32 = workers.values().sum();
    let capacity = official.capacity(config);
    if requested > capacity {
        return Err(CommandError::OverCapacity { capacity, requested });
    }

    let check_labor = |skill: Option<&str>| -> Result<(), CommandError> {
        for (group, count) in workers {
            if let Some(skill) = skill
                && !city.groups[group].has_skill(skill)
            {
                return Err(CommandError::MissingSkill { group: *group, skill: skill.to_string() });
            }
            let available = city.unmanaged(*group);
            if *count > available {
                return Err(CommandError::NotEnoughUnmanaged { group: *group, available, requested: *count });
            }
        }
        Ok(())
    };

    let mut materials = None;
    match task {
        Task::Produce { recipe, .. } => {
            let def = config.recipes.get(recipe).ok_or_else(|| CommandError::NoSuchRecipe(recipe.clone()))?;
            if let Some(resource) = &def.resource
                && !city.resources.contains(resource)
            {
                return Err(CommandError::MissingResource(resource.clone()));
            }
            if let Some(facility) = &def.facility
                && !city.has_building(facility)
            {
                return Err(CommandError::MissingFacility(facility.clone()));
            }
            check_labor(Some(&def.skill))?;
        }
        Task::Construct { building } => {
            let def = config
                .buildings
                .get(building)
                .ok_or_else(|| CommandError::NoSuchBuilding(building.clone()))?;
            if !city.treasury.contains_all(&def.materials) {
                return Err(CommandError::MissingMaterials);
            }
            check_labor(def.skill.as_deref())?;
            materials = Some(def.materials.clone());
        }
        Task::CollectTax => {
            for (group, count) in workers {
                let available = city.groups[group].size.saturating_sub(city.tax_covered(*group));
                if *count > available {
                    return Err(CommandError::NotEnoughUntaxed { group: *group, available, requested: *count });
                }
            }
        }
        Task::Sacrifice => {
            if !workers.is_empty() {
                return Err(CommandError::SacrificeTakesNoWorkers);
            }
            let vessels: f64 = config.sacrifice.vessel_goods.iter().map(|g| city.treasury.get(g).floor()).sum();
            if vessels < 1.0 {
                return Err(CommandError::NoVessel);
            }
        }
    }

    let id = state.alloc_id();
    let city = state.cities.get_mut(&city_id).expect("city checked above");
    if let Some(materials) = materials {
        for (good, amount) in materials.iter() {
            city.treasury.take(good, *amount);
        }
    }
    city.assignments.insert(
        id,
        Assignment { id, shi, task: task.clone(), workers: workers.clone(), progress: 0.0 },
    );
    Ok(vec![id])
}
