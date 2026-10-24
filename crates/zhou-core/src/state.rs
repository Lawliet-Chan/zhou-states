use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::{Config, Pool};
use crate::inventory::Inventory;

/// 所有实体共用一个递增的 id 空间。
pub type Id = u32;

/// 整个游戏世界的状态。所有容器都用 BTreeMap，保证遍历顺序固定、结算结果可复现。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GameState {
    /// 下一个要结算的回合，从 0 开始
    pub turn: u32,
    pub next_id: Id,
    pub characters: BTreeMap<Id, Character>,
    pub cities: BTreeMap<Id, City>,
}

impl GameState {
    pub fn alloc_id(&mut self) -> Id {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn season_index(&self, config: &Config) -> usize {
        self.turn as usize % config.seasons.len()
    }

    pub fn year(&self, config: &Config) -> u32 {
        self.turn / config.seasons.len() as u32 + 1
    }
}

/// 人物：玩家，或者出仕成为人物的士。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Character {
    pub id: Id,
    pub name: String,
    /// 爵位：tianzi / gong / hou / bo / zi / nan / none
    pub rank: String,
    pub reputation: f64,
    /// 已经因僭越扣过风评的超出量。僭越只扣一次，超出得更多时再扣多出的部分
    #[serde(default)]
    pub punished_ding_excess: u32,
    /// 城市 id → 已经扣过风评的超出地块数
    #[serde(default)]
    pub punished_tile_excess: BTreeMap<Id, u32>,
    /// 私库：不属于任何城市的个人财物，比如管理者上缴的物资
    #[serde(default)]
    pub private_store: Inventory,
    /// 人物 id → 这个人对他的信任，没有记录时为初始值
    #[serde(default)]
    pub trust: BTreeMap<Id, f64>,
}

impl Character {
    pub fn new(id: Id, name: impl Into<String>, rank: impl Into<String>, reputation: f64) -> Self {
        Character {
            id,
            name: name.into(),
            rank: rank.into(),
            reputation,
            punished_ding_excess: 0,
            punished_tile_excess: BTreeMap::new(),
            private_store: Inventory::default(),
            trust: BTreeMap::new(),
        }
    }

