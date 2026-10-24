//! 指令：玩家和 AI 的所有操作都通过这里，先校验，再执行。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::Config;
use crate::inventory::Inventory;
use crate::state::{Assignment, City, CityStatus, GameState, Id, Learning, Mode, Official, Quota, Task};

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
    /// 赏赐民众：把府库中的物资转入民间财富
    Grant { city: Id, goods: Inventory },
    /// 让一群人中的 `count` 人学一项新技能，不需要士人主事。正在做工事的人也可以学，学习期间照常干活。
    /// 技能槽已满时需要指定丢弃的旧技能。学成后这些人分出去，或并入技能槽相同的人群
    LearnSkill {
        city: Id,
        group: Id,
        count: u32,
        skill: String,
        #[serde(default)]
        drop_skill: Option<String>,
    },
    /// 所有者任命某人为管理者，对方接受后才生效
    OfferManagement { city: Id, to: Id },
    /// 被任命的人接受任命，成为管理者
    AcceptManagement { city: Id },
    /// 被任命的人拒绝任命
    DeclineManagement { city: Id },
    /// 所有者收回管理权，自己管理。不需要管理者同意
    RevokeManagement { city: Id },
    /// 所有者给管理者设定规定任务：每年要上缴的物资。为空表示取消
    SetQuota { city: Id, goods: Inventory },
    /// 管理者从府库上缴物资到所有者的私库，计入今年的规定任务
    Deliver { city: Id, goods: Inventory },
    /// 丢弃技能、调整技能顺序，当回合生效。`skills` 只能由这群人现有的技能组成，顺序即优先顺序。
    /// 正在学技能的人要等学成再调整
    ArrangeSkills { city: Id, group: Id, count: u32, skills: Vec<String> },
}

impl Command {
    pub fn city(&self) -> Id {
        match self {
            Command::AppointShi { city, .. }
            | Command::DismissShi { city, .. }
            | Command::StartAssignment { city, .. }
            | Command::CancelAssignment { city, .. }
            | Command::SetTaxRate { city, .. }
            | Command::Grant { city, .. }
            | Command::LearnSkill { city, .. }
            | Command::ArrangeSkills { city, .. }
            | Command::OfferManagement { city, .. }
            | Command::AcceptManagement { city }
            | Command::DeclineManagement { city }
            | Command::RevokeManagement { city }
            | Command::SetQuota { city, .. }
            | Command::Deliver { city, .. } => *city,
        }
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum CommandError {
    #[error("城市 {0} 不存在")]
    NoSuchCity(Id),
    #[error("你不是城市 {0} 的管理者，城中的士人不听你的")]
    NotManager(Id),
    #[error("你不是城市 {0} 的所有者")]
    NotOwner(Id),
    #[error("城市 {0} 被攻占，所有者已经失去管理权")]
    CityOccupied(Id),
    #[error("人物 {0} 不存在")]
    NoSuchCharacter(Id),
    #[error("人物 {0} 已经是管理者")]
    AlreadyManager(Id),
    #[error("人物 {0} 已经在管理另一座城市，一个人只能直接管理一座")]
    ManagesAnotherCity(Id),
    #[error("没有任命你管理城市 {0}")]
    NotOffered(Id),
    #[error("所有者自己管理这座城，不需要规定任务")]
    OwnerIsManager,
    #[error("这座城没有所有者")]
    NoOwner,
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
    #[error("城中没有{}的「{facility}」", mode_name(*.mode))]
    MissingFacility { facility: String, mode: Mode },
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
    #[error("「{pool}」只能再容纳 {available} 人劳作，不足 {requested} 人")]
    NoJobs { pool: String, available: u32, requested: u32 },
    #[error("人群 {group} 的专精度不够，做不了「{recipe}」")]
    SpecializationTooLow { group: Id, recipe: String },
    #[error("地块 {0} 不存在")]
    NoSuchTile(Id),
    #[error("改良「{0}」不存在")]
    NoSuchImprovement(String),
    #[error("这块地不能做「{0}」")]
    CannotImprove(String),
    #[error("这块地的「{0}」已经到最高等级")]
    MaxImprovement(String),
    #[error("这块地已经在做「{0}」")]
    AlreadyImproving(String),
    #[error("技能槽无效：{0}")]
    InvalidSkills(String),
    #[error("技能「{0}」不存在")]
    NoSuchSkill(String),
    #[error("人群 {group} 只有 {available} 人没在学技能，不足 {requested} 人")]
    NotEnoughNotLearning { group: Id, available: u32, requested: u32 },
    #[error("这群人已经会「{0}」")]
    AlreadyHasSkill(String),
    #[error("这块地上的建筑已满")]
    NoBuildingSlot,
    #[error("「{0}」不能建在这种地上")]
    WrongTerrain(String),
    #[error("地块种类「{0}」不存在")]
    NoSuchTerrain(String),
    #[error("「{0}」开垦不出来")]
    CannotReclaim(String),
    #[error("物品「{0}」不存在")]
    NoSuchGood(String),
    #[error("府库中的「{0}」不够")]
    NotEnoughInTreasury(String),
}

fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Guan => "官营",
        Mode::Min => "民营",
    }
}

