//! 城市管理规则的测试，使用真实的 config/balance.toml。

use std::collections::BTreeMap;

use zhou_core::scenario::from_toml;
use zhou_core::{Building, Character, Command, CommandError, Config, GameState, Id, Inventory, Mode, Pool, Task, Tile, apply, end_turn};

fn config() -> Config {
    Config::from_toml(include_str!("../../../config/balance.toml")).expect("balance.toml is valid")
}

/// 一座城：农人（农·五谷、纺织）、四技能工匠、士族各一群，两名在任士人。
const CITY: &str = r#"
[[characters]]
key = "player"
name = "玩家"
rank = "nan"

[[cities]]
key = "city"
name = "测试城"
owner = "player"
manager = "player"
totem = "玄鸟"
tiles = { farmland = 2, forest = 1 }
guan_buildings = { foundry = 1 }
treasury = { grain = 100, salt = 100, bronze = 50 }
tax_rate = 0.2

[[cities.groups]]
key = "farmers"
name = "农人"
size = 100
skills = ["farming", "weaving"]
happiness = 600

[[cities.groups]]
key = "artisans"
name = "工匠"
size = 20
skills = ["smelting", "armory", "weaving", "hunting"]
happiness = 600

[[cities.groups]]
key = "gentry"
name = "士族"
size = 10
skills = ["shi"]
happiness = 600

[[cities.officials]]
key = "a"
name = "士甲"
from_group = "gentry"

[[cities.officials]]
key = "b"
name = "士乙"
from_group = "gentry"
"#;

struct Ids {
    city: Id,
    farmers: Id,
    artisans: Id,
    gentry: Id,
    shi_a: Id,
    shi_b: Id,
    player: Id,
}

fn setup(config: &Config) -> (GameState, Ids) {
    let state = from_toml(CITY, config).expect("scenario loads");
    let (&city, c) = state.cities.iter().next().unwrap();
    let group = |name: &str| *c.groups.iter().find(|(_, g)| g.name == name).unwrap().0;
    let official = |name: &str| *c.officials.iter().find(|(_, o)| o.name == name).unwrap().0;
    let ids = Ids {
        city,
        farmers: group("农人"),
        artisans: group("工匠"),
        gentry: group("士族"),
        shi_a: official("士甲"),
        shi_b: official("士乙"),
        player: *state.characters.keys().next().unwrap(),
    };
    (state, ids)
}

fn workers(pairs: &[(Id, u32)]) -> BTreeMap<Id, u32> {
    pairs.iter().copied().collect()
}

fn start(state: &mut GameState, config: &Config, ids: &Ids, shi: Id, task: Task, w: &[(Id, u32)]) -> Result<Vec<Id>, CommandError> {
    apply(state, config, ids.player, &Command::StartAssignment { city: ids.city, shi, task, workers: workers(w) })
}

fn forest_tile(state: &GameState, ids: &Ids) -> Id {
    *state.cities[&ids.city].tiles.iter().find(|(_, t)| t.terrain == "forest").unwrap().0
}

/// 直接在山林上放建筑，跳过营建。
fn place(state: &mut GameState, ids: &Ids, building: &str, mode: Mode, count: u32) {
    let tile = forest_tile(state, ids);
    let tile = state.cities.get_mut(&ids.city).unwrap().tiles.get_mut(&tile).unwrap();
    for _ in 0..count {
        tile.buildings.push(Building { kind: building.into(), mode });
    }
}

fn produce(recipe: &str, mode: Mode) -> Task {
    Task::Produce { recipe: recipe.into(), mode }
}

#[test]
fn demo_scenario_loads() {
    let config = config();
    from_toml(include_str!("../../../scenarios/demo.toml"), &config).expect("demo.toml is valid");
}

#[test]
fn appoint_from_group_with_shi_skill_keeps_skills() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let created = apply(&mut state, &config, ids.player, &Command::AppointShi { city: ids.city, group: ids.gentry, count: 2, drop_skill: None }).unwrap();
    let city = &state.cities[&ids.city];
    assert_eq!(created.len(), 2);
    assert_eq!(city.groups[&ids.gentry].size, 8);
    assert_eq!(city.officials[&created[0]].skills, vec!["shi".to_string()]);
}

#[test]
fn appoint_without_shi_skill_learns_it_immediately() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let created = apply(&mut state, &config, ids.player, &Command::AppointShi { city: ids.city, group: ids.farmers, count: 1, drop_skill: None }).unwrap();
    let city = &state.cities[&ids.city];
    assert_eq!(city.officials[&created[0]].skills, vec!["farming", "weaving", "shi"]);
    // 人群里的其他人不受影响
    assert_eq!(city.groups[&ids.farmers].skills, vec!["farming", "weaving"]);
    // 新士人当回合就能主事
    start(&mut state, &config, &ids, created[0], produce("grain", Mode::Min), &[(ids.farmers, 10)]).unwrap();
}

#[test]
fn appoint_with_full_skill_slots_requires_dropping_one() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let appoint = |drop: Option<&str>| Command::AppointShi { city: ids.city, group: ids.artisans, count: 1, drop_skill: drop.map(Into::into) };
    assert_eq!(apply(&mut state, &config, ids.player, &appoint(None)), Err(CommandError::SkillSlotsFull));
    assert_eq!(apply(&mut state, &config, ids.player, &appoint(Some("farming"))), Err(CommandError::NoSuchSkillToDrop("farming".into())));
    let created = apply(&mut state, &config, ids.player, &appoint(Some("hunting"))).unwrap();
    assert_eq!(state.cities[&ids.city].officials[&created[0]].skills, vec!["smelting", "armory", "weaving", "shi"]);
}

#[test]
fn appoint_only_from_unmanaged_people() {
    let config = config();
    let (mut state, ids) = setup(&config);
    start(&mut state, &config, &ids, ids.shi_a, produce("grain", Mode::Min), &[(ids.farmers, 40)]).unwrap();
    start(&mut state, &config, &ids, ids.shi_b, produce("cloth", Mode::Min), &[(ids.farmers, 40)]).unwrap();
    let err = apply(&mut state, &config, ids.player, &Command::AppointShi { city: ids.city, group: ids.farmers, count: 21, drop_skill: None });
    assert_eq!(err, Err(CommandError::NotEnoughUnmanaged { group: ids.farmers, available: 20, requested: 21 }));
}

