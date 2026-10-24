//! 城市管理规则的测试，使用真实的 config/balance.toml。

use std::collections::BTreeMap;

use zhou_core::scenario::from_toml;
use zhou_core::{Character, Command, CommandError, Config, GameState, Id, Mode, Task, apply, end_turn};

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
resources = ["farmland"]
buildings = { foundry = 1 }
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
    let veteran = city.officials[&ids.shi_a].capacity(&config);
    let fresh = city.officials[&created[0]].capacity(&config);
    assert!(fresh < veteran, "新士人 {fresh} 应少于老士人 {veteran}");
}

#[test]
fn assignment_rejects_over_capacity_busy_shi_and_missing_skill() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let capacity = state.cities[&ids.city].officials[&ids.shi_a].capacity(&config);
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
resources = ["farmland", "salt_pond"]

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
    start(&mut state, &config, &ids, ids.shi_a, Task::Construct { building: "city_wall".into() }, &[(ids.farmers, 40)]).unwrap();
    for _ in 0..40 {
        end_turn(&mut state, &config);
        if state.cities[&ids.city].buildings.contains_key("city_wall") {
            break;
        }
    }
    let city = &state.cities[&ids.city];
    assert_eq!(city.buildings.get("city_wall"), Some(&1));
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
    assert_eq!(state.characters[&ids.player].reputation, -2.0 * config.ritual.excess_reputation_penalty);
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
    state.characters.insert(steward, Character { id: steward, name: "家宰".into(), rank: "none".into(), reputation: 0.0 });
    state.cities.get_mut(&ids.city).unwrap().manager = Some(steward);

    let command = Command::SetTaxRate { city: ids.city, rate: 0.3 };
    // 所有者不兼任管理者时，不能直接改税率
    assert_eq!(apply(&mut state, &config, ids.player, &command), Err(CommandError::NotManager(ids.city)));
    assert_eq!(state.cities[&ids.city].tax_rate, 0.2);
    apply(&mut state, &config, steward, &command).unwrap();
    assert_eq!(state.cities[&ids.city].tax_rate, 0.3);
}