/// 人物 `actor` 发出一条指令：先检查权限，再执行。返回新创建的实体 id（任用的士人、新开的工事、学技能的批次、调整技能后归入的人群）。
pub fn apply(state: &mut GameState, config: &Config, actor: Id, command: &Command) -> Result<Vec<Id>, CommandError> {
    let city_id = command.city();
    let city = state.cities.get(&city_id).ok_or(CommandError::NoSuchCity(city_id))?;
    match command {
        Command::OfferManagement { to, .. } => {
            check_owner(city, actor)?;
            if !state.characters.contains_key(to) {
                return Err(CommandError::NoSuchCharacter(*to));
            }
            if city.manager == Some(*to) {
                return Err(CommandError::AlreadyManager(*to));
            }
            city_mut(state, city_id)?.manager_offer = Some(*to);
            Ok(vec![])
        }
        Command::AcceptManagement { .. } => {
            if city.manager_offer != Some(actor) {
                return Err(CommandError::NotOffered(city_id));
            }
            check_manages_no_other(state, city_id, actor)?;
            city_mut(state, city_id)?.change_manager(actor, config);
            Ok(vec![])
        }
        Command::DeclineManagement { .. } => {
            if city.manager_offer != Some(actor) {
                return Err(CommandError::NotOffered(city_id));
            }
            city_mut(state, city_id)?.manager_offer = None;
            Ok(vec![])
        }
        Command::RevokeManagement { .. } => {
            check_owner(city, actor)?;
            if city.manager == Some(actor) {
                return Err(CommandError::AlreadyManager(actor));
            }
            check_manages_no_other(state, city_id, actor)?;
            city_mut(state, city_id)?.change_manager(actor, config);
            Ok(vec![])
        }
        Command::SetQuota { goods, .. } => {
            check_owner(city, actor)?;
            if city.manager == Some(actor) {
                return Err(CommandError::OwnerIsManager);
            }
            check_goods(config, goods)?;
            let city = city_mut(state, city_id)?;
            if goods.iter().next().is_none() {
                city.quota = None;
            } else {
                // 改任务不清零今年已经上缴的
                let delivered = city.quota.take().map(|q| q.delivered).unwrap_or_default();
                city.quota = Some(Quota { goods: goods.clone(), delivered });
            }
            Ok(vec![])
        }
        // 其余指令都是管理城内事务，只有管理者能发
        _ => {
            if city.manager != Some(actor) {
                return Err(CommandError::NotManager(city_id));
            }
            execute(state, config, command)
        }
    }
}

/// 所有者才能发的指令。城市被攻占时，所有者已经失去管理权。
fn check_owner(city: &City, actor: Id) -> Result<(), CommandError> {
    if city.owner != Some(actor) {
        return Err(CommandError::NotOwner(city.id));
    }
    if city.status == CityStatus::Occupied {
        return Err(CommandError::CityOccupied(city.id));
    }
    Ok(())
}

/// 一个人只能直接管理一座城。
fn check_manages_no_other(state: &GameState, city: Id, who: Id) -> Result<(), CommandError> {
    if state.cities.values().any(|c| c.id != city && c.manager == Some(who)) {
        return Err(CommandError::ManagesAnotherCity(who));
    }
    Ok(())
}

/// 物品都存在、数量都大于 0。
fn check_goods(config: &Config, goods: &Inventory) -> Result<(), CommandError> {
    for (good, amount) in goods.iter() {
        if !config.goods.contains_key(good) {
            return Err(CommandError::NoSuchGood(good.clone()));
        }
        if *amount <= 0.0 {
            return Err(CommandError::ZeroCount);
        }
    }
    Ok(())
}