#[test]
fn new_shi_manages_fewer_people_than_veterans() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let created = apply(&mut state, &config, ids.player, &Command::AppointShi { city: ids.city, group: ids.gentry, count: 1, drop_skill: None }).unwrap();
    let city = &state.cities[&ids.city];
    let veteran = city.official_capacity(ids.shi_a, &config);
    let fresh = city.official_capacity(created[0], &config);
    assert!(fresh < veteran, "新士人 {fresh} 应少于老士人 {veteran}");
}

#[test]
fn assignment_rejects_over_capacity_busy_shi_and_missing_skill() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let capacity = state.cities[&ids.city].official_capacity(ids.shi_a, &config);
    let err = start(&mut state, &config, &ids, ids.shi_a, produce("grain", Mode::Min), &[(ids.farmers, capacity + 1)]);
    assert_eq!(err, Err(CommandError::OverCapacity { capacity, requested: capacity + 1 }));

    let err = start(&mut state, &config, &ids, ids.shi_a, produce("grain", Mode::Min), &[(ids.artisans, 5)]);
    assert_eq!(err, Err(CommandError::MissingSkill { group: ids.artisans, skill: "farming".into() }));

    start(&mut state, &config, &ids, ids.shi_a, produce("grain", Mode::Min), &[(ids.farmers, 10)]).unwrap();
    let err = start(&mut state, &config, &ids, ids.shi_a, produce("cloth", Mode::Min), &[(ids.farmers, 10)]);
    assert_eq!(err, Err(CommandError::OfficialBusy(ids.shi_a)));
}

#[test]
fn guan_output_goes_to_treasury_min_output_to_people() {
    let config = config();
    let (mut state, ids) = setup(&config);
    start(&mut state, &config, &ids, ids.shi_a, produce("weapon", Mode::Guan), &[(ids.artisans, 20)]).unwrap();
    end_turn(&mut state, &config);
    let city = &state.cities[&ids.city];
    assert!(city.treasury.get("weapon") > 0.0);
    assert_eq!(city.private_wealth.get("weapon"), 0.0);

    let (mut state, ids) = setup(&config);
    start(&mut state, &config, &ids, ids.shi_a, produce("cloth", Mode::Min), &[(ids.farmers, 30)]).unwrap();
    end_turn(&mut state, &config);
    let city = &state.cities[&ids.city];
    assert!(city.private_wealth.get("cloth") > 0.0);
    assert_eq!(city.treasury.get("cloth"), 0.0);
}

#[test]
fn guan_is_less_efficient_than_min() {
    let config = config();
    let output = |mode: Mode| {
        let (mut state, ids) = setup(&config);
        start(&mut state, &config, &ids, ids.shi_a, produce("cloth", mode), &[(ids.farmers, 30)]).unwrap();
        // 其余农人按第一项技能种地，不产布，所以布全部来自这 30 人
        end_turn(&mut state, &config);
        let city = &state.cities[&ids.city];
        city.treasury.get("cloth") + city.private_wealth.get("cloth")
    };
    let guan = output(Mode::Guan);
    let min = output(Mode::Min);
    assert!((guan / min - config.economy.guan_efficiency).abs() < 1e-9, "官营 {guan}，民营 {min}");
}

#[test]
fn guan_lowers_happiness_more_than_min() {
    let config = config();
    let happiness = |mode: Mode| {
        let (mut state, ids) = setup(&config);
        start(&mut state, &config, &ids, ids.shi_a, produce("cloth", mode), &[(ids.farmers, 30)]).unwrap();
        end_turn(&mut state, &config);
        state.cities[&ids.city].groups[&ids.farmers].happiness
    };
    assert!(happiness(Mode::Guan) < happiness(Mode::Min));
}

#[test]
fn unmanaged_people_work_their_first_skill_for_themselves() {
    let config = config();
    let (mut state, ids) = setup(&config);
    // 秋季收获，跳到第 3 回合
    state.turn = 2;
    end_turn(&mut state, &config);
    let city = &state.cities[&ids.city];
    assert!(city.private_wealth.get("grain") > 0.0);
    assert_eq!(city.private_wealth.get("cloth"), 0.0);
}

#[test]
fn tax_takes_rate_of_covered_peoples_income() {
    let config = config();
    // 秋季：农人收获，比较收税覆盖 0 / 20 / 40 人时府库和民间的差别
    let run = |covered: u32| {
        let (mut state, ids) = setup(&config);
        state.turn = 2;
        if covered > 0 {
            start(&mut state, &config, &ids, ids.shi_a, Task::CollectTax, &[(ids.farmers, covered)]).unwrap();
        }
        end_turn(&mut state, &config);
        let city = &state.cities[&ids.city];
        (city.treasury.get("grain"), city.private_wealth.get("grain"))
    };
    let (treasury_0, wealth_0) = run(0);
    let (treasury_20, wealth_20) = run(20);
    let (treasury_40, _) = run(40);
    let tax_20 = treasury_20 - treasury_0;
    assert!(tax_20 > 0.0);
    assert!((tax_20 - (wealth_0 - wealth_20)).abs() < 1e-9, "税从民间财富转入府库");
    assert!((treasury_40 - treasury_0 - 2.0 * tax_20).abs() < 1e-9, "覆盖人数加倍，税收加倍");
}

#[test]
fn consumption_uses_private_wealth_before_treasury() {
    let config = config();
    let (mut state, ids) = setup(&config);
    state.cities.get_mut(&ids.city).unwrap().private_wealth.add("salt", 1.0);
    end_turn(&mut state, &config);
    let city = &state.cities[&ids.city];
    // 全城 130 人需盐 1.3：民间的 1.0 先用完，剩下 0.3 由府库补足
    assert_eq!(city.private_wealth.get("salt"), 0.0);
    assert!((city.treasury.get("salt") - (100.0 - 0.3)).abs() < 1e-9);
}

#[test]
fn richer_people_are_happier() {
    let config = config();
    let happiness = |private_grain: f64| {
        let (mut state, ids) = setup(&config);
        state.cities.get_mut(&ids.city).unwrap().private_wealth.add("grain", private_grain);
        end_turn(&mut state, &config);
        state.cities[&ids.city].groups[&ids.farmers].happiness
    };
    assert!(happiness(50.0) > happiness(0.0));
}

#[test]
fn goods_made_by_one_group_feed_the_whole_city() {
    let config = config();
    let scenario = r#"
[[cities]]
key = "city"
name = "盐城"
totem = "玄鸟"
tiles = { farmland = 2, salt_pond = 1 }

[[cities.groups]]
key = "farmers"
name = "农人"
size = 100
skills = ["farming"]
happiness = 600

[[cities.groups]]
key = "salters"
name = "盐户"
size = 20
skills = ["salt_making"]
happiness = 600
"#;
    let mut state = from_toml(scenario, &config).unwrap();
    // 秋季：农人产粮、盐户产盐，府库为空，全城靠民间互通吃饱
    state.turn = 2;
    let report = end_turn(&mut state, &config);
    assert!(report.events.iter().all(|e| !e.contains("缺")), "{:?}", report.events);
}

