use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::{Config, Pool};
use crate::deal::Deal;
use crate::inventory::Inventory;
use crate::map::{Hex, Map};

/// 所有实体共用一个递增的 id 空间。
pub type Id = u32;

/// 整个游戏世界的状态。所有容器都用 BTreeMap，保证遍历顺序固定、结算结果可复现。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GameState {
    /// 下一个要结算的回合，从 0 开始
    pub turn: u32,
    pub next_id: Id,
    #[serde(default)]
    pub map: Map,
    pub characters: BTreeMap<Id, Character>,
    pub cities: BTreeMap<Id, City>,
    #[serde(default)]
    pub countries: BTreeMap<Id, Country>,
    /// 还没答复的赠与、索取、交易
    #[serde(default)]
    pub deals: BTreeMap<Id, Deal>,
}

/// 国家：独立存在，不随某个人物存在或消失。国籍指向国家，城市隶属于所有者国籍所在的国家。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Country {
    pub id: Id,
    pub name: String,
    /// 国君
    pub ruler: Id,
    /// 都城
    pub capital: Option<Id>,
    /// 国君发布了国战：国内所有的城对本国的人开放通行（等军队做出来后由国战设置）
    #[serde(default)]
    pub national_war: bool,
}

impl GameState {
    pub fn alloc_id(&mut self) -> Id {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// 当前回合所在的季节
    /// 这一格属于哪座城，野地返回 None
    /// 城市隶属的国家：所有者的国籍
    pub fn country_of(&self, city: &City) -> Option<Id> {
        city.owner.and_then(|o| self.characters.get(&o)).and_then(|c| c.nationality)
    }

    /// 邻城增益：同一个所有者的其他城，治所相距不超过 `neighbor.range` 格的，每座给这座城的产出加一成，有上限。
    /// 返回产出的倍数。
    pub fn neighbor_mult(&self, config: &Config, city: &City) -> f64 {
        let Some(owner) = city.owner else { return 1.0 };
        let n = self
            .cities
            .values()
            .filter(|c| c.id != city.id && c.owner == Some(owner) && c.seat.distance(city.seat) <= config.neighbor.range)
            .count();
        1.0 + (n as f64 * config.neighbor.bonus).min(config.neighbor.max)
    }

    /// 人物能待在这座城：城是他自己的；自己一座城都没有时，只能待在他管理的城。
    pub fn can_stay(&self, who: Id, city: Id) -> bool {
        let Some(c) = self.cities.get(&city) else { return false };
        c.owner == Some(who) || (c.manager == Some(who) && !self.cities.values().any(|c| c.owner == Some(who)))
    }

    /// 所在的城不再能待（城送了人、封了出去、被灭了）或者还没有所在的城时，回到默认的地方：
    /// 先是自己管理的自己的城，再是自己拥有的城；自己一座城都没有时待在他管理的城；都没有就无处可待。
    pub fn settle_locations(&mut self) {
        let ids: Vec<Id> = self.characters.keys().copied().collect();
        for id in ids {
            let current = self.characters[&id].location;
            if current.is_some_and(|c| self.can_stay(id, c)) {
                continue;
            }
            let mut owned = self.cities.values().filter(|c| c.owner == Some(id));
            let home = owned
                .clone()
                .find(|c| c.manager == Some(id))
                .or_else(|| owned.next())
                .or_else(|| self.cities.values().find(|c| c.manager == Some(id)))
                .map(|c| c.id);
            self.characters.get_mut(&id).expect("listed above").location = home;
        }
    }

    /// `who` 的军队能不能经过这一格。野地谁都能走，商旅哪里都能走。城的地：
    /// - 所有者自己能走，有通行权、还没到期的人能走
    /// - 国君的军队可以自由出入本国所有的城
    /// - 国君发布国战时，国内所有的城对本国的人开放
    ///
    /// 硬闯就是宣战（等军队做出来后实现）。
    pub fn can_pass(&self, who: Id, pos: Hex, merchant: bool) -> bool {
        if merchant {
            return true;
        }
        let Some(city) = self.city_at(pos).and_then(|id| self.cities.get(&id)) else { return true };
        if city.owner == Some(who) || city.passage.get(&who).is_some_and(|until| self.turn < *until) {
            return true;
        }
        let Some(country) = self.country_of(city).and_then(|id| self.countries.get(&id)) else { return false };
        country.ruler == who
            || (country.national_war && self.characters.get(&who).and_then(|c| c.nationality) == Some(country.id))
    }

    pub fn city_at(&self, pos: Hex) -> Option<Id> {
        self.cities.values().find(|c| c.tile_at(pos).is_some()).map(|c| c.id)
    }

    pub fn season_index(&self, config: &Config) -> usize {
        self.turn as usize / config.months_per_season as usize % config.seasons.len()
    }

    /// 当前回合是一年中的第几个月，从 1 开始
    pub fn month(&self, config: &Config) -> u32 {
        self.turn % config.turns_per_year() + 1
    }

    pub fn year(&self, config: &Config) -> u32 {
        self.turn / config.turns_per_year() + 1
    }

    /// 当前回合是不是一年的最后一个月
    pub fn is_year_end(&self, config: &Config) -> bool {
        self.month(config) == config.turns_per_year()
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
    /// 国籍，None 为无国籍
    #[serde(default)]
    pub nationality: Option<Id>,
    /// 最近一次僭越（因用鼎、地块超出身份被扣风评）的回合
    #[serde(default)]
    pub usurped_turn: Option<u32>,
    /// 所在的城市，同一时间只能在一座城里。默认在自己管理的城；平时只能搬去自己拥有的城
    #[serde(default)]
    pub location: Option<Id>,
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
            nationality: None,
            usurped_turn: None,
            location: None,
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
    /// 治所所在的格子，城与城之间的距离从这里算
    pub seat: Hex,
    /// 城市的地块，每块对应地图上的一格
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
    /// 配方 → 今年播种的量（按有效人数折算的平均值）。没有记录时收成不受限，比如开局还没经过播种季
    #[serde(default)]
    pub sown: BTreeMap<String, f64>,
    /// 封君：把这座城封给现在的所有者的人。封地不能收回
    #[serde(default)]
    pub liege: Option<Id>,
    /// 所有者发出、对方还没答复的封授
    #[serde(default)]
    pub fief_offer: Option<Id>,
    /// 封君手动指定的纳贡；没有时每年年末按府库当年总产出自动纳贡
    #[serde(default)]
    pub tribute: Option<Tribute>,
    /// 合理的纳贡连续几年没交够
    #[serde(default)]
    pub tribute_shortfall_years: u32,
    /// 府库今年的总产出：收上来的税和官营产出，年末结算后清零
    #[serde(default)]
    pub income: Inventory,
    /// 府库去年的总产出，判断纳贡是否过火时用
    #[serde(default)]
    pub last_income: Option<Inventory>,
    /// 通行权：人物 id → 到第几回合为止（不含）。有通行权的人，军队可以从这座城的地上经过。
    /// 通行权由所有者在赠与、交易中给出，有期限；没有的人，哪怕是同一国的，军队也过不去
    #[serde(default)]
    pub passage: BTreeMap<Id, u32>,
}

/// 封君手动指定的纳贡：每年要纳的物资。封臣随时纳贡，进封君的私库，年末结算后清零重新累计。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Tribute {
    pub goods: Inventory,
    pub delivered: Inventory,
    /// 纳贡过火时封臣可以拒绝，拒绝后不用交
    pub refused: bool,
}

/// 规定任务：每年要上缴给所有者的物资，年末结算后清零重新累计。
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

