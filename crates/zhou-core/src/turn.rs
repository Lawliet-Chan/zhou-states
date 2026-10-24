//! 回合结算。顺序：
//! 1. 工事生产与未被管理人口的自主民营（受地块和建筑的劳作容量限制）
//! 2. 营建、改良地块、开垦、学技能的进度
//! 3. 收税
//! 4. 祭祀
//! 5. 人口消耗
//! 6. 士人开支
//! 7. 愉悦度
//! 8. 人口增减（受居住上限限制）
//! 9. 礼法检查：列鼎制度、城市面积
//! 10. 冬季结算规定任务

use std::collections::BTreeMap;

use serde::Serialize;

use crate::config::{Config, Pool, RecipeDef};
use crate::inventory::Inventory;
use crate::state::{Building, City, CityStatus, GameState, Id, JobSlot, Learning, Mode, PopGroup, Quota, Task, Tile};

#[derive(Debug, Clone, Serialize)]
pub struct TurnReport {
    pub turn: u32,
    pub year: u32,
    pub season: String,
    pub events: Vec<String>,
    pub cities: Vec<CityReport>,
    pub characters: Vec<CharacterReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CityReport {
    pub id: Id,
    pub name: String,
    pub owner: Option<Id>,
    pub manager: Option<Id>,
    /// 未答复的管理权任命
    pub manager_offer: Option<Id>,
    pub quota: Option<Quota>,
    pub population: u32,
    pub housing: u32,
    pub treasury: Inventory,
    pub private_wealth: Inventory,
    pub buildings: BTreeMap<String, u32>,
    pub tiles: Vec<TileReport>,
    pub jobs: Vec<JobReport>,
    pub officials: u32,
    pub idle_officials: u32,
    pub groups: Vec<GroupReport>,
    pub assignments: Vec<AssignmentReport>,
    pub learning: Vec<Learning>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TileReport {
    pub id: Id,
    pub terrain: String,
    pub improvements: BTreeMap<String, u32>,
    pub buildings: Vec<Building>,
}

/// 本回合一个容量来源的使用情况。
#[derive(Debug, Clone, Serialize)]
pub struct JobReport {
    pub pool: Pool,
    pub used: u32,
    pub capacity: u32,
    /// 想劳作却没有位置的人数
    pub turned_away: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssignmentReport {
    pub id: Id,
    pub shi: String,
    pub task: Task,
    pub workers: u32,
    pub progress: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupReport {
    pub id: Id,
    pub name: String,
    pub size: u32,
    pub skills: Vec<String>,
    pub managed: u32,
    pub happiness: f64,
    pub culture: f64,
    pub loyalty: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CharacterReport {
    pub id: Id,
    pub name: String,
    pub rank: String,
    pub reputation: f64,
    pub private_store: Inventory,
    /// 人物 id → 对他的信任
    pub trust: BTreeMap<Id, f64>,
}

/// 本回合每群人的结算记录。
#[derive(Default)]
struct Ledger {
    /// 这群人本回合的民营收入，收税的依据
    income: Inventory,
    /// 参与官营和营建的人数
    guan_labor: u32,
    taxed: u32,
    /// 消耗缺口比例，0 表示吃饱
    unfed: f64,
}

/// 本回合各容量来源剩余的劳作位置。
struct Jobs {
    slots: BTreeMap<Pool, Vec<JobSlot>>,
    capacity: BTreeMap<Pool, u32>,
    turned_away: BTreeMap<Pool, u32>,
}

impl Jobs {
    fn new(city: &City, config: &Config) -> Self {
        let mut slots = BTreeMap::new();
        for recipe in config.recipes.keys() {
            if let Some(pool) = config.pool_of(recipe) {
                slots.entry(pool).or_insert_with_key(|pool| city.job_slots(config, pool));
            }
        }
        let capacity = slots.iter().map(|(p, s)| (p.clone(), s.iter().map(|s| s.count).sum())).collect();
        Jobs { slots, capacity, turned_away: BTreeMap::new() }
    }

    /// 为 `workers` 人占用这种经营方式能用的劳作位置，返回折算产量倍率后的有效人数。
    fn take(&mut self, config: &Config, recipe: &str, workers: u32, mode: Mode) -> f64 {
        let Some(pool) = config.pool_of(recipe) else { return workers as f64 };
        let slots = self.slots.get_mut(&pool).expect("pool built for every recipe");
        let mut left = workers;
        let mut effective = 0.0;
        for slot in slots.iter_mut().filter(|s| s.mode.is_none_or(|m| m == mode)) {
            let n = left.min(slot.count);
            slot.count -= n;
            left -= n;
            effective += n as f64 * slot.mult;
        }
        if left > 0 {
            *self.turned_away.entry(pool).or_insert(0) += left;
        }
        effective
    }

    fn report(&self) -> Vec<JobReport> {
        self.slots
            .iter()
            .map(|(pool, slots)| {
                let capacity = self.capacity[pool];
                let free: u32 = slots.iter().map(|s| s.count).sum();
                JobReport {
                    pool: pool.clone(),
                    used: capacity - free,
                    capacity,
                    turned_away: self.turned_away.get(pool).copied().unwrap_or(0),
                }
            })
            .collect()
    }
}

/// 结算当前回合，返回结算报告，回合数加一。
pub fn end_turn(state: &mut GameState, config: &Config) -> TurnReport {
    let season = state.season_index(config);
    let year = state.year(config);
    let mut events = Vec::new();

    let mut jobs = BTreeMap::new();
    let next_id = &mut state.next_id;
    for city in state.cities.values_mut() {
        jobs.insert(city.id, resolve_city(city, config, season, next_id, &mut events));
    }
    check_ritual(state, config, &mut events);
    if season + 1 == config.seasons.len() {
        settle_quotas(state, config, &mut events);
    }

    let report = TurnReport {
        turn: state.turn,
        year,
        season: config.seasons[season].clone(),
        events,
        cities: state
            .cities
            .values()
            .map(|c| city_report(c, config, jobs.remove(&c.id).unwrap_or_default()))
            .collect(),
        characters: state
            .characters
            .values()
            .map(|c| CharacterReport {
                id: c.id,
                name: c.name.clone(),
                rank: c.rank.clone(),
                reputation: c.reputation,
                private_store: c.private_store.clone(),
                trust: c.trust.clone(),
            })
            .collect(),
    };
    state.turn += 1;
    report
}

fn resolve_city(
    city: &mut City,
    config: &Config,
    season: usize,
    next_id: &mut Id,
    events: &mut Vec<String>,
) -> Vec<JobReport> {
    clamp_assignments(city, events);
    let city_mult = city.efficiency(config);
    let mut ledgers: BTreeMap<Id, Ledger> = city.groups.keys().map(|id| (*id, Ledger::default())).collect();
    let mut jobs = Jobs::new(city, config);

    // 1. 工事生产：工事先占劳作位置，产量高的地块先被占用
    let assignments: Vec<_> = city.assignments.values().cloned().collect();
    // 忠诚度低的人群罢工：派去官营的人只有一部分肯出工，其余的人回去自主民营
    let mut working: BTreeMap<(Id, Id), u32> = BTreeMap::new();
    let mut strikers: BTreeMap<Id, u32> = BTreeMap::new();
    for a in &assignments {
        let guan = matches!(
            a.task,
            Task::Produce { mode: Mode::Guan, .. } | Task::Construct { .. } | Task::Improve { .. } | Task::Reclaim { .. }
        );
        for (group, count) in &a.workers {
            let turnout = if guan { config.disloyalty_mult(city.groups[group].loyalty, config.loyalty.guan_turnout) } else { 1.0 };
            let n = (*count as f64 * turnout).floor() as u32;
            working.insert((a.id, *group), n);
            *strikers.entry(*group).or_insert(0) += count - n;
        }
    }
    for (group, n) in &strikers {
        if *n > 0 {
            events.push(format!("{}：{}对管理者不够忠诚，{} 人罢工，回去自己干活", city.name, city.groups[group].name, n));
        }
    }
    for a in &assignments {
        if let Task::Produce { recipe, mode } = &a.task {
            let def = &config.recipes[recipe];
            for group in a.workers.keys() {
                let count = working[&(a.id, *group)];
                let effective = jobs.take(config, recipe, count, *mode);
                produce(city, config, def, *group, count, effective, *mode, season, city_mult, &mut ledgers);
            }
        }
    }
    // 未被管理的人口和罢工的人按技能槽第一项技能自主民营，用剩下的劳作位置
    let group_ids: Vec<Id> = city.groups.keys().copied().collect();
    for group in &group_ids {
        let unmanaged = city.unmanaged(*group) + strikers.get(group).copied().unwrap_or(0);
        let recipe = city.groups[group]
            .skills
            .first()
            .and_then(|skill| config.skills[skill].default_recipe.as_ref());
        if let Some(recipe) = recipe {
            let def = &config.recipes[recipe];
            if can_make(city, config, def, &city.groups[group]) {
                let effective = jobs.take(config, recipe, unmanaged, Mode::Min);
                produce(city, config, def, *group, unmanaged, effective, Mode::Min, season, city_mult, &mut ledgers);
            }
        }
    }
    let job_report = jobs.report();
    for job in &job_report {
        if job.turned_away > 0 {
            events.push(format!(
                "{}：「{}」已满，{} 人无处劳作",
                city.name,
                config.pool_name(&job.pool),
                job.turned_away
            ));
        }
    }

    // 2. 营建、改良地块、开垦：都按投入的人力累积进度，视为官营
    for a in &assignments {
        let (labor, season_mult, name) = match &a.task {
            Task::Construct { building, .. } => {
                let def = &config.buildings[building];
                (def.labor, &def.season_mult, &def.name)
            }
            Task::Improve { improvement, .. } => {
                let def = &config.improvements[improvement];
                (def.labor, &def.season_mult, &def.name)
            }
            Task::Reclaim { terrain } => {
                let def = &config.terrains[terrain];
                (def.reclaim_labor, &config.reclaim.season_mult, &def.name)
            }
            _ => continue,
        };
        let mut progress = 0.0;
        for group in a.workers.keys() {
            let count = working[&(a.id, *group)];
            let happiness = config.happiness_output_mult(city.groups[group].happiness);
            progress += count as f64 * season_mult[season] * happiness * city_mult;
            ledgers.get_mut(group).expect("ledger per group").guan_labor += count;
        }
        let assignment = city.assignments.get_mut(&a.id).expect("assignment exists");
        assignment.progress += progress;
        if assignment.progress < labor {
            continue;
        }
        city.assignments.remove(&a.id);
        match &a.task {
            Task::Construct { building, tile, mode } => {
                if let Some(tile) = city.tiles.get_mut(tile) {
                    tile.buildings.push(Building { kind: building.clone(), mode: *mode });
                }
                events.push(format!("{}：{}竣工", city.name, name));
            }
            Task::Improve { tile, improvement } => {
                let max = config.improvements[improvement].max_level;
                if let Some(tile) = city.tiles.get_mut(tile) {
                    let level = tile.improvements.entry(improvement.clone()).or_insert(0);
                    *level = (*level + 1).min(max);
                    let terrain = &config.terrains[&tile.terrain].name;
                    events.push(format!("{}：{}{}完成，现为 {} 级", city.name, terrain, name, level));
                }
            }
            Task::Reclaim { terrain } => {
                let id = *next_id;
                *next_id += 1;
                city.tiles.insert(id, Tile::new(id, terrain.clone()));
                events.push(format!("{}：开垦出一块{}", city.name, name));
            }
            _ => unreachable!("only construct, improve and reclaim reach here"),
        }
    }

    // 学技能：学满回合数后，这些人带着新的技能槽分出去，或并入技能和图腾都相同的人群
    let learning: Vec<_> = city.learning.values().cloned().collect();
    for l in &learning {
        let entry = city.learning.get_mut(&l.id).expect("learning exists");
        entry.turns += 1;
        if entry.turns < config.learning.turns {
            continue;
        }
        city.learning.remove(&l.id);
        if l.count == 0 {
            continue;
        }
        let source_name = city.groups[&l.group].name.clone();
        let target_id = city.move_people(config, l.group, l.count, &l.skills, next_id);
        ledgers.entry(target_id).or_default();
        events.push(format!(
            "{}：{} {} 人学会{}，归入{}",
            city.name,
            source_name,
            l.count,
            config.skill_name(&l.skill),
            city.groups[&target_id].name
        ));
    }

    // 3. 收税：从被覆盖人群本回合的民营收入中，按税率征收
    for a in &assignments {
        if !matches!(a.task, Task::CollectTax) {
            continue;
        }
        for (group_id, count) in &a.workers {
            let size = city.groups[group_id].size;
            if size == 0 {
                continue;
            }
            // 民众对管理者不够忠诚时，税收不上来
            let loyalty = config.disloyalty_mult(city.groups[group_id].loyalty, config.loyalty.tax_share);
            let share = (*count as f64 / size as f64).min(1.0) * loyalty;
            let ledger = ledgers.get_mut(group_id).expect("ledger per group");
            ledger.taxed += count;
            for (good, income) in ledger.income.iter() {
                let taken = city.private_wealth.take(good, income * city.tax_rate * share);
                city.treasury.add(good, taken);
            }
        }
    }

    // 4. 祭祀：精品礼器折合更多件
    let mut sacrificed = false;
    for a in &assignments {
        if !matches!(a.task, Task::Sacrifice) {
            continue;
        }
        let sacrifice = &config.sacrifice;
        let mut count = 0.0;
        let mut weight = 0.0;
        for (good, w) in &sacrifice.vessels {
            let n = city.treasury.get(good).floor();
            count += n;
            weight += n * w;
        }
        if count >= 1.0 {
            for group in city.groups.values_mut() {
                let room = (1.0 - group.culture / sacrifice.culture_max).max(0.0);
                group.culture += sacrifice.culture_per_vessel * weight * room;
            }
            sacrificed = true;
            events.push(format!("{}：举行祭祀，用礼器 {} 件", city.name, count));
        }
        city.assignments.remove(&a.id);
    }

    // 5. 人口消耗：全城人口共用民间财富，不足时由府库补足；仍有缺口时各群人按同一比例挨饿
    let pop = &config.population;
    let mut unfed: f64 = 0.0;
    for rule in &pop.consumption {
        let need = city.population() as f64 / pop.per_people * rule.amount;
        let mut got = 0.0;
        for good in &rule.goods {
            got += city.private_wealth.take(good, need - got);
        }
        if pop.treasury_covers_shortfall {
            for good in &rule.goods {
                got += city.treasury.take(good, need - got);
            }
        }
        if need > 0.0 && got + 1e-9 < need {
            let shortfall = 1.0 - got / need;
            unfed = unfed.max(shortfall);
            events.push(format!("{}：全城缺{}，缺口 {:.0}%", city.name, rule.name, shortfall * 100.0));
        }
    }
    for ledger in ledgers.values_mut() {
        ledger.unfed = unfed;
    }

    // 6. 士人开支
    let shi = &config.shi;
    let mut unpaid = 0;
    for official in city.officials.values_mut() {
        let paid = city.treasury.contains_all(&shi.upkeep);
        for (good, amount) in shi.upkeep.iter() {
            city.treasury.take(good, *amount);
        }
        if paid {
            official.loyalty = (official.loyalty + shi.paid_loyalty_gain).min(shi.loyalty_max);
        } else {
            official.loyalty = (official.loyalty - shi.unpaid_loyalty_penalty).max(0.0);
            unpaid += 1;
        }
        official.service_turns += 1;
    }
    if unpaid > 0 {
        events.push(format!("{}：府库无力供养 {} 名士人", city.name, unpaid));
    }

    // 7. 愉悦度
    let h = &config.happiness;
    let population = city.population();
    let housing = city.housing(config);
    // 藏富于民：全城民间财富的人均价值越高，所有人群的愉悦度越高
    let wealth_bonus = if population == 0 || h.wealth_full_per_person <= 0.0 {
        0.0
    } else {
        let per_person = config.value_of(&city.private_wealth) / population as f64;
        h.wealth_bonus * (per_person / h.wealth_full_per_person).min(1.0)
    };
    let building_bonus: f64 = city.buildings().iter().map(|(id, n)| config.buildings[id].happiness * *n as f64).sum();
    // 人口超出居住上限时拥挤，超出一倍时扣满
    let crowding = if population > housing {
        events.push(format!("{}：人口 {} 超出居住上限 {}，拥挤不堪", city.name, population, housing));
        ((population - housing) as f64 / housing.max(1) as f64).min(1.0)
    } else {
        0.0
    };
    for group in city.groups.values_mut() {
        if group.size == 0 {
            continue;
        }
        let ledger = &ledgers[&group.id];
        let size = group.size as f64;
        let guan_share = (ledger.guan_labor as f64 / size).min(1.0);
        let taxed_share = (ledger.taxed as f64 / size).min(1.0);
        let mut delta = (h.target - group.happiness) * h.drift_rate + wealth_bonus + building_bonus;
        delta -= h.guan_penalty * guan_share;
        delta -= h.tax_penalty * city.tax_rate * taxed_share;
        delta -= h.hunger_penalty * ledger.unfed;
        delta -= h.crowding_penalty * crowding;
        if sacrificed {
            delta += h.sacrifice_bonus;
        }
        group.happiness = (group.happiness + delta).clamp(0.0, h.max);

        // 忠诚度：税越低越忠诚，税重了就下降
        let l = &config.loyalty;
        let max_rate = config.economy.max_tax_rate;
        let tax_burden = if max_rate > 0.0 { city.tax_rate * taxed_share / max_rate } else { 0.0 };
        let loyalty_delta = l.low_tax_gain - l.high_tax_loss * tax_burden;
        group.loyalty = (group.loyalty + loyalty_delta).clamp(0.0, l.max);
    }

    // 8. 人口增减：增长不超过居住上限，各群人按想增长的多少分配剩余的空间
    let wanted: BTreeMap<Id, f64> = city
        .groups
        .values()
        .filter(|g| ledgers[&g.id].unfed <= 0.0 && g.happiness >= pop.growth_min_happiness)
        .map(|g| (g.id, g.size as f64 * pop.growth_rate * g.happiness / h.max))
        .collect();
    let total_wanted: f64 = wanted.values().sum();
    let room = housing.saturating_sub(population) as f64;
    let growth_factor = if total_wanted > 0.0 { (room / total_wanted).min(1.0) } else { 0.0 };
    for group in city.groups.values_mut() {
        let ledger = &ledgers[&group.id];
        let size = group.size as f64;
        if ledger.unfed > 0.0 {
            let dead = (size * ledger.unfed * pop.starvation_rate).ceil() as u32;
            group.size = group.size.saturating_sub(dead);
            if dead > 0 {
                events.push(format!("{}：{}饿死 {} 人", city.name, group.name, dead));
            }
        } else if let Some(want) = wanted.get(&group.id) {
            group.growth_carry += want * growth_factor;
            let born = group.growth_carry.floor();
            group.growth_carry -= born;
            group.size += born as u32;
        }
        if group.happiness < pop.emigration_threshold {
            let left = (group.size as f64 * pop.emigration_rate).ceil() as u32;
            group.size = group.size.saturating_sub(left);
            if left > 0 {
                events.push(format!("{}：{}因不满外流 {} 人", city.name, group.name, left));
            }
        }
    }
    clamp_assignments(city, events);
    job_report
}

/// 城市有没有条件、这群人有没有本事自主民营这个配方。
fn can_make(city: &City, config: &Config, recipe: &RecipeDef, group: &PopGroup) -> bool {
    recipe.resource.as_ref().is_none_or(|r| city.has_terrain(r))
        && recipe.facility.as_ref().is_none_or(|f| city.has_building(f, Mode::Min))
        && recipe
            .min_specialization
            .is_none_or(|min| config.specialization(group.skills.len()) + 1e-9 >= min)
}

#[allow(clippy::too_many_arguments)]
fn produce(
    city: &mut City,
    config: &Config,
    recipe: &RecipeDef,
    group_id: Id,
    workers: u32,
    effective: f64,
    mode: Mode,
    season: usize,
    city_mult: f64,
    ledgers: &mut BTreeMap<Id, Ledger>,
) {
    if workers == 0 {
        return;
    }
    let ledger = ledgers.get_mut(&group_id).expect("ledger per group");
    if mode == Mode::Guan {
        ledger.guan_labor += workers;
    }
    let group = &city.groups[&group_id];
    let skill_count = group.skills.len();
    let mut scale = effective
        * recipe.season_mult[season]
        * config.specialization(skill_count)
        * config.happiness_output_mult(group.happiness)
        * city_mult;
    if mode == Mode::Guan {
        scale *= config.economy.guan_efficiency;
    } else {
        // 民营不罢工，但对管理者不够忠诚时产量下降
        scale *= config.disloyalty_mult(group.loyalty, config.loyalty.min_output);
    }
    if scale <= 0.0 {
        return;
    }

    // 官营的原料和产出都在府库，民营的在全城共有的民间财富中
    let stock = match mode {
        Mode::Guan => &mut city.treasury,
        Mode::Min => &mut city.private_wealth,
    };
    // 原料不足时按比例减产
    let mut factor: f64 = 1.0;
    for (good, per_worker) in recipe.inputs.iter() {
        let need = per_worker * scale;
        if need > 0.0 {
            factor = factor.min(stock.get(good) / need);
        }
    }
    let scale = scale * factor.clamp(0.0, 1.0);
    for (good, per_worker) in recipe.inputs.iter() {
        stock.take(good, per_worker * scale);
    }
    // 专精度越高，产出中精品的比例越高
    let fine_share = config.fine_share(skill_count);
    for (good, per_worker) in recipe.outputs.iter() {
        let amount = per_worker * scale;
        let made = match &config.goods[good].fine {
            Some(fine) => vec![(good.as_str(), amount * (1.0 - fine_share)), (fine.as_str(), amount * fine_share)],
            None => vec![(good.as_str(), amount)],
        };
        for (good, amount) in made {
            stock.add(good, amount);
            if mode == Mode::Min {
                ledger.income.add(good, amount);
            }
        }
    }
}

/// 人群缩小后，削减超出人数的工事投入和学技能的人数，后开的先削减。
/// 削减到没有人的工事就此结束，主事的士人空出来。
fn clamp_assignments(city: &mut City, events: &mut Vec<String>) {
    for (group_id, group) in &city.groups {
        let mut learning = city.learning_in(*group_id);
        for l in city.learning.values_mut().rev().filter(|l| l.group == *group_id) {
            let cut = l.count.min(learning.saturating_sub(group.size));
            l.count -= cut;
            learning -= cut;
        }
        for labor in [true, false] {
            let mut total: u32 = city
                .assignments
                .values()
                .filter(|a| a.task.uses_labor() == labor)
                .filter_map(|a| a.workers.get(group_id))
                .sum();
            for a in city.assignments.values_mut().rev() {
                if total <= group.size {
                    break;
                }
                if a.task.uses_labor() != labor {
                    continue;
                }
                if let Some(count) = a.workers.get_mut(group_id) {
                    let cut = (*count).min(total - group.size);
                    *count -= cut;
                    total -= cut;
                    if *count == 0 {
                        a.workers.remove(group_id);
                    }
                }
            }
        }
    }
    for a in city.end_empty_assignments() {
        let shi = city.officials.get(&a.shi).map_or("", |o| o.name.as_str());
        events.push(format!("{}：{}主事的工事已经没有人，就此结束", city.name, shi));
    }
}

/// 礼法检查，超出爵位允许的都视为僭越，扣除风评：
/// - 列鼎制度：所有者名下城市府库中的鼎
/// - 城市面积：所有者名下每座城市的地块数
///
/// 同一次僭越只扣一次风评。超出得更多时只扣多出的部分；
/// 回到上限以内后又超出，算作新的僭越，再扣一次。
fn check_ritual(state: &mut GameState, config: &Config, events: &mut Vec<String>) {
    let ritual = &config.ritual;
    for character in state.characters.values_mut() {
        let owned: Vec<&City> = state.cities.values().filter(|c| c.owner == Some(character.id)).collect();
        let dings: u32 = owned
            .iter()
            .map(|c| ritual.ding_goods.iter().map(|g| c.treasury.get(g).floor() as u32).sum::<u32>())
            .sum();
        let limit = ritual.ding_limit.get(&character.rank).copied().unwrap_or(0);
        let excess = dings.saturating_sub(limit);
        if excess > character.punished_ding_excess {
            character.reputation -= (excess - character.punished_ding_excess) as f64 * ritual.excess_reputation_penalty;
            events.push(format!("{}：拥有鼎 {} 只，超出身份上限 {} 只，僭越，风评下降", character.name, dings, limit));
        }
        character.punished_ding_excess = excess;

        let tile_limit = ritual.tile_limit.get(&character.rank).copied().unwrap_or(0);
        let mut punished = BTreeMap::new();
        for city in owned {
            let tiles = city.tiles.len() as u32;
            let excess = tiles.saturating_sub(tile_limit);
            let before = character.punished_tile_excess.get(&city.id).copied().unwrap_or(0);
            if excess > before {
                character.reputation -= (excess - before) as f64 * ritual.excess_tile_penalty;
                events.push(format!(
                    "{}：{}有地 {} 块，超出身份上限 {} 块，僭越，风评下降",
                    character.name, city.name, tiles, tile_limit
                ));
            }
            if excess > 0 {
                punished.insert(city.id, excess);
            }
        }
        character.punished_tile_excess = punished;
    }
}

/// 冬季结算规定任务：看全年累计上缴的物资交够没有，调整所有者对管理者的信任，然后清零重新累计。
/// 缺口和超额都按物品价值折算。
fn settle_quotas(state: &mut GameState, config: &Config, events: &mut Vec<String>) {
    let q = &config.quota;
    for city in state.cities.values_mut() {
        let (Some(owner), Some(manager)) = (city.owner, city.manager) else { continue };
        if owner == manager || city.status == CityStatus::Occupied {
            continue;
        }
        let Some(quota) = &mut city.quota else { continue };
        let value = |good: &str| config.goods.get(good).map_or(0.0, |g| g.value);
        let required: f64 = quota.goods.iter().map(|(g, n)| value(g) * n).sum();
        let credited: f64 = quota.goods.iter().map(|(g, n)| value(g) * quota.delivered.get(g).min(*n)).sum();
        let delivered: f64 = quota.goods.iter().map(|(g, _)| value(g) * quota.delivered.get(g)).sum();
        quota.delivered = Inventory::default();
        if required <= 0.0 {
            continue;
        }
        let ratio = credited / required;
        let (change, result) = if ratio + 1e-9 < 1.0 {
            (-q.fail_trust_penalty * (1.0 - ratio), format!("只完成 {:.0}%", ratio * 100.0))
        } else if delivered + 1e-9 >= required * q.exceed_ratio {
            (q.exceed_trust, "超额完成".to_string())
        } else {
            (q.complete_trust, "完成".to_string())
        };
        let Some(owner) = state.characters.get_mut(&owner) else { continue };
        let trust = (owner.trust_in(manager, config) + change).clamp(0.0, q.max_trust);
        owner.trust.insert(manager, trust);
        events.push(format!("{}：今年的规定任务{}，{}对管理者的信任变为 {:.0}", city.name, result, owner.name, trust));
    }
}

fn city_report(city: &City, config: &Config, jobs: Vec<JobReport>) -> CityReport {
    let assignments = city
        .assignments
        .values()
        .map(|a| AssignmentReport {
            id: a.id,
            shi: city.officials.get(&a.shi).map_or_else(String::new, |o| o.name.clone()),
            task: a.task.clone(),
            workers: a.worker_count(),
            progress: a.progress,
        })
        .collect();
    let busy: Vec<Id> = city.assignments.values().map(|a| a.shi).collect();
    CityReport {
        id: city.id,
        name: city.name.clone(),
        owner: city.owner,
        manager: city.manager,
        manager_offer: city.manager_offer,
        quota: city.quota.clone(),
        population: city.population(),
        housing: city.housing(config),
        treasury: city.treasury.clone(),
        private_wealth: city.private_wealth.clone(),
        buildings: city.buildings(),
        tiles: city
            .tiles
            .values()
            .map(|t| TileReport {
                id: t.id,
                terrain: t.terrain.clone(),
                improvements: t.improvements.clone(),
                buildings: t.buildings.clone(),
            })
            .collect(),
        jobs,
        officials: city.officials.len() as u32,
        idle_officials: city.officials.keys().filter(|id| !busy.contains(id)).count() as u32,
        groups: city
            .groups
            .values()
            .map(|g| GroupReport {
                id: g.id,
                name: g.name.clone(),
                size: g.size,
                skills: g.skills.clone(),
                managed: city.labor_assigned(g.id),
                happiness: g.happiness,
                culture: g.culture,
                loyalty: g.loyalty,
            })
            .collect(),
        assignments,
        learning: city.learning.values().cloned().collect(),
    }
}