#[test]
fn shi_upkeep_comes_from_treasury_and_unpaid_shi_lose_loyalty() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let loyalty = state.cities[&ids.city].officials[&ids.shi_a].loyalty;
    state.cities.get_mut(&ids.city).unwrap().treasury.take("grain", 1000.0);
    end_turn(&mut state, &config);
    let city = &state.cities[&ids.city];
    assert!(city.officials[&ids.shi_a].loyalty < loyalty);
}

#[test]
fn construction_finishes_and_adds_building() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let tile = forest_tile(&state, &ids);
    let wall = Task::Construct { building: "city_wall".into(), tile, mode: Mode::Guan };
    start(&mut state, &config, &ids, ids.shi_a, wall, &[(ids.farmers, 40)]).unwrap();
    for _ in 0..40 {
        end_turn(&mut state, &config);
        if state.cities[&ids.city].has_building("city_wall", Mode::Guan) {
            break;
        }
    }
    let city = &state.cities[&ids.city];
    assert_eq!(city.tiles[&tile].buildings, vec![Building { kind: "city_wall".into(), mode: Mode::Guan }]);
    assert!(city.assignment_of(ids.shi_a).is_none(), "竣工后士人空闲");
}

#[test]
fn sacrifice_needs_vessels_and_raises_culture() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let err = start(&mut state, &config, &ids, ids.shi_a, Task::Sacrifice, &[]);
    assert_eq!(err, Err(CommandError::NoVessel));

    state.cities.get_mut(&ids.city).unwrap().treasury.add("gui", 2.0);
    let before = state.cities[&ids.city].groups[&ids.farmers].culture;
    start(&mut state, &config, &ids, ids.shi_a, Task::Sacrifice, &[]).unwrap();
    end_turn(&mut state, &config);
    let city = &state.cities[&ids.city];
    assert!(city.groups[&ids.farmers].culture > before);
    assert!(city.assignment_of(ids.shi_a).is_none(), "祭祀只持续一回合");
}

#[test]
fn owning_more_ding_than_rank_allows_costs_reputation() {
    let config = config();
    let (mut state, ids) = setup(&config);
    // 男爵上限三鼎
    state.cities.get_mut(&ids.city).unwrap().treasury.add("ding", 3.0);
    end_turn(&mut state, &config);
    assert_eq!(state.characters[&ids.player].reputation, 0.0);

    state.cities.get_mut(&ids.city).unwrap().treasury.add("ding", 2.0);
    end_turn(&mut state, &config);
    let penalty = config.ritual.excess_reputation_penalty;
    assert_eq!(state.characters[&ids.player].reputation, -2.0 * penalty);

    // 一直保持超出不再扣
    end_turn(&mut state, &config);
    assert_eq!(state.characters[&ids.player].reputation, -2.0 * penalty);
    // 超出得更多，只扣多出的部分
    state.cities.get_mut(&ids.city).unwrap().treasury.add("ding_fine", 1.0);
    end_turn(&mut state, &config);
    assert_eq!(state.characters[&ids.player].reputation, -3.0 * penalty);
    // 减到上限以内后又超出，再扣一次
    state.cities.get_mut(&ids.city).unwrap().treasury.take("ding", 3.0);
    end_turn(&mut state, &config);
    state.cities.get_mut(&ids.city).unwrap().treasury.add("ding", 1.0);
    end_turn(&mut state, &config);
    assert_eq!(state.characters[&ids.player].reputation, -4.0 * penalty);
}

#[test]
fn shrinking_groups_trim_assignments() {
    let config = config();
    let (mut state, ids) = setup(&config);
    start(&mut state, &config, &ids, ids.shi_a, produce("grain", Mode::Min), &[(ids.farmers, 30)]).unwrap();
    state.cities.get_mut(&ids.city).unwrap().groups.get_mut(&ids.farmers).unwrap().size = 10;
    end_turn(&mut state, &config);
    let city = &state.cities[&ids.city];
    assert!(city.labor_assigned(ids.farmers) <= city.groups[&ids.farmers].size);
}

#[test]
fn only_the_manager_can_command_the_city() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let steward = state.alloc_id();
    state.characters.insert(steward, Character::new(steward, "家宰", "none", 0.0));
    state.cities.get_mut(&ids.city).unwrap().manager = Some(steward);

    let command = Command::SetTaxRate { city: ids.city, rate: 0.3 };
    // 所有者不兼任管理者时，不能直接改税率
    assert_eq!(apply(&mut state, &config, ids.player, &command), Err(CommandError::NotManager(ids.city)));
    assert_eq!(state.cities[&ids.city].tax_rate, 0.2);
    apply(&mut state, &config, steward, &command).unwrap();
    assert_eq!(state.cities[&ids.city].tax_rate, 0.3);
}

fn farmland_tile(state: &GameState, ids: &Ids) -> Id {
    *state.cities[&ids.city].tiles.iter().find(|(_, t)| t.terrain == "farmland").unwrap().0
}

#[test]
fn farmland_limits_how_many_people_can_farm() {
    let config = config();
    let (mut state, ids) = setup(&config);
    // 两块耕地共 120 个位置，农人 100 人
    state.cities.get_mut(&ids.city).unwrap().groups.get_mut(&ids.farmers).unwrap().size = 150;
    state.turn = 2;
    let report = end_turn(&mut state, &config);
    let farmland = report.cities[0].jobs.iter().find(|j| j.pool == Pool::Terrain("farmland".into())).unwrap();
    assert_eq!((farmland.used, farmland.capacity, farmland.turned_away), (120, 120, 30));
}

#[test]
fn assignments_cannot_exceed_job_capacity() {
    let config = config();
    let (mut state, ids) = setup(&config);
    // 只留一块耕地，共 60 个位置
    let tile = farmland_tile(&state, &ids);
    state.cities.get_mut(&ids.city).unwrap().tiles.remove(&tile);
    start(&mut state, &config, &ids, ids.shi_a, produce("grain", Mode::Min), &[(ids.farmers, 45)]).unwrap();
    // 工事已占 45 个位置，只剩 15 个；未被管理的农人不占工事的名额
    let err = start(&mut state, &config, &ids, ids.shi_b, produce("grain", Mode::Min), &[(ids.farmers, 16)]);
    assert_eq!(err, Err(CommandError::NoJobs { pool: "耕地".into(), available: 15, requested: 16 }));
    start(&mut state, &config, &ids, ids.shi_b, produce("grain", Mode::Min), &[(ids.farmers, 15)]).unwrap();
}

