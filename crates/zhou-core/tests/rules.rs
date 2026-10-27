//! 城市管理规则的测试，使用真实的 config/balance.toml。

use std::collections::BTreeMap;

use zhou_core::scenario::from_toml;
use zhou_core::{
    Building, Character, Command, CommandError, Config, GameState, Hex, Id, Inventory, Mode, Pool,
    Task, Tile, apply, end_turn,
};

fn config() -> Config {
    Config::from_toml(include_str!("../../../config/balance.toml")).expect("balance.toml is valid")
}

/// 秋季第一个月的回合数
fn autumn(config: &Config) -> u32 {
    2 * config.months_per_season
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

fn start(
    state: &mut GameState,
    config: &Config,
    ids: &Ids,
    shi: Id,
    task: Task,
    w: &[(Id, u32)],
) -> Result<Vec<Id>, CommandError> {
    apply(
        state,
        config,
        ids.player,
        &Command::StartAssignment {
            city: ids.city,
            shi,
            task,
            workers: workers(w),
        },
    )
}

fn forest_tile(state: &GameState, ids: &Ids) -> Id {
    *state.cities[&ids.city]
        .tiles
        .iter()
        .find(|(_, t)| t.terrain == "forest")
        .unwrap()
        .0
}

/// 直接在山林上放建筑，跳过营建。
fn place(state: &mut GameState, ids: &Ids, building: &str, mode: Mode, count: u32) {
    let tile = forest_tile(state, ids);
    let tile = state
        .cities
        .get_mut(&ids.city)
        .unwrap()
        .tiles
        .get_mut(&tile)
        .unwrap();
    for _ in 0..count {
        tile.buildings.push(Building {
            kind: building.into(),
            mode,
        });
    }
}

fn produce(recipe: &str, mode: Mode) -> Task {
    Task::Produce {
        recipe: recipe.into(),
        mode,
    }
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
    let created = apply(
        &mut state,
        &config,
        ids.player,
        &Command::AppointShi {
            city: ids.city,
            group: ids.gentry,
            count: 2,
            drop_skill: None,
        },
    )
    .unwrap();
    let city = &state.cities[&ids.city];
    assert_eq!(created.len(), 2);
    assert_eq!(city.groups[&ids.gentry].size, 8);
    assert_eq!(city.officials[&created[0]].skills, vec!["shi".to_string()]);
}

#[test]
fn appoint_without_shi_skill_learns_it_immediately() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let created = apply(
        &mut state,
        &config,
        ids.player,
        &Command::AppointShi {
            city: ids.city,
            group: ids.farmers,
            count: 1,
            drop_skill: None,
        },
    )
    .unwrap();
    let city = &state.cities[&ids.city];
    assert_eq!(
        city.officials[&created[0]].skills,
        vec!["farming", "weaving", "shi"]
    );
    // 人群里的其他人不受影响
    assert_eq!(city.groups[&ids.farmers].skills, vec!["farming", "weaving"]);
    // 新士人当回合就能主事
    start(
        &mut state,
        &config,
        &ids,
        created[0],
        produce("grain", Mode::Min),
        &[(ids.farmers, 10)],
    )
    .unwrap();
}

#[test]
fn appoint_with_full_skill_slots_requires_dropping_one() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let appoint = |drop: Option<&str>| Command::AppointShi {
        city: ids.city,
        group: ids.artisans,
        count: 1,
        drop_skill: drop.map(Into::into),
    };
    assert_eq!(
        apply(&mut state, &config, ids.player, &appoint(None)),
        Err(CommandError::SkillSlotsFull)
    );
    assert_eq!(
        apply(&mut state, &config, ids.player, &appoint(Some("farming"))),
        Err(CommandError::NoSuchSkillToDrop("farming".into()))
    );
    let created = apply(&mut state, &config, ids.player, &appoint(Some("hunting"))).unwrap();
    assert_eq!(
        state.cities[&ids.city].officials[&created[0]].skills,
        vec!["smelting", "armory", "weaving", "shi"]
    );
}

#[test]
fn appoint_only_from_unmanaged_people() {
    let config = config();
    let (mut state, ids) = setup(&config);
    start(
        &mut state,
        &config,
        &ids,
        ids.shi_a,
        produce("grain", Mode::Min),
        &[(ids.farmers, 40)],
    )
    .unwrap();
    start(
        &mut state,
        &config,
        &ids,
        ids.shi_b,
        produce("cloth", Mode::Min),
        &[(ids.farmers, 40)],
    )
    .unwrap();
    let err = apply(
        &mut state,
        &config,
        ids.player,
        &Command::AppointShi {
            city: ids.city,
            group: ids.farmers,
            count: 21,
            drop_skill: None,
        },
    );
    assert_eq!(
        err,
        Err(CommandError::NotEnoughUnmanaged {
            group: ids.farmers,
            available: 20,
            requested: 21
        })
    );
}

#[test]
fn new_shi_manages_fewer_people_than_veterans() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let created = apply(
        &mut state,
        &config,
        ids.player,
        &Command::AppointShi {
            city: ids.city,
            group: ids.gentry,
            count: 1,
            drop_skill: None,
        },
    )
    .unwrap();
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
    let err = start(
        &mut state,
        &config,
        &ids,
        ids.shi_a,
        produce("grain", Mode::Min),
        &[(ids.farmers, capacity + 1)],
    );
    assert_eq!(
        err,
        Err(CommandError::OverCapacity {
            capacity,
            requested: capacity + 1
        })
    );

    let err = start(
        &mut state,
        &config,
        &ids,
        ids.shi_a,
        produce("grain", Mode::Min),
        &[(ids.artisans, 5)],
    );
    assert_eq!(
        err,
        Err(CommandError::MissingSkill {
            group: ids.artisans,
            skill: "farming".into()
        })
    );

    start(
        &mut state,
        &config,
        &ids,
        ids.shi_a,
        produce("grain", Mode::Min),
        &[(ids.farmers, 10)],
    )
    .unwrap();
    let err = start(
        &mut state,
        &config,
        &ids,
        ids.shi_a,
        produce("cloth", Mode::Min),
        &[(ids.farmers, 10)],
    );
    assert_eq!(err, Err(CommandError::OfficialBusy(ids.shi_a)));
}

#[test]
fn guan_output_goes_to_treasury_min_output_to_people() {
    let config = config();
    let (mut state, ids) = setup(&config);
    start(
        &mut state,
        &config,
        &ids,
        ids.shi_a,
        produce("weapon", Mode::Guan),
        &[(ids.artisans, 20)],
    )
    .unwrap();
    end_turn(&mut state, &config);
    let city = &state.cities[&ids.city];
    assert!(city.treasury.get("weapon") > 0.0);
    assert_eq!(city.private_wealth.get("weapon"), 0.0);

    let (mut state, ids) = setup(&config);
    start(
        &mut state,
        &config,
        &ids,
        ids.shi_a,
        produce("cloth", Mode::Min),
        &[(ids.farmers, 30)],
    )
    .unwrap();
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
        start(
            &mut state,
            &config,
            &ids,
            ids.shi_a,
            produce("cloth", mode),
            &[(ids.farmers, 30)],
        )
        .unwrap();
        // 其余农人按第一项技能种地，不产布，所以布全部来自这 30 人
        end_turn(&mut state, &config);
        let city = &state.cities[&ids.city];
        city.treasury.get("cloth") + city.private_wealth.get("cloth")
    };
    let guan = output(Mode::Guan);
    let min = output(Mode::Min);
    assert!(
        (guan / min - config.economy.guan_efficiency).abs() < 1e-9,
        "官营 {guan}，民营 {min}"
    );
}

#[test]
fn guan_lowers_happiness_more_than_min() {
    let config = config();
    let happiness = |mode: Mode| {
        let (mut state, ids) = setup(&config);
        start(
            &mut state,
            &config,
            &ids,
            ids.shi_a,
            produce("cloth", mode),
            &[(ids.farmers, 30)],
        )
        .unwrap();
        end_turn(&mut state, &config);
        state.cities[&ids.city].groups[&ids.farmers].happiness
    };
    assert!(happiness(Mode::Guan) < happiness(Mode::Min));
}

#[test]
fn unmanaged_people_work_their_first_skill_for_themselves() {
    let config = config();
    let (mut state, ids) = setup(&config);
    // 秋季收获，跳到秋季第一个月
    state.turn = autumn(&config);
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
        state.turn = autumn(&config);
        if covered > 0 {
            start(
                &mut state,
                &config,
                &ids,
                ids.shi_a,
                Task::CollectTax,
                &[(ids.farmers, covered)],
            )
            .unwrap();
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
    assert!(
        (tax_20 - (wealth_0 - wealth_20)).abs() < 1e-9,
        "税从民间财富转入府库"
    );
    assert!(
        (treasury_40 - treasury_0 - 2.0 * tax_20).abs() < 1e-9,
        "覆盖人数加倍，税收加倍"
    );
}

#[test]
fn consumption_uses_private_wealth_before_treasury() {
    let config = config();
    let (mut state, ids) = setup(&config);
    // 全城 130 人每月需盐 need：民间只有 0.2，先用完，剩下的由府库补足
    let need = 130.0 / config.population.per_people * config.population.consumption[1].amount;
    state
        .cities
        .get_mut(&ids.city)
        .unwrap()
        .private_wealth
        .add("salt", 0.2);
    end_turn(&mut state, &config);
    let city = &state.cities[&ids.city];
    assert_eq!(city.private_wealth.get("salt"), 0.0);
    assert!((city.treasury.get("salt") - (100.0 - (need - 0.2))).abs() < 1e-9);
}

#[test]
fn richer_people_are_happier() {
    let config = config();
    let happiness = |private_grain: f64| {
        let (mut state, ids) = setup(&config);
        state
            .cities
            .get_mut(&ids.city)
            .unwrap()
            .private_wealth
            .add("grain", private_grain);
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
    state.turn = autumn(&config);
    let report = end_turn(&mut state, &config);
    assert!(
        report.events.iter().all(|e| !e.text.contains("缺")),
        "{:?}",
        report.events
    );
}

#[test]
fn shi_upkeep_comes_from_treasury_and_unpaid_shi_lose_loyalty() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let loyalty = state.cities[&ids.city].officials[&ids.shi_a].loyalty;
    state
        .cities
        .get_mut(&ids.city)
        .unwrap()
        .treasury
        .take("grain", 1000.0);
    end_turn(&mut state, &config);
    let city = &state.cities[&ids.city];
    assert!(city.officials[&ids.shi_a].loyalty < loyalty);
}

#[test]
fn construction_finishes_and_adds_building() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let tile = forest_tile(&state, &ids);
    let wall = Task::Construct {
        building: "city_wall".into(),
        tile,
        mode: Mode::Guan,
    };
    start(
        &mut state,
        &config,
        &ids,
        ids.shi_a,
        wall,
        &[(ids.farmers, 40)],
    )
    .unwrap();
    for _ in 0..120 {
        end_turn(&mut state, &config);
        if state.cities[&ids.city].has_building("city_wall", Mode::Guan) {
            break;
        }
    }
    let city = &state.cities[&ids.city];
    assert_eq!(
        city.tiles[&tile].buildings,
        vec![Building {
            kind: "city_wall".into(),
            mode: Mode::Guan
        }]
    );
    assert!(city.assignment_of(ids.shi_a).is_none(), "竣工后士人空闲");
}

#[test]
fn sacrifice_needs_vessels_and_raises_culture() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let err = start(&mut state, &config, &ids, ids.shi_a, Task::Sacrifice, &[]);
    assert_eq!(err, Err(CommandError::NoVessel));

    state
        .cities
        .get_mut(&ids.city)
        .unwrap()
        .treasury
        .add("gui", 2.0);
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
    state
        .cities
        .get_mut(&ids.city)
        .unwrap()
        .treasury
        .add("ding", 3.0);
    end_turn(&mut state, &config);
    assert_eq!(state.characters[&ids.player].reputation, 0.0);

    state
        .cities
        .get_mut(&ids.city)
        .unwrap()
        .treasury
        .add("ding", 2.0);
    end_turn(&mut state, &config);
    let penalty = config.ritual.excess_reputation_penalty;
    assert_eq!(state.characters[&ids.player].reputation, -2.0 * penalty);

    // 一直保持超出不再扣
    end_turn(&mut state, &config);
    assert_eq!(state.characters[&ids.player].reputation, -2.0 * penalty);
    // 超出得更多，只扣多出的部分
    state
        .cities
        .get_mut(&ids.city)
        .unwrap()
        .treasury
        .add("ding_fine", 1.0);
    end_turn(&mut state, &config);
    assert_eq!(state.characters[&ids.player].reputation, -3.0 * penalty);
    // 减到上限以内后又超出，再扣一次
    state
        .cities
        .get_mut(&ids.city)
        .unwrap()
        .treasury
        .take("ding", 3.0);
    end_turn(&mut state, &config);
    state
        .cities
        .get_mut(&ids.city)
        .unwrap()
        .treasury
        .add("ding", 1.0);
    end_turn(&mut state, &config);
    assert_eq!(state.characters[&ids.player].reputation, -4.0 * penalty);
}

#[test]
fn shrinking_groups_trim_assignments() {
    let config = config();
    let (mut state, ids) = setup(&config);
    start(
        &mut state,
        &config,
        &ids,
        ids.shi_a,
        produce("grain", Mode::Min),
        &[(ids.farmers, 30)],
    )
    .unwrap();
    state
        .cities
        .get_mut(&ids.city)
        .unwrap()
        .groups
        .get_mut(&ids.farmers)
        .unwrap()
        .size = 10;
    end_turn(&mut state, &config);
    let city = &state.cities[&ids.city];
    assert!(city.labor_assigned(ids.farmers) <= city.groups[&ids.farmers].size);
}

#[test]
fn only_the_manager_can_command_the_city() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let steward = state.alloc_id();
    state
        .characters
        .insert(steward, Character::new(steward, "家宰", "none", 0.0));
    state.cities.get_mut(&ids.city).unwrap().manager = Some(steward);

    let command = Command::SetTaxRate {
        city: ids.city,
        rate: 0.3,
    };
    // 所有者不兼任管理者时，不能直接改税率
    assert_eq!(
        apply(&mut state, &config, ids.player, &command),
        Err(CommandError::NotManager(ids.city))
    );
    assert_eq!(state.cities[&ids.city].tax_rate, 0.2);
    apply(&mut state, &config, steward, &command).unwrap();
    assert_eq!(state.cities[&ids.city].tax_rate, 0.3);
}

fn farmland_tile(state: &GameState, ids: &Ids) -> Id {
    *state.cities[&ids.city]
        .tiles
        .iter()
        .find(|(_, t)| t.terrain == "farmland")
        .unwrap()
        .0
}

#[test]
fn farmland_limits_how_many_people_can_farm() {
    let config = config();
    let (mut state, ids) = setup(&config);
    // 两块耕地共 120 个位置，农人 100 人
    state
        .cities
        .get_mut(&ids.city)
        .unwrap()
        .groups
        .get_mut(&ids.farmers)
        .unwrap()
        .size = 150;
    state.turn = autumn(&config);
    let report = end_turn(&mut state, &config);
    let farmland = report.cities[0]
        .jobs
        .iter()
        .find(|j| j.pool == Pool::Terrain("farmland".into()))
        .unwrap();
    assert_eq!(
        (farmland.used, farmland.capacity, farmland.turned_away),
        (120, 120, 30)
    );
}

#[test]
fn assignments_cannot_exceed_job_capacity() {
    let config = config();
    let (mut state, ids) = setup(&config);
    // 只留一块耕地，共 60 个位置
    let tile = farmland_tile(&state, &ids);
    state.cities.get_mut(&ids.city).unwrap().tiles.remove(&tile);
    start(
        &mut state,
        &config,
        &ids,
        ids.shi_a,
        produce("grain", Mode::Min),
        &[(ids.farmers, 45)],
    )
    .unwrap();
    // 工事已占 45 个位置，只剩 15 个；未被管理的农人不占工事的名额
    let err = start(
        &mut state,
        &config,
        &ids,
        ids.shi_b,
        produce("grain", Mode::Min),
        &[(ids.farmers, 16)],
    );
    assert_eq!(
        err,
        Err(CommandError::NoJobs {
            pool: "耕地".into(),
            available: 15,
            requested: 16
        })
    );
    start(
        &mut state,
        &config,
        &ids,
        ids.shi_b,
        produce("grain", Mode::Min),
        &[(ids.farmers, 15)],
    )
    .unwrap();
}