    pub fn trust_in(&self, other: Id, config: &Config) -> f64 {
        self.trust.get(&other).copied().unwrap_or(config.quota.initial_trust)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CityStatus {
    /// 所有权和管理权都不属于任何人
    Free,
    /// 被攻占
    Occupied,
    /// 被统治
    Ruled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct City {
    pub id: Id,
    pub name: String,
    pub owner: Option<Id>,
    pub manager: Option<Id>,
    pub status: CityStatus,
    pub totem: String,
    /// 城市的地块
    pub tiles: BTreeMap<Id, Tile>,
    pub treasury: Inventory,
    /// 民间财富：民营产出归全城民众共有，城内各群人之间自动互通
    pub private_wealth: Inventory,
    pub tax_rate: f64,
    pub groups: BTreeMap<Id, PopGroup>,
    pub officials: BTreeMap<Id, Official>,
    pub assignments: BTreeMap<Id, Assignment>,
    /// 正在学技能的人，不需要士人主事
    #[serde(default)]
    pub learning: BTreeMap<Id, Learning>,
    /// 所有者给管理者定的规定任务
    #[serde(default)]
    pub quota: Option<Quota>,
    /// 所有者发出、对方还没答复的管理权任命
    #[serde(default)]
    pub manager_offer: Option<Id>,
}

/// 规定任务：每年要上缴给所有者的物资，冬季结算后清零重新累计。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Quota {
    /// 每年要上缴的物资
    pub goods: Inventory,
    /// 今年已经上缴的物资
    #[serde(default)]
    pub delivered: Inventory,
}

impl City {
    /// 所有者、管理者、城市状态带来的产能倍率。
    pub fn efficiency(&self, config: &Config) -> f64 {
        let mut mult = 1.0;
        if self.owner != self.manager {
            mult *= config.economy.absentee_mult;
        }
        if self.status == CityStatus::Occupied {
            mult *= config.economy.occupied_mult;
        }
        mult
    }

    /// 全城的建筑：建筑 id → 数量，官营和民营合计。
    pub fn buildings(&self) -> BTreeMap<String, u32> {
        let mut all = BTreeMap::new();
        for b in self.tiles.values().flat_map(|t| &t.buildings) {
            *all.entry(b.kind.clone()).or_insert(0) += 1;
        }
        all
    }

    /// 城中有没有某种经营方式的某种建筑。
    pub fn has_building(&self, kind: &str, mode: Mode) -> bool {
        self.tiles.values().flat_map(|t| &t.buildings).any(|b| b.kind == kind && b.mode == mode)
    }

    /// 一块地上还能再建几座建筑，扣除正在营建的。
    pub fn free_building_slots(&self, tile: Id, config: &Config) -> u32 {
        let Some(t) = self.tiles.get(&tile) else { return 0 };
        let pending = self
            .assignments
            .values()
            .filter(|a| matches!(&a.task, Task::Construct { tile: target, .. } if *target == tile))
            .count() as u32;
        config.terrains[&t.terrain].building_slots.saturating_sub(t.buildings.len() as u32 + pending)
    }

    pub fn has_terrain(&self, terrain: &str) -> bool {
        self.tiles.values().any(|t| t.terrain == terrain)
    }

    /// 全城能居住的人口：地块加建筑。
    pub fn housing(&self, config: &Config) -> u32 {
        let tiles: u32 = self.tiles.values().map(|t| config.terrains[&t.terrain].housing).sum();
        let buildings: u32 = self.buildings().iter().map(|(id, n)| config.buildings[id].housing * n).sum();
        tiles + buildings
    }

    /// 一个容量来源的全部劳作位置。产量高的排在前面，先被占用；
    /// 产量相同时，只供一种经营方式的位置排在前面，把官营民营都能用的位置留到后面。
    pub fn job_slots(&self, config: &Config, pool: &Pool) -> Vec<JobSlot> {
        let mut slots = match pool {
            Pool::Terrain(terrain) => self
                .tiles
                .values()
                .filter(|t| &t.terrain == terrain)
                .map(|t| JobSlot { count: config.terrains[terrain].jobs, mult: t.yield_mult(config), mode: None })
                .collect(),
            Pool::Recipe(recipe) => {
                let base = config.recipes[recipe].base_jobs.unwrap_or(0);
                let mut slots = vec![JobSlot { count: base, mult: 1.0, mode: None }];
                for b in self.tiles.values().flat_map(|t| &t.buildings) {
                    if let Some(count) = config.buildings[&b.kind].jobs.get(recipe) {
                        slots.push(JobSlot { count: *count, mult: 1.0, mode: Some(b.mode) });
                    }
                }
                slots
            }
        };
        slots.sort_by(|a: &JobSlot, b| b.mult.total_cmp(&a.mult).then(b.mode.is_some().cmp(&a.mode.is_some())));
        slots
    }

    /// 一个容量来源的全部劳作位置数，官营和民营合计。
    pub fn job_capacity(&self, config: &Config, pool: &Pool) -> u32 {
        self.job_slots(config, pool).iter().map(|s| s.count).sum()
    }

    /// 某种经营方式的工事还能占用多少劳作位置。工事先占只供自己这种经营方式的位置，
    /// 不够再占两种都能用的位置。
    pub fn free_jobs(&self, config: &Config, pool: &Pool, mode: Mode) -> u32 {
        let slots = self.job_slots(config, pool);
        let count = |m: Option<Mode>| -> u32 { slots.iter().filter(|s| s.mode == m).map(|s| s.count).sum() };
        let managed = |m: Mode| -> u32 {
            self.assignments
                .values()
                .filter(|a| match &a.task {
                    Task::Produce { recipe, mode } => *mode == m && config.pool_of(recipe).as_ref() == Some(pool),
                    _ => false,
                })
                .map(Assignment::worker_count)
                .sum()
        };
        let other = match mode {
            Mode::Guan => Mode::Min,
            Mode::Min => Mode::Guan,
        };
        let shared_taken = managed(other).saturating_sub(count(Some(other)));
        (count(Some(mode)) + count(None)).saturating_sub(managed(mode) + shared_taken)
    }

    /// 某名士人在本城能管理的人口上限，包含建筑的加成。
    pub fn official_capacity(&self, official: Id, config: &Config) -> u32 {
        let Some(official) = self.officials.get(&official) else { return 0 };
        let bonus: f64 = self
            .buildings()
            .iter()
            .map(|(id, n)| config.buildings[id].shi_capacity_bonus * *n as f64)
            .sum();
        (official.base_capacity(config) * (1.0 + bonus)).floor() as u32
    }

    /// 某群人中被工事占用的劳力（生产、营建等）。学技能不占用劳力，可以一边干活一边学。
    pub fn labor_assigned(&self, group: Id) -> u32 {
        self.assignments
            .values()
            .filter(|a| a.task.uses_labor())
            .filter_map(|a| a.workers.get(&group))
            .sum()
    }

    /// 某群人中正在学技能的人数。
    pub fn learning_in(&self, group: Id) -> u32 {
        self.learning.values().filter(|l| l.group == group).map(|l| l.count).sum()
    }

    /// 从一群人中分出 `count` 人，技能槽改为 `skills`，返回他们归入的人群：
    /// 城中已有技能槽和文化图腾都相同的人群就并入，否则成为一群新的人。
    /// 优先分出未被管理的人；正在做工事的人被分出去后，带着原来的工事继续做，
    /// 只有新技能槽做不了这项工事时（比如丢弃了需要的技能）才撤下来。
    pub fn move_people(&mut self, config: &Config, source_id: Id, count: u32, skills: &[String], next_id: &mut Id) -> Id {
        let target_id = self.split_group(config, source_id, count, skills, next_id);
        if target_id == source_id {
            // 整群人直接改了技能槽：做不了的工事撤下来
            let group = self.groups[&source_id].clone();
            for a in self.assignments.values_mut() {
                if !a.task.can_be_done_by(config, &group) {
                    a.workers.remove(&source_id);
                }
            }
        } else {
            self.follow_people(config, source_id, target_id);
        }
        self.end_empty_assignments();
        target_id
    }

    /// 换管理者：各群人对新管理者的忠诚度归零重来，未答复的任命作废。
    pub fn change_manager(&mut self, manager: Id, config: &Config) {
        self.manager = Some(manager);
        self.manager_offer = None;
        for group in self.groups.values_mut() {
            group.loyalty = config.loyalty.reset;
        }
        // 所有者自己管理时不需要规定任务；交给别人时，今年已上缴的不再算给新管理者
        if self.owner == Some(manager) {
            self.quota = None;
        } else if let Some(quota) = &mut self.quota {
            quota.delivered = Inventory::default();
        }
    }

    /// 结束已经没有人的工事，主事的士人空出来，返回被结束的工事。祭祀本来就不投入人口，不算。
    pub fn end_empty_assignments(&mut self) -> Vec<Assignment> {
        let empty: Vec<Id> = self
            .assignments
            .values()
            .filter(|a| !matches!(a.task, Task::Sacrifice) && a.worker_count() == 0)
            .map(|a| a.id)
            .collect();
        empty.iter().filter_map(|id| self.assignments.remove(id)).collect()
    }

    /// 人群分出人后，原人群的工事投入超出人数时，把超出的部分转给分出去的人群。
    /// 后开的工事先转；收税覆盖的人数也一样处理。
    fn follow_people(&mut self, config: &Config, source_id: Id, target_id: Id) {
        let size = self.groups[&source_id].size;
        let target = self.groups[&target_id].clone();
        for labor in [true, false] {
            let mut total: u32 = self
                .assignments
                .values()
                .filter(|a| a.task.uses_labor() == labor)
                .filter_map(|a| a.workers.get(&source_id))
                .sum();
            for a in self.assignments.values_mut().rev() {
                if total <= size {
                    break;
                }
                if a.task.uses_labor() != labor {
                    continue;
                }
                let Some(count) = a.workers.get_mut(&source_id) else { continue };
                let moved = (*count).min(total - size);
                *count -= moved;
                total -= moved;
                if *count == 0 {
                    a.workers.remove(&source_id);
                }
                if a.task.can_be_done_by(config, &target) {
                    *a.workers.entry(target_id).or_insert(0) += moved;
                }
            }
        }
    }

    fn split_group(&mut self, config: &Config, source_id: Id, count: u32, skills: &[String], next_id: &mut Id) -> Id {
        let source = self.groups.get_mut(&source_id).expect("source group exists");
        let count = count.min(source.size);
        let (totem, culture, happiness, loyalty) = (source.totem.clone(), source.culture, source.happiness, source.loyalty);
        let existing = self
            .groups
            .values()
            .find(|g| g.id != source_id && g.skills == skills && g.totem == totem)
            .map(|g| g.id);
        let source = self.groups.get_mut(&source_id).expect("source group exists");
        // 整群人一起改，又没有可以并入的人群：直接改这群人的技能槽
        if existing.is_none() && count == source.size {
            source.skills = skills.to_vec();
            return source_id;
        }
        source.size -= count;
        let name = format!("{}·{}", source.name, config.skill_name(&skills[0]));
        if let Some(target) = existing.and_then(|id| self.groups.get_mut(&id)) {
            let total = (target.size + count).max(1) as f64;
            target.happiness = (target.happiness * target.size as f64 + happiness * count as f64) / total;
            target.culture = (target.culture * target.size as f64 + culture * count as f64) / total;
            target.loyalty = (target.loyalty * target.size as f64 + loyalty * count as f64) / total;
            target.size += count;
            return target.id;
        }
        let id = *next_id;
        *next_id += 1;
        self.groups.insert(
            id,
            PopGroup {
                id,
                name,
                size: count,
                skills: skills.to_vec(),
                totem,
                culture,
                happiness,
                loyalty,
                growth_carry: 0.0,
            },
        );
        id
    }

    /// 某群人中被收税工事覆盖的人数。
    pub fn tax_covered(&self, group: Id) -> u32 {
        self.assignments
            .values()
            .filter(|a| matches!(a.task, Task::CollectTax))
            .filter_map(|a| a.workers.get(&group))
            .sum()
    }

    /// 某群人中未被管理的人数。
    pub fn unmanaged(&self, group: Id) -> u32 {
        let size = self.groups.get(&group).map_or(0, |g| g.size);
        size.saturating_sub(self.labor_assigned(group))
    }

    pub fn assignment_of(&self, official: Id) -> Option<&Assignment> {
        self.assignments.values().find(|a| a.shi == official)
    }

    pub fn population(&self) -> u32 {
        self.groups.values().map(|g| g.size).sum()
    }
}

/// 一组劳作位置。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct JobSlot {
    pub count: u32,
    /// 产量倍率，比如水利
    pub mult: f64,
    /// 只供这种经营方式劳作，比如官营的市只容纳官营的商贾；None 表示都能用
    pub mode: Option<Mode>,
}

/// 一块地。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tile {
    pub id: Id,
    pub terrain: String,
    /// 改良 id → 等级
    #[serde(default)]
    pub improvements: BTreeMap<String, u32>,
    /// 建在这块地上的建筑。建筑不影响这块地的居住和劳作
    #[serde(default)]
    pub buildings: Vec<Building>,
}