#[test]
fn irrigation_raises_farmland_yield_up_to_max_level() {
    let config = config();
    let grain_after_harvest = |irrigated: u32| {
        let (mut state, ids) = setup(&config);
        let tile = farmland_tile(&state, &ids);
        if irrigated > 0 {
            state.cities.get_mut(&ids.city).unwrap().tiles.get_mut(&tile).unwrap().improvements.insert("irrigation".into(), irrigated);
        }
        state.turn = 2;
        end_turn(&mut state, &config);
        state.cities[&ids.city].private_wealth.get("grain")
    };
    assert!(grain_after_harvest(1) > grain_after_harvest(0));
    assert!(grain_after_harvest(2) > grain_after_harvest(1));

    let (mut state, ids) = setup(&config);
    // 测试城没有会水利的人，先让农人学
    let tile = farmland_tile(&state, &ids);
    let irrigate = Task::Improve { tile, improvement: "irrigation".into() };
    let err = start(&mut state, &config, &ids, ids.shi_a, irrigate.clone(), &[(ids.farmers, 10)]);
    assert_eq!(err, Err(CommandError::MissingSkill { group: ids.farmers, skill: "water".into() }));

    let forest = forest_tile(&state, &ids);
    let city = state.cities.get_mut(&ids.city).unwrap();
    city.groups.get_mut(&ids.farmers).unwrap().skills = vec!["farming".into(), "water".into()];
    let err = start(&mut state, &config, &ids, ids.shi_a, Task::Improve { tile: forest, improvement: "irrigation".into() }, &[(ids.farmers, 10)]);
    assert_eq!(err, Err(CommandError::CannotImprove("水利".into())));

    start(&mut state, &config, &ids, ids.shi_a, irrigate.clone(), &[(ids.farmers, 40)]).unwrap();
    let err = start(&mut state, &config, &ids, ids.shi_b, irrigate.clone(), &[(ids.farmers, 10)]);
    assert_eq!(err, Err(CommandError::AlreadyImproving("水利".into())));
    for _ in 0..40 {
        end_turn(&mut state, &config);
        if state.cities[&ids.city].assignment_of(ids.shi_a).is_none() {
            break;
        }
    }
    assert_eq!(state.cities[&ids.city].tiles[&tile].level("irrigation"), 1);

    state.cities.get_mut(&ids.city).unwrap().tiles.get_mut(&tile).unwrap().improvements.insert("irrigation".into(), 2);
    let err = start(&mut state, &config, &ids, ids.shi_a, irrigate, &[(ids.farmers, 10)]);
    assert_eq!(err, Err(CommandError::MaxImprovement("水利".into())));
}

#[test]
fn population_stops_growing_at_housing_limit() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let housing = state.cities[&ids.city].housing(&config);
    state.cities.get_mut(&ids.city).unwrap().groups.get_mut(&ids.farmers).unwrap().size = housing - 30;
    state.cities.get_mut(&ids.city).unwrap().treasury.add("grain", 1000.0);
    state.cities.get_mut(&ids.city).unwrap().treasury.add("salt", 1000.0);
    for _ in 0..40 {
        end_turn(&mut state, &config);
    }
    assert_eq!(state.cities[&ids.city].population(), housing);
}

#[test]
fn overcrowding_lowers_happiness() {
    let config = config();
    let happiness = |extra: u32| {
        let (mut state, ids) = setup(&config);
        let housing = state.cities[&ids.city].housing(&config);
        let city = state.cities.get_mut(&ids.city).unwrap();
        city.groups.get_mut(&ids.artisans).unwrap().size = housing - 110 + extra;
        city.treasury.add("grain", 1000.0);
        end_turn(&mut state, &config);
        state.cities[&ids.city].groups[&ids.farmers].happiness
    };
    assert!(happiness(50) < happiness(0));
}

fn learn(state: &mut GameState, config: &Config, ids: &Ids, group: Id, count: u32, skill: &str, drop: Option<&str>) -> Result<Vec<Id>, CommandError> {
    let command = Command::LearnSkill { city: ids.city, group, count, skill: skill.into(), drop_skill: drop.map(Into::into) };
    apply(state, config, ids.player, &command)
}

#[test]
fn learning_a_skill_takes_one_turn_without_a_shi_or_teacher() {
    let config = config();
    let (mut state, ids) = setup(&config);
    // 城中没人会制盐，也不用派士人：农人（农·五谷、纺织）中的 20 人学制盐
    learn(&mut state, &config, &ids, ids.farmers, 20, "salt_making", None).unwrap();
    // 学习期间照常干活；同一批人不能同时学两样
    assert_eq!(state.cities[&ids.city].unmanaged(ids.farmers), 100);
    let err = learn(&mut state, &config, &ids, ids.farmers, 81, "hunting", None);
    assert_eq!(err, Err(CommandError::NotEnoughNotLearning { group: ids.farmers, available: 80, requested: 81 }));
    end_turn(&mut state, &config);
    let skills = vec!["farming".to_string(), "weaving".to_string(), "salt_making".to_string()];
    let city = &state.cities[&ids.city];
    let learned = city.groups.values().find(|g| g.skills == skills).expect("学成的人分出去");
    assert_eq!(learned.size, 20);
    assert!(city.learning.is_empty());

    // 再学一批，并入已有的人群
    learn(&mut state, &config, &ids, ids.farmers, 10, "salt_making", None).unwrap();
    end_turn(&mut state, &config);
    let city = &state.cities[&ids.city];
    assert!(city.groups.values().find(|g| g.skills == skills).unwrap().size >= 30);
    assert_eq!(city.groups.values().filter(|g| g.skills == skills).count(), 1);
}

#[test]
fn learning_with_full_skill_slots_requires_dropping_one() {
    let config = config();
    let (mut state, ids) = setup(&config);
    // 工匠的 4 个技能槽已满
    let err = learn(&mut state, &config, &ids, ids.artisans, 5, "farming", None);
    assert_eq!(err, Err(CommandError::SkillSlotsFull));
    let err = learn(&mut state, &config, &ids, ids.artisans, 5, "armory", None);
    assert_eq!(err, Err(CommandError::AlreadyHasSkill("百工·军备".into())));
    learn(&mut state, &config, &ids, ids.artisans, 5, "farming", Some("hunting")).unwrap();
    end_turn(&mut state, &config);
    let skills: Vec<String> = ["smelting", "armory", "weaving", "farming"].map(String::from).to_vec();
    assert!(state.cities[&ids.city].groups.values().any(|g| g.skills == skills && g.size == 5));
}