/// 物品有效，并且府库里都有这么多。
fn check_treasury(config: &Config, city: &City, goods: &Inventory) -> Result<(), CommandError> {
    check_goods(config, goods)?;
    if goods.iter().next().is_none() {
        return Err(CommandError::ZeroCount);
    }
    for (good, amount) in goods.iter() {
        if city.treasury.get(good) + 1e-9 < *amount {
            return Err(CommandError::NotEnoughInTreasury(good.clone()));
        }
    }
    Ok(())
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
        Command::Grant { city, goods } => {
            let city = city_mut(state, *city)?;
            check_treasury(config, city, goods)?;
            for (good, amount) in goods.iter() {
                let taken = city.treasury.take(good, *amount);
                city.private_wealth.add(good, taken);
            }
            // 受了赏赐，民众对管理者更忠诚：按人均得到的价值折算
            let population = city.population();
            if population > 0 {
                let per_person = config.value_of(goods) / population as f64;
                let gain = per_person * config.loyalty.grant_gain_per_value;
                for group in city.groups.values_mut() {
                    group.loyalty = (group.loyalty + gain).min(config.loyalty.max);
                }
            }
            Ok(vec![])
        }
        Command::Deliver { city: city_id, goods } => {
            let city = state.cities.get(city_id).ok_or(CommandError::NoSuchCity(*city_id))?;
            // 被攻占的城和原所有者之间没有上下级关系，不用上缴
            if city.status == CityStatus::Occupied {
                return Err(CommandError::CityOccupied(*city_id));
            }
            check_treasury(config, city, goods)?;
            let owner = city.owner.ok_or(CommandError::NoOwner)?;
            let city = state.cities.get_mut(city_id).expect("city checked above");
            let mut delivered = Inventory::default();
            for (good, amount) in goods.iter() {
                delivered.add(good, city.treasury.take(good, *amount));
            }
            if let Some(quota) = &mut city.quota {
                quota.delivered.add_all(&delivered);
            }
            if let Some(owner) = state.characters.get_mut(&owner) {
                owner.private_store.add_all(&delivered);
            }
            Ok(vec![])
        }
        // 涉及所有权和管理权的指令要知道是谁发的，只能通过 apply
        Command::OfferManagement { city, .. }
        | Command::AcceptManagement { city }
        | Command::DeclineManagement { city }
        | Command::RevokeManagement { city }
        | Command::SetQuota { city, .. } => Err(CommandError::NotOwner(*city)),
        Command::LearnSkill { city, group, count, skill, drop_skill } => {
            learn_skill(state, config, *city, *group, *count, skill, drop_skill.as_deref())
        }
        Command::ArrangeSkills { city, group, count, skills } => {
            arrange_skills(state, config, *city, *group, *count, skills)
        }
    }
}

/// 检查一群人中有没有 `count` 名没在学技能的人。正在做工事的人也可以，一边干活一边学。
fn check_not_learning(city: &City, group: Id, count: u32) -> Result<(), CommandError> {
    if count == 0 {
        return Err(CommandError::ZeroCount);
    }
    let size = city.groups.get(&group).ok_or(CommandError::NoSuchGroup(group))?.size;
    let available = size.saturating_sub(city.learning_in(group));
    if count > available {
        return Err(CommandError::NotEnoughNotLearning { group, available, requested: count });
    }
    Ok(())
}

fn learn_skill(
    state: &mut GameState,
    config: &Config,
    city_id: Id,
    group_id: Id,
    count: u32,
    skill: &str,
    drop_skill: Option<&str>,
) -> Result<Vec<Id>, CommandError> {
    let city = state.cities.get(&city_id).ok_or(CommandError::NoSuchCity(city_id))?;
    check_not_learning(city, group_id, count)?;
    if !config.skills.contains_key(skill) {
        return Err(CommandError::NoSuchSkill(skill.to_string()));
    }
    let group = &city.groups[&group_id];
    if group.has_skill(skill) {
        return Err(CommandError::AlreadyHasSkill(config.skill_name(skill).to_string()));
    }
    let mut skills = group.skills.clone();
    if skills.len() >= config.population.max_skills {
        let drop = drop_skill.ok_or(CommandError::SkillSlotsFull)?;
        let index = skills
            .iter()
            .position(|s| s == drop)
            .ok_or_else(|| CommandError::NoSuchSkillToDrop(drop.to_string()))?;
        skills.remove(index);
    }
    skills.push(skill.to_string());

    let id = state.alloc_id();
    let city = state.cities.get_mut(&city_id).expect("city checked above");
    city.learning.insert(
        id,
        Learning { id, group: group_id, count, skills, skill: skill.to_string(), turns: 0 },
    );
    Ok(vec![id])
}