/// 一座建筑。官营还是民营在营建时定下，之后不能改。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Building {
    pub kind: String,
    pub mode: Mode,
}

impl Tile {
    pub fn new(id: Id, terrain: impl Into<String>) -> Self {
        Tile { id, terrain: terrain.into(), improvements: BTreeMap::new(), buildings: Vec::new() }
    }

    pub fn yield_mult(&self, config: &Config) -> f64 {
        self.improvements
            .iter()
            .map(|(id, level)| 1.0 + config.improvements[id].yield_bonus * *level as f64)
            .product()
    }

    pub fn level(&self, improvement: &str) -> u32 {
        self.improvements.get(improvement).copied().unwrap_or(0)
    }
}

/// 一群人：同一座城中文化和技能组合相同的人口。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PopGroup {
    pub id: Id,
    pub name: String,
    pub size: u32,
    /// 技能槽，顺序有意义：未被管理时按第一项技能自主民营
    pub skills: Vec<String>,
    pub totem: String,
    pub culture: f64,
    pub happiness: f64,
    /// 对现任管理者个人的忠诚度，换管理者时归零重来
    #[serde(default)]
    pub loyalty: f64,
    /// 人口增长的小数部分，攒满 1 人再加入
    pub growth_carry: f64,
}

impl PopGroup {
    pub fn has_skill(&self, skill: &str) -> bool {
        self.skills.iter().any(|s| s == skill)
    }
}