#[test]
fn people_keep_working_while_learning_and_after() {
    let config = config();
    let (mut state, ids) = setup(&config);
    // 农人 100 人中 90 人在纺织，其中有人也去学制盐：先分出未被管理的 10 人，其余的人带着纺织工事分出去
    start(&mut state, &config, &ids, ids.shi_a, produce("cloth", Mode::Min), &[(ids.farmers, 45)]).unwrap();
    start(&mut state, &config, &ids, ids.shi_b, produce("cloth", Mode::Min), &[(ids.farmers, 45)]).unwrap();
    learn(&mut state, &config, &ids, ids.farmers, 30, "salt_making", None).unwrap();
    let before = state.cities[&ids.city].private_wealth.get("cloth");
    end_turn(&mut state, &config);
    let city = &state.cities[&ids.city];
    assert!(city.private_wealth.get("cloth") > before, "学技能的回合照常纺织");
    let learned = city.groups.values().find(|g| g.has_skill("salt_making")).unwrap().id;
    let workers = |shi: Id, group: Id| city.assignment_of(shi).unwrap().workers.get(&group).copied().unwrap_or(0);
    assert_eq!(workers(ids.shi_a, ids.farmers) + workers(ids.shi_b, ids.farmers), 70);
    assert_eq!(workers(ids.shi_a, learned) + workers(ids.shi_b, learned), 20);
    assert_eq!(city.labor_assigned(learned), 20);

    // 丢掉纺织的人做不了纺织，从工事中撤下来
    let command = Command::ArrangeSkills { city: ids.city, group: learned, count: 30, skills: vec!["salt_making".into()] };
    apply(&mut state, &config, ids.player, &command).unwrap();
    let city = &state.cities[&ids.city];
    assert_eq!(city.labor_assigned(learned), 0);
}

#[test]
fn assignments_left_without_people_end_and_free_the_shi() {
    let config = config();
    let (mut state, ids) = setup(&config);
    start(&mut state, &config, &ids, ids.shi_a, produce("cloth", Mode::Min), &[(ids.farmers, 40)]).unwrap();
    start(&mut state, &config, &ids, ids.shi_b, produce("weapon", Mode::Guan), &[(ids.artisans, 20)]).unwrap();
    // 农人整群丢掉纺织，纺织工事没人了，士甲空出来
    let command = Command::ArrangeSkills { city: ids.city, group: ids.farmers, count: 100, skills: vec!["farming".into()] };
    apply(&mut state, &config, ids.player, &command).unwrap();
    assert!(state.cities[&ids.city].assignment_of(ids.shi_a).is_none());
    // 工匠全部饿死或外流后，铸兵器的工事也结束
    state.cities.get_mut(&ids.city).unwrap().groups.get_mut(&ids.artisans).unwrap().size = 0;
    let report = end_turn(&mut state, &config);
    assert!(state.cities[&ids.city].assignment_of(ids.shi_b).is_none());
    assert!(report.events.iter().any(|e| e.contains("就此结束")), "{:?}", report.events);
}

#[test]
fn dropping_and_reordering_skills_takes_effect_at_once() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let arrange = |skills: &[&str]| Command::ArrangeSkills {
        city: ids.city,
        group: ids.farmers,
        count: 100,
        skills: skills.iter().map(|s| s.to_string()).collect(),
    };
    let err = apply(&mut state, &config, ids.player, &arrange(&["salt_making"]));
    assert!(matches!(err, Err(CommandError::InvalidSkills(_))), "新技能不能靠调整获得");
    // 整群人以纺织为先：直接改这群人的技能槽，当回合生效
    apply(&mut state, &config, ids.player, &arrange(&["weaving", "farming"])).unwrap();
    assert_eq!(state.cities[&ids.city].groups[&ids.farmers].skills, vec!["weaving".to_string(), "farming".to_string()]);
    let report = end_turn(&mut state, &config);
    assert!(report.cities[0].private_wealth.get("cloth") > 0.0, "未被管理时按第一项技能纺织");
}

#[test]
fn groups_without_skills_get_the_initial_skill() {
    let config = config();
    let scenario = r#"
[[cities]]
key = "city"
name = "新城"
totem = "玄鸟"
tiles = { farmland = 1 }

[[cities.groups]]
key = "people"
name = "庶民"
size = 10
happiness = 600
"#;
    let state = from_toml(scenario, &config).unwrap();
    let group = state.cities.values().next().unwrap().groups.values().next().unwrap();
    assert_eq!(group.skills, vec![config.population.initial_skill.clone()]);
}

#[test]
fn granting_moves_goods_from_treasury_to_people() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let goods = |n: f64| Inventory(BTreeMap::from([("grain".to_string(), n)]));
    let err = apply(&mut state, &config, ids.player, &Command::Grant { city: ids.city, goods: goods(101.0) });
    assert_eq!(err, Err(CommandError::NotEnoughInTreasury("grain".into())));
    apply(&mut state, &config, ids.player, &Command::Grant { city: ids.city, goods: goods(40.0) }).unwrap();
    let city = &state.cities[&ids.city];
    assert_eq!(city.treasury.get("grain"), 60.0);
    assert_eq!(city.private_wealth.get("grain"), 40.0);
}

#[test]
fn specialists_make_fine_goods_and_generalists_do_not() {
    let config = config();
    // 农人两项技能做布，部分是细布；工匠四项技能做兵器，没有精品
    let (mut state, ids) = setup(&config);
    start(&mut state, &config, &ids, ids.shi_a, produce("cloth", Mode::Guan), &[(ids.farmers, 30)]).unwrap();
    start(&mut state, &config, &ids, ids.shi_b, produce("weapon", Mode::Guan), &[(ids.artisans, 20)]).unwrap();
    end_turn(&mut state, &config);
    let city = &state.cities[&ids.city];
    let (cloth, fine) = (city.treasury.get("cloth"), city.treasury.get("cloth_fine"));
    assert!((fine / (cloth + fine) - config.fine_share(2)).abs() < 1e-9);
    assert!(city.treasury.get("weapon") > 0.0);
    assert_eq!(city.treasury.get("weapon_fine"), 0.0);
}

