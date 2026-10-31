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
//! 10. 年末（冬季最后一个月）结算规定任务

use crate::fx::Fx;
use std::collections::BTreeMap;

use serde::Serialize;

use crate::config::{Config, Pool, RecipeDef};
use crate::event::Event;
use crate::fief::{self, Grievance};
use crate::inventory::Inventory;
use crate::map::Hex;
use crate::state::{
    Building, City, CityStatus, Country, GameState, Id, JobSlot, Learning, Mode, PopGroup, Quota,
    Task, Tile, Tribute,
};

#[derive(Debug, Clone, Serialize)]
pub struct TurnReport {
    pub turn: u32,
    pub year: u32,
    /// 一年中的第几个月，从 1 开始
    pub month: u32,
    pub season: String,
    pub events: Vec<Event>,
    /// 整张地图：每格的地形和所属城市
    pub map: Vec<CellReport>,
    pub cities: Vec<CityReport>,
    pub characters: Vec<CharacterReport>,
    pub countries: Vec<Country>,
    pub armies: Vec<ArmyReport>,
    pub wars: Vec<crate::war::War>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ArmyReport {
    pub id: Id,
    pub city: Id,
    pub commander: Id,
    pub pos: Hex,
    pub dest: Option<Hex>,
    pub moves_left: u32,
    /// 是否正在驻守城墙，供前端显示当前状态。
    pub on_wall: bool,
    /// 不在自己的城的地上，要靠粮道供粮
    pub on_campaign: bool,
    pub supply: Option<SupplyReport>,
    /// 连续断粮的回合数
    pub hunger: u32,
    /// 断粮后战斗力剩下的比例，1 为没有断粮
    pub strength: Fx,
    pub squads: Vec<SquadReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SupplyReport {
    /// 起点城
    pub from: Id,
    /// 辎重兵人数
    pub convoy: u32,
    /// 路线：起点城治所之后的每一格，到军队所在的格子为止。为空表示找不到路
    pub path: Vec<Hex>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SquadReport {
    pub unit: String,
    pub count: u32,
    pub group: Id,
    pub elite: bool,
    pub gear: Inventory,
    /// 必需装备的齐备程度，0 到 1
    pub equipped: Fx,
}

#[derive(Debug, Clone, Serialize)]
pub struct CellReport {
    /// 轴向坐标
    pub pos: Hex,
    pub terrain: String,
    /// 野地为 None
    pub city: Option<Id>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CityReport {
    pub id: Id,
    pub name: String,
    /// 治所所在的格子
    pub seat: Hex,
    /// 隶属的国家
    pub country: Option<Id>,
    /// 封君
    pub liege: Option<Id>,
    /// 未答复的封授
    pub fief_offer: Option<Id>,
    /// 封君指定的纳贡，None 为按比例自动纳贡
    pub tribute: Option<Tribute>,
    /// 这份纳贡是否过火
    pub tribute_excessive: bool,
    /// 封君征讨封臣的理由
    pub grievances: Vec<Grievance>,
    /// 府库今年的总产出
    pub income: Inventory,
    /// 通行权：人物 id → 之后还剩几回合
    pub passage: BTreeMap<Id, u32>,
    /// 邻城增益：产出增加的比例
    pub neighbor_bonus: Fx,
    /// 城墙还剩的防御值，没有城墙为 None；城中心还剩的防御值
    pub wall: Option<Fx>,
    pub center: Fx,
    pub occupied: bool,
    /// 攻占这座城的人
    pub occupier: Option<Id>,
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
    /// 税率（千分之一为单位）
    pub tax_rate: Fx,
    /// 士人名单
    pub officials: Vec<OfficialReport>,
    pub idle_officials: u32,
    pub groups: Vec<GroupReport>,
    pub assignments: Vec<AssignmentReport>,
    pub learning: Vec<Learning>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TileReport {
    pub id: Id,
    pub pos: Hex,
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
pub struct OfficialReport {
    pub id: Id,
    pub name: String,
    pub skills: Vec<String>,
    pub loyalty: Fx,
    pub service_turns: u32,
    /// 能管理的人数，含建筑加成
    pub capacity: u32,
    /// 正在主事的工事，闲着为 None
    pub assignment: Option<Id>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssignmentReport {
    pub id: Id,
    /// 主事的士人的名字
    pub shi: String,
    pub shi_id: Id,
    /// 人群 id → 人数
    pub groups: BTreeMap<Id, u32>,
    pub task: Task,
    pub workers: u32,
    pub progress: Fx,
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupReport {
    pub id: Id,
    pub name: String,
    pub size: u32,
    pub skills: Vec<String>,
    pub managed: u32,
    pub happiness: Fx,
    pub culture: Fx,
    pub loyalty: Fx,
    /// 对封君的敌视
    pub hostility: Fx,
}

#[derive(Debug, Clone, Serialize)]
pub struct CharacterReport {
    pub id: Id,
    pub name: String,
    pub rank: String,
    pub reputation: Fx,
    pub private_store: Inventory,
    /// 人物 id → 对他的信任
    pub trust: BTreeMap<Id, Fx>,
    pub nationality: Option<Id>,
    /// 所在的城市
    pub location: Option<Id>,
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
    unfed: Fx,
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
                slots
                    .entry(pool)
                    .or_insert_with_key(|pool| city.job_slots(config, pool));
            }
        }
        let capacity = slots
            .iter()
            .map(|(p, s)| (p.clone(), s.iter().map(|s| s.count).sum()))
            .collect();
        Jobs {
            slots,
            capacity,
            turned_away: BTreeMap::new(),
        }
    }

    /// 为 `workers` 人占用这种经营方式能用的劳作位置，返回折算产量倍率后的有效人数。
    fn take(&mut self, config: &Config, recipe: &str, workers: u32, mode: Mode) -> Fx {
        let Some(pool) = config.pool_of(recipe) else {
            return Fx::from(workers);
        };
        let slots = self
            .slots
            .get_mut(&pool)
            .expect("pool built for every recipe");
        let mut left = workers;
        let mut effective = Fx::ZERO;
        for slot in slots
            .iter_mut()
            .filter(|s| s.mode.is_none_or(|m| m == mode))
        {
            let n = left.min(slot.count);
            slot.count -= n;
            left -= n;
            effective += Fx::from(n) * slot.mult;
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
    let month = state.month(config);
    let mut events = std::mem::take(&mut state.pending_events);
    expire_offers(state, &mut events);

    let mut jobs = BTreeMap::new();
    let mut claims = Vec::new();
    let calendar = Calendar {
        season,
        month,
        first_month: state.turn.is_multiple_of(config.months_per_season),
    };
    let neighbor: BTreeMap<Id, Fx> = state
        .cities
        .values()
        .map(|c| (c.id, state.neighbor_mult(config, c)))
        .collect();
    let soldiers = crate::army::mouths(state, config);
    let next_id = &mut state.next_id;
    for city in state.cities.values_mut() {
        let outside = Outside {
            neighbor: neighbor[&city.id],
            soldiers: soldiers.get(&city.id).copied().unwrap_or_default(),
        };
        jobs.insert(
            city.id,
            resolve_city(
                city,
                config,
                calendar,
                outside,
                next_id,
                &mut claims,
                &mut events,
            ),
        );
    }
    settle_claims(state, claims, &mut events);
    // 起点城养活自己之后，再往前线送粮
    crate::supply::feed(state, config, &mut events);
    check_ritual(state, config, &mut events);
    check_countries(state, &mut events);
    crate::war::end_turn(state);
    state.settle_locations();
    if state.is_year_end(config) {
        settle_tributes(state, config, &mut events);
        for city in state.cities.values_mut() {
            city.last_income = Some(std::mem::take(&mut city.income));
        }
        settle_quotas(state, config, &mut events);
    }
    expire_passage(state, &mut events);
    for city in state.cities.values() {
        if city.status == CityStatus::Occupied && city.manager.is_none() {
            events
                .push(Event::new(format!("{}被攻占后还没有人接管", city.name)).to(city.insiders()));
        }
    }
    // 新回合开始：军队的行军力回满，没走完的行军接着走（到期的通行权上面已经去掉了）
    crate::army::start_turn(state, config, &mut events);
    crate::supply::refresh_all(state, config);
    crate::vision::observe(state, config);

    let report = build_report(state, config, events, jobs);
    state.turn += 1;
    report
}

/// 回合中的报告：还没结算，事件是回合中已经发生的，工作记录为空。服务器在玩家下指令后用它推送最新情况。
pub fn snapshot(state: &GameState, config: &Config) -> TurnReport {
    build_report(state, config, state.pending_events.clone(), BTreeMap::new())
}

fn build_report(
    state: &GameState,
    config: &Config,
    events: Vec<Event>,
    mut jobs: BTreeMap<Id, Vec<JobReport>>,
) -> TurnReport {
    let season = state.season_index(config);
    let year = state.year(config);
    let month = state.month(config);
    TurnReport {
        turn: state.turn,
        year,
        month,
        season: config.seasons[season].clone(),
        events,
        map: map_report(state),
        countries: state.countries.values().cloned().collect(),
        wars: state.wars.values().cloned().collect(),
        armies: state
            .armies
            .values()
            .map(|a| ArmyReport {
                id: a.id,
                city: a.city,
                commander: a.commander,
                pos: a.pos,
                dest: a.dest,
                moves_left: a.moves_left,
                on_wall: a.on_wall,
                on_campaign: a.on_campaign(state),
                supply: a.supply.as_ref().map(|l| SupplyReport {
                    from: l.from,
                    convoy: l.convoy.count,
                    path: l.path.clone(),
                }),
                hunger: a.hunger,
                strength: a.fed_mult(config),
                squads: a
                    .squads
                    .iter()
                    .map(|s| SquadReport {
                        unit: s.unit.clone(),
                        count: s.count,
                        group: s.group,
                        elite: s.is_elite(config),
                        gear: s.gear.clone(),
                        equipped: s.equipped(config),
                    })
                    .collect(),
            })
            .collect(),
        cities: state
            .cities
            .values()
            .map(|c| {
                city_report(
                    state,
                    c,
                    config,
                    month,
                    jobs.remove(&c.id).unwrap_or_default(),
                )
            })
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
                nationality: c.nationality,
                location: c.location,
            })
            .collect(),
    }
}

/// 结算一座城要用到的城外的情况。
#[derive(Clone, Copy)]
struct Outside {
    /// 邻城增益的产出倍数
    neighbor: Fx,
    /// 这座城要供养的军中人数：(脱产精兵, 征发来的兵)
    soldiers: (u32, u32),
}

/// 当前回合在一年中的位置。
#[derive(Clone, Copy)]
struct Calendar {
    season: usize,
    /// 一年中的第几个月，从 1 开始
    month: u32,
    /// 是否是这一季的第一个月
    first_month: bool,
}

fn resolve_city(
    city: &mut City,
    config: &Config,
    calendar: Calendar,
    outside: Outside,
    next_id: &mut Id,
    claims: &mut Vec<Claim>,
    events: &mut Vec<Event>,
) -> Vec<JobReport> {
    let Calendar {
        season,
        month,
        first_month,
    } = calendar;
    let insiders = city.insiders();
    clamp_assignments(city, events);
    let city_mult = city.efficiency(config);
    let mut ledgers: BTreeMap<Id, Ledger> = city
        .groups
        .keys()
        .map(|id| (*id, Ledger::default()))
        .collect();
    let mut jobs = Jobs::new(city, config);

    // 1. 工事生产：工事先占劳作位置，产量高的地块先被占用
    let assignments: Vec<_> = city.assignments.values().cloned().collect();
    // 忠诚度低的人群罢工：派去官营的人只有一部分肯出工，其余的人回去自主民营
    let mut working: BTreeMap<(Id, Id), u32> = BTreeMap::new();
    let mut strikers: BTreeMap<Id, u32> = BTreeMap::new();
    for a in &assignments {
        let guan = matches!(
            a.task,
            Task::Produce {
                mode: Mode::Guan,
                ..
            } | Task::Construct { .. }
                | Task::Improve { .. }
                | Task::Reclaim { .. }
                | Task::RepairWall
        );
        for (group, count) in &a.workers {
            let turnout = if guan {
                config.disloyalty_mult(city.groups[group].loyalty, config.loyalty.guan_turnout)
            } else {
                Fx::ONE
            };
            let n = (turnout * *count).floor_u32();
            working.insert((a.id, *group), n);
            *strikers.entry(*group).or_insert(0) += count - n;
        }
    }
    for (group, n) in &strikers {
        if *n > 0 {
            events.push(
                Event::new(format!(
                    "{}：{}对管理者不够忠诚，{} 人罢工，回去自己干活",
                    city.name, city.groups[group].name, n
                ))
                .to(insiders.iter().copied()),
            );
        }
    }
    // 先排好每一份劳作占到的位置，播种和收成要看全城干这一行的总人数
    let mut batches: Vec<(&String, Id, u32, Fx, Mode)> = Vec::new();
    for a in &assignments {
        if let Task::Produce { recipe, mode } = &a.task {
            for group in a.workers.keys() {
                let count = working[&(a.id, *group)];
                let effective = jobs.take(config, recipe, count, *mode);
                batches.push((recipe, *group, count, effective, *mode));
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
                batches.push((recipe, *group, unmanaged, effective, Mode::Min));
            }
        }
    }
    // 播种季记下种了多少；其他季节收成不超过播种量，多出来的人白忙
    let mut harvest: BTreeMap<&String, Fx> = BTreeMap::new();
    for (recipe, def) in &config.recipes {
        let Some(sowing) = def.sowing_season else {
            continue;
        };
        let total: Fx = batches.iter().filter(|b| b.0 == recipe).map(|b| b.3).sum();
        if season == sowing {
            if first_month {
                city.sown.insert(recipe.clone(), Fx::ZERO);
            }
            *city.sown.entry(recipe.clone()).or_insert(Fx::ZERO) +=
                total / config.months_per_season;
        } else if let Some(sown) = city.sown.get(recipe)
            && total > *sown
        {
            harvest.insert(recipe, sown / total);
        }
    }
    for (recipe, group, count, effective, mode) in batches {
        let def = &config.recipes[recipe];
        let effective = effective * harvest.get(recipe).copied().unwrap_or(Fx::ONE);
        // 邻城增益只加在产出上，不加在营建、开垦的进度上
        produce(
            city,
            config,
            def,
            group,
            count,
            effective,
            mode,
            season,
            city_mult * outside.neighbor,
            &mut ledgers,
        );
    }
    let job_report = jobs.report();
    for job in &job_report {
        if job.turned_away > 0 {
            events.push(
                Event::new(format!(
                    "{}：「{}」已满，{} 人无处劳作",
                    city.name,
                    config.pool_name(&job.pool),
                    job.turned_away
                ))
                .to(insiders.iter().copied()),
            );
        }
    }

    // 2. 营建、改良地块、开垦：都按投入的人力累积进度，视为官营。修城墙直接回城墙的防御值
    for a in &assignments {
        if a.task == Task::RepairWall {
            let mut repaired = Fx::ZERO;
            for group in a.workers.keys() {
                let count = working[&(a.id, *group)];
                let happiness = config.happiness_output_mult(city.groups[group].happiness);
                repaired += config.siege.repair_per_100 * count / 100 * happiness * city_mult;
                ledgers.get_mut(group).expect("ledger per group").guan_labor += count;
            }
            if city.wall_attacked {
                events.push(
                    Event::new(format!("{}：城墙正在挨打，修不了", city.name))
                        .to(insiders.iter().copied()),
                );
                continue;
            }
            city.wall_damage = (city.wall_damage - repaired).max(Fx::ZERO);
            if city.wall_damage <= Fx::ZERO {
                city.assignments.remove(&a.id);
                events.push(
                    Event::new(format!("{}：城墙修好了", city.name)).to(insiders.iter().copied()),
                );
            }
            continue;
        }
        let (labor, season_mult, name) = match &a.task {
            Task::Construct { building, .. } => {
                let def = &config.buildings[building];
                (def.labor, &def.season_mult, &def.name)
            }
            Task::Improve { improvement, .. } => {
                let def = &config.improvements[improvement];
                (def.labor, &def.season_mult, &def.name)
            }
            Task::Reclaim { terrain, .. } => {
                let def = &config.terrains[terrain];
                (def.reclaim_labor, &config.reclaim.season_mult, &def.name)
            }
            _ => continue,
        };
        let mut progress = Fx::ZERO;
        for group in a.workers.keys() {
            let count = working[&(a.id, *group)];
            let happiness = config.happiness_output_mult(city.groups[group].happiness);
            progress += Fx::from(count) * season_mult[season] * happiness * city_mult;
            ledgers.get_mut(group).expect("ledger per group").guan_labor += count;
        }
        let assignment = city.assignments.get_mut(&a.id).expect("assignment exists");
        assignment.progress += progress;
        if assignment.progress < labor {
            continue;
        }
        city.assignments.remove(&a.id);
        match &a.task {
            Task::Construct {
                building,
                tile,
                mode,
            } => {
                if let Some(tile) = city.tiles.get_mut(tile) {
                    tile.buildings.push(Building {
                        kind: building.clone(),
                        mode: *mode,
                    });
                }
                events.push(
                    Event::new(format!("{}：{}竣工", city.name, name)).to(insiders.iter().copied()),
                );
            }
            Task::Improve { tile, improvement } => {
                let max = config.improvements[improvement].max_level;
                if let Some(tile) = city.tiles.get_mut(tile) {
                    let level = tile.improvements.entry(improvement.clone()).or_insert(0);
                    *level = (*level + 1).min(max);
                    let terrain = &config.terrains[&tile.terrain].name;
                    events.push(
                        Event::new(format!(
                            "{}：{}{}完成，现为 {} 级",
                            city.name, terrain, name, level
                        ))
                        .to(insiders.iter().copied()),
                    );
                }
            }
            Task::Reclaim { at, terrain } => {
                claims.push(Claim {
                    city: city.id,
                    at: *at,
                    terrain: terrain.clone(),
                    name: name.clone(),
                });
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
        events.push(
            Event::new(format!(
                "{}：{} {} 人学会{}，归入{}",
                city.name,
                source_name,
                l.count,
                config.skill_name(&l.skill),
                city.groups[&target_id].name
            ))
            .to(insiders.iter().copied()),
        );
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
            let loyalty =
                config.disloyalty_mult(city.groups[group_id].loyalty, config.loyalty.tax_share);
            let share = Fx::ratio(*count as i64, size as i64).min(Fx::ONE) * loyalty;
            let ledger = ledgers.get_mut(group_id).expect("ledger per group");
            ledger.taxed += count;
            for (good, income) in ledger.income.iter() {
                let taken = city
                    .private_wealth
                    .take(good, income * city.tax_rate * share);
                city.treasury.add(good, taken);
                city.income.add(good, taken);
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
        let mut count = 0;
        let mut weight = Fx::ZERO;
        for (good, w) in &sacrifice.vessels {
            let n = city.treasury.get(good).floor();
            count += n;
            weight += *w * n;
        }
        if count >= 1 {
            for group in city.groups.values_mut() {
                let room = (Fx::ONE - group.culture / sacrifice.culture_max).max(Fx::ZERO);
                group.culture += sacrifice.culture_per_vessel * weight * room;
            }
            sacrificed = true;
            events.push(
                Event::new(format!("{}：举行祭祀，用礼器 {} 件", city.name, count))
                    .to(insiders.iter().copied()),
            );
        }
        city.assignments.remove(&a.id);
    }

    // 5. 人口消耗：全城人口共用民间财富，不足时由府库补足；仍有缺口时各群人按同一比例挨饿。
    // 脱产精兵（技能槽只有「军武」）不产粮，由府库供养。这座城编成的军队也由这座城供养，
    // 精兵吃府库，征发来的兵吃民间（等粮道做出来，出征的军队改由粮道供粮）
    let pop = &config.population;
    let elite = city
        .groups
        .values()
        .filter(|g| crate::army::is_elite(&g.skills, config))
        .map(|g| g.size)
        .sum::<u32>()
        + outside.soldiers.0;
    let commons = city.population() + outside.soldiers.0 + outside.soldiers.1 - elite;
    let mut unfed: Fx = Fx::ZERO;
    for rule in &pop.consumption {
        let (elite_need, need) = (
            rule.amount * elite / pop.per_people,
            rule.amount * (elite + commons) / pop.per_people,
        );
        let mut got = Fx::ZERO;
        for good in &rule.goods {
            got += city.treasury.take(good, elite_need - got);
        }
        for good in &rule.goods {
            got += city.private_wealth.take(good, need - got);
        }
        if pop.treasury_covers_shortfall {
            for good in &rule.goods {
                got += city.treasury.take(good, need - got);
            }
        }
        if need > Fx::ZERO && got < need {
            let shortfall = Fx::ONE - got / need;
            unfed = unfed.max(shortfall);
            events.push(
                Event::new(format!(
                    "{}：全城缺{}，缺口 {:.0}%",
                    city.name,
                    rule.name,
                    shortfall * Fx::int(100)
                ))
                .to(insiders.iter().copied()),
            );
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
            official.loyalty = (official.loyalty - shi.unpaid_loyalty_penalty).max(Fx::ZERO);
            unpaid += 1;
        }
        official.service_turns += 1;
    }
    if unpaid > 0 {
        events.push(
            Event::new(format!("{}：府库无力供养 {} 名士人", city.name, unpaid))
                .to(insiders.iter().copied()),
        );
    }

    // 7. 愉悦度
    let h = &config.happiness;
    let population = city.population();
    let housing = city.housing(config);
    // 藏富于民：全城民间财富的人均价值越高，所有人群的愉悦度越高
    let wealth_bonus = if population == 0 || h.wealth_full_per_person <= Fx::ZERO {
        Fx::ZERO
    } else {
        let per_person = config.value_of(&city.private_wealth) / population;
        h.wealth_bonus * (per_person / h.wealth_full_per_person).min(Fx::ONE)
    };
    let building_bonus: Fx = city
        .buildings()
        .iter()
        .map(|(id, n)| config.buildings[id].happiness * *n)
        .sum();
    // 人口超出居住上限时拥挤，超出一倍时扣满
    let crowding = if population > housing {
        events.push(
            Event::new(format!(
                "{}：人口 {} 超出居住上限 {}，拥挤不堪",
                city.name, population, housing
            ))
            .to(insiders.iter().copied()),
        );
        Fx::ratio((population - housing) as i64, housing.max(1) as i64).min(Fx::ONE)
    } else {
        Fx::ZERO
    };
    // 封君要的纳贡过火，封地百姓敌视封君；不过火时慢慢消退
    let f = &config.fief;
    let hostility_delta = match fief::overreach(city, config, month) {
        Some(overreach) => f.hostility_gain * overreach.min(Fx::int(2)),
        None => -f.hostility_decay,
    };
    for group in city.groups.values_mut() {
        group.hostility = (group.hostility + hostility_delta).clamp(Fx::ZERO, f.hostility_max);
        if group.size == 0 {
            continue;
        }
        let ledger = &ledgers[&group.id];
        let size = group.size as i64;
        let guan_share = Fx::ratio(ledger.guan_labor as i64, size).min(Fx::ONE);
        let taxed_share = Fx::ratio(ledger.taxed as i64, size).min(Fx::ONE);
        let mut delta = (h.target - group.happiness) * h.drift_rate + wealth_bonus + building_bonus;
        delta -= h.guan_penalty * guan_share;
        delta -= h.tax_penalty * city.tax_rate * taxed_share;
        delta -= h.hunger_penalty * ledger.unfed;
        delta -= h.crowding_penalty * crowding;
        // 农忙时被调去做别的工事
        if h.busy_penalty[season] > Fx::ZERO && group.has_skill(&h.busy_skill) {
            let taken: u32 = city
                .assignments
                .values()
                .filter(|a| a.task.uses_labor() && !is_busy_work(&a.task, config, &h.busy_skill))
                .filter_map(|a| a.workers.get(&group.id))
                .sum();
            delta -= h.busy_penalty[season] * Fx::ratio(taken as i64, size).min(Fx::ONE);
        }
        if sacrificed {
            delta += h.sacrifice_bonus;
        }
        group.happiness = (group.happiness + delta).clamp(Fx::ZERO, h.max);

        // 忠诚度：税越低越忠诚，税重了就下降
        let l = &config.loyalty;
        let max_rate = config.economy.max_tax_rate;
        let tax_burden = if max_rate > Fx::ZERO {
            city.tax_rate * taxed_share / max_rate
        } else {
            Fx::ZERO
        };
        let loyalty_delta = l.low_tax_gain - l.high_tax_loss * tax_burden;
        group.loyalty = (group.loyalty + loyalty_delta).clamp(Fx::ZERO, l.max);
    }

    // 8. 人口增减：增长不超过居住上限，各群人按想增长的多少分配剩余的空间
    let wanted: BTreeMap<Id, Fx> = city
        .groups
        .values()
        .filter(|g| ledgers[&g.id].unfed <= Fx::ZERO && g.happiness >= pop.growth_min_happiness)
        .map(|g| (g.id, pop.growth_rate * g.size * g.happiness / h.max))
        .collect();
    let total_wanted: Fx = wanted.values().sum();
    let room = Fx::from(housing.saturating_sub(population));
    let growth_factor = if total_wanted > Fx::ZERO {
        (room / total_wanted).min(Fx::ONE)
    } else {
        Fx::ZERO
    };
    for group in city.groups.values_mut() {
        let ledger = &ledgers[&group.id];
        if ledger.unfed > Fx::ZERO {
            let dead = (ledger.unfed * pop.starvation_rate * group.size)
                .ceil()
                .max(0) as u32;
            group.size = group.size.saturating_sub(dead);
            if dead > 0 {
                events.push(
                    Event::new(format!("{}：{}饿死 {} 人", city.name, group.name, dead))
                        .to(insiders.iter().copied()),
                );
            }
        } else if let Some(want) = wanted.get(&group.id) {
            group.growth_carry += want * growth_factor;
            let born = group.growth_carry.floor();
            group.growth_carry -= Fx::int(born);
            group.size += born.max(0) as u32;
        }
        if group.happiness < pop.emigration_threshold {
            let left = (pop.emigration_rate * group.size).ceil().max(0) as u32;
            group.size = group.size.saturating_sub(left);
            if left > 0 {
                events.push(
                    Event::new(format!(
                        "{}：{}因不满外流 {} 人",
                        city.name, group.name, left
                    ))
                    .to(insiders.iter().copied()),
                );
            }
        }
    }
    clamp_assignments(city, events);
    job_report
}

/// 城市有没有条件、这群人有没有本事自主民营这个配方。
fn can_make(city: &City, config: &Config, recipe: &RecipeDef, group: &PopGroup) -> bool {
    recipe.resource.as_ref().is_none_or(|r| city.has_terrain(r))
        && recipe
            .facility
            .as_ref()
            .is_none_or(|f| city.has_building(f, Mode::Min))
        && recipe
            .min_specialization
            .is_none_or(|min| config.specialization(group.skills.len()) >= min)
}

#[allow(clippy::too_many_arguments)]
fn produce(
    city: &mut City,
    config: &Config,
    recipe: &RecipeDef,
    group_id: Id,
    workers: u32,
    effective: Fx,
    mode: Mode,
    season: usize,
    city_mult: Fx,
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
    if scale <= Fx::ZERO {
        return;
    }

    // 官营的原料和产出都在府库，民营的在全城共有的民间财富中
    let (stock, mut year_income) = match mode {
        Mode::Guan => (&mut city.treasury, Some(&mut city.income)),
        Mode::Min => (&mut city.private_wealth, None),
    };
    // 原料不足时按比例减产
    let mut factor: Fx = Fx::ONE;
    for (good, per_worker) in recipe.inputs.iter() {
        let need = per_worker * scale;
        if need > Fx::ZERO {
            factor = factor.min(stock.get(good) / need);
        }
    }
    let scale = scale * factor.clamp(Fx::ZERO, Fx::ONE);
    for (good, per_worker) in recipe.inputs.iter() {
        stock.take(good, per_worker * scale);
    }
    // 专精度越高，产出中精品的比例越高
    let fine_share = config.fine_share(skill_count);
    for (good, per_worker) in recipe.outputs.iter() {
        let amount = per_worker * scale;
        let made = match &config.goods[good].fine {
            Some(fine) => vec![
                (good.as_str(), amount * (Fx::ONE - fine_share)),
                (fine.as_str(), amount * fine_share),
            ],
            None => vec![(good.as_str(), amount)],
        };
        for (good, amount) in made {
            stock.add(good, amount);
            match &mut year_income {
                Some(income) => income.add(good, amount),
                None => ledger.income.add(good, amount),
            }
        }
    }
}

/// 人群缩小后，削减超出人数的工事投入和学技能的人数，后开的先削减。
/// 削减到没有人的工事就此结束，主事的士人空出来。
fn clamp_assignments(city: &mut City, events: &mut Vec<Event>) {
    let insiders = city.insiders();
    for (group_id, group) in &city.groups {
        let mut learning = city.learning_in(*group_id);
        for l in city
            .learning
            .values_mut()
            .rev()
            .filter(|l| l.group == *group_id)
        {
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
        events.push(
            Event::new(format!(
                "{}：{}主事的工事已经没有人，就此结束",
                city.name, shi
            ))
            .to(insiders.iter().copied()),
        );
    }
}

/// 礼法检查，超出爵位允许的都视为僭越，扣除风评：
/// - 列鼎制度：所有者名下城市府库中的鼎
/// - 城市面积：所有者名下每座城市的地块数
///
/// 同一次僭越只扣一次风评。超出得更多时只扣多出的部分；
/// 回到上限以内后又超出，算作新的僭越，再扣一次。
fn check_ritual(state: &mut GameState, config: &Config, events: &mut Vec<Event>) {
    let ritual = &config.ritual;
    let turn = state.turn;
    for character in state.characters.values_mut() {
        let owned: Vec<&City> = state
            .cities
            .values()
            .filter(|c| c.owner == Some(character.id))
            .collect();
        let dings: u32 = owned
            .iter()
            .map(|c| {
                ritual
                    .ding_goods
                    .iter()
                    .map(|g| c.treasury.get(g).floor_u32())
                    .sum::<u32>()
            })
            .sum();
        let limit = ritual.ding_limit.get(&character.rank).copied().unwrap_or(0);
        let excess = dings.saturating_sub(limit);
        if excess > character.punished_ding_excess {
            character.reputation -=
                ritual.excess_reputation_penalty * (excess - character.punished_ding_excess);
            character.usurped_turn = Some(turn);
            events.push(Event::public(format!(
                "{}：拥有鼎 {} 只，超出身份上限 {} 只，僭越，风评下降",
                character.name, dings, limit
            )));
        }
        character.punished_ding_excess = excess;

        let tile_limit = ritual.tile_limit.get(&character.rank).copied().unwrap_or(0);
        let mut punished = BTreeMap::new();
        for city in owned {
            let tiles = city.tiles.len() as u32;
            let excess = tiles.saturating_sub(tile_limit);
            let before = character
                .punished_tile_excess
                .get(&city.id)
                .copied()
                .unwrap_or(0);
            if excess > before {
                character.reputation -= ritual.excess_tile_penalty * (excess - before);
                character.usurped_turn = Some(turn);
                events.push(Event::public(format!(
                    "{}：{}有地 {} 块，超出身份上限 {} 块，僭越，风评下降",
                    character.name, city.name, tiles, tile_limit
                )));
            }
            if excess > 0 {
                punished.insert(city.id, excess);
            }
        }
        character.punished_tile_excess = punished;
    }
}

/// 赠与、索取、交易、任命管理者、封授都只在发起的当回合有效：回合结束时还没答复就自动作废。
fn expire_offers(state: &mut GameState, events: &mut Vec<Event>) {
    let name = |id: Id| state.characters.get(&id).map_or("?", |c| c.name.as_str());
    for deal in state.deals.values() {
        events.push(
            Event::new(format!(
                "{}向{}提出的{}没有在当回合答复，已作废",
                name(deal.from),
                name(deal.to),
                deal.kind_name()
            ))
            .to([deal.from, deal.to]),
        );
    }
    for city in state.cities.values() {
        if let Some(to) = city.manager_offer {
            events.push(
                Event::new(format!(
                    "{}：任命{}为管理者，没有在当回合答复，已作废",
                    city.name,
                    name(to)
                ))
                .to(city.insiders())
                .to([to]),
            );
        }
        if let Some(to) = city.fief_offer {
            events.push(
                Event::new(format!(
                    "{}：封授给{}，没有在当回合答复，已作废",
                    city.name,
                    name(to)
                ))
                .to(city.insiders())
                .to([to]),
            );
        }
    }
    state.deals.clear();
    for city in state.cities.values_mut() {
        city.manager_offer = None;
        city.fief_offer = None;
    }
}

/// 通行权到这一回合为止的，回合结束时到期。
fn expire_passage(state: &mut GameState, events: &mut Vec<Event>) {
    let next = state.turn + 1;
    for city in state.cities.values_mut() {
        let expired: Vec<Id> = city
            .passage
            .iter()
            .filter(|(_, until)| **until <= next)
            .map(|(who, _)| *who)
            .collect();
        for who in expired {
            city.passage.remove(&who);
            let name = state.characters.get(&who).map_or("?", |c| c.name.as_str());
            events.push(
                Event::new(format!("{}：{}的通行权到期", city.name, name))
                    .to(city.insiders())
                    .to([who]),
            );
        }
    }
}

/// 国君自己拥有的城全部被攻占（或者国君手里已经没有城），这个国家就灭亡了；
/// 国人都成了无国籍，包括国内的封臣。
fn check_countries(state: &mut GameState, events: &mut Vec<Event>) {
    let fallen: Vec<Id> = state
        .countries
        .values()
        .filter(|country| {
            !state
                .cities
                .values()
                .any(|c| c.owner == Some(country.ruler) && c.status != CityStatus::Occupied)
        })
        .map(|country| country.id)
        .collect();
    for id in fallen {
        let country = state.countries.remove(&id).expect("country exists");
        for character in state
            .characters
            .values_mut()
            .filter(|c| c.nationality == Some(id))
        {
            character.nationality = None;
        }
        events.push(Event::public(format!(
            "{}国君自己的城已经全部失守，就此灭亡",
            country.name
        )));
    }
}

/// 年末结算纳贡。封君没有指定纳贡时，自动从府库纳当年总产出的一定比例；
/// 指定了的看今年纳够没有，合理的纳贡没纳够会成为征讨的理由。
fn settle_tributes(state: &mut GameState, config: &Config, events: &mut Vec<Event>) {
    let months = config.turns_per_year();
    for city in state.cities.values_mut() {
        let Some(liege_id) = city.liege else { continue };
        if city.status == CityStatus::Occupied {
            continue;
        }
        let liege_name = state
            .characters
            .get(&liege_id)
            .map_or_else(String::new, |c| c.name.clone());
        let excessive = fief::overreach(city, config, months).is_some();
        let audience = [city.insiders(), vec![liege_id]].concat();
        let Some(tribute) = &mut city.tribute else {
            let mut paid = Inventory::default();
            for (good, amount) in city.income.iter() {
                paid.add(
                    good,
                    city.treasury.take(good, amount * config.fief.auto_rate),
                );
            }
            city.tribute_shortfall_years = 0;
            if let Some(liege) = state.characters.get_mut(&liege_id) {
                liege.private_store.add_all(&paid);
            }
            events.push(
                Event::new(format!(
                    "{}：按例向{}纳贡 {}",
                    city.name,
                    liege_name,
                    describe(&paid, config)
                ))
                .to(audience.iter().copied()),
            );
            continue;
        };
        let required = config.value_of(&tribute.goods);
        let credited: Fx = tribute
            .goods
            .iter()
            .map(|(g, n)| {
                config.goods.get(g).map_or(Fx::ZERO, |d| d.value) * tribute.delivered.get(g).min(*n)
            })
            .sum();
        tribute.delivered = Inventory::default();
        if tribute.refused {
            events.push(
                Event::new(format!("{}：拒绝了{}过火的纳贡", city.name, liege_name))
                    .to(audience.iter().copied()),
            );
        } else if credited >= required {
            city.tribute_shortfall_years = 0;
            events.push(
                Event::new(format!("{}：今年向{}纳贡已足", city.name, liege_name))
                    .to(audience.iter().copied()),
            );
        } else if excessive {
            events.push(
                Event::new(format!(
                    "{}：{}要的纳贡过火，今年没有纳够",
                    city.name, liege_name
                ))
                .to(audience.iter().copied()),
            );
        } else {
            city.tribute_shortfall_years += 1;
            events.push(
                Event::new(format!(
                    "{}：今年向{}纳贡只纳了 {:.0}%，已连续 {} 年不足",
                    city.name,
                    liege_name,
                    credited / required.max(Fx::EPS) * Fx::int(100),
                    city.tribute_shortfall_years
                ))
                .to(audience.iter().copied()),
            );
        }
    }
}

fn describe(goods: &Inventory, config: &Config) -> String {
    let parts: Vec<String> = goods
        .iter()
        .filter(|(_, n)| **n >= Fx::milli(5))
        .map(|(g, n)| format!("{} {n:.2}", config.good_name(g)))
        .collect();
    if parts.is_empty() {
        "无".into()
    } else {
        parts.join("、")
    }
}

/// 是不是本行的农活：种这一行的配方。
fn is_busy_work(task: &Task, config: &Config, skill: &str) -> bool {
    matches!(task, Task::Produce { recipe, .. } if config.recipes[recipe].skill == skill)
}

/// 开垦完成、等待划入城市的一格。
struct Claim {
    city: Id,
    at: Hex,
    terrain: String,
    name: String,
}

/// 各城都结算完后，再把开垦好的格子划进城里。同一回合有两座城开垦同一格时，先结算的城得到它；
/// 晚了一步的城白费人力。
fn settle_claims(state: &mut GameState, claims: Vec<Claim>, events: &mut Vec<Event>) {
    for claim in claims {
        if let Some(owner) = state.city_at(claim.at) {
            let (owner, city) = (&state.cities[&owner].name, &state.cities[&claim.city].name);
            events.push(
                Event::new(format!(
                    "{city}：{} 的{}被{owner}抢先占了，开垦白费",
                    claim.at, claim.name
                ))
                .to(state.cities[&claim.city].insiders()),
            );
            continue;
        }
        let id = state.alloc_id();
        let city = state
            .cities
            .get_mut(&claim.city)
            .expect("claiming city exists");
        city.tiles
            .insert(id, Tile::new(id, claim.at, claim.terrain));
        events.push(
            Event::new(format!(
                "{}：开垦出 {} 的一块{}",
                city.name, claim.at, claim.name
            ))
            .to(city.insiders()),
        );
    }
}

/// 年末结算规定任务：看全年累计上缴的物资交够没有，调整所有者对管理者的信任，然后清零重新累计。
/// 缺口和超额都按物品价值折算。
fn settle_quotas(state: &mut GameState, config: &Config, events: &mut Vec<Event>) {
    let q = &config.quota;
    for city in state.cities.values_mut() {
        let (Some(owner), Some(manager)) = (city.owner, city.manager) else {
            continue;
        };
        if owner == manager || city.status == CityStatus::Occupied {
            continue;
        }
        let Some(quota) = &mut city.quota else {
            continue;
        };
        let value = |good: &str| config.goods.get(good).map_or(Fx::ZERO, |g| g.value);
        let required: Fx = quota.goods.iter().map(|(g, n)| value(g) * n).sum();
        let credited: Fx = quota
            .goods
            .iter()
            .map(|(g, n)| value(g) * quota.delivered.get(g).min(*n))
            .sum();
        let delivered: Fx = quota
            .goods
            .iter()
            .map(|(g, _)| value(g) * quota.delivered.get(g))
            .sum();
        quota.delivered = Inventory::default();
        if required <= Fx::ZERO {
            continue;
        }
        let ratio = credited / required;
        let (change, result) = if ratio < Fx::ONE {
            (
                -q.fail_trust_penalty * (Fx::ONE - ratio),
                format!("只完成 {:.0}%", ratio * Fx::int(100)),
            )
        } else if delivered >= required * q.exceed_ratio {
            (q.exceed_trust, "超额完成".to_string())
        } else {
            (q.complete_trust, "完成".to_string())
        };
        let Some(owner) = state.characters.get_mut(&owner) else {
            continue;
        };
        let trust = (owner.trust_in(manager, config) + change).clamp(Fx::ZERO, q.max_trust);
        owner.trust.insert(manager, trust);
        events.push(
            Event::new(format!(
                "{}：今年的规定任务{}，{}对管理者的信任变为 {:.0}",
                city.name, result, owner.name, trust
            ))
            .to(city.insiders()),
        );
    }
}

fn map_report(state: &GameState) -> Vec<CellReport> {
    let owners: BTreeMap<Hex, Id> = state
        .cities
        .values()
        .flat_map(|c| c.tiles.values().map(move |t| (t.pos, c.id)))
        .collect();
    state
        .map
        .cells
        .values()
        .map(|c| CellReport {
            pos: c.pos,
            terrain: c.terrain.clone(),
            city: owners.get(&c.pos).copied(),
        })
        .collect()
}

fn city_report(
    state: &GameState,
    city: &City,
    config: &Config,
    month: u32,
    jobs: Vec<JobReport>,
) -> CityReport {
    let assignments = city
        .assignments
        .values()
        .map(|a| AssignmentReport {
            id: a.id,
            shi: city
                .officials
                .get(&a.shi)
                .map_or_else(String::new, |o| o.name.clone()),
            shi_id: a.shi,
            groups: a.workers.clone(),
            task: a.task.clone(),
            workers: a.worker_count(),
            progress: a.progress,
        })
        .collect();
    let busy: Vec<Id> = city.assignments.values().map(|a| a.shi).collect();
    CityReport {
        id: city.id,
        name: city.name.clone(),
        seat: city.seat,
        country: state.country_of(city),
        liege: city.liege,
        fief_offer: city.fief_offer,
        tribute: city.tribute.clone(),
        // 年末结算后今年的产出已经清零，按去年的算
        tribute_excessive: fief::overreach(city, config, month % config.turns_per_year()).is_some(),
        grievances: fief::grievances(state, config, city),
        income: city.income.clone(),
        passage: city
            .passage
            .iter()
            .map(|(who, until)| (*who, until.saturating_sub(state.turn + 1)))
            .collect(),
        neighbor_bonus: state.neighbor_mult(config, city) - Fx::ONE,
        wall: city.wall_hp(config),
        center: (config.siege.center_hp - city.center_damage).max(Fx::ZERO),
        occupied: city.status == CityStatus::Occupied,
        occupier: city.occupier,
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
                pos: t.pos,
                terrain: t.terrain.clone(),
                improvements: t.improvements.clone(),
                buildings: t.buildings.clone(),
            })
            .collect(),
        jobs,
        tax_rate: city.tax_rate,
        officials: city
            .officials
            .values()
            .map(|o| OfficialReport {
                id: o.id,
                name: o.name.clone(),
                skills: o.skills.clone(),
                loyalty: o.loyalty,
                service_turns: o.service_turns,
                capacity: city.official_capacity(o.id, config),
                assignment: city
                    .assignments
                    .values()
                    .find(|a| a.shi == o.id)
                    .map(|a| a.id),
            })
            .collect(),
        idle_officials: city
            .officials
            .keys()
            .filter(|id| !busy.contains(id))
            .count() as u32,
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
                hostility: g.hostility,
            })
            .collect(),
        assignments,
        learning: city.learning.values().cloned().collect(),
    }
}