#[test]
fn irrigation_raises_farmland_yield_up_to_max_level() {
    let config = config();
    let grain_after_harvest = |irrigated: u32| {
        let (mut state, ids) = setup(&config);
        let tile = farmland_tile(&state, &ids);
        if irrigated > 0 {
            state
                .cities
                .get_mut(&ids.city)
                .unwrap()
                .tiles
                .get_mut(&tile)
                .unwrap()
                .improvements
                .insert("irrigation".into(), irrigated);
        }
        state.turn = autumn(&config);
        end_turn(&mut state, &config);
        state.cities[&ids.city].private_wealth.get("grain")
    };
    assert!(grain_after_harvest(1) > grain_after_harvest(0));
    assert!(grain_after_harvest(2) > grain_after_harvest(1));

    let (mut state, ids) = setup(&config);
    // 测试城没有会水利的人，先让农人学
    let tile = farmland_tile(&state, &ids);
    let irrigate = Task::Improve {
        tile,
        improvement: "irrigation".into(),
    };
    let err = start(
        &mut state,
        &config,
        &ids,
        ids.shi_a,
        irrigate.clone(),
        &[(ids.farmers, 10)],
    );
    assert_eq!(
        err,
        Err(CommandError::MissingSkill {
            group: ids.farmers,
            skill: "water".into()
        })
    );

    let forest = forest_tile(&state, &ids);
    let city = state.cities.get_mut(&ids.city).unwrap();
    city.groups.get_mut(&ids.farmers).unwrap().skills = vec!["farming".into(), "water".into()];
    let err = start(
        &mut state,
        &config,
        &ids,
        ids.shi_a,
        Task::Improve {
            tile: forest,
            improvement: "irrigation".into(),
        },
        &[(ids.farmers, 10)],
    );
    assert_eq!(err, Err(CommandError::CannotImprove("水利".into())));

    start(
        &mut state,
        &config,
        &ids,
        ids.shi_a,
        irrigate.clone(),
        &[(ids.farmers, 40)],
    )
    .unwrap();
    let err = start(
        &mut state,
        &config,
        &ids,
        ids.shi_b,
        irrigate.clone(),
        &[(ids.farmers, 10)],
    );
    assert_eq!(err, Err(CommandError::AlreadyImproving("水利".into())));
    for _ in 0..40 {
        end_turn(&mut state, &config);
        if state.cities[&ids.city].assignment_of(ids.shi_a).is_none() {
            break;
        }
    }
    assert_eq!(state.cities[&ids.city].tiles[&tile].level("irrigation"), 1);

    state
        .cities
        .get_mut(&ids.city)
        .unwrap()
        .tiles
        .get_mut(&tile)
        .unwrap()
        .improvements
        .insert("irrigation".into(), 2);
    let err = start(
        &mut state,
        &config,
        &ids,
        ids.shi_a,
        irrigate,
        &[(ids.farmers, 10)],
    );
    assert_eq!(err, Err(CommandError::MaxImprovement("水利".into())));
}

#[test]
fn population_stops_growing_at_housing_limit() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let housing = state.cities[&ids.city].housing(&config);
    state
        .cities
        .get_mut(&ids.city)
        .unwrap()
        .groups
        .get_mut(&ids.farmers)
        .unwrap()
        .size = housing - 30;
    state
        .cities
        .get_mut(&ids.city)
        .unwrap()
        .treasury
        .add("grain", 1000.0);
    state
        .cities
        .get_mut(&ids.city)
        .unwrap()
        .treasury
        .add("salt", 1000.0);
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

fn learn(
    state: &mut GameState,
    config: &Config,
    ids: &Ids,
    group: Id,
    count: u32,
    skill: &str,
    drop: Option<&str>,
) -> Result<Vec<Id>, CommandError> {
    let command = Command::LearnSkill {
        city: ids.city,
        group,
        count,
        skill: skill.into(),
        drop_skill: drop.map(Into::into),
    };
    apply(state, config, ids.player, &command)
}

#[test]
fn learning_a_skill_takes_one_turn_without_a_shi_or_teacher() {
    let config = config();
    let (mut state, ids) = setup(&config);
    // 城中没人会制盐，也不用派士人：农人（农·五谷、纺织）中的 20 人学制盐
    learn(
        &mut state,
        &config,
        &ids,
        ids.farmers,
        20,
        "salt_making",
        None,
    )
    .unwrap();
    // 学习期间照常干活；同一批人不能同时学两样
    assert_eq!(state.cities[&ids.city].unmanaged(ids.farmers), 100);
    let err = learn(&mut state, &config, &ids, ids.farmers, 81, "hunting", None);
    assert_eq!(
        err,
        Err(CommandError::NotEnoughNotLearning {
            group: ids.farmers,
            available: 80,
            requested: 81
        })
    );
    end_turn(&mut state, &config);
    let skills = vec![
        "farming".to_string(),
        "weaving".to_string(),
        "salt_making".to_string(),
    ];
    let city = &state.cities[&ids.city];
    let learned = city
        .groups
        .values()
        .find(|g| g.skills == skills)
        .expect("学成的人分出去");
    assert_eq!(learned.size, 20);
    assert!(city.learning.is_empty());

    // 再学一批，并入已有的人群
    learn(
        &mut state,
        &config,
        &ids,
        ids.farmers,
        10,
        "salt_making",
        None,
    )
    .unwrap();
    end_turn(&mut state, &config);
    let city = &state.cities[&ids.city];
    assert!(
        city.groups
            .values()
            .find(|g| g.skills == skills)
            .unwrap()
            .size
            >= 30
    );
    assert_eq!(
        city.groups.values().filter(|g| g.skills == skills).count(),
        1
    );
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
    learn(
        &mut state,
        &config,
        &ids,
        ids.artisans,
        5,
        "farming",
        Some("hunting"),
    )
    .unwrap();
    end_turn(&mut state, &config);
    let skills: Vec<String> = ["smelting", "armory", "weaving", "farming"]
        .map(String::from)
        .to_vec();
    assert!(
        state.cities[&ids.city]
            .groups
            .values()
            .any(|g| g.skills == skills && g.size == 5)
    );
}

#[test]
fn people_keep_working_while_learning_and_after() {
    let config = config();
    let (mut state, ids) = setup(&config);
    // 农人 100 人中 90 人在纺织，其中有人也去学制盐：先分出未被管理的 10 人，其余的人带着纺织工事分出去
    start(
        &mut state,
        &config,
        &ids,
        ids.shi_a,
        produce("cloth", Mode::Min),
        &[(ids.farmers, 45)],
    )
    .unwrap();
    start(
        &mut state,
        &config,
        &ids,
        ids.shi_b,
        produce("cloth", Mode::Min),
        &[(ids.farmers, 45)],
    )
    .unwrap();
    learn(
        &mut state,
        &config,
        &ids,
        ids.farmers,
        30,
        "salt_making",
        None,
    )
    .unwrap();
    let before = state.cities[&ids.city].private_wealth.get("cloth");
    end_turn(&mut state, &config);
    let city = &state.cities[&ids.city];
    assert!(
        city.private_wealth.get("cloth") > before,
        "学技能的回合照常纺织"
    );
    let learned = city
        .groups
        .values()
        .find(|g| g.has_skill("salt_making"))
        .unwrap()
        .id;
    let workers = |shi: Id, group: Id| {
        city.assignment_of(shi)
            .unwrap()
            .workers
            .get(&group)
            .copied()
            .unwrap_or(0)
    };
    assert_eq!(
        workers(ids.shi_a, ids.farmers) + workers(ids.shi_b, ids.farmers),
        70
    );
    assert_eq!(
        workers(ids.shi_a, learned) + workers(ids.shi_b, learned),
        20
    );
    assert_eq!(city.labor_assigned(learned), 20);

    // 丢掉纺织的人做不了纺织，从工事中撤下来
    let command = Command::ArrangeSkills {
        city: ids.city,
        group: learned,
        count: 30,
        skills: vec!["salt_making".into()],
    };
    apply(&mut state, &config, ids.player, &command).unwrap();
    let city = &state.cities[&ids.city];
    assert_eq!(city.labor_assigned(learned), 0);
}

#[test]
fn assignments_left_without_people_end_and_free_the_shi() {
    let config = config();
    let (mut state, ids) = setup(&config);
    start(
        &mut state,
        &config,
        &ids,
        ids.shi_a,
        produce("cloth", Mode::Min),
        &[(ids.farmers, 40)],
    )
    .unwrap();
    start(
        &mut state,
        &config,
        &ids,
        ids.shi_b,
        produce("weapon", Mode::Guan),
        &[(ids.artisans, 20)],
    )
    .unwrap();
    // 农人整群丢掉纺织，纺织工事没人了，士甲空出来
    let command = Command::ArrangeSkills {
        city: ids.city,
        group: ids.farmers,
        count: 100,
        skills: vec!["farming".into()],
    };
    apply(&mut state, &config, ids.player, &command).unwrap();
    assert!(state.cities[&ids.city].assignment_of(ids.shi_a).is_none());
    // 工匠全部饿死或外流后，铸兵器的工事也结束
    state
        .cities
        .get_mut(&ids.city)
        .unwrap()
        .groups
        .get_mut(&ids.artisans)
        .unwrap()
        .size = 0;
    let report = end_turn(&mut state, &config);
    assert!(state.cities[&ids.city].assignment_of(ids.shi_b).is_none());
    assert!(
        report.events.iter().any(|e| e.text.contains("就此结束")),
        "{:?}",
        report.events
    );
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
    assert!(
        matches!(err, Err(CommandError::InvalidSkills(_))),
        "新技能不能靠调整获得"
    );
    // 整群人以纺织为先：直接改这群人的技能槽，当回合生效
    apply(
        &mut state,
        &config,
        ids.player,
        &arrange(&["weaving", "farming"]),
    )
    .unwrap();
    assert_eq!(
        state.cities[&ids.city].groups[&ids.farmers].skills,
        vec!["weaving".to_string(), "farming".to_string()]
    );
    let report = end_turn(&mut state, &config);
    assert!(
        report.cities[0].private_wealth.get("cloth") > 0.0,
        "未被管理时按第一项技能纺织"
    );
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
    let group = state
        .cities
        .values()
        .next()
        .unwrap()
        .groups
        .values()
        .next()
        .unwrap();
    assert_eq!(group.skills, vec![config.population.initial_skill.clone()]);
}

#[test]
fn granting_moves_goods_from_treasury_to_people() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let goods = |n: f64| Inventory(BTreeMap::from([("grain".to_string(), n)]));
    let err = apply(
        &mut state,
        &config,
        ids.player,
        &Command::Grant {
            city: ids.city,
            goods: goods(101.0),
        },
    );
    assert_eq!(err, Err(CommandError::NotEnoughInTreasury("grain".into())));
    apply(
        &mut state,
        &config,
        ids.player,
        &Command::Grant {
            city: ids.city,
            goods: goods(40.0),
        },
    )
    .unwrap();
    let city = &state.cities[&ids.city];
    assert_eq!(city.treasury.get("grain"), 60.0);
    assert_eq!(city.private_wealth.get("grain"), 40.0);
}

#[test]
fn specialists_make_fine_goods_and_generalists_do_not() {
    let config = config();
    // 农人两项技能做布，部分是细布；工匠四项技能做兵器，没有精品
    let (mut state, ids) = setup(&config);
    start(
        &mut state,
        &config,
        &ids,
        ids.shi_a,
        produce("cloth", Mode::Guan),
        &[(ids.farmers, 30)],
    )
    .unwrap();
    start(
        &mut state,
        &config,
        &ids,
        ids.shi_b,
        produce("weapon", Mode::Guan),
        &[(ids.artisans, 20)],
    )
    .unwrap();
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
    city.groups.get_mut(&ids.artisans).unwrap().skills =
        vec!["ritual_craft".into(), "smelting".into(), "armory".into()];
    let err = start(
        &mut state,
        &config,
        &ids,
        ids.shi_a,
        produce("ding", Mode::Guan),
        &[(ids.artisans, 10)],
    );
    assert_eq!(
        err,
        Err(CommandError::SpecializationTooLow {
            group: ids.artisans,
            recipe: "铸鼎".into()
        })
    );
    // 簋没有专精度要求
    start(
        &mut state,
        &config,
        &ids,
        ids.shi_a,
        produce("gui", Mode::Guan),
        &[(ids.artisans, 10)],
    )
    .unwrap();
}

#[test]
fn market_holds_more_merchants_and_earns_the_people_more() {
    let config = config();
    let coins = |markets: u32| {
        let (mut state, ids) = setup(&config);
        state
            .cities
            .get_mut(&ids.city)
            .unwrap()
            .groups
            .get_mut(&ids.artisans)
            .unwrap()
            .skills = vec!["commerce".into()];
        place(&mut state, &ids, "market", Mode::Min, markets);
        end_turn(&mut state, &config);
        state.cities[&ids.city].private_wealth.get("coin")
    };
    // 没有市只能容纳 10 名商贾，有民营的市后 20 名工匠都能经商
    let without = coins(0);
    let with = coins(1);
    assert!(without > 0.0);
    assert!(
        (with / without - 2.0).abs() < 1e-9,
        "无市 {without}，有市 {with}"
    );
}

#[test]
fn guan_market_only_holds_guan_merchants() {
    let config = config();
    let (mut state, ids) = setup(&config);
    state
        .cities
        .get_mut(&ids.city)
        .unwrap()
        .groups
        .get_mut(&ids.artisans)
        .unwrap()
        .skills = vec!["commerce".into()];
    place(&mut state, &ids, "market", Mode::Guan, 1);
    // 官营的市不容纳自主民营的商贾：20 人中只有 10 人有位置
    let report = end_turn(&mut state, &config);
    let trade = report.cities[0]
        .jobs
        .iter()
        .find(|j| j.pool == Pool::Recipe("trade".into()))
        .unwrap();
    assert_eq!(
        (trade.used, trade.capacity, trade.turned_away),
        (10, 50, 10)
    );

    // 官营工事先占官营的市，再占街头的 10 个位置；民营工事只能用街头的位置
    let city = &state.cities[&ids.city];
    let pool = Pool::Recipe("trade".into());
    assert_eq!(city.free_jobs(&config, &pool, Mode::Guan), 50);
    assert_eq!(city.free_jobs(&config, &pool, Mode::Min), 10);
    start(
        &mut state,
        &config,
        &ids,
        ids.shi_a,
        produce("trade", Mode::Guan),
        &[(ids.artisans, 15)],
    )
    .unwrap();
    let city = &state.cities[&ids.city];
    assert_eq!(city.free_jobs(&config, &pool, Mode::Min), 10);
    let report = end_turn(&mut state, &config);
    assert!(
        state.cities[&ids.city].treasury.get("coin") > 0.0,
        "官营经商的钱币进府库"
    );
    let trade = report.cities[0]
        .jobs
        .iter()
        .find(|j| j.pool == pool)
        .unwrap();
    assert_eq!((trade.used, trade.turned_away), (20, 0));
}

#[test]
fn guan_and_min_production_need_a_building_of_their_own_kind() {
    let config = config();
    let (mut state, ids) = setup(&config);
    // 测试城只有官营冶厂：官营铸兵器可以，民营不行
    let err = start(
        &mut state,
        &config,
        &ids,
        ids.shi_a,
        produce("weapon", Mode::Min),
        &[(ids.artisans, 10)],
    );
    assert_eq!(
        err,
        Err(CommandError::MissingFacility {
            facility: "冶厂".into(),
            mode: Mode::Min
        })
    );
    start(
        &mut state,
        &config,
        &ids,
        ids.shi_a,
        produce("weapon", Mode::Guan),
        &[(ids.artisans, 10)],
    )
    .unwrap();

    // 营建时定下民营，建成后就能民营铸兵器，产出归民众
    let tile = forest_tile(&state, &ids);
    let city = state.cities.get_mut(&ids.city).unwrap();
    city.groups.get_mut(&ids.farmers).unwrap().skills = vec!["farming".into(), "building".into()];
    let foundry = Task::Construct {
        building: "foundry".into(),
        tile,
        mode: Mode::Min,
    };
    start(
        &mut state,
        &config,
        &ids,
        ids.shi_b,
        foundry,
        &[(ids.farmers, 40)],
    )
    .unwrap();
    for _ in 0..30 {
        end_turn(&mut state, &config);
    }
    let city = &state.cities[&ids.city];
    assert!(city.has_building("foundry", Mode::Min));
    let shi_b = ids.shi_b;
    let artisans = ids.artisans;
    start(
        &mut state,
        &config,
        &ids,
        shi_b,
        produce("weapon", Mode::Min),
        &[(artisans, 10)],
    )
    .unwrap();
    state
        .cities
        .get_mut(&ids.city)
        .unwrap()
        .private_wealth
        .add("bronze", 10.0);
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
    for pos in Hex::new(0, 0).spiral().skip(3).take(9) {
        let id = state.next_id;
        state.next_id += 1;
        city.tiles.insert(id, Tile::new(id, pos, "forest"));
    }
    end_turn(&mut state, &config);
    end_turn(&mut state, &config);
    assert_eq!(
        state.characters[&ids.player].reputation,
        -2.0 * config.ritual.excess_tile_penalty,
        "只扣一次"
    );
}

