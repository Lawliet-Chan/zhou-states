//! 回合结算。顺序：
//! 1. 工事生产与未被管理人口的自主民营
//! 2. 营建进度
//! 3. 收税
//! 4. 祭祀
//! 5. 人口消耗
//! 6. 士人开支
//! 7. 愉悦度
//! 8. 人口增减
//! 9. 列鼎制度检查

use std::collections::BTreeMap;

use serde::Serialize;

use crate::config::{Config, RecipeDef};
use crate::inventory::Inventory;
use crate::state::{City, GameState, Id, Mode, Task};

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
    pub population: u32,
    pub treasury: Inventory,
    pub private_wealth: Inventory,
    pub buildings: BTreeMap<String, u32>,
    pub officials: u32,
    pub idle_officials: u32,
    pub groups: Vec<GroupReport>,
    pub assignments: Vec<AssignmentReport>,
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
    pub managed: u32,
    pub happiness: f64,
    pub culture: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CharacterReport {
    pub id: Id,
    pub name: String,
    pub rank: String,
    pub reputation: f64,
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

/// 结算当前回合，返回结算报告，回合数加一。
pub fn end_turn(state: &mut GameState, config: &Config) -> TurnReport {
    let season = state.season_index(config);
    let year = state.year(config);
    let mut events = Vec::new();

    for city in state.cities.values_mut() {
        resolve_city(city, config, season, &mut events);
    }
    check_ritual(state, config, &mut events);

    let report = TurnReport {
        turn: state.turn,
        year,
        season: config.seasons[season].clone(),
        events,
        cities: state.cities.values().map(city_report).collect(),
        characters: state
            .characters
            .values()
            .map(|c| CharacterReport {
                id: c.id,
                name: c.name.clone(),
                rank: c.rank.clone(),
                reputation: c.reputation,
            })
            .collect(),
    };
    state.turn += 1;
    report
}

fn resolve_city(city: &mut City, config: &Config, season: usize, events: &mut Vec<String>) {
    clamp_assignments(city);
    let city_mult = city.efficiency(config);
    let mut ledgers: BTreeMap<Id, Ledger> = city.groups.keys().map(|id| (*id, Ledger::default())).collect();

    // 1. 工事生产
    let assignments: Vec<_> = city.assignments.values().cloned().collect();
    for a in &assignments {
        if let Task::Produce { recipe, mode } = &a.task {
            let def = &config.recipes[recipe];
            for (group, count) in &a.workers {
                produce(city, config, def, *group, *count, *mode, season, city_mult, &mut ledgers);
            }
        }
    }
    // 未被管理的人口按技能槽第一项技能自主民营
    let group_ids: Vec<Id> = city.groups.keys().copied().collect();
    for group in &group_ids {
        let unmanaged = city.unmanaged(*group);
        let recipe = city.groups[group]
            .skills
            .first()
            .and_then(|skill| config.skills[skill].default_recipe.as_ref());
        if let Some(recipe) = recipe {
            let def = &config.recipes[recipe];
            if meets_requirements(city, def) {
                produce(city, config, def, *group, unmanaged, Mode::Min, season, city_mult, &mut ledgers);
            }
        }
    }

    // 2. 营建
    for a in &assignments {
        let Task::Construct { building } = &a.task else { continue };
        let def = &config.buildings[building];
        let mut progress = 0.0;
        for (group, count) in &a.workers {
            let happiness = config.happiness_output_mult(city.groups[group].happiness);
            progress += *count as f64 * def.season_mult[season] * happiness * city_mult;
            ledgers.get_mut(group).expect("ledger per group").guan_labor += count;
        }
        let assignment = city.assignments.get_mut(&a.id).expect("assignment exists");
        assignment.progress += progress;
        if assignment.progress >= def.labor {
            city.assignments.remove(&a.id);
            *city.buildings.entry(building.clone()).or_insert(0) += 1;
            events.push(format!("{}：{}竣工", city.name, def.name));
        }
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
            let share = (*count as f64 / size as f64).min(1.0);
            let ledger = ledgers.get_mut(group_id).expect("ledger per group");
            ledger.taxed += count;
            for (good, income) in ledger.income.iter() {
                let taken = city.private_wealth.take(good, income * city.tax_rate * share);
                city.treasury.add(good, taken);
            }
        }
    }

    // 4. 祭祀
    let mut sacrificed = false;
    for a in &assignments {
        if !matches!(a.task, Task::Sacrifice) {
            continue;
        }
        let sacrifice = &config.sacrifice;
        let vessels: f64 = sacrifice.vessel_goods.iter().map(|g| city.treasury.get(g).floor()).sum();
        if vessels >= 1.0 {
            for group in city.groups.values_mut() {
                let room = (1.0 - group.culture / sacrifice.culture_max).max(0.0);
                group.culture += sacrifice.culture_per_vessel * vessels * room;
            }
            sacrificed = true;
            events.push(format!("{}：举行祭祀，用礼器 {} 件", city.name, vessels));
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
    // 藏富于民：全城民间财富的人均价值越高，所有人群的愉悦度越高
    let population = city.population();
    let wealth_bonus = if population == 0 || h.wealth_full_per_person <= 0.0 {
        0.0
    } else {
        let per_person = config.value_of(&city.private_wealth) / population as f64;
        h.wealth_bonus * (per_person / h.wealth_full_per_person).min(1.0)
    };
    for group in city.groups.values_mut() {
        if group.size == 0 {
            continue;
        }
        let ledger = &ledgers[&group.id];
        let size = group.size as f64;
        let guan_share = (ledger.guan_labor as f64 / size).min(1.0);
        let taxed_share = (ledger.taxed as f64 / size).min(1.0);
        let mut delta = (h.target - group.happiness) * h.drift_rate + wealth_bonus;
        delta -= h.guan_penalty * guan_share;
        delta -= h.tax_penalty * city.tax_rate * taxed_share;
        delta -= h.hunger_penalty * ledger.unfed;
        if sacrificed {
            delta += h.sacrifice_bonus;
        }
        group.happiness = (group.happiness + delta).clamp(0.0, h.max);
    }

    // 8. 人口增减
    for group in city.groups.values_mut() {
        let ledger = &ledgers[&group.id];
        let size = group.size as f64;
        if ledger.unfed > 0.0 {
            let dead = (size * ledger.unfed * pop.starvation_rate).ceil() as u32;
            group.size = group.size.saturating_sub(dead);
            if dead > 0 {
                events.push(format!("{}：{}饿死 {} 人", city.name, group.name, dead));
            }
        } else if group.happiness >= pop.growth_min_happiness {
            group.growth_carry += size * pop.growth_rate * group.happiness / h.max;
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
    clamp_assignments(city);
}

fn meets_requirements(city: &City, recipe: &RecipeDef) -> bool {
    recipe.resource.as_ref().is_none_or(|r| city.resources.contains(r))
        && recipe.facility.as_ref().is_none_or(|f| city.has_building(f))
}

#[allow(clippy::too_many_arguments)]
fn produce(
    city: &mut City,
    config: &Config,
    recipe: &RecipeDef,
    group_id: Id,
    workers: u32,
    mode: Mode,
    season: usize,
    city_mult: f64,
    ledgers: &mut BTreeMap<Id, Ledger>,
) {
    if workers == 0 {
        return;
    }
    let group = &city.groups[&group_id];
    let mut scale = workers as f64
        * recipe.season_mult[season]
        * config.specialization(group.skills.len())
        * config.happiness_output_mult(group.happiness)
        * city_mult;
    if mode == Mode::Guan {
        scale *= config.economy.guan_efficiency;
    }
    let ledger = ledgers.get_mut(&group_id).expect("ledger per group");
    if mode == Mode::Guan {
        ledger.guan_labor += workers;
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
    for (good, per_worker) in recipe.outputs.iter() {
        let amount = per_worker * scale;
        stock.add(good, amount);
        if mode == Mode::Min {
            ledger.income.add(good, amount);
        }
    }
}

/// 人群缩小后，削减超出人数的工事投入：先削减后开的工事。
fn clamp_assignments(city: &mut City) {
    for (group_id, group) in &city.groups {
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
}

/// 列鼎制度：所有者名下城市府库中的鼎超过爵位上限时，视为僭越，扣除风评。
fn check_ritual(state: &mut GameState, config: &Config, events: &mut Vec<String>) {
    let ritual = &config.ritual;
    for character in state.characters.values_mut() {
        let dings: f64 = state
            .cities
            .values()
            .filter(|c| c.owner == Some(character.id))
            .map(|c| c.treasury.get(&ritual.ding_good).floor())
            .sum();
        let limit = ritual.ding_limit.get(&character.rank).copied().unwrap_or(0) as f64;
        if dings > limit {
            let excess = dings - limit;
            character.reputation -= excess * ritual.excess_reputation_penalty;
            events.push(format!(
                "{}：拥有鼎 {} 只，超出身份上限 {} 只，僭越，风评下降",
                character.name, dings, limit
            ));
        }
    }
}

fn city_report(city: &City) -> CityReport {
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
        population: city.population(),
        treasury: city.treasury.clone(),
        private_wealth: city.private_wealth.clone(),
        buildings: city.buildings.clone(),
        officials: city.officials.len() as u32,
        idle_officials: city.officials.keys().filter(|id| !busy.contains(id)).count() as u32,
        groups: city
            .groups
            .values()
            .map(|g| GroupReport {
                id: g.id,
                name: g.name.clone(),
                size: g.size,
                managed: city.labor_assigned(g.id),
                happiness: g.happiness,
                culture: g.culture,
            })
            .collect(),
        assignments,
    }
}