    pub fn tile_at(&self, pos: Hex) -> Option<&Tile> {
        self.tiles.values().find(|t| t.pos == pos)
    }

    /// 这一格是否和城市的某块地相邻
    pub fn borders(&self, pos: Hex) -> bool {
        self.tile_at(pos).is_none() && self.tiles.values().any(|t| t.pos.is_adjacent(pos))
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
        let (totem, culture, happiness, loyalty, hostility) =
            (source.totem.clone(), source.culture, source.happiness, source.loyalty, source.hostility);
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
            target.hostility = (target.hostility * target.size as f64 + hostility * count as f64) / total;
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
                hostility,
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
    /// 在地图上的位置
    pub pos: Hex,
    /// 和地图上这一格的地形相同
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
    pub fn new(id: Id, pos: Hex, terrain: impl Into<String>) -> Self {
        Tile { id, pos, terrain: terrain.into(), improvements: BTreeMap::new(), buildings: Vec::new() }
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
    /// 对封君的敌视：封君要的纳贡过火时产生。封君对封地发起的事，这些人的产能会降低，
    /// 面对封君的讨伐战斗力会升高（等征调和军队做出来后生效）
    #[serde(default)]
    pub hostility: f64,
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
    /// 把和城市相邻的一格野地开垦进来。`terrain` 必须和地图上这一格的地形相同，决定需要什么技能；
    /// 期间参与的人一直被占用
    Reclaim { at: Hex, terrain: String },
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
            Task::Reclaim { terrain, .. } => config.terrains[terrain].reclaim_skill.as_ref(),
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