#[test]
fn fine_vessels_count_more_in_sacrifice() {
    let config = config();
    let culture = |vessel: &str| {
        let (mut state, ids) = setup(&config);
        state
            .cities
            .get_mut(&ids.city)
            .unwrap()
            .treasury
            .add(vessel, 1.0);
        start(&mut state, &config, &ids, ids.shi_a, Task::Sacrifice, &[]).unwrap();
        end_turn(&mut state, &config);
        state.cities[&ids.city].groups[&ids.farmers].culture
    };
    assert!(culture("gui_fine") > culture("gui"));
}

#[test]
fn reclaiming_takes_a_bordering_wild_cell_and_occupies_workers() {
    let config = config();
    let (mut state, ids) = setup(&config);
    // 测试城没画地图：治所 (0,0) 是耕地，旁边一圈从 (-1,1) 起依次是耕地、山林
    let seat = state.cities[&ids.city].seat;
    let (salt, wild, far) = (Hex::new(1, -1), Hex::new(1, 0), Hex::new(5, 0));
    state.map.insert(salt, "salt_pond");
    state.map.insert(wild, "farmland");
    state.map.insert(far, "farmland");
    let reclaim = |at: Hex, terrain: &str| Task::Reclaim {
        at,
        terrain: terrain.into(),
    };
    let mut attempt = |task: Task| {
        start(
            &mut state,
            &config,
            &ids,
            ids.shi_a,
            task,
            &[(ids.farmers, 40)],
        )
    };
    assert_eq!(
        attempt(reclaim(Hex::new(0, -1), "farmland")),
        Err(CommandError::NoSuchCell(Hex::new(0, -1)))
    );
    assert_eq!(
        attempt(reclaim(far, "farmland")),
        Err(CommandError::NotBordering(far))
    );
    assert_eq!(
        attempt(reclaim(seat, "farmland")),
        Err(CommandError::CellTaken {
            at: seat,
            city: "测试城".into()
        })
    );
    assert_eq!(
        attempt(reclaim(wild, "forest")),
        Err(CommandError::TerrainMismatch {
            at: wild,
            actual: "耕地".into()
        })
    );
    assert_eq!(
        attempt(reclaim(salt, "salt_pond")),
        Err(CommandError::MissingSkill {
            group: ids.farmers,
            skill: "salt_making".into()
        })
    );

    // 农人开垦耕地：40 人，开垦期间这 40 人一直被占用
    attempt(reclaim(wild, "farmland")).unwrap();
    let mut turns = 0;
    while state.cities[&ids.city].assignment_of(ids.shi_a).is_some() {
        assert_eq!(state.city_at(wild), None);
        assert_eq!(state.cities[&ids.city].labor_assigned(ids.farmers), 40);
        end_turn(&mut state, &config);
        turns += 1;
        assert!(turns < 60);
    }
    // 人力满的当回合到手，地图上这一格归了本城
    assert_eq!(state.city_at(wild), Some(ids.city));
    assert_eq!(
        state.cities[&ids.city].tile_at(wild).unwrap().terrain,
        "farmland"
    );
    // 新地挨着的野地也能接着开垦
    assert!(state.cities[&ids.city].borders(Hex::new(2, 0)));
}

/// 两座城夹着一格野地，手画的地图。
const TWO_CITIES: &str = r#"
[map]
rows = [
  "耕 耕 耕 耕",
  " 林 耕 耕 林",
]
owners = [
  "甲 甲 · 乙",
  " 甲 · 乙 乙",
]

[[characters]]
key = "player"
name = "玩家"
rank = "nan"

[[cities]]
key = "a"
name = "甲城"
owner = "player"
manager = "player"
totem = "玄鸟"
treasury = { grain = 300, salt = 100 }
seat = [0, 0]

[[cities.groups]]
key = "a_farmers"
name = "甲城农人"
size = 100
happiness = 600

[[cities.officials]]
key = "a_shi"
name = "甲城士"
from_group = "a_farmers"

[[cities.assignments]]
key = "a_reclaim"
shi = "a_shi"
task = { kind = "reclaim", at = [2, 0] }
workers = { a_farmers = 30 }

[[cities]]
key = "b"
name = "乙城"
owner = "player"
manager = "player"
totem = "玄鸟"
treasury = { grain = 300, salt = 100 }

[[cities.groups]]
key = "b_farmers"
name = "乙城农人"
size = 100
happiness = 600

[[cities.officials]]
key = "b_shi"
name = "乙城士"
from_group = "b_farmers"

[[cities.assignments]]
key = "b_reclaim"
shi = "b_shi"
task = { kind = "reclaim", at = [2, 0] }
workers = { b_farmers = 30 }
"#;

#[test]
fn hand_drawn_map_gives_each_city_its_cells() {
    let config = config();
    let state = from_toml(TWO_CITIES, &config).unwrap();
    let city = |name: &str| state.cities.values().find(|c| c.name == name).unwrap();
    let (a, b) = (city("甲城"), city("乙城"));
    assert_eq!(state.map.cells.len(), 8);
    assert_eq!(a.tiles.len(), 3);
    assert_eq!(b.tiles.len(), 3);
    assert_eq!(a.seat, Hex::from_offset(0, 0));
    // 乙城没写治所，取它的第一块地
    assert_eq!(b.seat, Hex::from_offset(3, 0));
    assert_eq!(a.tile_at(Hex::from_offset(0, 1)).unwrap().terrain, "forest");
    assert_eq!(state.city_at(Hex::from_offset(1, 1)), None);
    assert_eq!(state.city_at(Hex::from_offset(2, 1)), Some(b.id));
}

#[test]
fn mountains_and_rivers_cannot_be_crossed_settled_or_reclaimed() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let (plain, wood, peak, river) = (
        Hex::new(1, 0),
        Hex::new(2, 0),
        Hex::new(1, -1),
        Hex::new(3, 0),
    );
    state.map.insert(plain, "farmland");
    state.map.insert(wood, "forest");
    state.map.insert(peak, "mountain");
    state.map.insert(river, "river");
    assert_eq!(state.map.move_cost(plain, &config), Some(1));
    assert_eq!(state.map.move_cost(wood, &config), Some(2));
    assert_eq!(state.map.move_cost(peak, &config), None);
    assert_eq!(state.map.move_cost(river, &config), None);
    assert_eq!(state.map.move_cost(Hex::new(9, 9), &config), None);

    // 高山不能开垦进城
    let task = Task::Reclaim {
        at: peak,
        terrain: "mountain".into(),
    };
    assert_eq!(
        start(
            &mut state,
            &config,
            &ids,
            ids.shi_a,
            task,
            &[(ids.farmers, 40)]
        ),
        Err(CommandError::CannotReclaim("高山".into()))
    );

    // 剧本里过不去的地形不能画进城里
    let text = TWO_CITIES.replacen("\"耕 耕 耕 耕\"", "\"耕 山 耕 耕\"", 1);
    let err = from_toml(&text, &config).unwrap_err().to_string();
    assert!(err.contains("高山"), "{err}");

    // 示例剧本的野地上画了高山和大河
    let demo = from_toml(include_str!("../../../scenarios/demo.toml"), &config).unwrap();
    assert!(demo.map.cells.values().any(|c| c.terrain == "mountain"));
    assert!(demo.map.cells.values().any(|c| c.terrain == "river"));
}

#[test]
fn when_two_cities_reclaim_the_same_cell_the_first_to_finish_gets_it() {
    let config = config();
    let mut state = from_toml(TWO_CITIES, &config).unwrap();
    let target = Hex::from_offset(2, 0);
    let a = state.cities.values().find(|c| c.name == "甲城").unwrap().id;
    let mut turns = 0;
    let lost = loop {
        let report = end_turn(&mut state, &config);
        if let Some(e) = report.events.iter().find(|e| e.text.contains("抢先")) {
            break e.text.clone();
        }
        turns += 1;
        assert!(turns < 120);
    };
    // 两城同时完工，先结算的甲城得到这一格，乙城白费人力
    assert_eq!(state.city_at(target), Some(a));
    assert!(lost.starts_with("乙城"), "{lost}");
    assert!(state.cities.values().all(|c| c.assignments.is_empty()));
}

#[test]
fn each_tile_holds_a_limited_number_of_buildings() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let farmland = farmland_tile(&state, &ids);
    let forest = forest_tile(&state, &ids);
    state
        .cities
        .get_mut(&ids.city)
        .unwrap()
        .groups
        .get_mut(&ids.farmers)
        .unwrap()
        .skills = vec!["farming".into(), "building".into()];
    let construct = |building: &str, tile: Id, mode: Mode| Task::Construct {
        building: building.into(),
        tile,
        mode,
    };

    // 每块地 3 座：开局的官营冶厂在耕地上，再放一座民营冶厂，官营民营可以叠在同一块地上
    assert_eq!(state.cities[&ids.city].tiles[&farmland].buildings.len(), 1);
    state
        .cities
        .get_mut(&ids.city)
        .unwrap()
        .tiles
        .get_mut(&farmland)
        .unwrap()
        .buildings
        .push(Building {
            kind: "foundry".into(),
            mode: Mode::Min,
        });
    state
        .cities
        .get_mut(&ids.city)
        .unwrap()
        .treasury
        .add("bronze", 100.0);
    // 再开工 1 座就满了，正在营建的也占位置
    start(
        &mut state,
        &config,
        &ids,
        ids.shi_a,
        construct("market", farmland, Mode::Min),
        &[(ids.farmers, 10)],
    )
    .unwrap();
    let err = start(
        &mut state,
        &config,
        &ids,
        ids.shi_b,
        construct("foundry", farmland, Mode::Guan),
        &[(ids.farmers, 10)],
    );
    assert_eq!(err, Err(CommandError::NoBuildingSlot));
    // 建筑不影响这块地的劳作
    assert_eq!(
        state.cities[&ids.city].job_capacity(&config, &Pool::Terrain("farmland".into())),
        120
    );
    // 地块没有主次：市、宫殿、城墙建在哪块地上都可以
    start(
        &mut state,
        &config,
        &ids,
        ids.shi_b,
        construct("palace", forest, Mode::Guan),
        &[(ids.farmers, 10)],
    )
    .unwrap();
}

/// 加一名人物「家宰」，返回他的 id。
fn add_steward(state: &mut GameState) -> Id {
    let steward = state.alloc_id();
    state
        .characters
        .insert(steward, Character::new(steward, "家宰", "none", 0.0));
    steward
}

/// 所有者任命家宰，家宰接受。
fn hand_over(state: &mut GameState, config: &Config, ids: &Ids) -> Id {
    let steward = add_steward(state);
    apply(
        state,
        config,
        ids.player,
        &Command::OfferManagement {
            city: ids.city,
            to: steward,
        },
    )
    .unwrap();
    apply(
        state,
        config,
        steward,
        &Command::AcceptManagement { city: ids.city },
    )
    .unwrap();
    steward
}

#[test]
fn management_is_offered_accepted_and_revoked() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let steward = add_steward(&mut state);
    let outsider = add_steward(&mut state);
    let offer = Command::OfferManagement {
        city: ids.city,
        to: steward,
    };
    let accept = Command::AcceptManagement { city: ids.city };
    assert_eq!(
        apply(&mut state, &config, steward, &offer),
        Err(CommandError::NotOwner(ids.city))
    );
    // 对方没答复之前不生效，别人也不能接
    apply(&mut state, &config, ids.player, &offer).unwrap();
    assert_eq!(state.cities[&ids.city].manager, Some(ids.player));
    assert_eq!(
        apply(&mut state, &config, outsider, &accept),
        Err(CommandError::NotOffered(ids.city))
    );
    // 拒绝之后任命作废
    apply(
        &mut state,
        &config,
        steward,
        &Command::DeclineManagement { city: ids.city },
    )
    .unwrap();
    assert_eq!(
        apply(&mut state, &config, steward, &accept),
        Err(CommandError::NotOffered(ids.city))
    );

    apply(&mut state, &config, ids.player, &offer).unwrap();
    apply(&mut state, &config, steward, &accept).unwrap();
    let city = &state.cities[&ids.city];
    assert_eq!(city.manager, Some(steward));
    // 换了管理者，民众对新管理者的忠诚度归零重来
    assert!(
        city.groups
            .values()
            .all(|g| g.loyalty == config.loyalty.reset)
    );
    // 所有者不能直接管城内事务，但可以随时收回管理权，不用管理者同意
    let tax = Command::SetTaxRate {
        city: ids.city,
        rate: 0.3,
    };
    assert_eq!(
        apply(&mut state, &config, ids.player, &tax),
        Err(CommandError::NotManager(ids.city))
    );
    apply(
        &mut state,
        &config,
        ids.player,
        &Command::RevokeManagement { city: ids.city },
    )
    .unwrap();
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
    apply(
        &mut state,
        &config,
        ids.player,
        &Command::OfferManagement {
            city: ids.city,
            to: steward,
        },
    )
    .unwrap();
    let err = apply(
        &mut state,
        &config,
        steward,
        &Command::AcceptManagement { city: ids.city },
    );
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
    assert_eq!(
        apply(&mut state, &config, ids.player, &quota(&[("grain", 40.0)])),
        Err(CommandError::OwnerIsManager)
    );
    let steward = hand_over(&mut state, &config, &ids);
    assert_eq!(
        apply(&mut state, &config, steward, &quota(&[("grain", 40.0)])),
        Err(CommandError::NotOwner(ids.city))
    );
    apply(&mut state, &config, ids.player, &quota(&[("grain", 40.0)])).unwrap();

    let deliver = |n: f64| Command::Deliver {
        city: ids.city,
        goods: Inventory([("grain".to_string(), n)].into()),
    };
    assert_eq!(
        apply(&mut state, &config, steward, &deliver(1000.0)),
        Err(CommandError::NotEnoughInTreasury("grain".into()))
    );
    apply(&mut state, &config, steward, &deliver(30.0)).unwrap();
    assert_eq!(
        state.characters[&ids.player].private_store.get("grain"),
        30.0
    );
    assert_eq!(
        state.cities[&ids.city]
            .quota
            .as_ref()
            .unwrap()
            .delivered
            .get("grain"),
        30.0
    );
    assert_eq!(state.cities[&ids.city].treasury.get("grain"), 70.0);

    // 所有者收回管理权、自己管理后，规定任务就没有了
    apply(
        &mut state,
        &config,
        ids.player,
        &Command::RevokeManagement { city: ids.city },
    )
    .unwrap();
    assert!(state.cities[&ids.city].quota.is_none());
}

/// 家宰管城、规定每年上缴 40 粮，今年交了 `delivered` 粮，返回年底所有者对家宰的信任。
fn trust_after_year(delivered: f64) -> f64 {
    let config = config();
    let (mut state, ids) = setup(&config);
    let steward = hand_over(&mut state, &config, &ids);
    let goods = Inventory([("grain".to_string(), 40.0)].into());
    apply(
        &mut state,
        &config,
        ids.player,
        &Command::SetQuota {
            city: ids.city,
            goods,
        },
    )
    .unwrap();
    if delivered > 0.0 {
        let goods = Inventory([("grain".to_string(), delivered)].into());
        apply(
            &mut state,
            &config,
            steward,
            &Command::Deliver {
                city: ids.city,
                goods,
            },
        )
        .unwrap();
    }
    for _ in 0..config.turns_per_year() {
        end_turn(&mut state, &config);
    }
    // 结算后今年的上缴清零
    assert_eq!(
        state.cities[&ids.city].quota.as_ref().unwrap().delivered,
        Inventory::default()
    );
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
        start(
            &mut state,
            &config,
            &ids,
            ids.shi_a,
            Task::CollectTax,
            &[(ids.farmers, 45)],
        )
        .unwrap();
        end_turn(&mut state, &config);
        state.cities[&ids.city].groups[&ids.farmers].loyalty
    };
    let initial = config.loyalty.initial;
    assert!(loyalty(0.0) > initial);
    assert!(loyalty(0.5) < initial, "重税让人离心");

    let (mut state, ids) = setup(&config);
    let goods = Inventory([("grain".to_string(), 50.0)].into());
    apply(
        &mut state,
        &config,
        ids.player,
        &Command::Grant {
            city: ids.city,
            goods,
        },
    )
    .unwrap();
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
        start(
            &mut state,
            &config,
            &ids,
            ids.shi_a,
            produce("weapon", Mode::Guan),
            &[(ids.artisans, 20)],
        )
        .unwrap();
        let report = end_turn(&mut state, &config);
        let city = &state.cities[&ids.city];
        (
            city.treasury.get("weapon"),
            city.private_wealth.get("hide"),
            report.events.iter().any(|e| e.text.contains("罢工")),
        )
    };
    let (loyal_weapons, loyal_hide, loyal_strike) = run(1000.0);
    let (weapons, hide, strike) = run(0.0);
    assert!(!loyal_strike && strike);
    assert_eq!(loyal_hide, 0.0);
    // 忠诚度为 0 时一半人罢工，回去自主民营狩猎
    assert!(
        (weapons / loyal_weapons - config.loyalty.guan_turnout).abs() < 0.05,
        "忠诚 {loyal_weapons}，不忠 {weapons}"
    );
    assert!(hide > 0.0);
}