/// 被任用的士人。他已经离开原来的人群，由府库供养。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Official {
    pub id: Id,
    pub name: String,
    pub skills: Vec<String>,
    pub loyalty: f64,
    /// 已任职的回合数，影响管理上限
    pub service_turns: u32,
    /// 原来所属的人群，罢免后回到这里
    pub origin_group: Id,
}

impl Official {
    /// 能管理的人口上限，不含建筑加成。城中的实际上限见 `City::official_capacity`。
    pub fn base_capacity(&self, config: &Config) -> f64 {
        let shi = &config.shi;
        let experience = if shi.ramp_turns == 0 {
            1.0
        } else {
            (self.service_turns as f64 / shi.ramp_turns as f64).min(1.0)
        };
        let ramp = shi.new_capacity_ratio + (1.0 - shi.new_capacity_ratio) * experience;
        let loyalty = (self.loyalty / shi.loyalty_max).clamp(0.0, 1.0);
        let loyalty_mult = shi.loyalty_min_mult + (1.0 - shi.loyalty_min_mult) * loyalty;
        shi.base_capacity * config.specialization(self.skills.len()) * ramp * loyalty_mult
    }
}

/// 经营方式。建筑的经营方式在营建时定下：在官营建筑里劳作只能官营，在民营建筑里只能民营。
/// 不靠建筑的劳作（种地、捕鱼等）由工事选择。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// 官营：产出全部归府库
    Guan,
    /// 民营：产出归民众，府库只能通过收税获得
    Min,
}