#[test]
fn high_tier_goods_need_enough_specialization() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let city = state.cities.get_mut(&ids.city).unwrap();
    city.groups.get_mut(&ids.artisans).unwrap().skills = vec!["ritual_craft".into(), "smelting".into(), "armory".into()];
    let err = start(&mut state, &config, &ids, ids.shi_a, produce("ding", Mode::Guan), &[(ids.artisans, 10)]);
    assert_eq!(err, Err(CommandError::SpecializationTooLow { group: ids.artisans, recipe: "铸鼎".into() }));
    // 簋没有专精度要求
    start(&mut state, &config, &ids, ids.shi_a, produce("gui", Mode::Guan), &[(ids.artisans, 10)]).unwrap();
}

#[test]
fn market_holds_more_merchants_and_earns_the_people_more() {
    let config = config();
    let coins = |markets: u32| {
        let (mut state, ids) = setup(&config);
        state.cities.get_mut(&ids.city).unwrap().groups.get_mut(&ids.artisans).unwrap().skills = vec!["commerce".into()];
        place(&mut state, &ids, "market", Mode::Min, markets);
        end_turn(&mut state, &config);
        state.cities[&ids.city].private_wealth.get("coin")
    };
    // 没有市只能容纳 10 名商贾，有民营的市后 20 名工匠都能经商
    let without = coins(0);
    let with = coins(1);
    assert!(without > 0.0);
    assert!((with / without - 2.0).abs() < 1e-9, "无市 {without}，有市 {with}");
}

#[test]
fn guan_market_only_holds_guan_merchants() {
    let config = config();
    let (mut state, ids) = setup(&config);
    state.cities.get_mut(&ids.city).unwrap().groups.get_mut(&ids.artisans).unwrap().skills = vec!["commerce".into()];
    place(&mut state, &ids, "market", Mode::Guan, 1);
    // 官营的市不容纳自主民营的商贾：20 人中只有 10 人有位置
    let report = end_turn(&mut state, &config);
    let trade = report.cities[0].jobs.iter().find(|j| j.pool == Pool::Recipe("trade".into())).unwrap();
    assert_eq!((trade.used, trade.capacity, trade.turned_away), (10, 50, 10));

    // 官营工事先占官营的市，再占街头的 10 个位置；民营工事只能用街头的位置
    let city = &state.cities[&ids.city];
    let pool = Pool::Recipe("trade".into());
    assert_eq!(city.free_jobs(&config, &pool, Mode::Guan), 50);
    assert_eq!(city.free_jobs(&config, &pool, Mode::Min), 10);
    start(&mut state, &config, &ids, ids.shi_a, produce("trade", Mode::Guan), &[(ids.artisans, 15)]).unwrap();
    let city = &state.cities[&ids.city];
    assert_eq!(city.free_jobs(&config, &pool, Mode::Min), 10);
    let report = end_turn(&mut state, &config);
    assert!(state.cities[&ids.city].treasury.get("coin") > 0.0, "官营经商的钱币进府库");
    let trade = report.cities[0].jobs.iter().find(|j| j.pool == pool).unwrap();
    assert_eq!((trade.used, trade.turned_away), (20, 0));
}

#[test]
fn guan_and_min_production_need_a_building_of_their_own_kind() {
    let config = config();
    let (mut state, ids) = setup(&config);
    // 测试城只有官营冶厂：官营铸兵器可以，民营不行
    let err = start(&mut state, &config, &ids, ids.shi_a, produce("weapon", Mode::Min), &[(ids.artisans, 10)]);
    assert_eq!(err, Err(CommandError::MissingFacility { facility: "冶厂".into(), mode: Mode::Min }));
    start(&mut state, &config, &ids, ids.shi_a, produce("weapon", Mode::Guan), &[(ids.artisans, 10)]).unwrap();

    // 营建时定下民营，建成后就能民营铸兵器，产出归民众
    let tile = forest_tile(&state, &ids);
    let city = state.cities.get_mut(&ids.city).unwrap();
    city.groups.get_mut(&ids.farmers).unwrap().skills = vec!["farming".into(), "building".into()];
    let foundry = Task::Construct { building: "foundry".into(), tile, mode: Mode::Min };
    start(&mut state, &config, &ids, ids.shi_b, foundry, &[(ids.farmers, 40)]).unwrap();
    for _ in 0..10 {
        end_turn(&mut state, &config);
    }
    let city = &state.cities[&ids.city];
    assert!(city.has_building("foundry", Mode::Min));
    let shi_b = ids.shi_b;
    let artisans = ids.artisans;
    start(&mut state, &config, &ids, shi_b, produce("weapon", Mode::Min), &[(artisans, 10)]).unwrap();
    state.cities.get_mut(&ids.city).unwrap().private_wealth.add("bronze", 10.0);
    let before = state.cities[&ids.city].private_wealth.get("weapon");
    end_turn(&mut state, &config);
    assert!(state.cities[&ids.city].private_wealth.get("weapon") > before);
}

#[test]
fn palace_raises_shi_capacity_and_wall_raises_happiness() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let before = state.cities[&ids.city].official_capacity(ids.shi_a, &config);
    place(&mut state, &ids, "palace", Mode::Guan, 1);
    assert!(state.cities[&ids.city].official_capacity(ids.shi_a, &config) > before);

    let happiness = |wall: bool| {
        let (mut state, ids) = setup(&config);
        if wall {
            place(&mut state, &ids, "city_wall", Mode::Guan, 1);
        }
        end_turn(&mut state, &config);
        state.cities[&ids.city].groups[&ids.farmers].happiness
    };
    assert!(happiness(true) > happiness(false));
}

#[test]
fn city_larger_than_rank_allows_costs_reputation() {
    let config = config();
    let (mut state, ids) = setup(&config);
    // 男爵每城上限 10 块地，测试城有 3 块
    let city = state.cities.get_mut(&ids.city).unwrap();
    for _ in 0..9 {
        let id = state.next_id;
        state.next_id += 1;
        city.tiles.insert(id, Tile::new(id, "forest"));
    }
    end_turn(&mut state, &config);
    end_turn(&mut state, &config);
    assert_eq!(state.characters[&ids.player].reputation, -2.0 * config.ritual.excess_tile_penalty, "只扣一次");
}

#[test]
fn fine_vessels_count_more_in_sacrifice() {
    let config = config();
    let culture = |vessel: &str| {
        let (mut state, ids) = setup(&config);
        state.cities.get_mut(&ids.city).unwrap().treasury.add(vessel, 1.0);
        start(&mut state, &config, &ids, ids.shi_a, Task::Sacrifice, &[]).unwrap();
        end_turn(&mut state, &config);
        state.cities[&ids.city].groups[&ids.farmers].culture
    };
    assert!(culture("gui_fine") > culture("gui"));
}