#[test]
fn disloyal_min_workers_do_not_strike_but_pay_less_tax_and_make_less() {
    let config = config();
    let run = |loyalty: f64| {
        let (mut state, ids) = setup(&config);
        state.turn = autumn(&config); // 秋收
        state
            .cities
            .get_mut(&ids.city)
            .unwrap()
            .groups
            .get_mut(&ids.farmers)
            .unwrap()
            .loyalty = loyalty;
        start(
            &mut state,
            &config,
            &ids,
            ids.shi_a,
            Task::CollectTax,
            &[(ids.farmers, 45)],
        )
        .unwrap();
        let before = state.cities[&ids.city].treasury.get("grain");
        let report = end_turn(&mut state, &config);
        let city = &state.cities[&ids.city];
        let farm = report.cities[0]
            .jobs
            .iter()
            .find(|j| j.pool == Pool::Terrain("farmland".into()))
            .unwrap()
            .used;
        let grain = city.private_wealth.get("grain") + city.treasury.get("grain") - before;
        (city.treasury.get("grain") - before, grain, farm)
    };
    let (loyal_tax, loyal_grain, loyal_farm) = run(1000.0);
    let (tax, grain, farm) = run(0.0);
    assert_eq!(loyal_farm, farm, "民营不罢工");
    assert!(grain < loyal_grain, "产量下降");
    assert!(
        tax < loyal_tax * config.loyalty.tax_share + 1e-6,
        "税收不上来：忠诚 {loyal_tax}，不忠 {tax}"
    );
}

#[test]
fn spring_sowing_limits_the_autumn_harvest() {
    let config = config();
    // 春天：农人播种不产粮，记下平均播种的有效人数
    let (mut state, ids) = setup(&config);
    let before = state.cities[&ids.city].private_wealth.get("grain");
    for _ in 0..config.months_per_season {
        end_turn(&mut state, &config);
    }
    let city = &state.cities[&ids.city];
    assert!(city.private_wealth.get("grain") <= before, "春天不产粮");
    let sown = city.sown["grain"];
    assert!(sown > 0.0);

    // 秋天：播种得少，收成按比例打折
    let harvest = |sown: Option<f64>| {
        let (mut state, ids) = setup(&config);
        state.turn = autumn(&config);
        if let Some(sown) = sown {
            state
                .cities
                .get_mut(&ids.city)
                .unwrap()
                .sown
                .insert("grain".into(), sown);
        }
        let before = state.cities[&ids.city].private_wealth.get("grain");
        end_turn(&mut state, &config);
        state.cities[&ids.city].private_wealth.get("grain") - before
    };
    let full = harvest(None);
    assert_eq!(harvest(Some(1000.0)), full, "种得够，收成不受限");
    let half = harvest(Some(sown / 2.0));
    assert!(
        half < full * 0.75,
        "春天只种了一半，秋天收不满：{half} / {full}"
    );
}

#[test]
fn taking_farmers_away_in_spring_or_autumn_costs_more_happiness() {
    let config = config();
    let happiness_after = |turn: u32| {
        let (mut state, ids) = setup(&config);
        state.turn = turn;
        let tile = forest_tile(&state, &ids);
        let wall = Task::Construct {
            building: "city_wall".into(),
            tile,
            mode: Mode::Guan,
        };
        start(
            &mut state,
            &config,
            &ids,
            ids.shi_a,
            wall,
            &[(ids.farmers, 40)],
        )
        .unwrap();
        end_turn(&mut state, &config);
        state.cities[&ids.city].groups[&ids.farmers].happiness
    };
    let m = config.months_per_season;
    let (spring, summer, autumn_, winter) = (
        happiness_after(0),
        happiness_after(m),
        happiness_after(2 * m),
        happiness_after(3 * m),
    );
    assert!(spring < summer, "春播被调走更不满");
    assert!(autumn_ < winter, "秋收被调走更不满");
}

/// 加一个国家，国君是 `ruler`。
fn add_country(state: &mut GameState, name: &str, ruler: Id) -> Id {
    let id = state.alloc_id();
    state.countries.insert(
        id,
        zhou_core::Country {
            id,
            name: name.into(),
            ruler,
            capital: None,
            national_war: false,
        },
    );
    state.characters.get_mut(&ruler).unwrap().nationality = Some(id);
    id
}

/// 玩家把测试城封给一个新人物，对方接受。返回封臣。
fn enfeoff(state: &mut GameState, config: &Config, ids: &Ids) -> Id {
    let vassal = add_steward(state);
    apply(
        state,
        config,
        ids.player,
        &Command::OfferFief {
            city: ids.city,
            to: vassal,
        },
    )
    .unwrap();
    apply(
        state,
        config,
        vassal,
        &Command::AcceptFief { city: ids.city },
    )
    .unwrap();
    vassal
}

#[test]
fn enfeoffment_needs_acceptance_and_cannot_be_taken_back() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let country = add_country(&mut state, "卫", ids.player);
    // 国君至少要留一座城在自己手里
    let seat = state.cities[&ids.city].seat;
    copy_city(&mut state, &ids, ids.player, seat);
    let steward = hand_over(&mut state, &config, &ids);
    let vassal = add_steward(&mut state);
    let offer = Command::OfferFief {
        city: ids.city,
        to: vassal,
    };
    let accept = Command::AcceptFief { city: ids.city };
    assert_eq!(
        apply(
            &mut state,
            &config,
            ids.player,
            &Command::OfferFief {
                city: ids.city,
                to: ids.player
            }
        ),
        Err(CommandError::CannotEnfeoffSelf)
    );
    apply(&mut state, &config, ids.player, &offer).unwrap();
    assert_eq!(
        apply(&mut state, &config, steward, &accept),
        Err(CommandError::NotOfferedFief)
    );
    assert_eq!(
        state.cities[&ids.city].owner,
        Some(ids.player),
        "接受之前不生效"
    );

    apply(&mut state, &config, vassal, &accept).unwrap();
    let city = &state.cities[&ids.city];
    assert_eq!(
        (city.owner, city.liege, city.manager),
        (Some(vassal), Some(ids.player), Some(vassal)),
        "管理权归受封者，原管理者卸任"
    );
    assert!(city.quota.is_none());
    assert_eq!(
        state.characters[&vassal].nationality,
        Some(country),
        "无国籍的人受封后加入封君的国家"
    );
    assert_eq!(state.country_of(city), Some(country));

    // 封地不能收回，封君也不能再管城里的事；封地也不能再转封
    assert_eq!(
        apply(
            &mut state,
            &config,
            ids.player,
            &Command::RevokeManagement { city: ids.city }
        ),
        Err(CommandError::NotOwner(ids.city))
    );
    assert_eq!(
        apply(
            &mut state,
            &config,
            ids.player,
            &Command::SetTaxRate {
                city: ids.city,
                rate: 0.3
            }
        ),
        Err(CommandError::NotManager(ids.city))
    );
    let other = add_steward(&mut state);
    assert_eq!(
        apply(
            &mut state,
            &config,
            vassal,
            &Command::OfferFief {
                city: ids.city,
                to: other
            }
        ),
        Err(CommandError::AlreadyFief)
    );
}

#[test]
fn cannot_enfeoff_your_superior_or_someone_from_another_country() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let foreigner = add_steward(&mut state);
    add_country(&mut state, "郑", foreigner);
    let offer = |to| Command::OfferFief { city: ids.city, to };
    assert_eq!(
        apply(&mut state, &config, ids.player, &offer(foreigner)),
        Err(CommandError::DifferentCountry(foreigner))
    );

    // 玩家替上司管着另一座城，就不能把自己的城封给上司
    let superior = add_steward(&mut state);
    let mut other = state.cities[&ids.city].clone();
    other.id = state.alloc_id();
    other.tiles.clear();
    other.owner = Some(superior);
    other.manager = Some(ids.player);
    state.cities.get_mut(&ids.city).unwrap().manager = None;
    state.cities.insert(other.id, other);
    assert_eq!(
        apply(&mut state, &config, ids.player, &offer(superior)),
        Err(CommandError::CannotEnfeoffSuperior(superior))
    );
}

#[test]
fn fiefs_pay_a_share_of_the_years_output_by_default() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let vassal = enfeoff(&mut state, &config, &ids);
    start(
        &mut state,
        &config,
        &ids,
        ids.shi_a,
        Task::CollectTax,
        &[(ids.farmers, 40)],
    )
    .ok();
    apply(
        &mut state,
        &config,
        vassal,
        &Command::StartAssignment {
            city: ids.city,
            shi: ids.shi_a,
            task: Task::CollectTax,
            workers: workers(&[(ids.farmers, 40)]),
        },
    )
    .unwrap();
    for _ in 1..config.turns_per_year() {
        end_turn(&mut state, &config);
    }
    let income = state.cities[&ids.city].income.get("grain");
    assert!(income > 0.0, "秋收的税进了府库");
    let report = end_turn(&mut state, &config);
    let paid = state.characters[&ids.player].private_store.get("grain");
    assert!(
        (paid - income * config.fief.auto_rate).abs() < 1e-6,
        "{paid} vs {income}"
    );
    assert!(report.events.iter().any(|e| e.text.contains("按例")));
    // 年末结算后今年的产出清零，记为去年的
    let city = &state.cities[&ids.city];
    assert_eq!(city.income.get("grain"), 0.0);
    assert!((city.last_income.as_ref().unwrap().get("grain") - income).abs() < 1e-9);
}

#[test]
fn reasonable_tribute_left_unpaid_becomes_a_reason_for_war() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let vassal = enfeoff(&mut state, &config, &ids);
    // 去年府库产出 200 粮，要 40 粮不算过火（上限是 30% 即 60 粮）
    let city = state.cities.get_mut(&ids.city).unwrap();
    city.last_income = Some(Inventory([("grain".to_string(), 200.0)].into()));
    let goods = Inventory([("grain".to_string(), 40.0)].into());
    apply(
        &mut state,
        &config,
        ids.player,
        &Command::SetTribute {
            city: ids.city,
            goods,
        },
    )
    .unwrap();
    assert_eq!(
        apply(
            &mut state,
            &config,
            vassal,
            &Command::RefuseTribute { city: ids.city }
        ),
        Err(CommandError::TributeNotExcessive)
    );
    // 只有封君能指定纳贡
    let goods = Inventory([("grain".to_string(), 1.0)].into());
    assert_eq!(
        apply(
            &mut state,
            &config,
            vassal,
            &Command::SetTribute {
                city: ids.city,
                goods
            }
        ),
        Err(CommandError::NotLiege(ids.city))
    );

    // 纳了一半，一年下来不足
    let half = Inventory([("grain".to_string(), 20.0)].into());
    apply(
        &mut state,
        &config,
        vassal,
        &Command::PayTribute {
            city: ids.city,
            goods: half,
        },
    )
    .unwrap();
    assert_eq!(
        state.characters[&ids.player].private_store.get("grain"),
        20.0
    );
    for _ in 0..config.turns_per_year() {
        end_turn(&mut state, &config);
    }
    assert_eq!(state.cities[&ids.city].tribute_shortfall_years, 1);
    assert!(
        fief_grievances(&state, &config, &ids).is_empty(),
        "一年不足还不算"
    );
    // 第二年还是不纳，满两年成为征讨的理由；封地照样收不回来
    state.cities.get_mut(&ids.city).unwrap().last_income =
        Some(Inventory([("grain".to_string(), 200.0)].into()));
    for _ in 0..config.turns_per_year() {
        end_turn(&mut state, &config);
    }
    assert_eq!(
        fief_grievances(&state, &config, &ids),
        vec![zhou_core::fief::Grievance::TributeShortfall { years: 2 }]
    );
    assert_eq!(state.cities[&ids.city].owner, Some(vassal));
}

fn fief_grievances(
    state: &GameState,
    config: &Config,
    ids: &Ids,
) -> Vec<zhou_core::fief::Grievance> {
    zhou_core::fief::grievances(state, config, &state.cities[&ids.city])
}

#[test]
fn excessive_tribute_can_be_refused_and_breeds_hostility() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let vassal = enfeoff(&mut state, &config, &ids);
    state.cities.get_mut(&ids.city).unwrap().last_income =
        Some(Inventory([("grain".to_string(), 100.0)].into()));
    // 上限 30 粮，要 90 粮：是上限的 3 倍
    let goods = Inventory([("grain".to_string(), 90.0)].into());
    apply(
        &mut state,
        &config,
        ids.player,
        &Command::SetTribute {
            city: ids.city,
            goods,
        },
    )
    .unwrap();
    end_turn(&mut state, &config);
    let hostility = state.cities[&ids.city].groups[&ids.farmers].hostility;
    assert_eq!(
        hostility,
        config.fief.hostility_gain * 2.0,
        "超出越多敌视越深，最多两倍"
    );

    // 拒绝了也不消气，只要封君还这么要
    apply(
        &mut state,
        &config,
        vassal,
        &Command::RefuseTribute { city: ids.city },
    )
    .unwrap();
    end_turn(&mut state, &config);
    assert!(state.cities[&ids.city].groups[&ids.farmers].hostility > hostility);
    // 拒绝过火的纳贡，年底不算纳贡不足
    for _ in 2..config.turns_per_year() {
        end_turn(&mut state, &config);
    }
    assert_eq!(state.cities[&ids.city].tribute_shortfall_years, 0);

    // 封君改回合理的要求，敌视慢慢消退。测试城没人收税，府库今年没有产出，按去年 100 粮算
    state.cities.get_mut(&ids.city).unwrap().last_income =
        Some(Inventory([("grain".to_string(), 100.0)].into()));
    let before = state.cities[&ids.city].groups[&ids.farmers].hostility;
    let goods = Inventory([("grain".to_string(), 1.0)].into());
    apply(
        &mut state,
        &config,
        ids.player,
        &Command::SetTribute {
            city: ids.city,
            goods,
        },
    )
    .unwrap();
    end_turn(&mut state, &config);
    assert_eq!(
        state.cities[&ids.city].groups[&ids.farmers].hostility,
        before - config.fief.hostility_decay
    );
}

#[test]
fn low_reputation_and_usurpation_are_reasons_for_war() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let vassal = enfeoff(&mut state, &config, &ids);
    assert!(fief_grievances(&state, &config, &ids).is_empty());
    state.characters.get_mut(&vassal).unwrap().reputation = config.fief.low_reputation - 1.0;
    // 封臣无爵位，每城上限 7 块地；给他 9 块，僭越
    let city = state.cities.get_mut(&ids.city).unwrap();
    for pos in Hex::new(0, 0).spiral().skip(3).take(6) {
        let id = state.next_id;
        state.next_id += 1;
        city.tiles.insert(id, Tile::new(id, pos, "forest"));
    }
    end_turn(&mut state, &config);
    use zhou_core::fief::Grievance;
    assert_eq!(
        fief_grievances(&state, &config, &ids),
        vec![Grievance::LowReputation, Grievance::Usurpation]
    );
}

#[test]
fn a_country_with_no_cities_left_falls() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let wei = add_country(&mut state, "卫", ids.player);
    let landless = add_steward(&mut state);
    let zheng = add_country(&mut state, "郑", landless);
    let report = end_turn(&mut state, &config);
    assert!(state.countries.contains_key(&wei));
    assert!(!state.countries.contains_key(&zheng));
    assert_eq!(
        state.characters[&landless].nationality, None,
        "亡国之人成了无国籍"
    );
    assert!(
        report
            .events
            .iter()
            .any(|e| e.text.contains("郑国君自己的城已经全部失守"))
    );

    // 国君自己的城全被攻占，国家就亡了，哪怕封臣手里还有城；封臣都丧失国籍
    let (mut state, ids) = setup(&config);
    let wei = add_country(&mut state, "卫", ids.player);
    let seat = state.cities[&ids.city].seat;
    let fief = copy_city(&mut state, &ids, ids.player, seat);
    let vassal = add_steward(&mut state);
    apply(
        &mut state,
        &config,
        ids.player,
        &Command::OfferFief {
            city: fief,
            to: vassal,
        },
    )
    .unwrap();
    apply(
        &mut state,
        &config,
        vassal,
        &Command::AcceptFief { city: fief },
    )
    .unwrap();
    assert_eq!(state.characters[&vassal].nationality, Some(wei));
    state.cities.get_mut(&ids.city).unwrap().status = zhou_core::state::CityStatus::Occupied;
    end_turn(&mut state, &config);
    assert!(!state.countries.contains_key(&wei));
    assert_eq!(state.characters[&vassal].nationality, None);
}