fn arrange_skills(
    state: &mut GameState,
    config: &Config,
    city_id: Id,
    group_id: Id,
    count: u32,
    skills: &[String],
) -> Result<Vec<Id>, CommandError> {
    let city = state.cities.get(&city_id).ok_or(CommandError::NoSuchCity(city_id))?;
    check_not_learning(city, group_id, count)?;
    let current = &city.groups[&group_id].skills;
    let invalid = |msg: String| Err(CommandError::InvalidSkills(msg));
    if skills.is_empty() {
        return invalid("至少要保留一项技能".into());
    }
    if skills == current.as_slice() {
        return invalid("和现在的技能槽相同".into());
    }
    for (i, skill) in skills.iter().enumerate() {
        if !current.contains(skill) {
            return invalid(format!("这群人不会「{}」，新技能要通过学技能获得", config.skill_name(skill)));
        }
        if skills[..i].contains(skill) {
            return invalid(format!("技能「{}」重复", config.skill_name(skill)));
        }
    }
    let GameState { cities, next_id, .. } = state;
    let city = cities.get_mut(&city_id).expect("city checked above");
    Ok(vec![city.move_people(config, group_id, count, skills, next_id)])
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
    if !city.officials.contains_key(&shi) {
        return Err(CommandError::NoSuchOfficial(shi));
    }
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
    let capacity = city.official_capacity(shi, config);
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
        Task::Produce { recipe, mode } => {
            let def = config.recipes.get(recipe).ok_or_else(|| CommandError::NoSuchRecipe(recipe.clone()))?;
            if let Some(resource) = &def.resource
                && !city.has_terrain(resource)
            {
                return Err(CommandError::MissingResource(resource.clone()));
            }
            // 官营只能在官营的建筑里做，民营只能在民营的建筑里做
            if let Some(facility) = &def.facility
                && !city.has_building(facility, *mode)
            {
                return Err(CommandError::MissingFacility { facility: config.buildings[facility].name.clone(), mode: *mode });
            }
            check_labor(Some(&def.skill))?;
            if let Some(min) = def.min_specialization {
                for group in workers.keys() {
                    if config.specialization(city.groups[group].skills.len()) + 1e-9 < min {
                        return Err(CommandError::SpecializationTooLow { group: *group, recipe: def.name.clone() });
                    }
                }
            }
            // 工事优先于未被管理的人口占用劳作位置，只需要和其他工事比
            if let Some(pool) = config.pool_of(recipe) {
                let available = city.free_jobs(config, &pool, *mode);
                if requested > available {
                    return Err(CommandError::NoJobs { pool: config.pool_name(&pool).to_string(), available, requested });
                }
            }
        }
        Task::Improve { tile, improvement } => {
            let def = config
                .improvements
                .get(improvement)
                .ok_or_else(|| CommandError::NoSuchImprovement(improvement.clone()))?;
            let target = city.tiles.get(tile).ok_or(CommandError::NoSuchTile(*tile))?;
            if !def.terrains.contains(&target.terrain) {
                return Err(CommandError::CannotImprove(def.name.clone()));
            }
            if target.level(improvement) >= def.max_level {
                return Err(CommandError::MaxImprovement(def.name.clone()));
            }
            if city.assignments.values().any(|a| &a.task == task) {
                return Err(CommandError::AlreadyImproving(def.name.clone()));
            }
            if !city.treasury.contains_all(&def.materials) {
                return Err(CommandError::MissingMaterials);
            }
            check_labor(def.skill.as_deref())?;
            materials = Some(def.materials.clone());
        }
        Task::Construct { building, tile, .. } => {
            let def = config
                .buildings
                .get(building)
                .ok_or_else(|| CommandError::NoSuchBuilding(building.clone()))?;
            let target = city.tiles.get(tile).ok_or(CommandError::NoSuchTile(*tile))?;
            if !def.terrains.is_empty() && !def.terrains.contains(&target.terrain) {
                return Err(CommandError::WrongTerrain(def.name.clone()));
            }
            if city.free_building_slots(*tile, config) == 0 {
                return Err(CommandError::NoBuildingSlot);
            }
            if !city.treasury.contains_all(&def.materials) {
                return Err(CommandError::MissingMaterials);
            }
            check_labor(def.skill.as_deref())?;
            materials = Some(def.materials.clone());
        }
        Task::Reclaim { terrain } => {
            let def = config.terrains.get(terrain).ok_or_else(|| CommandError::NoSuchTerrain(terrain.clone()))?;
            let skill = def.reclaim_skill.as_deref().ok_or_else(|| CommandError::CannotReclaim(def.name.clone()))?;
            check_labor(Some(skill))?;
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
            let vessels: f64 = config.sacrifice.vessels.keys().map(|g| city.treasury.get(g).floor()).sum();
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