#[test]
fn reclaiming_needs_matching_skill_and_occupies_workers() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let reclaim = |terrain: &str| Task::Reclaim { terrain: terrain.into() };
    let err = start(&mut state, &config, &ids, ids.shi_a, reclaim("salt_pond"), &[(ids.farmers, 10)]);
    assert_eq!(err, Err(CommandError::MissingSkill { group: ids.farmers, skill: "salt_making".into() }));

    // 农人开垦耕地：40 人，春季人力倍率 1.0，开垦期间这 40 人一直被占用
    start(&mut state, &config, &ids, ids.shi_a, reclaim("farmland"), &[(ids.farmers, 40)]).unwrap();
    let farmland = |state: &GameState| state.cities[&ids.city].tiles.values().filter(|t| t.terrain == "farmland").count();
    let mut turns = 0;
    while state.cities[&ids.city].assignment_of(ids.shi_a).is_some() {
        assert_eq!(farmland(&state), 2);
        assert_eq!(state.cities[&ids.city].labor_assigned(ids.farmers), 40);
        end_turn(&mut state, &config);
        turns += 1;
        assert!(turns < 40);
    }
    // 人力满的当回合到手
    assert_eq!(farmland(&state), 3);
}

#[test]
fn each_tile_holds_a_limited_number_of_buildings() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let farmland = farmland_tile(&state, &ids);
    let forest = forest_tile(&state, &ids);
    state.cities.get_mut(&ids.city).unwrap().groups.get_mut(&ids.farmers).unwrap().skills = vec!["farming".into(), "building".into()];
    let construct = |building: &str, tile: Id, mode: Mode| Task::Construct { building: building.into(), tile, mode };

    // 每块地 3 座：开局的官营冶厂在耕地上，再放一座民营冶厂，官营民营可以叠在同一块地上
    assert_eq!(state.cities[&ids.city].tiles[&farmland].buildings.len(), 1);
    state.cities.get_mut(&ids.city).unwrap().tiles.get_mut(&farmland).unwrap().buildings.push(Building { kind: "foundry".into(), mode: Mode::Min });
    state.cities.get_mut(&ids.city).unwrap().treasury.add("bronze", 100.0);
    // 再开工 1 座就满了，正在营建的也占位置
    start(&mut state, &config, &ids, ids.shi_a, construct("market", farmland, Mode::Min), &[(ids.farmers, 10)]).unwrap();
    let err = start(&mut state, &config, &ids, ids.shi_b, construct("foundry", farmland, Mode::Guan), &[(ids.farmers, 10)]);
    assert_eq!(err, Err(CommandError::NoBuildingSlot));
    // 建筑不影响这块地的劳作
    assert_eq!(state.cities[&ids.city].job_capacity(&config, &Pool::Terrain("farmland".into())), 120);
    // 地块没有主次：市、宫殿、城墙建在哪块地上都可以
    start(&mut state, &config, &ids, ids.shi_b, construct("palace", forest, Mode::Guan), &[(ids.farmers, 10)]).unwrap();
}

/// 加一名人物「家宰」，返回他的 id。
fn add_steward(state: &mut GameState) -> Id {
    let steward = state.alloc_id();
    state.characters.insert(steward, Character::new(steward, "家宰", "none", 0.0));
    steward
}

/// 所有者任命家宰，家宰接受。
fn hand_over(state: &mut GameState, config: &Config, ids: &Ids) -> Id {
    let steward = add_steward(state);
    apply(state, config, ids.player, &Command::OfferManagement { city: ids.city, to: steward }).unwrap();
    apply(state, config, steward, &Command::AcceptManagement { city: ids.city }).unwrap();
    steward
}

#[test]
fn management_is_offered_accepted_and_revoked() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let steward = add_steward(&mut state);
    let outsider = add_steward(&mut state);
    let offer = Command::OfferManagement { city: ids.city, to: steward };
    let accept = Command::AcceptManagement { city: ids.city };
    assert_eq!(apply(&mut state, &config, steward, &offer), Err(CommandError::NotOwner(ids.city)));
    // 对方没答复之前不生效，别人也不能接
    apply(&mut state, &config, ids.player, &offer).unwrap();
    assert_eq!(state.cities[&ids.city].manager, Some(ids.player));
    assert_eq!(apply(&mut state, &config, outsider, &accept), Err(CommandError::NotOffered(ids.city)));
    // 拒绝之后任命作废
    apply(&mut state, &config, steward, &Command::DeclineManagement { city: ids.city }).unwrap();
    assert_eq!(apply(&mut state, &config, steward, &accept), Err(CommandError::NotOffered(ids.city)));

    apply(&mut state, &config, ids.player, &offer).unwrap();
    apply(&mut state, &config, steward, &accept).unwrap();
    let city = &state.cities[&ids.city];
    assert_eq!(city.manager, Some(steward));
    // 换了管理者，民众对新管理者的忠诚度归零重来
    assert!(city.groups.values().all(|g| g.loyalty == config.loyalty.reset));
    // 所有者不能直接管城内事务，但可以随时收回管理权，不用管理者同意
    let tax = Command::SetTaxRate { city: ids.city, rate: 0.3 };
    assert_eq!(apply(&mut state, &config, ids.player, &tax), Err(CommandError::NotManager(ids.city)));
    apply(&mut state, &config, ids.player, &Command::RevokeManagement { city: ids.city }).unwrap();
    assert_eq!(state.cities[&ids.city].manager, Some(ids.player));
    apply(&mut state, &config, ids.player, &tax).unwrap();
}

#[test]
fn one_character_manages_only_one_city() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let steward = add_steward(&mut state);
    // 家宰已经在管另一座城
    let mut other = state.cities[&ids.city].clone();
    other.id = state.alloc_id();
    other.manager = Some(steward);
    state.cities.insert(other.id, other);
    apply(&mut state, &config, ids.player, &Command::OfferManagement { city: ids.city, to: steward }).unwrap();
    let err = apply(&mut state, &config, steward, &Command::AcceptManagement { city: ids.city });
    assert_eq!(err, Err(CommandError::ManagesAnotherCity(steward)));
}