#[test]
fn a_ruler_must_keep_one_city_of_his_own() {
    let config = config();
    let (mut state, ids) = setup(&config);
    add_country(&mut state, "卫", ids.player);
    let other = add_steward(&mut state);
    // 手里只有一座城，封不出去、送不出去
    assert_eq!(
        apply(
            &mut state,
            &config,
            ids.player,
            &Command::OfferFief {
                city: ids.city,
                to: other
            }
        ),
        Err(CommandError::RulerMustKeepACity)
    );
    let give = zhou_core::Stake {
        cities: vec![ids.city],
        ..Default::default()
    };
    assert_eq!(
        propose(
            &mut state,
            &config,
            ids.player,
            other,
            give,
            Default::default(),
            None
        ),
        Err(CommandError::RulerMustKeepACity)
    );
    // 有两座就能封出一座
    let seat = state.cities[&ids.city].seat;
    copy_city(&mut state, &ids, ids.player, seat);
    apply(
        &mut state,
        &config,
        ids.player,
        &Command::OfferFief {
            city: ids.city,
            to: other,
        },
    )
    .unwrap();
}

fn goods(pairs: &[(&str, f64)]) -> Inventory {
    Inventory(pairs.iter().map(|(g, n)| (g.to_string(), *n)).collect())
}

fn propose(
    state: &mut GameState,
    config: &Config,
    from: Id,
    to: Id,
    give: zhou_core::Stake,
    take: zhou_core::Stake,
    into: Option<Id>,
) -> Result<Id, CommandError> {
    apply(
        state,
        config,
        from,
        &Command::ProposeDeal {
            to,
            give,
            take,
            into,
        },
    )
    .map(|ids| ids[0])
}

#[test]
fn a_gift_takes_effect_once_accepted() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let friend = add_steward(&mut state);
    let stranger = add_steward(&mut state);
    state.characters.get_mut(&ids.player).unwrap().private_store = goods(&[("grain", 50.0)]);
    let give = zhou_core::Stake {
        private: goods(&[("grain", 20.0)]),
        ..Default::default()
    };
    let too_much = zhou_core::Stake {
        private: goods(&[("grain", 80.0)]),
        ..Default::default()
    };
    assert_eq!(
        propose(
            &mut state,
            &config,
            ids.player,
            friend,
            too_much,
            Default::default(),
            None
        ),
        Err(CommandError::NotEnoughInPrivateStore("grain".into()))
    );
    assert_eq!(
        propose(
            &mut state,
            &config,
            ids.player,
            friend,
            Default::default(),
            Default::default(),
            None
        ),
        Err(CommandError::EmptyDeal)
    );
    let deal = propose(
        &mut state,
        &config,
        ids.player,
        friend,
        give,
        Default::default(),
        None,
    )
    .unwrap();
    assert_eq!(state.deals[&deal].kind_name(), "赠与");
    assert_eq!(
        state.characters[&ids.player].private_store.get("grain"),
        50.0,
        "对方接受之前不动"
    );
    assert_eq!(
        apply(
            &mut state,
            &config,
            stranger,
            &Command::AcceptDeal { deal, into: None }
        ),
        Err(CommandError::NotYourDeal(deal))
    );
    apply(
        &mut state,
        &config,
        friend,
        &Command::AcceptDeal { deal, into: None },
    )
    .unwrap();
    assert_eq!(
        state.characters[&ids.player].private_store.get("grain"),
        30.0
    );
    assert_eq!(state.characters[&friend].private_store.get("grain"), 20.0);
    assert!(state.deals.is_empty());
}

#[test]
fn owners_trade_from_their_cities_treasuries_without_asking_the_manager() {
    let config = config();
    let (mut state, ids) = setup(&config);
    hand_over(&mut state, &config, &ids);
    let merchant = add_steward(&mut state);
    state.characters.get_mut(&merchant).unwrap().private_store = goods(&[("salt", 30.0)]);
    // 玩家拿城里府库的青铜换商人的盐，换来的盐放进这座城的府库
    let give = zhou_core::Stake {
        treasuries: [(ids.city, goods(&[("bronze", 10.0)]))].into(),
        ..Default::default()
    };
    let take = zhou_core::Stake {
        private: goods(&[("salt", 30.0)]),
        ..Default::default()
    };
    let deal = propose(
        &mut state,
        &config,
        ids.player,
        merchant,
        give,
        take,
        Some(ids.city),
    )
    .unwrap();
    assert_eq!(state.deals[&deal].kind_name(), "交易");
    apply(
        &mut state,
        &config,
        merchant,
        &Command::AcceptDeal { deal, into: None },
    )
    .unwrap();
    let city = &state.cities[&ids.city];
    assert_eq!(city.treasury.get("bronze"), 40.0);
    assert_eq!(city.treasury.get("salt"), 130.0);
    assert_eq!(
        state.characters[&merchant].private_store.get("bronze"),
        10.0
    );
    assert_eq!(state.characters[&merchant].private_store.get("salt"), 0.0);
}

#[test]
fn a_deal_is_checked_again_when_accepted() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let lord = add_steward(&mut state);
    state.characters.get_mut(&lord).unwrap().private_store = goods(&[("grain", 10.0)]);
    // 玩家索取别人的粮；对方答复前已经把粮送光了
    let take = zhou_core::Stake {
        private: goods(&[("grain", 10.0)]),
        ..Default::default()
    };
    let deal = propose(
        &mut state,
        &config,
        ids.player,
        lord,
        Default::default(),
        take,
        None,
    )
    .unwrap();
    assert_eq!(state.deals[&deal].kind_name(), "索取");
    state.characters.get_mut(&lord).unwrap().private_store = Inventory::default();
    assert_eq!(
        apply(
            &mut state,
            &config,
            lord,
            &Command::AcceptDeal { deal, into: None }
        ),
        Err(CommandError::NotEnoughInPrivateStore("grain".into()))
    );
    apply(&mut state, &config, lord, &Command::DeclineDeal { deal }).unwrap();
    assert!(state.deals.is_empty());
}

#[test]
fn deals_left_unanswered_expire_at_the_end_of_the_turn() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let friend = add_steward(&mut state);
    state.characters.get_mut(&ids.player).unwrap().private_store = goods(&[("grain", 10.0)]);
    let give = zhou_core::Stake {
        private: goods(&[("grain", 10.0)]),
        ..Default::default()
    };
    let deal = propose(
        &mut state,
        &config,
        ids.player,
        friend,
        give,
        Default::default(),
        None,
    )
    .unwrap();
    let report = end_turn(&mut state, &config);
    assert!(state.deals.is_empty());
    assert!(
        report
            .events
            .iter()
            .any(|e| e.text.contains("没有在当回合答复"))
    );
    assert_eq!(
        apply(
            &mut state,
            &config,
            friend,
            &Command::AcceptDeal { deal, into: None }
        ),
        Err(CommandError::NoSuchDeal(deal))
    );
    assert_eq!(
        state.characters[&ids.player].private_store.get("grain"),
        10.0
    );
}

#[test]
fn appointments_and_enfeoffments_left_unanswered_expire_too() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let steward = add_steward(&mut state);
    apply(
        &mut state,
        &config,
        ids.player,
        &Command::OfferManagement {
            city: ids.city,
            to: steward,
        },
    )
    .unwrap();
    end_turn(&mut state, &config);
    assert_eq!(
        apply(
            &mut state,
            &config,
            steward,
            &Command::AcceptManagement { city: ids.city }
        ),
        Err(CommandError::NotOffered(ids.city))
    );

    let vassal = add_steward(&mut state);
    apply(
        &mut state,
        &config,
        ids.player,
        &Command::OfferFief {
            city: ids.city,
            to: vassal,
        },
    )
    .unwrap();
    let report = end_turn(&mut state, &config);
    assert!(
        report
            .events
            .iter()
            .any(|e| e.text.contains("封授") && e.text.contains("已作废"))
    );
    assert_eq!(
        apply(
            &mut state,
            &config,
            vassal,
            &Command::AcceptFief { city: ids.city }
        ),
        Err(CommandError::NotOfferedFief)
    );
    assert_eq!(state.cities[&ids.city].owner, Some(ids.player));
}

#[test]
fn whole_cities_can_change_hands_but_fiefs_cannot() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let buyer = add_steward(&mut state);
    add_country(&mut state, "郑", buyer);
    state.characters.get_mut(&buyer).unwrap().private_store = goods(&[("bronze", 100.0)]);
    let give = zhou_core::Stake {
        cities: vec![ids.city],
        ..Default::default()
    };
    let take = zhou_core::Stake {
        private: goods(&[("bronze", 100.0)]),
        ..Default::default()
    };
    let deal = propose(
        &mut state,
        &config,
        ids.player,
        buyer,
        give.clone(),
        take,
        None,
    )
    .unwrap();
    apply(
        &mut state,
        &config,
        buyer,
        &Command::AcceptDeal { deal, into: None },
    )
    .unwrap();
    let city = &state.cities[&ids.city];
    // 直接易手，不形成封君封臣关系；城随新主人归了郑国
    assert_eq!(
        (city.owner, city.manager, city.liege),
        (Some(buyer), Some(buyer), None)
    );
    assert_eq!(state.country_of(city), state.characters[&buyer].nationality);
    assert_eq!(
        state.characters[&ids.player].private_store.get("bronze"),
        100.0
    );

    // 都城不能易手
    let (mut state, ids) = setup(&config);
    let wei = add_country(&mut state, "卫", ids.player);
    state.countries.get_mut(&wei).unwrap().capital = Some(ids.city);
    let buyer = add_steward(&mut state);
    let give = zhou_core::Stake {
        cities: vec![ids.city],
        ..Default::default()
    };
    assert_eq!(
        propose(
            &mut state,
            &config,
            ids.player,
            buyer,
            give,
            Default::default(),
            None
        ),
        Err(CommandError::CapitalCannotChangeHands(ids.city))
    );

    // 封地不能转手
    let (mut state, ids) = setup(&config);
    let vassal = enfeoff(&mut state, &config, &ids);
    let other = add_steward(&mut state);
    let give = zhou_core::Stake {
        cities: vec![ids.city],
        ..Default::default()
    };
    assert_eq!(
        propose(
            &mut state,
            &config,
            vassal,
            other,
            give,
            Default::default(),
            None
        ),
        Err(CommandError::AlreadyFief)
    );
}

/// 照着这座城复制一座给 `owner`，治所放在 `seat`，返回新城的 id。
fn copy_city(state: &mut GameState, ids: &Ids, owner: Id, seat: Hex) -> Id {
    let id = state.alloc_id();
    let mut city = state.cities[&ids.city].clone();
    city.id = id;
    city.name = format!("城{id}");
    city.owner = Some(owner);
    city.manager = None;
    city.seat = seat;
    state.cities.insert(id, city);
    id
}

#[test]
fn cities_of_one_owner_close_together_produce_more() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let seat = state.cities[&ids.city].seat;
    let mult = |state: &GameState| state.neighbor_mult(&config, &state.cities[&ids.city]);
    assert_eq!(mult(&state), 1.0);
    // 别人的城、离得太远的城都不算
    let other = add_steward(&mut state);
    copy_city(&mut state, &ids, other, seat);
    let far = Hex::new(seat.q - config.neighbor.range as i32 - 1, seat.r);
    copy_city(&mut state, &ids, ids.player, far);
    assert_eq!(mult(&state), 1.0);
    // 8 格之内的每座自己的城加 5%，最多 15%
    let near = Hex::new(seat.q + config.neighbor.range as i32, seat.r);
    let first = copy_city(&mut state, &ids, ids.player, near);
    assert!((mult(&state) - 1.05).abs() < 1e-9);
    assert!(
        (state.neighbor_mult(&config, &state.cities[&first]) - 1.05).abs() < 1e-9,
        "两座城都有增益"
    );
    for _ in 0..3 {
        copy_city(&mut state, &ids, ids.player, near);
    }
    assert!((mult(&state) - 1.15).abs() < 1e-9);

    // 增益加在产出上：同样的官营种粮，有邻城的多收 5%
    let harvest = |mine: bool| {
        let (mut state, ids) = setup(&config);
        state.turn = autumn(&config);
        let owner = if mine {
            ids.player
        } else {
            add_steward(&mut state)
        };
        let seat = state.cities[&ids.city].seat;
        copy_city(&mut state, &ids, owner, seat);
        start(
            &mut state,
            &config,
            &ids,
            ids.shi_a,
            produce("grain", Mode::Guan),
            &[(ids.farmers, 30)],
        )
        .unwrap();
        end_turn(&mut state, &config);
        state.cities[&ids.city].income.get("grain")
    };
    let alone = harvest(false);
    let together = harvest(true);
    assert!(alone > 0.0);
    assert!(
        (together / alone - 1.05).abs() < 1e-6,
        "{together} / {alone}"
    );
}

#[test]
fn characters_stay_in_one_city_and_move_only_to_their_own() {
    let config = config();
    let (mut state, ids) = setup(&config);
    // 默认待在自己管理的城
    assert_eq!(state.characters[&ids.player].location, Some(ids.city));
    let seat = state.cities[&ids.city].seat;
    let second = copy_city(&mut state, &ids, ids.player, seat);
    let move_to = |state: &mut GameState, who: Id, city: Id| {
        apply(state, &config, who, &Command::MoveTo { city })
    };
    move_to(&mut state, ids.player, second).unwrap();
    assert_eq!(state.characters[&ids.player].location, Some(second));
    assert_eq!(
        move_to(&mut state, ids.player, second),
        Err(CommandError::AlreadyThere(second))
    );
    // 别人的城去不了
    let other = add_steward(&mut state);
    let theirs = copy_city(&mut state, &ids, other, seat);
    assert_eq!(
        move_to(&mut state, ids.player, theirs),
        Err(CommandError::NotOwner(theirs))
    );
    state.settle_locations();
    assert_eq!(
        state.characters[&other].location,
        Some(theirs),
        "没有管理的城就待在自己拥有的城"
    );

    // 所在的城送了人，回到自己管理的城
    let give = zhou_core::Stake {
        cities: vec![second],
        ..Default::default()
    };
    let deal = propose(
        &mut state,
        &config,
        ids.player,
        other,
        give,
        Default::default(),
        None,
    )
    .unwrap();
    apply(
        &mut state,
        &config,
        other,
        &Command::AcceptDeal { deal, into: None },
    )
    .unwrap();
    assert_eq!(state.characters[&ids.player].location, Some(ids.city));

    // 无处可待的人接受任命后待在他管的城
    let steward = add_steward(&mut state);
    assert_eq!(state.characters[&steward].location, None);
    apply(
        &mut state,
        &config,
        ids.player,
        &Command::OfferManagement {
            city: ids.city,
            to: steward,
        },
    )
    .unwrap();
    apply(
        &mut state,
        &config,
        steward,
        &Command::AcceptManagement { city: ids.city },
    )
    .unwrap();
    assert_eq!(state.characters[&steward].location, Some(ids.city));
    // 所有者不再管这座城，但城还是他的，照样可以待在这里
    assert_eq!(state.characters[&ids.player].location, Some(ids.city));

    // 自己有城的人替别人管城，也只能待在自己的城
    let (mut state, ids) = setup(&config);
    let steward = add_steward(&mut state);
    let seat = state.cities[&ids.city].seat;
    let own = copy_city(&mut state, &ids, steward, seat);
    apply(
        &mut state,
        &config,
        ids.player,
        &Command::OfferManagement {
            city: ids.city,
            to: steward,
        },
    )
    .unwrap();
    apply(
        &mut state,
        &config,
        steward,
        &Command::AcceptManagement { city: ids.city },
    )
    .unwrap();
    assert_eq!(state.characters[&steward].location, Some(own));
    assert!(!state.can_stay(steward, ids.city));
    // 他的城送了人，一座城都没有了，就待在他管理的城
    let give = zhou_core::Stake {
        cities: vec![own],
        ..Default::default()
    };
    let deal = propose(
        &mut state,
        &config,
        steward,
        ids.player,
        give,
        Default::default(),
        None,
    )
    .unwrap();
    apply(
        &mut state,
        &config,
        ids.player,
        &Command::AcceptDeal { deal, into: None },
    )
    .unwrap();
    assert_eq!(state.characters[&steward].location, Some(ids.city));
}