/// 工事的具体内容。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Task {
    Produce { recipe: String, mode: Mode },
    /// 在一块地上营建。`mode` 是建成后这座建筑的经营方式；营建本身一律由府库出材料、视为官营
    Construct { building: String, tile: Id, mode: Mode },
    /// 开垦一块新地，需要会对应技能的人，期间参与的人一直被占用。
    /// 地图做出来之前由指令指定地形，以后改为开垦城市旁边的一格，地形由地图决定
    Reclaim { terrain: String },
    /// 改良一块地，比如水利。和营建一样视为官营
    Improve { tile: Id, improvement: String },
    CollectTax,
    /// 祭祀只持续一个回合
    Sacrifice,
}

impl Task {
    /// 这群人能不能做这项工事：技能和专精度。收税、祭祀对人没有要求。
    pub fn can_be_done_by(&self, config: &Config, group: &PopGroup) -> bool {
        let skill = match self {
            Task::Produce { recipe, .. } => {
                let def = &config.recipes[recipe];
                if def.min_specialization.is_some_and(|min| config.specialization(group.skills.len()) + 1e-9 < min) {
                    return false;
                }
                Some(&def.skill)
            }
            Task::Construct { building, .. } => config.buildings[building].skill.as_ref(),
            Task::Improve { improvement, .. } => config.improvements[improvement].skill.as_ref(),
            Task::Reclaim { terrain } => config.terrains[terrain].reclaim_skill.as_ref(),
            Task::CollectTax | Task::Sacrifice => None,
        };
        skill.is_none_or(|s| group.has_skill(s))
    }

    /// 是否占用投入人口的劳力。收税只是覆盖这些人，不占用他们干活。
    pub fn uses_labor(&self) -> bool {
        !matches!(self, Task::CollectTax | Task::Sacrifice)
    }
}

/// 一项工事：一名士人主事，带着若干人做一件事。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Assignment {
    pub id: Id,
    pub shi: Id,
    pub task: Task,
    /// 人群 id → 人数。收税时表示被覆盖的人数
    pub workers: BTreeMap<Id, u32>,
    /// 营建、改良、开垦的进度（人 × 回合）
    pub progress: f64,
}

impl Assignment {
    pub fn worker_count(&self) -> u32 {
        self.workers.values().sum()
    }
}

/// 一批正在学技能的人。管理者直接下令，不需要士人主事；学习期间照常干活。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Learning {
    pub id: Id,
    pub group: Id,
    pub count: u32,
    /// 学成后的技能槽
    pub skills: Vec<String>,
    /// 新学的技能
    pub skill: String,
    /// 已经学了几个回合
    pub turns: u32,
}