#[test]
fn delivered_goods_go_to_the_owners_private_store_and_count_toward_the_quota() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let quota = |goods: &[(&str, f64)]| Command::SetQuota {
        city: ids.city,
        goods: Inventory(goods.iter().map(|(g, n)| (g.to_string(), *n)).collect()),
    };
    // 所有者自己管理时不需要规定任务
    assert_eq!(apply(&mut state, &config, ids.player, &quota(&[("grain", 40.0)])), Err(CommandError::OwnerIsManager));
    let steward = hand_over(&mut state, &config, &ids);
    assert_eq!(apply(&mut state, &config, steward, &quota(&[("grain", 40.0)])), Err(CommandError::NotOwner(ids.city)));
    apply(&mut state, &config, ids.player, &quota(&[("grain", 40.0)])).unwrap();

    let deliver = |n: f64| Command::Deliver { city: ids.city, goods: Inventory([("grain".to_string(), n)].into()) };
    assert_eq!(apply(&mut state, &config, steward, &deliver(1000.0)), Err(CommandError::NotEnoughInTreasury("grain".into())));
    apply(&mut state, &config, steward, &deliver(30.0)).unwrap();
    assert_eq!(state.characters[&ids.player].private_store.get("grain"), 30.0);
    assert_eq!(state.cities[&ids.city].quota.as_ref().unwrap().delivered.get("grain"), 30.0);
    assert_eq!(state.cities[&ids.city].treasury.get("grain"), 70.0);

    // 所有者收回管理权、自己管理后，规定任务就没有了
    apply(&mut state, &config, ids.player, &Command::RevokeManagement { city: ids.city }).unwrap();
    assert!(state.cities[&ids.city].quota.is_none());
}

/// 家宰管城、规定每年上缴 40 粮，今年交了 `delivered` 粮，返回年底所有者对家宰的信任。
fn trust_after_year(delivered: f64) -> f64 {
    let config = config();
    let (mut state, ids) = setup(&config);
    let steward = hand_over(&mut state, &config, &ids);
    let goods = Inventory([("grain".to_string(), 40.0)].into());
    apply(&mut state, &config, ids.player, &Command::SetQuota { city: ids.city, goods }).unwrap();
    if delivered > 0.0 {
        let goods = Inventory([("grain".to_string(), delivered)].into());
        apply(&mut state, &config, steward, &Command::Deliver { city: ids.city, goods }).unwrap();
    }
    for _ in 0..4 {
        end_turn(&mut state, &config);
    }
    // 结算后今年的上缴清零
    assert_eq!(state.cities[&ids.city].quota.as_ref().unwrap().delivered, Inventory::default());
    state.characters[&ids.player].trust_in(steward, &config)
}

#[test]
fn quota_is_settled_in_winter_and_changes_trust() {
    let config = config();
    let q = &config.quota;
    let initial = q.initial_trust;
    assert_eq!(trust_after_year(40.0), initial + q.complete_trust);
    assert_eq!(trust_after_year(50.0), initial + q.exceed_trust);
    assert_eq!(trust_after_year(20.0), initial - q.fail_trust_penalty * 0.5);
    assert_eq!(trust_after_year(0.0), initial - q.fail_trust_penalty);
}

#[test]
fn low_taxes_and_grants_win_loyalty() {
    let config = config();
    let loyalty = |rate: f64| {
        let (mut state, ids) = setup(&config);
        state.cities.get_mut(&ids.city).unwrap().tax_rate = rate;
        start(&mut state, &config, &ids, ids.shi_a, Task::CollectTax, &[(ids.farmers, 45)]).unwrap();
        end_turn(&mut state, &config);
        state.cities[&ids.city].groups[&ids.farmers].loyalty
    };
    let initial = config.loyalty.initial;
    assert!(loyalty(0.0) > initial);
    assert!(loyalty(0.5) < initial, "重税让人离心");

    let (mut state, ids) = setup(&config);
    let goods = Inventory([("grain".to_string(), 50.0)].into());
    apply(&mut state, &config, ids.player, &Command::Grant { city: ids.city, goods }).unwrap();
    assert!(state.cities[&ids.city].groups[&ids.farmers].loyalty > initial);
}

#[test]
fn disloyal_guan_workers_strike_and_work_for_themselves() {
    let config = config();
    // 工匠改为（狩猎、军备），官营铸兵器；罢工的人按第一项技能自主狩猎。肉会被当口粮吃掉，看皮革
    let run = |loyalty: f64| {
        let (mut state, ids) = setup(&config);
        let city = state.cities.get_mut(&ids.city).unwrap();
        let artisans = city.groups.get_mut(&ids.artisans).unwrap();
        artisans.loyalty = loyalty;
        artisans.skills = vec!["hunting".into(), "armory".into()];
        start(&mut state, &config, &ids, ids.shi_a, produce("weapon", Mode::Guan), &[(ids.artisans, 20)]).unwrap();
        let report = end_turn(&mut state, &config);
        let city = &state.cities[&ids.city];
        (city.treasury.get("weapon"), city.private_wealth.get("hide"), report.events.iter().any(|e| e.contains("罢工")))
    };
    let (loyal_weapons, loyal_hide, loyal_strike) = run(1000.0);
    let (weapons, hide, strike) = run(0.0);
    assert!(!loyal_strike && strike);
    assert_eq!(loyal_hide, 0.0);
    // 忠诚度为 0 时一半人罢工，回去自主民营狩猎
    assert!((weapons / loyal_weapons - config.loyalty.guan_turnout).abs() < 0.05, "忠诚 {loyal_weapons}，不忠 {weapons}");
    assert!(hide > 0.0);
}

#[test]
fn disloyal_min_workers_do_not_strike_but_pay_less_tax_and_make_less() {
    let config = config();
    let run = |loyalty: f64| {
        let (mut state, ids) = setup(&config);
        state.turn = 2; // 秋收
        state.cities.get_mut(&ids.city).unwrap().groups.get_mut(&ids.farmers).unwrap().loyalty = loyalty;
        start(&mut state, &config, &ids, ids.shi_a, Task::CollectTax, &[(ids.farmers, 45)]).unwrap();
        let before = state.cities[&ids.city].treasury.get("grain");
        let report = end_turn(&mut state, &config);
        let city = &state.cities[&ids.city];
        let farm = report.cities[0].jobs.iter().find(|j| j.pool == Pool::Terrain("farmland".into())).unwrap().used;
        let grain = city.private_wealth.get("grain") + city.treasury.get("grain") - before;
        (city.treasury.get("grain") - before, grain, farm)
    };
    let (loyal_tax, loyal_grain, loyal_farm) = run(1000.0);
    let (tax, grain, farm) = run(0.0);
    assert_eq!(loyal_farm, farm, "民营不罢工");
    assert!(grain < loyal_grain, "产量下降");
    assert!(tax < loyal_tax * config.loyalty.tax_share + 1e-6, "税收不上来：忠诚 {loyal_tax}，不忠 {tax}");
}