#[test]
fn armies_pass_a_city_only_with_the_owners_permission() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let steward = hand_over(&mut state, &config, &ids);
    let neighbor = add_steward(&mut state);
    let wei = add_country(&mut state, "卫", ids.player);
    state.characters.get_mut(&neighbor).unwrap().nationality = Some(wei);
    let seat = state.cities[&ids.city].seat;
    // 所有者自己能走；野地谁都能走，商旅哪里都能走
    assert!(state.can_pass(ids.player, seat, false));
    assert!(state.can_pass(neighbor, Hex::new(100, 100), false));
    assert!(state.can_pass(neighbor, seat, true));
    // 同一国的人、管理者，没有许可都过不去
    assert!(!state.can_pass(neighbor, seat, false));
    assert!(!state.can_pass(steward, seat, false));

    // 通行权在交易里给：邻人拿青铜换 6 回合（默认期限）的通行权
    state.characters.get_mut(&neighbor).unwrap().private_store = goods(&[("bronze", 10.0)]);
    let passage = |turns: Option<u32>| zhou_core::Stake {
        passage: vec![zhou_core::Passage {
            city: ids.city,
            turns,
        }],
        ..Default::default()
    };
    let pay = zhou_core::Stake {
        private: goods(&[("bronze", 10.0)]),
        ..Default::default()
    };
    let deal = propose(
        &mut state,
        &config,
        neighbor,
        ids.player,
        pay,
        passage(None),
        None,
    )
    .unwrap();
    // 管理者给不了通行权，只有所有者能给
    assert_eq!(
        propose(
            &mut state,
            &config,
            steward,
            neighbor,
            passage(None),
            Default::default(),
            None
        ),
        Err(CommandError::NotOwner(ids.city))
    );
    assert_eq!(
        propose(
            &mut state,
            &config,
            ids.player,
            neighbor,
            passage(Some(0)),
            Default::default(),
            None
        ),
        Err(CommandError::ZeroPassageTurns)
    );
    apply(
        &mut state,
        &config,
        ids.player,
        &Command::AcceptDeal { deal, into: None },
    )
    .unwrap();
    assert!(state.can_pass(neighbor, seat, false));
    assert_eq!(config.passage.default_turns, 6);
    // 当回合算第一回合，6 回合后到期
    for _ in 0..5 {
        end_turn(&mut state, &config);
        assert!(state.can_pass(neighbor, seat, false));
    }
    let report = end_turn(&mut state, &config);
    assert!(
        report.events.iter().any(|e| e.text.contains("通行权到期")),
        "{:?}",
        report.events
    );
    assert!(!state.can_pass(neighbor, seat, false));
    // 再给一次，期限可以自己定；城易手后通行权作废
    let deal = propose(
        &mut state,
        &config,
        ids.player,
        neighbor,
        passage(Some(2)),
        Default::default(),
        None,
    )
    .unwrap();
    apply(
        &mut state,
        &config,
        neighbor,
        &Command::AcceptDeal { deal, into: None },
    )
    .unwrap();
    assert_eq!(state.cities[&ids.city].passage[&neighbor], state.turn + 2);
    // 国君要留一座城在自己手里，先给他另一座
    copy_city(&mut state, &ids, ids.player, seat);
    let give = zhou_core::Stake {
        cities: vec![ids.city],
        ..Default::default()
    };
    let deal = propose(
        &mut state,
        &config,
        ids.player,
        steward,
        give,
        Default::default(),
        None,
    )
    .unwrap();
    apply(
        &mut state,
        &config,
        steward,
        &Command::AcceptDeal { deal, into: None },
    )
    .unwrap();
    assert!(!state.can_pass(neighbor, seat, false));

    // 国君的军队可以自由出入本国所有的城
    let (mut state, ids) = setup(&config);
    let ruler = add_steward(&mut state);
    let wei = add_country(&mut state, "卫", ruler);
    state.characters.get_mut(&ids.player).unwrap().nationality = Some(wei);
    let other = add_steward(&mut state);
    state.characters.get_mut(&other).unwrap().nationality = Some(wei);
    let seat = state.cities[&ids.city].seat;
    assert!(state.can_pass(ruler, seat, false));
    assert!(!state.can_pass(other, seat, false));
    // 国君发布国战时，国内所有的城对本国的人开放，外国人照样过不去
    state.countries.get_mut(&wei).unwrap().national_war = true;
    assert!(state.can_pass(other, seat, false));
    let foreigner = add_steward(&mut state);
    assert!(!state.can_pass(foreigner, seat, false));
}

fn squad(group: Id, unit: &str, count: u32, extras: &[&str]) -> zhou_core::SquadOrder {
    zhou_core::SquadOrder {
        group,
        unit: unit.into(),
        count,
        extras: extras.iter().map(|e| e.to_string()).collect(),
    }
}

#[test]
fn owners_raise_armies_from_martial_people_and_disband_them_at_home() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let city = state.cities.get_mut(&ids.city).unwrap();
    city.groups
        .get_mut(&ids.farmers)
        .unwrap()
        .skills
        .push("martial".into());
    city.treasury.add("weapon", 42.0);
    city.treasury.add("armor", 10.0);
    let grain = Task::Produce {
        recipe: "grain".into(),
        mode: Mode::Min,
    };
    let work = start(
        &mut state,
        &config,
        &ids,
        ids.shi_a,
        grain,
        &[(ids.farmers, 40)],
    )
    .unwrap()[0];
    let other = state.alloc_id();
    state
        .characters
        .insert(other, Character::new(other, "旁人", "none", 0.0));

    let raise = |squads| Command::RaiseArmy {
        city: ids.city,
        squads,
    };
    // 只有城的所有者能征发；不会「军武」的人不能当兵；步兵不能带马
    assert_eq!(
        apply(
            &mut state,
            &config,
            other,
            &raise(vec![squad(ids.farmers, "infantry", 10, &[])])
        ),
        Err(CommandError::NotOwner(ids.city))
    );
    assert_eq!(
        apply(
            &mut state,
            &config,
            ids.player,
            &raise(vec![squad(ids.artisans, "infantry", 10, &[])])
        ),
        Err(CommandError::MissingSkill {
            group: ids.artisans,
            skill: "martial".into()
        })
    );
    assert_eq!(
        apply(
            &mut state,
            &config,
            ids.player,
            &raise(vec![squad(ids.farmers, "infantry", 10, &["horse"])])
        ),
        Err(CommandError::NotOptionalGear {
            unit: "步兵".into(),
            good: "horse".into()
        })
    );
    assert_eq!(
        apply(
            &mut state,
            &config,
            ids.player,
            &raise(vec![squad(ids.farmers, "infantry", 101, &[])])
        ),
        Err(CommandError::NotEnoughNotLearning {
            group: ids.farmers,
            available: 100,
            requested: 101
        })
    );

    // 70 人当步兵、带甲：没在做工事的 60 人先上，再从种地里抽 10 人
    let army = apply(
        &mut state,
        &config,
        ids.player,
        &raise(vec![squad(ids.farmers, "infantry", 70, &["armor"])]),
    )
    .unwrap()[0];
    let city = &state.cities[&ids.city];
    assert_eq!(city.groups[&ids.farmers].size, 30);
    assert_eq!(city.assignments[&work].workers[&ids.farmers], 30);
    assert_eq!(city.treasury.get("weapon"), 0.0);
    assert_eq!(city.treasury.get("armor"), 0.0);
    let a = &state.armies[&army];
    assert_eq!((a.commander, a.pos, a.size()), (ids.player, city.seat, 70));
    // 兵器只够 42 人：装备齐备 60%，战斗力打 0.6 + 0.4 × 0.5 = 0.8
    let s = &a.squads[0];
    assert!((s.equipped(&config) - 0.6).abs() < 1e-9);
    assert!((s.gear_mult(&config) - 0.8).abs() < 1e-9);
    assert_eq!(s.gear.get("armor"), 10.0);

    // 别人不能遣散；离开自己的城不能遣散
    let disband = Command::DisbandArmy { army };
    assert_eq!(
        apply(&mut state, &config, other, &disband),
        Err(CommandError::NotCommander(army))
    );
    state.armies.get_mut(&army).unwrap().pos = Hex::new(20, 0);
    assert_eq!(
        apply(&mut state, &config, ids.player, &disband),
        Err(CommandError::NotHome(army))
    );

    // 回到自己的城就能遣散：人回原来的岗位，装备还回府库
    let edge = state.cities[&ids.city].tiles.values().last().unwrap().pos;
    state.armies.get_mut(&army).unwrap().pos = edge;
    apply(&mut state, &config, ids.player, &disband).unwrap();
    let city = &state.cities[&ids.city];
    assert!(state.armies.is_empty());
    assert_eq!(city.groups[&ids.farmers].size, 100);
    assert_eq!(city.assignments[&work].workers[&ids.farmers], 40);
    assert_eq!(city.treasury.get("weapon"), 42.0);
    assert_eq!(city.treasury.get("armor"), 10.0);
}

#[test]
fn elite_troops_eat_from_the_treasury_and_armies_follow_their_city() {
    let config = config();
    let (mut state, ids) = setup(&config);
    let city = state.cities.get_mut(&ids.city).unwrap();
    city.private_wealth.add("grain", 1000.0);
    city.private_wealth.add("salt", 1000.0);
    let mut elite = state.clone();
    elite
        .cities
        .get_mut(&ids.city)
        .unwrap()
        .groups
        .get_mut(&ids.artisans)
        .unwrap()
        .skills = vec!["martial".into()];
    let treasury_grain = |s: &GameState| s.cities[&ids.city].treasury.get("grain");
    let before = treasury_grain(&state);
    end_turn(&mut state, &config);
    end_turn(&mut elite, &config);
    // 20 名精兵每回合的口粮由府库出
    let food = 20.0 * config.population.consumption[0].amount / config.population.per_people;
    let diff = (before - treasury_grain(&elite)) - (before - treasury_grain(&state));
    assert!((diff - food).abs() < 1e-6, "{diff} vs {food}");

    // 精兵编成军队后照样吃府库；城整座送人，军队跟着归新主人统领
    let army = apply(
        &mut elite,
        &config,
        ids.player,
        &Command::RaiseArmy {
            city: ids.city,
            squads: vec![squad(ids.artisans, "supply", 20, &[])],
        },
    )
    .unwrap()[0];
    let other = elite.alloc_id();
    elite
        .characters
        .insert(other, Character::new(other, "旁人", "none", 0.0));
    let give = zhou_core::Stake {
        cities: vec![ids.city],
        ..Default::default()
    };
    let deal = apply(
        &mut elite,
        &config,
        ids.player,
        &Command::ProposeDeal {
            to: other,
            give,
            take: Default::default(),
            into: None,
        },
    )
    .unwrap()[0];
    apply(
        &mut elite,
        &config,
        other,
        &Command::AcceptDeal { deal, into: None },
    )
    .unwrap();
    assert_eq!(elite.armies[&army].commander, other);
    assert_eq!(
        apply(
            &mut elite,
            &config,
            ids.player,
            &Command::DisbandArmy { army }
        ),
        Err(CommandError::NotCommander(army))
    );
    apply(&mut elite, &config, other, &Command::DisbandArmy { army }).unwrap();
}

/// 测试城东边一条野地走廊 (1,0)…(5,0)，农人会「军武」，编一支军队。
fn army_on_corridor(config: &Config, units: &[&str]) -> (GameState, Ids, Id) {
    let (mut state, ids) = setup(config);
    for q in 1..=5 {
        state.map.insert(Hex::new(q, 0), "farmland");
    }
    let city = state.cities.get_mut(&ids.city).unwrap();
    city.groups
        .get_mut(&ids.farmers)
        .unwrap()
        .skills
        .push("martial".into());
    let squads = units
        .iter()
        .map(|u| squad(ids.farmers, u, 10, &[]))
        .collect();
    let army = apply(
        &mut state,
        config,
        ids.player,
        &Command::RaiseArmy {
            city: ids.city,
            squads,
        },
    )
    .unwrap()[0];
    (state, ids, army)
}

#[test]
fn armies_march_to_a_destination_over_several_turns() {
    let config = config();
    let (mut state, ids, army) = army_on_corridor(&config, &["infantry"]);
    let march = |to| Command::MarchArmy {
        army,
        to,
        force: false,
    };
    let at = |s: &GameState| (s.armies[&army].pos, s.armies[&army].dest);
    // 步兵每回合 2 点行军力，耕地每格 1：下令当回合就走 2 格，之后每回合开始时接着走
    apply(&mut state, &config, ids.player, &march(Hex::new(5, 0))).unwrap();
    assert_eq!(at(&state), (Hex::new(2, 0), Some(Hex::new(5, 0))));
    assert_eq!(state.armies[&army].moves_left, 0);
    end_turn(&mut state, &config);
    assert_eq!(at(&state), (Hex::new(4, 0), Some(Hex::new(5, 0))));
    let report = end_turn(&mut state, &config);
    assert_eq!(at(&state), (Hex::new(5, 0), None));
    assert_eq!(report.armies[0].pos, Hex::new(5, 0));
    assert_eq!(state.armies[&army].moves_left, 1);

    // 不在自己的城里不能遣散；当回合可以改命令往回走
    assert_eq!(
        apply(
            &mut state,
            &config,
            ids.player,
            &Command::DisbandArmy { army }
        ),
        Err(CommandError::NotHome(army))
    );
    apply(&mut state, &config, ids.player, &march(Hex::new(0, 0))).unwrap();
    assert_eq!(at(&state), (Hex::new(4, 0), Some(Hex::new(0, 0))));
    // 目的地是现在的位置就是原地待命
    apply(&mut state, &config, ids.player, &march(Hex::new(4, 0))).unwrap();
    assert_eq!(at(&state), (Hex::new(4, 0), None));

    // 地图外、别人的军队不能指挥
    assert_eq!(
        apply(&mut state, &config, ids.player, &march(Hex::new(9, 9))),
        Err(CommandError::NoPath(Hex::new(9, 9)))
    );
    let other = state.alloc_id();
    state
        .characters
        .insert(other, Character::new(other, "旁人", "none", 0.0));
    assert_eq!(
        apply(&mut state, &config, other, &march(Hex::new(0, 0))),
        Err(CommandError::NotCommander(army))
    );
}

#[test]
fn terrain_and_unit_type_decide_how_far_an_army_goes() {
    let config = config();
    let (mut state, ids, army) = army_on_corridor(&config, &["infantry", "chariot"]);
    let a = &state.armies[&army];
    // 混编按最慢的走：步兵 2 点行军力；车兵进山林要 3 点
    assert_eq!(a.moves(&config), 2);
    assert_eq!(a.terrain_cost(&config, "farmland"), Some(1));
    assert_eq!(a.terrain_cost(&config, "forest"), Some(3));
    assert_eq!(a.terrain_cost(&config, "mountain"), None);

    // 行军力不够走下一格时，满行军力也能走一格
    state.map.insert(Hex::new(1, 0), "forest");
    apply(
        &mut state,
        &config,
        ids.player,
        &Command::MarchArmy {
            army,
            to: Hex::new(2, 0),
            force: false,
        },
    )
    .unwrap();
    assert_eq!(state.armies[&army].pos, Hex::new(1, 0));
    assert_eq!(state.armies[&army].moves_left, 0);

    // 高山挡路，走不过去
    state.map.insert(Hex::new(3, 0), "mountain");
    assert_eq!(
        apply(
            &mut state,
            &config,
            ids.player,
            &Command::MarchArmy {
                army,
                to: Hex::new(5, 0),
                force: false,
            }
        ),
        Err(CommandError::NoPath(Hex::new(5, 0)))
    );
    // 旁边有野地就绕过去
    state.map.insert(Hex::new(3, -1), "farmland");
    state.map.insert(Hex::new(4, -1), "farmland");
    apply(
        &mut state,
        &config,
        ids.player,
        &Command::MarchArmy {
            army,
            to: Hex::new(5, 0),
            force: false,
        },
    )
    .unwrap();
    for _ in 0..3 {
        end_turn(&mut state, &config);
    }
    assert_eq!(state.armies[&army].pos, Hex::new(5, 0));
}

#[test]
fn marches_go_around_cities_without_passage_and_other_armies() {
    let config = config();
    let (mut state, ids, army) = army_on_corridor(&config, &["infantry"]);
    let other = state.alloc_id();
    state
        .characters
        .insert(other, Character::new(other, "旁人", "none", 0.0));
    // 走廊中间的 (3,0) 是别人的城
    let city = copy_city(&mut state, &ids, other, Hex::new(3, 0));
    let c = state.cities.get_mut(&city).unwrap();
    let tile = *c.tiles.keys().next().unwrap();
    c.tiles.retain(|id, _| *id == tile);
    c.tiles.get_mut(&tile).unwrap().pos = Hex::new(3, 0);
    let march = Command::MarchArmy {
        army,
        to: Hex::new(5, 0),
        force: false,
    };
    assert_eq!(
        apply(&mut state, &config, ids.player, &march),
        Err(CommandError::NoPath(Hex::new(5, 0)))
    );

    // 有了通行权就能走
    let until = state.turn + 6;
    state
        .cities
        .get_mut(&city)
        .unwrap()
        .passage
        .insert(ids.player, until);
    apply(&mut state, &config, ids.player, &march).unwrap();
    assert_eq!(state.armies[&army].pos, Hex::new(2, 0));

    // 前面被别人的军队挡住、没有别的路：停下，等新的命令
    let mut blocker = state.armies[&army].clone();
    blocker.id = state.alloc_id();
    blocker.commander = other;
    blocker.pos = Hex::new(4, 0);
    blocker.dest = None;
    state.armies.insert(blocker.id, blocker);
    let report = end_turn(&mut state, &config);
    assert_eq!(state.armies[&army].pos, Hex::new(2, 0));
    assert_eq!(state.armies[&army].dest, None);
    assert!(
        report.events.iter().any(|e| e.text.contains("路被挡住")),
        "{:?}",
        report.events
    );
}

/// 交战用：走廊上玩家一支 50 人的步兵，兵器齐全；旁人和玩家开战。
fn war_on_corridor(config: &Config) -> (GameState, Ids, Id, Id) {
    let (mut state, ids) = setup(config);
    for q in 1..=5 {
        state.map.insert(Hex::new(q, 0), "farmland");
    }
    let city = state.cities.get_mut(&ids.city).unwrap();
    city.groups
        .get_mut(&ids.farmers)
        .unwrap()
        .skills
        .push("martial".into());
    city.treasury.add("weapon", 100.0);
    city.treasury.add("siege_engine", 10.0);
    let army = apply(
        &mut state,
        config,
        ids.player,
        &Command::RaiseArmy {
            city: ids.city,
            squads: vec![squad(ids.farmers, "infantry", 50, &["siege_engine"])],
        },
    )
    .unwrap()[0];
    let other = state.alloc_id();
    state
        .characters
        .insert(other, Character::new(other, "敌人", "none", 0.0));
    state.set_war(ids.player, other, true);
    (state, ids, army, other)
}

/// 复制一支军队给别人，放在 `pos`，人数改成 `count`。
fn enemy_army(state: &mut GameState, template: Id, commander: Id, pos: Hex, count: u32) -> Id {
    let mut a = state.armies[&template].clone();
    a.id = state.alloc_id();
    a.commander = commander;
    a.pos = pos;
    a.dest = None;
    let s = &mut a.squads[0];
    s.gear = Inventory::default();
    s.gear.add("weapon", count as f64);
    s.count = count;
    state.armies.insert(a.id, a.clone());
    a.id
}

/// 复制测试城给别人，整座平移到治所在 `seat`，带城墙。
fn enemy_city(state: &mut GameState, ids: &Ids, owner: Id, seat: Hex) -> Id {
    let origin = state.cities[&ids.city].seat;
    let city = copy_city(state, ids, owner, seat);
    let c = state.cities.get_mut(&city).unwrap();
    let mut cells = Vec::new();
    for t in c.tiles.values_mut() {
        t.pos = Hex::new(t.pos.q + seat.q - origin.q, t.pos.r + seat.r - origin.r);
        cells.push((t.pos, t.terrain.clone()));
    }
    let first = c.tiles.values_mut().next().unwrap();
    first.buildings.push(Building {
        kind: "city_wall".into(),
        mode: Mode::Guan,
    });
    for (pos, terrain) in cells {
        state.map.insert(pos, terrain);
    }
    city
}

#[test]
fn armies_fight_like_civilization() {
    let config = config();
    let (mut state, ids, army, other) = war_on_corridor(&config);
    state.armies.get_mut(&army).unwrap().pos = Hex::new(1, 0);
    let enemy = enemy_army(&mut state, army, other, Hex::new(2, 0), 30);
    let attack = |at, ranged| Command::Attack { army, at, ranged };

    // 远了打不到；没有弓矢不能远程
    assert_eq!(
        apply(
            &mut state,
            &config,
            ids.player,
            &attack(Hex::new(3, 0), false)
        ),
        Err(CommandError::OutOfRange(Hex::new(3, 0)))
    );
    assert_eq!(
        apply(
            &mut state,
            &config,
            ids.player,
            &attack(Hex::new(2, 0), true)
        ),
        Err(CommandError::CannotAttack(army))
    );
    // 近战：50 点攻击杀 5 人，对方 30 点防御还手杀 3 人
    apply(
        &mut state,
        &config,
        ids.player,
        &attack(Hex::new(2, 0), false),
    )
    .unwrap();
    assert_eq!(state.armies[&army].size(), 47);
    assert_eq!(state.armies[&enemy].size(), 25);
    assert_eq!(state.armies[&enemy].squads[0].gear.get("weapon"), 25.0);
    // 每回合只能攻击一次，攻击后不能再走
    assert_eq!(state.armies[&army].moves_left, 0);
    assert_eq!(
        apply(
            &mut state,
            &config,
            ids.player,
            &attack(Hex::new(2, 0), false)
        ),
        Err(CommandError::AlreadyActed(army))
    );
    let report = end_turn_fed(&mut state, &config);
    assert!(
        report.events.iter().any(|e| e.text.contains("杀敌 5 人")),
        "{:?}",
        report.events
    );

    // 远程：带弓矢的步兵隔一格射击，对方不能还手；守在山林里防御更高，但远程不看防御
    state.armies.get_mut(&army).unwrap().squads[0]
        .gear
        .add("bow", 47.0);
    state.armies.get_mut(&army).unwrap().pos = Hex::new(0, 0);
    state.map.insert(Hex::new(2, 0), "forest");
    apply(
        &mut state,
        &config,
        ids.player,
        &attack(Hex::new(2, 0), true),
    )
    .unwrap();
    assert_eq!(state.armies[&army].size(), 47);
    assert_eq!(state.armies[&enemy].size(), 20);

    // 没在交战的人打不了
    state.set_war(ids.player, other, false);
    end_turn_fed(&mut state, &config);
    assert_eq!(
        apply(
            &mut state,
            &config,
            ids.player,
            &attack(Hex::new(2, 0), true)
        ),
        Err(CommandError::NothingToAttack(Hex::new(2, 0)))
    );

    // 打光了就全军覆没
    state.set_war(ids.player, other, true);
    state.armies.get_mut(&enemy).unwrap().squads[0].count = 3;
    apply(
        &mut state,
        &config,
        ids.player,
        &attack(Hex::new(2, 0), true),
    )
    .unwrap();
    assert!(!state.armies.contains_key(&enemy));

    // 行军途中走到敌军旁边，停下直接开打
    end_turn_fed(&mut state, &config);
    state.map.insert(Hex::new(2, 0), "farmland");
    let enemy = enemy_army(&mut state, army, other, Hex::new(4, 0), 30);
    apply(
        &mut state,
        &config,
        ids.player,
        &Command::MarchArmy {
            army,
            to: Hex::new(5, 0),
            force: false,
        },
    )
    .unwrap();
    // 走到 (2,0) 时敌军进入视野，停下待命
    let a = &state.armies[&army];
    assert_eq!((a.pos, a.dest, a.attacked), (Hex::new(2, 0), None, false));
    // 重新下令：看得到的敌军不会再让它停下
    apply(
        &mut state,
        &config,
        ids.player,
        &Command::MarchArmy {
            army,
            to: Hex::new(5, 0),
            force: false,
        },
    )
    .unwrap();
    assert_eq!(state.armies[&army].dest, Some(Hex::new(5, 0)));
    // 新回合接着走，走到 (3,0) 和敌军相邻，停下开打
    end_turn_fed(&mut state, &config);
    let a = &state.armies[&army];
    assert_eq!((a.pos, a.dest, a.attacked), (Hex::new(3, 0), None, true));
    assert!(state.armies[&enemy].size() < 30);
}

#[test]
fn cities_fall_after_the_wall_and_the_center_are_broken() {
    let config = config();
    let (mut state, ids, army, other) = war_on_corridor(&config);
    let city = enemy_city(&mut state, &ids, other, Hex::new(5, 0));
    let garrison = enemy_army(&mut state, army, other, Hex::new(5, 0), 20);
    let inner = enemy_army(&mut state, army, other, Hex::new(5, 1), 10);
    apply(
        &mut state,
        &config,
        other,
        &Command::ManWall {
            army: garrison,
            on: true,
        },
    )
    .unwrap();
    state.armies.get_mut(&army).unwrap().pos = Hex::new(3, 0);
    let attack = |at, ranged| Command::Attack { army, at, ranged };

    // 城墙立着只能打城墙：隔一格用攻城机，每架打掉 5 点；墙上守军没有弓矢，打不着远处的人
    apply(
        &mut state,
        &config,
        ids.player,
        &attack(Hex::new(5, 0), true),
    )
    .unwrap();
    assert_eq!(state.cities[&city].wall_damage, 5.0);
    assert_eq!(state.armies[&army].size(), 50);
    assert_eq!(state.armies[&garrison].size(), 20);
    // 墙还立着，墙里的军队打不到，城中心也打不到
    let report = end_turn_fed(&mut state, &config);
    assert!(
        report
            .cities
            .iter()
            .any(|c| c.id == city && c.wall == Some(195.0))
    );

    // 近战打城墙，墙上守军还手：20 人 × 防御 1 × 城墙加成 2 × 0.1 = 杀 4 人
    state.armies.get_mut(&army).unwrap().pos = Hex::new(4, 0);
    apply(
        &mut state,
        &config,
        ids.player,
        &attack(Hex::new(5, 0), false),
    )
    .unwrap();
    assert_eq!(state.armies[&army].size(), 46);

    // 城墙这回合挨了打，修不了
    let city_mut = state.cities.get_mut(&city).unwrap();
    city_mut.manager = Some(other);
    let shi = *city_mut.officials.keys().next().unwrap();
    city_mut
        .groups
        .values_mut()
        .for_each(|g| g.skills = vec!["building".into()]);
    let builders = *city_mut.groups.keys().next().unwrap();
    apply(
        &mut state,
        &config,
        other,
        &Command::StartAssignment {
            city,
            shi,
            task: Task::RepairWall,
            workers: workers(&[(builders, 40)]),
        },
    )
    .unwrap();
    let damage = state.cities[&city].wall_damage;
    end_turn_fed(&mut state, &config);
    assert_eq!(state.cities[&city].wall_damage, damage);
    // 没挨打的回合就能修
    end_turn_fed(&mut state, &config);
    assert!(state.cities[&city].wall_damage < damage);
    state.cities.get_mut(&city).unwrap().assignments.clear();

    // 城墙被攻破：墙上的守军随之覆灭
    state.cities.get_mut(&city).unwrap().wall_damage = 199.9;
    apply(
        &mut state,
        &config,
        ids.player,
        &attack(Hex::new(5, 0), false),
    )
    .unwrap();
    assert!(!state.armies.contains_key(&garrison));
    end_turn_fed(&mut state, &config);
    // 墙里还有守军，要先打败他们
    assert_eq!(
        apply(
            &mut state,
            &config,
            ids.player,
            &attack(Hex::new(5, 0), false)
        ),
        Err(CommandError::GarrisonRemains(city))
    );
    state.armies.remove(&inner);
    // 城中心 20 点，每点攻击打掉 0.1：破墙时又折了 4 人，42 人每次打掉 4.2，打满 20 城被攻占，攻方进城
    apply(
        &mut state,
        &config,
        ids.player,
        &attack(Hex::new(5, 0), false),
    )
    .unwrap();
    assert!((state.cities[&city].center_damage - 4.2).abs() < 1e-9);
    state.cities.get_mut(&city).unwrap().center_damage = 19.0;
    end_turn_fed(&mut state, &config);
    apply(
        &mut state,
        &config,
        ids.player,
        &attack(Hex::new(5, 0), false),
    )
    .unwrap();
    let c = &state.cities[&city];
    assert_eq!(c.status, zhou_core::state::CityStatus::Occupied);
    // 原来的管理者失去管理权，占领者要选人接管
    assert_eq!(
        (c.owner, c.occupier, c.manager),
        (Some(other), Some(ids.player), None)
    );
    assert_eq!(state.armies[&army].pos, Hex::new(5, 0));
    let appoint = |appointee| Command::AppointOccupationManager { city, appointee };
    // 自己已经在管别的城，不能再管；别人不能替占领者选人
    assert_eq!(
        apply(
            &mut state,
            &config,
            ids.player,
            &appoint(zhou_core::Appointee::Character { id: ids.player })
        ),
        Err(CommandError::ManagesAnotherCity(ids.player))
    );
    assert_eq!(
        apply(
            &mut state,
            &config,
            other,
            &appoint(zhou_core::Appointee::Character { id: other })
        ),
        Err(CommandError::NotOccupier(city))
    );
    // 从自己管的城里派一名士人：他出仕成为人物，接管这座城
    let shi = Command::AppointOccupationManager {
        city,
        appointee: zhou_core::Appointee::Shi {
            city: ids.city,
            shi: ids.shi_b,
        },
    };
    let steward = apply(&mut state, &config, ids.player, &shi).unwrap()[0];
    let c = &state.cities[&city];
    assert_eq!(c.manager, Some(steward));
    assert_eq!(state.characters[&steward].name, "士乙");
    assert!(!state.cities[&ids.city].officials.contains_key(&ids.shi_b));
    // 接管的人风评 0：城中人群对他的忠诚度从 300 起
    assert!(c.groups.values().all(|g| (g.loyalty - 300.0).abs() < 1e-9));

    // 劫掠：按物品类别挑着抢，不挑就全拿；每座城每回合只能劫掠一次
    let c = state.cities.get_mut(&city).unwrap();
    c.treasury.add("weapon", 10.0);
    c.private_wealth = Inventory::default();
    c.private_wealth.add("coin", 50.0);
    c.private_wealth.add("grain", 50.0);
    let treasury = state.cities[&city].treasury.clone();
    let plunder = |target, categories: &[&str]| Command::Plunder {
        city,
        target,
        categories: categories.iter().map(|c| c.to_string()).collect(),
    };
    use zhou_core::PlunderTarget::{Citizens, Treasury};
    assert_eq!(
        apply(&mut state, &config, other, &plunder(Treasury, &[])),
        Err(CommandError::NotOccupier(city))
    );
    assert_eq!(
        apply(
            &mut state,
            &config,
            ids.player,
            &plunder(Treasury, &["gold"])
        ),
        Err(CommandError::NoSuchCategory("gold".into()))
    );
    apply(
        &mut state,
        &config,
        ids.player,
        &plunder(Treasury, &["food", "military"]),
    )
    .unwrap();
    let me = &state.characters[&ids.player];
    assert_eq!(me.private_store.get("grain"), treasury.get("grain"));
    assert_eq!(me.private_store.get("weapon"), treasury.get("weapon"));
    let c = &state.cities[&city];
    assert_eq!(c.treasury.get("grain"), 0.0);
    assert_eq!(c.treasury.get("bronze"), treasury.get("bronze"));
    assert_eq!(
        apply(&mut state, &config, ids.player, &plunder(Citizens, &[])),
        Err(CommandError::AlreadyPlundered(city))
    );
    // 下一回合再抢市民：只抢钱币，占民间财富价值的一半，代价也只算一半
    state.cities.get_mut(&city).unwrap().plundered = false;
    apply(
        &mut state,
        &config,
        ids.player,
        &plunder(Citizens, &["money"]),
    )
    .unwrap();
    let me = &state.characters[&ids.player];
    assert_eq!(me.private_store.get("coin"), 50.0);
    assert_eq!(me.reputation, -5.0);
    let c = &state.cities[&city];
    assert_eq!(c.private_wealth.get("grain"), 50.0);
    assert!(c.groups.values().all(|g| (g.loyalty - 150.0).abs() < 1e-9));
    // 毁掉城墙
    let tile = *c.tiles.keys().next().unwrap();
    apply(
        &mut state,
        &config,
        ids.player,
        &Command::Raze {
            city,
            tile,
            building: "city_wall".into(),
        },
    )
    .unwrap();
    assert_eq!(state.cities[&city].wall_hp(&config), None);

    // 原主打回来就收复
    state.armies.get_mut(&army).unwrap().pos = Hex::new(3, 0);
    let back = enemy_army(&mut state, army, other, Hex::new(4, 0), 300);
    end_turn(&mut state, &config);
    apply(
        &mut state,
        &config,
        other,
        &Command::Attack {
            army: back,
            at: Hex::new(5, 0),
            ranged: false,
        },
    )
    .unwrap();
    let c = &state.cities[&city];
    assert_eq!(c.status, zhou_core::state::CityStatus::Ruled);
    assert_eq!(c.manager, Some(other));
}

struct Demo {
    state: GameState,
    king: Id,
    player: Id,
    steward: Id,
    wei_bo: Id,
    zhou: Id,
    wei: Id,
}

fn demo_world(config: &Config) -> Demo {
    let state = from_toml(include_str!("../../../scenarios/demo.toml"), config).unwrap();
    let person = |name: &str| {
        *state
            .characters
            .iter()
            .find(|(_, c)| c.name == name)
            .unwrap()
            .0
    };
    let country = |name: &str| {
        *state
            .countries
            .iter()
            .find(|(_, c)| c.name == name)
            .unwrap()
            .0
    };
    Demo {
        king: person("周王"),
        player: person("姬虎"),
        steward: person("史伯"),
        wei_bo: person("卫伯"),
        zhou: country("周"),
        wei: country("卫"),
        state,
    }
}

#[test]
fn only_rulers_declare_war_on_countries_and_unjustified_wars_cost_reputation() {
    let config = config();
    let Demo {
        mut state,
        king,
        player,
        wei_bo,
        zhou,
        wei,
        ..
    } = demo_world(&config);
    let declare = |target| Command::DeclareWar { target };
    let country = |id| zhou_core::Party::Country { id };
    let person = |id| zhou_core::Party::Person { id };

    // 封臣不能对别国宣战，对别国的人宣战也不行
    assert_eq!(
        apply(&mut state, &config, player, &declare(country(zhou))),
        Err(CommandError::VassalCannotDeclare)
    );
    assert_eq!(
        apply(&mut state, &config, player, &declare(person(king))),
        Err(CommandError::VassalCannotDeclare)
    );
    assert_eq!(
        apply(&mut state, &config, wei_bo, &declare(country(wei))),
        Err(CommandError::OwnCountry)
    );

    // 国君对别国宣战：没有理由扣 20 风评；两国的人都卷进来，国内的城对本国的人开放
    let war = apply(&mut state, &config, wei_bo, &declare(person(king))).unwrap()[0];
    assert_eq!(state.characters[&wei_bo].reputation, 280.0);
    assert!(state.at_war(player, king));
    assert!(!state.at_war(player, wei_bo));
    assert!(state.countries[&zhou].national_war && state.countries[&wei].national_war);
    assert_eq!(
        apply(&mut state, &config, wei_bo, &declare(country(zhou))),
        Err(CommandError::AlreadyAtWar)
    );

    // 无国籍的人打某国的人，就是和这个国家开战，不用理由
    let drifter = state.alloc_id();
    state
        .characters
        .insert(drifter, Character::new(drifter, "游士", "none", 50.0));
    apply(&mut state, &config, drifter, &declare(person(player))).unwrap();
    assert!(state.at_war(drifter, wei_bo));
    assert_eq!(state.characters[&drifter].reputation, 50.0);

    // 讲和：周王投降，交出私库里的钱；卫伯接受，战争结束，12 回合内不能再开战
    state
        .characters
        .get_mut(&king)
        .unwrap()
        .private_store
        .add("coin", 100.0);
    let give = zhou_core::Stake {
        private: Inventory(BTreeMap::from([("coin".into(), 100.0)])),
        ..Default::default()
    };
    assert_eq!(
        apply(
            &mut state,
            &config,
            player,
            &Command::ProposePeace {
                war,
                give: give.clone(),
                take: Default::default()
            }
        ),
        Err(CommandError::NotWarLeader(war))
    );
    apply(
        &mut state,
        &config,
        king,
        &Command::ProposePeace {
            war,
            give,
            take: Default::default(),
        },
    )
    .unwrap();
    apply(&mut state, &config, wei_bo, &Command::AcceptPeace { war }).unwrap();
    assert!(!state.at_war(player, king));
    assert_eq!(state.characters[&wei_bo].private_store.get("coin"), 100.0);
    assert!(!state.countries[&zhou].national_war);
    assert_eq!(
        apply(&mut state, &config, wei_bo, &declare(country(zhou))),
        Err(CommandError::Truce)
    );
    for _ in 0..config.war.truce_turns {
        end_turn(&mut state, &config);
    }
    apply(&mut state, &config, wei_bo, &declare(country(zhou))).unwrap();
}

#[test]
fn rulers_halt_feuds_and_hand_out_tasks_and_disobedience_becomes_a_grievance() {
    let config = config();
    let Demo {
        mut state,
        king,
        player,
        steward,
        wei_bo,
        ..
    } = demo_world(&config);
    let declare = |target| Command::DeclareWar { target };
    let person = |id| zhou_core::Party::Person { id };

    // 同一国的封臣之间可以开打，但要理由
    let feud = apply(&mut state, &config, player, &declare(person(steward))).unwrap()[0];
    assert_eq!(state.characters[&player].reputation, 480.0);
    assert!(state.at_war(player, steward) && !state.at_war(player, wei_bo));
    // 只有国君能制止
    assert_eq!(
        apply(&mut state, &config, king, &Command::HaltWar { war: feud }),
        Err(CommandError::NotRuler)
    );
    apply(&mut state, &config, wei_bo, &Command::HaltWar { war: feud }).unwrap();

    // 制止后还动手：扣风评，成了国君征讨他的理由
    let bei = *state.cities.iter().find(|(_, c)| c.name == "邶").unwrap().0;
    let group = *state.cities[&bei]
        .groups
        .iter()
        .find(|(_, g)| g.name == "东郭农人")
        .unwrap()
        .0;
    state
        .cities
        .get_mut(&bei)
        .unwrap()
        .groups
        .get_mut(&group)
        .unwrap()
        .skills
        .push("martial".into());
    let army = apply(
        &mut state,
        &config,
        player,
        &Command::RaiseArmy {
            city: bei,
            squads: vec![squad(group, "infantry", 20, &[])],
        },
    )
    .unwrap()[0];
    let seat = state.armies[&army].pos;
    let next = seat
        .neighbors()
        .find(|h| state.map.get(*h).is_some())
        .unwrap();
    let rival = enemy_army(&mut state, army, steward, next, 10);
    apply(
        &mut state,
        &config,
        player,
        &Command::Attack {
            army,
            at: next,
            ranged: false,
        },
    )
    .unwrap();
    assert!(state.armies[&rival].size() < 10);
    assert_eq!(state.characters[&player].reputation, 470.0);
    let reasons = zhou_core::war::reasons(&state, &config, wei_bo, person(player));
    assert!(
        reasons.contains(&zhou_core::fief::Grievance::Defiance),
        "{reasons:?}"
    );
    // 有了理由，国君征讨封臣不扣风评；征讨封臣也是国战，其他封臣站在国君一边
    let punish = apply(&mut state, &config, wei_bo, &declare(person(player))).unwrap()[0];
    assert_eq!(state.characters[&wei_bo].reputation, 300.0);
    assert!(state.at_war(steward, player) && state.at_war(wei_bo, player));
    assert!(!state.at_war(steward, wei_bo));
    apply(
        &mut state,
        &config,
        player,
        &Command::ProposePeace {
            war: punish,
            give: Default::default(),
            take: Default::default(),
        },
    )
    .unwrap();
    apply(
        &mut state,
        &config,
        wei_bo,
        &Command::AcceptPeace { war: punish },
    )
    .unwrap();

    // 国战中给封臣定任务：讲和时还没去打，扣风评
    let war = apply(&mut state, &config, wei_bo, &declare(person(king))).unwrap()[0];
    let chengzhou = *state
        .cities
        .iter()
        .find(|(_, c)| c.name == "成周")
        .unwrap()
        .0;
    assert_eq!(
        apply(
            &mut state,
            &config,
            wei_bo,
            &Command::AssignMilitaryTask {
                war,
                vassal: player,
                city: bei
            }
        ),
        Err(CommandError::NotEnemyCity(bei))
    );
    apply(
        &mut state,
        &config,
        wei_bo,
        &Command::AssignMilitaryTask {
            war,
            vassal: player,
            city: chengzhou,
        },
    )
    .unwrap();
    let before = state.characters[&player].reputation;
    apply(
        &mut state,
        &config,
        king,
        &Command::ProposePeace {
            war,
            give: Default::default(),
            take: Default::default(),
        },
    )
    .unwrap();
    apply(&mut state, &config, wei_bo, &Command::AcceptPeace { war }).unwrap();
    assert_eq!(
        state.characters[&player].reputation,
        before - config.war.task_penalty
    );
}

#[test]
fn forcing_through_a_city_declares_war_and_peace_returns_or_cedes_occupied_cities() {
    let config = config();
    let (mut state, ids, army, other) = war_on_corridor(&config);
    state.set_war(ids.player, other, false);
    let city = enemy_city(&mut state, &ids, other, Hex::new(3, -1));
    // 去掉城墙，把城的一块地放在走廊上
    let c = state.cities.get_mut(&city).unwrap();
    c.tiles.values_mut().for_each(|t| t.buildings.clear());
    let tile = *c.tiles.keys().next().unwrap();
    c.tiles.get_mut(&tile).unwrap().pos = Hex::new(3, 0);
    let march = |force| Command::MarchArmy {
        army,
        to: Hex::new(5, 0),
        force,
    };
    assert_eq!(
        apply(&mut state, &config, ids.player, &march(false)),
        Err(CommandError::NoPath(Hex::new(5, 0)))
    );
    // 硬闯：走进别人的城就是宣战，无国籍的人之间不用理由
    apply(&mut state, &config, ids.player, &march(true)).unwrap();
    end_turn(&mut state, &config);
    assert!(state.at_war(ids.player, other));
    assert_eq!(state.city_at(state.armies[&army].pos), Some(city));
    assert_eq!(state.characters[&ids.player].reputation, 0.0);

    // 讲和：割让的城归攻占者，不再算被攻占；没割让的还给原主
    let kept = enemy_city(&mut state, &ids, other, Hex::new(9, 0));
    for id in [city, kept] {
        let c = state.cities.get_mut(&id).unwrap();
        c.status = zhou_core::state::CityStatus::Occupied;
        c.occupier = Some(ids.player);
        c.manager = None;
    }
    let war = state.war_between(ids.player, other).unwrap();
    let give = zhou_core::Stake {
        cities: vec![city],
        ..Default::default()
    };
    apply(
        &mut state,
        &config,
        other,
        &Command::ProposePeace {
            war,
            give,
            take: Default::default(),
        },
    )
    .unwrap();
    apply(
        &mut state,
        &config,
        ids.player,
        &Command::AcceptPeace { war },
    )
    .unwrap();
    let ceded = &state.cities[&city];
    assert_eq!(
        (ceded.owner, ceded.status, ceded.occupier),
        (Some(ids.player), zhou_core::state::CityStatus::Ruled, None)
    );
    let back = &state.cities[&kept];
    assert_eq!(
        (back.owner, back.status, back.manager),
        (
            Some(other),
            zhou_core::state::CityStatus::Ruled,
            Some(other)
        )
    );
}

/// 结束回合，但不让军队断粮：只考作战的测试用。
fn end_turn_fed(state: &mut GameState, config: &Config) -> zhou_core::TurnReport {
    let report = end_turn(state, config);
    for army in state.armies.values_mut() {
        army.hunger = 0;
    }
    report
}

#[test]
fn campaigning_armies_live_on_supply_lines_and_starve_without_them() {
    let config = config();
    let (mut state, ids, army) = army_on_corridor(&config, &["infantry"]);
    state
        .cities
        .get_mut(&ids.city)
        .unwrap()
        .treasury
        .add("grain", 500.0);
    let farmers = |s: &GameState| s.cities[&ids.city].groups[&ids.farmers].size;
    let power = state.armies[&army].attack(&config);

    // 在自己的城里由城供养，不用粮道
    end_turn(&mut state, &config);
    assert_eq!(state.armies[&army].hunger, 0);

    // 出征没有粮道：断粮，战斗力下降
    state.armies.get_mut(&army).unwrap().pos = Hex::new(5, 0);
    let report = end_turn(&mut state, &config);
    assert!(
        report
            .events
            .iter()
            .any(|e| e.text.contains("断粮：没有粮道"))
    );
    assert!(report.armies[0].on_campaign);
    assert_eq!(state.armies[&army].hunger, 1);
    assert!((state.armies[&army].attack(&config) - power * 0.9).abs() < 1e-9);

    // 开粮道：从城里抽 2 名辎重兵，撤掉就回来
    let open = Command::OpenSupplyLine {
        army,
        from: ids.city,
        group: ids.farmers,
    };
    let before = farmers(&state);
    apply(&mut state, &config, ids.player, &open).unwrap();
    assert_eq!(farmers(&state), before - 2);
    assert_eq!(
        apply(&mut state, &config, ids.player, &open),
        Err(CommandError::SupplyLineExists(army))
    );
    apply(
        &mut state,
        &config,
        ids.player,
        &Command::CloseSupplyLine { army },
    )
    .unwrap();
    assert_eq!(farmers(&state), before);
    apply(&mut state, &config, ids.player, &open).unwrap();

    // 供上粮，战斗力直接回满；粮从起点城的府库出，粮道越长耗得越多
    let report = end_turn(&mut state, &config);
    assert_eq!(state.armies[&army].hunger, 0);
    assert!((state.armies[&army].attack(&config) - power).abs() < 1e-9);
    let line = report.armies[0].supply.as_ref().unwrap();
    assert_eq!(line.convoy, 2);
    assert_eq!(line.path.last(), Some(&Hex::new(5, 0)));
    assert!(line.path.contains(&Hex::new(3, 0)));

    // 找不到路的地方开不了粮道
    let mut lost = state.clone();
    let a = lost.armies.get_mut(&army).unwrap();
    a.supply = None;
    a.pos = Hex::new(9, 9);
    assert_eq!(
        apply(&mut lost, &config, ids.player, &open),
        Err(CommandError::NoSupplyRoute(army))
    );

    // 敌军袭击粮道经过的格子：辎重兵全被杀，前线断粮
    let other = state.alloc_id();
    state
        .characters
        .insert(other, Character::new(other, "敌人", "none", 0.0));
    state.set_war(ids.player, other, true);
    let raider = enemy_army(&mut state, army, other, Hex::new(3, -1), 10);
    let size = farmers(&state);
    apply(
        &mut state,
        &config,
        other,
        &Command::Attack {
            army: raider,
            at: Hex::new(3, 0),
            ranged: false,
        },
    )
    .unwrap();
    assert!(state.armies[&army].supply.is_none());
    assert_eq!(farmers(&state), size);
    let report = end_turn(&mut state, &config);
    assert!(report.events.iter().any(|e| e.text.contains("被袭")));
    assert_eq!(state.armies[&army].hunger, 1);

    // 断得越久降得越快：90%、70%、40%，第 4 回合溃散
    end_turn(&mut state, &config);
    assert!((state.armies[&army].fed_mult(&config) - 0.7).abs() < 1e-9);
    end_turn(&mut state, &config);
    assert!((state.armies[&army].fed_mult(&config) - 0.4).abs() < 1e-9);
    let report = end_turn(&mut state, &config);
    assert!(!state.armies.contains_key(&army));
    assert!(report.events.iter().any(|e| e.text.contains("就地溃散")));
}

#[test]
fn players_only_see_what_is_in_their_sight() {
    let config = config();
    let (mut state, ids, army, other) = war_on_corridor(&config);
    let city = enemy_city(&mut state, &ids, other, Hex::new(8, 0));
    let enemy = enemy_army(&mut state, army, other, Hex::new(5, 0), 30);
    let me = ids.player;
    assert!(!zhou_core::vision::vision(&state, &config, me).contains(&Hex::new(5, 0)));

    // 看不到的敌军、敌城不在报告里；格子属于哪座城也不知道；别人的消息看不到
    let report = end_turn_fed(&mut state, &config);
    let mine = zhou_core::player_report(&state, &config, &report, me);
    assert!(mine.foreign_armies.is_empty());
    assert!(mine.foreign_cities.is_empty());
    assert!(mine.cities.iter().all(|c| c.id != city));
    assert!(mine.map.iter().all(|c| c.city != Some(city)));
    let theirs = zhou_core::player_report(&state, &config, &report, other);
    assert_eq!(theirs.cities.len(), 1);
    assert!(theirs.armies.iter().any(|a| a.id == enemy));
    let private: Vec<_> = report
        .events
        .iter()
        .filter(|e| !e.public && !e.to.contains(&me) && e.at.is_none())
        .collect();
    assert!(!private.is_empty());
    assert!(private.iter().all(|e| !mine.events.contains(e)));
    assert!(mine.wars.len() == 1, "战争是公开的");

    // 军队走近：敌军进入视野看得到人数；敌城看得到外观，看不到内情
    state.armies.get_mut(&army).unwrap().pos = Hex::new(6, 0);
    let report = end_turn_fed(&mut state, &config);
    let mine = zhou_core::player_report(&state, &config, &report, me);
    let seen = &mine.foreign_armies[0];
    assert!(seen.visible);
    assert_eq!(seen.sighting.pos, Hex::new(5, 0));
    assert_eq!(seen.sighting.units["infantry"], 30);
    let c = mine
        .foreign_cities
        .iter()
        .find(|c| c.sighting.id == city)
        .unwrap();
    assert!(c.visible && c.sighting.wall);
    assert_eq!(c.sighting.size, "小邑");
    assert_eq!(c.sighting.owner, Some(other));
    assert!(mine.cities.iter().all(|c| c.id != city));
    assert!(mine.map.iter().any(|c| c.city == Some(city)));

    // 离开视野：只记得最后一次看到的位置和时间
    let seen_turn = seen.sighting.turn;
    state.armies.get_mut(&army).unwrap().pos = Hex::new(0, 0);
    state.armies.get_mut(&enemy).unwrap().pos = Hex::new(6, 0);
    let report = end_turn_fed(&mut state, &config);
    let mine = zhou_core::player_report(&state, &config, &report, me);
    let seen = &mine.foreign_armies[0];
    assert!(!seen.visible);
    assert_eq!(
        (seen.sighting.pos, seen.sighting.turn),
        (Hex::new(5, 0), seen_turn)
    );
    assert!(!mine.foreign_cities[0].visible);

    // 回去一看人不在那儿了，就忘掉
    state.armies.get_mut(&enemy).unwrap().pos = Hex::new(9, 3);
    state.armies.get_mut(&army).unwrap().pos = Hex::new(3, 0);
    let report = end_turn_fed(&mut state, &config);
    let mine = zhou_core::player_report(&state, &config, &report, me);
    assert!(mine.foreign_armies.is_empty());
}
