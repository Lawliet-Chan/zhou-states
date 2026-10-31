//! 指令：玩家和 AI 的所有操作都通过这里，先校验，再执行。

use crate::fx::Fx;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::army::{self, SquadOrder};
use crate::combat::{self, Appointee, PlunderTarget};
use crate::config::Config;
use crate::deal::{self, Deal, Stake};
use crate::fief;
use crate::inventory::Inventory;
use crate::map::Hex;
use crate::state::{
    Assignment, City, CityStatus, GameState, Id, Learning, Mode, Official, Quota, Task, Tribute,
};
use crate::war::{self, Party};

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
    DismissShi {
        city: Id,
        shi: Id,
    },
    /// 派士人主事一项工事
    StartAssignment {
        city: Id,
        shi: Id,
        task: Task,
        #[serde(default, deserialize_with = "deserialize_workers")]
        workers: BTreeMap<Id, u32>,
    },
    CancelAssignment {
        city: Id,
        assignment: Id,
    },
    SetTaxRate {
        city: Id,
        rate: Fx,
    },
    /// 赏赐民众：把府库中的物资转入民间财富
    Grant {
        city: Id,
        goods: Inventory,
    },
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
    OfferManagement {
        city: Id,
        to: Id,
    },
    /// 被任命的人接受任命，成为管理者
    AcceptManagement {
        city: Id,
    },
    /// 被任命的人拒绝任命
    DeclineManagement {
        city: Id,
    },
    /// 所有者收回管理权，自己管理。不需要管理者同意
    RevokeManagement {
        city: Id,
    },
    /// 所有者给管理者设定规定任务：每年要上缴的物资。为空表示取消
    SetQuota {
        city: Id,
        goods: Inventory,
    },
    /// 管理者从府库上缴物资到所有者的私库，计入今年的规定任务
    Deliver {
        city: Id,
        goods: Inventory,
    },
    /// 所有者把城市封授给别人，对方接受后才生效。封出去的城不能收回
    OfferFief {
        city: Id,
        to: Id,
    },
    /// 受封：成为这座城的所有者，原所有者成为封君
    AcceptFief {
        city: Id,
    },
    DeclineFief {
        city: Id,
    },
    /// 封君指定每年的纳贡，取代按比例自动纳贡。为空表示恢复自动纳贡
    SetTribute {
        city: Id,
        goods: Inventory,
    },
    /// 封臣拒绝过火的纳贡
    RefuseTribute {
        city: Id,
    },
    /// 封臣从府库纳贡到封君的私库，计入今年指定的纳贡
    PayTribute {
        city: Id,
        goods: Inventory,
    },
    /// 向某人提出赠与（只给）、索取（只要）或交易（有给有要），对方接受就立即成交。
    /// `into`：自己收到的物资放进哪座城的府库，不填为私库
    ProposeDeal {
        to: Id,
        #[serde(default)]
        give: Stake,
        #[serde(default)]
        take: Stake,
        #[serde(default)]
        into: Option<Id>,
    },
    /// 接受提议，立即成交。`into`：收到的物资放进哪座城的府库，不填为私库
    AcceptDeal {
        deal: Id,
        #[serde(default)]
        into: Option<Id>,
    },
    DeclineDeal {
        deal: Id,
    },
    /// 提议的人撤回
    CancelDeal {
        deal: Id,
    },
    /// 搬去另一座城待着。平时只能搬去自己拥有的城
    MoveTo {
        city: Id,
    },
    /// 征发：城主从城里会「军武」的人中抽人编成一支军队，装备从府库领。城被攻占时由占领者征发
    RaiseArmy {
        city: Id,
        squads: Vec<SquadOrder>,
    },
    /// 行军：只指定目的地，自动按最短路线走，没有通行权的城自动绕开。这回合走不到，之后的回合接着走。
    /// 目的地就是现在的位置时，取消行军
    MarchArmy {
        army: Id,
        to: Hex,
        /// 硬闯：没有通行权的城也穿过去，走进去就是对城主宣战
        #[serde(default)]
        force: bool,
    },
    /// 攻击某一格：敌城的地就攻城（城墙立着时打城墙），有敌军就打敌军。
    /// 近战要相邻；`ranged` 为远程（弓矢、攻城机），隔一格也能打
    Attack {
        army: Id,
        at: Hex,
        #[serde(default)]
        ranged: bool,
    },
    /// 宣战。只有国君能对别国宣战（国战）；对别国的人宣战就是对这个国家宣战。
    /// 国与国之间、同一国的封臣之间没有理由开战扣风评；牵涉无国籍的人不用理由
    DeclareWar {
        target: Party,
    },
    /// 国君制止本国两名封臣之间的战争，封臣再动手就扣风评
    HaltWar {
        war: Id,
    },
    /// 国战中国君给封臣定军事任务：攻打哪座城
    AssignMilitaryTask {
        war: Id,
        vassal: Id,
        city: Id,
    },
    /// 提出讲和：`give` 是自己交出的，`take` 是要对方交出的。只给不要就是投降。当回合有效
    ProposePeace {
        war: Id,
        #[serde(default)]
        give: crate::deal::Stake,
        #[serde(default)]
        take: crate::deal::Stake,
    },
    /// 接受讲和：交割条件，战争结束，之后若干回合不能再开战
    AcceptPeace {
        war: Id,
    },
    DeclinePeace {
        war: Id,
    },
    /// 进驻或离开城墙：墙上的守军能还手，城墙一破就随之覆灭
    ManWall {
        army: Id,
        on: bool,
    },
    /// 开粮道：从起点城 `from` 的一群会「军武」的人里抽出辎重兵，给出征的军队供粮
    OpenSupplyLine {
        army: Id,
        from: Id,
        group: Id,
    },
    /// 撤粮道：辎重兵回起点城
    CloseSupplyLine {
        army: Id,
    },
    /// 遣散：军队回到编成它的城的地上就能遣散，人回原来的岗位，装备还回府库
    DisbandArmy {
        army: Id,
    },
    /// 占领者选人接管被攻占的城：一个人物，或者自己管的城里的一名士人
    AppointOccupationManager {
        city: Id,
        appointee: Appointee,
    },
    /// 占领者劫掠府库或民间的财物：`categories` 是要抢的物品类别，不写就全拿。每座城每回合只能劫掠一次
    Plunder {
        city: Id,
        target: PlunderTarget,
        #[serde(default)]
        categories: Vec<String>,
    },
    /// 占领者毁掉一块地上的一座建筑
    Raze {
        city: Id,
        tile: Id,
        building: String,
    },
    /// 丢弃技能、调整技能顺序，当回合生效。`skills` 只能由这群人现有的技能组成，顺序即优先顺序。
    /// 正在学技能的人要等学成再调整
    ArrangeSkills {
        city: Id,
        group: Id,
        count: u32,
        skills: Vec<String>,
    },
}

/// 内部标记枚举会先缓存 JSON 对象，数字键此时已成为字符串，需要显式读回人群编号。
fn deserialize_workers<'de, D>(deserializer: D) -> Result<BTreeMap<Id, u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw = BTreeMap::<String, u32>::deserialize(deserializer)?;
    raw.into_iter()
        .map(|(group, count)| {
            group
                .parse::<Id>()
                .map(|id| (id, count))
                .map_err(serde::de::Error::custom)
        })
        .collect()
}

impl Command {
    /// 指令针对的城市；赠与、索取、交易是人与人之间的，搬家是人物自己的事，都没有城市。
    pub fn city(&self) -> Option<Id> {
        Some(match self {
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
            | Command::Deliver { city, .. }
            | Command::OfferFief { city, .. }
            | Command::AcceptFief { city }
            | Command::DeclineFief { city }
            | Command::SetTribute { city, .. }
            | Command::RefuseTribute { city }
            | Command::PayTribute { city, .. }
            | Command::RaiseArmy { city, .. }
            | Command::Plunder { city, .. }
            | Command::AppointOccupationManager { city, .. }
            | Command::Raze { city, .. } => *city,
            Command::ProposeDeal { .. }
            | Command::AcceptDeal { .. }
            | Command::DeclineDeal { .. }
            | Command::CancelDeal { .. }
            | Command::MoveTo { .. }
            | Command::DisbandArmy { .. }
            | Command::OpenSupplyLine { .. }
            | Command::CloseSupplyLine { .. }
            | Command::MarchArmy { .. }
            | Command::Attack { .. }
            | Command::ManWall { .. }
            | Command::DeclareWar { .. }
            | Command::HaltWar { .. }
            | Command::AssignMilitaryTask { .. }
            | Command::ProposePeace { .. }
            | Command::AcceptPeace { .. }
            | Command::DeclinePeace { .. } => return None,
        })
    }
}

#[derive(Debug, Error, PartialEq)]
pub enum CommandError {
    #[error("国君至少要留一座城在自己手里")]
    RulerMustKeepACity,
    #[error("通行权的期限至少要 1 回合")]
    ZeroPassageTurns,
    #[error("你已经在城市 {0} 了")]
    AlreadyThere(Id),
    #[error("军队里一个兵都没有")]
    EmptyArmy,
    #[error("兵种「{0}」不存在")]
    NoSuchUnit(String),
    #[error("{unit}不能带「{good}」")]
    NotOptionalGear { unit: String, good: String },
    #[error("军队 {0} 不存在")]
    NoSuchArmy(Id),
    #[error("你不统领军队 {0}")]
    NotCommander(Id),
    #[error("走不到 {0}：过不去的地形、没有通行权的城或者别人的军队挡住了路")]
    NoPath(Hex),
    #[error("国家 {0} 不存在")]
    NoSuchCountry(Id),
    #[error("不能对自己或自己的国家宣战")]
    OwnCountry,
    #[error("封臣不能对别国宣战，只有国君才能发动国战")]
    VassalCannotDeclare,
    #[error("已经在和对方交战")]
    AlreadyAtWar,
    #[error("还在停战期内，不能开战")]
    Truce,
    #[error("战争 {0} 不存在")]
    NoSuchWar(Id),
    #[error("你不是国君，或者这不是你国内的事")]
    NotRuler,
    #[error("人物 {0} 不是你国内的封臣")]
    NotYourVassal(Id),
    #[error("城市 {0} 不是敌方的城")]
    NotEnemyCity(Id),
    #[error("你不是战争 {0} 里拿主意的人")]
    NotWarLeader(Id),
    #[error("战争 {0} 没有等你答复的讲和")]
    NoPeaceOffer(Id),
    #[error("军队 {0} 这回合已经攻击过，或者没有行军力了")]
    AlreadyActed(Id),
    #[error("{0} 不在攻击范围内：近战要相邻，远程最多隔一格")]
    OutOfRange(Hex),
    #[error("军队 {0} 没有能这样攻击的兵（远程要带弓矢或攻城机）")]
    CannotAttack(Id),
    #[error("{0} 没有能攻击的敌人")]
    NothingToAttack(Hex),
    #[error("城市 {0} 里还有守军，要先打败他们")]
    GarrisonRemains(Id),
    #[error("你没有攻占城市 {0}")]
    NotOccupier(Id),
    #[error("物品类别「{0}」不存在")]
    NoSuchCategory(String),
    #[error("城市 {0} 这回合已经被劫掠过")]
    AlreadyPlundered(Id),
    #[error("军队 {0} 已经有粮道")]
    SupplyLineExists(Id),
    #[error("军队 {0} 没有粮道")]
    NoSupplyLine(Id),
    #[error("找不到给军队 {0} 送粮的路")]
    NoSupplyRoute(Id),
    #[error("军队 {0} 要回到自己的城里才能遣散")]
    NotHome(Id),
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
    #[error("这座城是封地，不能再封给别人")]
    AlreadyFief,
    #[error("不能把城封给自己")]
    CannotEnfeoffSelf,
    #[error("人物 {0} 是你的上司，不能封给他")]
    CannotEnfeoffSuperior(Id),
    #[error("人物 {0} 是别国的人，要先投奔过来才能受封")]
    DifferentCountry(Id),
    #[error("没有封授这座城给你")]
    NotOfferedFief,
    #[error("城市 {0} 不是封地")]
    NotAFief(Id),
    #[error("你不是城市 {0} 的封君")]
    NotLiege(Id),
    #[error("封君没有指定纳贡")]
    NoTributeDemand,
    #[error("这次纳贡不算过火，不能拒绝")]
    TributeNotExcessive,
    #[error("提议 {0} 不存在")]
    NoSuchDeal(Id),
    #[error("提议 {0} 不归你答复或撤回")]
    NotYourDeal(Id),
    #[error("不能和自己往来")]
    DealWithSelf,
    #[error("什么都没给，也什么都没要")]
    EmptyDeal,
    #[error("私库中的「{0}」不够")]
    NotEnoughInPrivateStore(String),
    #[error("城市 {0} 重复了")]
    DuplicateCity(Id),
    #[error("城市 {0} 是都城，不能送人或拿来交易")]
    CapitalCannotChangeHands(Id),
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
    NotEnoughUnmanaged {
        group: Id,
        available: u32,
        requested: u32,
    },
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
    NotEnoughUntaxed {
        group: Id,
        available: u32,
        requested: u32,
    },
    #[error("祭祀不需要投入人口")]
    SacrificeTakesNoWorkers,
    #[error("府库中没有礼器，无法祭祀")]
    NoVessel,
    #[error("税率必须在 0 到 {0} 之间（千分之一为单位）")]
    InvalidTaxRate(i64),
    #[error("「{pool}」只能再容纳 {available} 人劳作，不足 {requested} 人")]
    NoJobs {
        pool: String,
        available: u32,
        requested: u32,
    },
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
    NotEnoughNotLearning {
        group: Id,
        available: u32,
        requested: u32,
    },
    #[error("这群人已经会「{0}」")]
    AlreadyHasSkill(String),
    #[error("这块地上的建筑已满")]
    NoBuildingSlot,
    #[error("「{0}」不能建在这种地上")]
    WrongTerrain(String),
    #[error("城里没有城墙")]
    NoWall,
    #[error("城墙完好，不用修")]
    WallIntact,
    #[error("已经在修城墙了")]
    AlreadyRepairing,
    #[error("地图上没有 {0} 这一格")]
    NoSuchCell(Hex),
    #[error("{at} 已经是{city}的地")]
    CellTaken { at: Hex, city: String },
    #[error("{0} 不和本城的地相邻")]
    NotBordering(Hex),
    #[error("{at} 是{actual}")]
    TerrainMismatch { at: Hex, actual: String },
    #[error("已经在开垦 {0}")]
    AlreadyReclaiming(Hex),
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
pub fn apply(
    state: &mut GameState,
    config: &Config,
    actor: Id,
    command: &Command,
) -> Result<Vec<Id>, CommandError> {
    let result = apply_inner(state, config, actor, command);
    // 城送了人、封了出去之后，原来待在那里的人回到默认的地方
    if result.is_ok() {
        state.settle_locations();
        crate::vision::observe(state, config);
    }
    result
}

fn apply_inner(
    state: &mut GameState,
    config: &Config,
    actor: Id,
    command: &Command,
) -> Result<Vec<Id>, CommandError> {
    let Some(city_id) = command.city() else {
        return apply_personal(state, config, actor, command);
    };
    let city = state
        .cities
        .get(&city_id)
        .ok_or(CommandError::NoSuchCity(city_id))?;
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
                city.quota = Some(Quota {
                    goods: goods.clone(),
                    delivered,
                });
            }
            Ok(vec![])
        }
        Command::OfferFief { to, .. } => {
            check_owner(city, actor)?;
            if city.liege.is_some() {
                return Err(CommandError::AlreadyFief);
            }
            if *to == actor {
                return Err(CommandError::CannotEnfeoffSelf);
            }
            let target = state
                .characters
                .get(to)
                .ok_or(CommandError::NoSuchCharacter(*to))?;
            // 上司：自己替他管城的人
            if state
                .cities
                .values()
                .any(|c| c.owner == Some(*to) && c.manager == Some(actor))
            {
                return Err(CommandError::CannotEnfeoffSuperior(*to));
            }
            check_same_country(state, actor, target.id)?;
            if !state.ruler_keeps_a_city(actor, &[city_id]) {
                return Err(CommandError::RulerMustKeepACity);
            }
            city_mut(state, city_id)?.fief_offer = Some(*to);
            Ok(vec![])
        }
        Command::AcceptFief { .. } => {
            if city.fief_offer != Some(actor) {
                return Err(CommandError::NotOfferedFief);
            }
            let liege = city.owner.ok_or(CommandError::NoOwner)?;
            check_same_country(state, liege, actor)?;
            if !state.ruler_keeps_a_city(liege, &[city_id]) {
                return Err(CommandError::RulerMustKeepACity);
            }
            let manages_other = state
                .cities
                .values()
                .any(|c| c.id != city_id && c.manager == Some(actor));
            let nationality = state.characters.get(&liege).and_then(|c| c.nationality);
            let city = city_mut(state, city_id)?;
            city.owner = Some(actor);
            city.liege = Some(liege);
            city.fief_offer = None;
            city.manager_offer = None;
            city.quota = None;
            city.tribute = None;
            city.tribute_shortfall_years = 0;
            city.passage.clear();
            for group in city.groups.values_mut() {
                group.hostility = Fx::ZERO;
            }
            army::transfer(state, city_id, actor);
            let city = city_mut(state, city_id)?;
            // 管理权归受封者；他已经在管别的城时，这座城先没有管理者，等他任命别人
            if manages_other {
                city.manager = None;
                for group in city.groups.values_mut() {
                    group.loyalty = config.loyalty.reset;
                }
            } else {
                city.change_manager(actor, config);
            }
            // 无国籍的人受封后加入封君的国家
            if let Some(vassal) = state.characters.get_mut(&actor)
                && vassal.nationality.is_none()
            {
                vassal.nationality = nationality;
            }
            Ok(vec![])
        }
        Command::DeclineFief { .. } => {
            if city.fief_offer != Some(actor) {
                return Err(CommandError::NotOfferedFief);
            }
            city_mut(state, city_id)?.fief_offer = None;
            Ok(vec![])
        }
        Command::SetTribute { goods, .. } => {
            check_liege(city, actor)?;
            check_goods(config, goods)?;
            let city = city_mut(state, city_id)?;
            if goods.iter().next().is_none() {
                city.tribute = None;
            } else {
                // 改了纳贡要重新答复；今年已经纳的照样算
                let delivered = city.tribute.take().map(|t| t.delivered).unwrap_or_default();
                city.tribute = Some(Tribute {
                    goods: goods.clone(),
                    delivered,
                    refused: false,
                });
            }
            Ok(vec![])
        }
        Command::RefuseTribute { .. } => {
            check_owner(city, actor)?;
            if city.liege.is_none() {
                return Err(CommandError::NotAFief(city_id));
            }
            if city.tribute.is_none() {
                return Err(CommandError::NoTributeDemand);
            }
            let months = state.turn % config.turns_per_year();
            if fief::overreach(city, config, months).is_none() {
                return Err(CommandError::TributeNotExcessive);
            }
            city_mut(state, city_id)?
                .tribute
                .as_mut()
                .expect("checked above")
                .refused = true;
            Ok(vec![])
        }
        Command::PayTribute { goods, .. } => {
            check_owner(city, actor)?;
            let liege = city.liege.ok_or(CommandError::NotAFief(city_id))?;
            check_treasury(config, city, goods)?;
            let city = city_mut(state, city_id)?;
            let mut paid = Inventory::default();
            for (good, amount) in goods.iter() {
                paid.add(good, city.treasury.take(good, *amount));
            }
            if let Some(tribute) = &mut city.tribute {
                tribute.delivered.add_all(&paid);
            }
            if let Some(liege) = state.characters.get_mut(&liege) {
                liege.private_store.add_all(&paid);
            }
            Ok(vec![])
        }
        Command::RaiseArmy { squads, .. } => army::raise(state, config, actor, city_id, squads),
        Command::Plunder {
            target, categories, ..
        } => combat::plunder(state, config, actor, city_id, *target, categories),
        Command::AppointOccupationManager { appointee, .. } => {
            combat::appoint_occupation_manager(state, config, actor, city_id, appointee)
        }
        Command::Raze { tile, building, .. } => {
            combat::raze(state, actor, city_id, *tile, building)
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

/// 人物自己的事：赠与、索取、交易，搬家。
fn apply_personal(
    state: &mut GameState,
    config: &Config,
    actor: Id,
    command: &Command,
) -> Result<Vec<Id>, CommandError> {
    let pending = |state: &GameState, id: Id| {
        state
            .deals
            .get(&id)
            .cloned()
            .ok_or(CommandError::NoSuchDeal(id))
    };
    match command {
        Command::ProposeDeal {
            to,
            give,
            take,
            into,
        } => {
            if !state.characters.contains_key(to) {
                return Err(CommandError::NoSuchCharacter(*to));
            }
            if *to == actor {
                return Err(CommandError::DealWithSelf);
            }
            if give.is_empty() && take.is_empty() {
                return Err(CommandError::EmptyDeal);
            }
            deal::check_stake(state, config, actor, give)?;
            deal::check_stake(state, config, *to, take)?;
            deal::check_into(state, actor, *into, give)?;
            let id = state.alloc_id();
            let deal = Deal {
                id,
                from: actor,
                to: *to,
                give: give.clone(),
                take: take.clone(),
                into: *into,
            };
            state.deals.insert(id, deal);
            Ok(vec![id])
        }
        Command::AcceptDeal { deal, into } => {
            let d = pending(state, *deal)?;
            if d.to != actor {
                return Err(CommandError::NotYourDeal(*deal));
            }
            deal::settle(state, config, &d, *into)?;
            state.deals.remove(deal);
            Ok(vec![])
        }
        Command::DeclineDeal { deal } => {
            if pending(state, *deal)?.to != actor {
                return Err(CommandError::NotYourDeal(*deal));
            }
            state.deals.remove(deal);
            Ok(vec![])
        }
        Command::CancelDeal { deal } => {
            if pending(state, *deal)?.from != actor {
                return Err(CommandError::NotYourDeal(*deal));
            }
            state.deals.remove(deal);
            Ok(vec![])
        }
        Command::MoveTo { city } => {
            let target = state
                .cities
                .get(city)
                .ok_or(CommandError::NoSuchCity(*city))?;
            check_owner(target, actor)?;
            let character = state
                .characters
                .get_mut(&actor)
                .ok_or(CommandError::NoSuchCharacter(actor))?;
            if character.location == Some(*city) {
                return Err(CommandError::AlreadyThere(*city));
            }
            character.location = Some(*city);
            Ok(vec![])
        }
        Command::DisbandArmy { army } => army::disband(state, config, actor, *army),
        Command::OpenSupplyLine { army, from, group } => {
            crate::supply::open(state, config, actor, *army, *from, *group)
        }
        Command::CloseSupplyLine { army } => crate::supply::close(state, config, actor, *army),
        Command::MarchArmy { army, to, force } => {
            army::order_march(state, config, actor, *army, *to, *force)
        }
        Command::Attack { army, at, ranged } => {
            combat::attack(state, config, actor, *army, *at, *ranged)
        }
        Command::ManWall { army, on } => combat::man_wall(state, config, actor, *army, *on),
        Command::DeclareWar { target } => {
            let mut events = Vec::new();
            let id = war::declare(state, config, actor, *target, &mut events)?;
            state.pending_events.extend(events);
            Ok(vec![id])
        }
        Command::HaltWar { war } => war::halt(state, actor, *war).map(|_| vec![]),
        Command::AssignMilitaryTask { war, vassal, city } => {
            war::assign_task(state, actor, *war, *vassal, *city).map(|_| vec![])
        }
        Command::ProposePeace { war, give, take } => {
            war::propose_peace(state, config, actor, *war, give, take).map(|_| vec![])
        }
        Command::AcceptPeace { war } => {
            let mut events = Vec::new();
            war::accept_peace(state, config, actor, *war, &mut events)?;
            state.pending_events.extend(events);
            Ok(vec![])
        }
        Command::DeclinePeace { war } => war::decline_peace(state, actor, *war).map(|_| vec![]),
        _ => unreachable!("only personal commands have no city"),
    }
}

/// 封君才能发的指令。
fn check_liege(city: &City, actor: Id) -> Result<(), CommandError> {
    match city.liege {
        None => Err(CommandError::NotAFief(city.id)),
        Some(liege) if liege != actor => Err(CommandError::NotLiege(city.id)),
        Some(_) => Ok(()),
    }
}

/// 封授只能在同一国的人之间，或者给无国籍的人。
fn check_same_country(state: &GameState, liege: Id, vassal: Id) -> Result<(), CommandError> {
    let nationality = |id: Id| state.characters.get(&id).and_then(|c| c.nationality);
    match nationality(vassal) {
        Some(country) if Some(country) != nationality(liege) => {
            Err(CommandError::DifferentCountry(vassal))
        }
        _ => Ok(()),
    }
}

/// 一个人只能直接管理一座城。
fn check_manages_no_other(state: &GameState, city: Id, who: Id) -> Result<(), CommandError> {
    if state
        .cities
        .values()
        .any(|c| c.id != city && c.manager == Some(who))
    {
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
        if *amount <= Fx::ZERO {
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
        if city.treasury.get(good) < *amount {
            return Err(CommandError::NotEnoughInTreasury(good.clone()));
        }
    }
    Ok(())
}

/// 不检查权限直接执行，只用于剧本加载这类开局设置。
pub(crate) fn execute(
    state: &mut GameState,
    config: &Config,
    command: &Command,
) -> Result<Vec<Id>, CommandError> {
    match command {
        Command::AppointShi {
            city,
            group,
            count,
            drop_skill,
        } => appoint_shi(state, config, *city, *group, *count, drop_skill.as_deref()),
        Command::DismissShi { city, shi } => {
            let city = city_mut(state, *city)?;
            let official = city
                .officials
                .get(shi)
                .ok_or(CommandError::NoSuchOfficial(*shi))?;
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
        Command::StartAssignment {
            city,
            shi,
            task,
            workers,
        } => start_assignment(state, config, *city, *shi, task, workers),
        Command::CancelAssignment { city, assignment } => {
            let city = city_mut(state, *city)?;
            city.assignments
                .remove(assignment)
                .ok_or(CommandError::NoSuchAssignment(*assignment))?;
            Ok(vec![])
        }
        Command::SetTaxRate { city, rate } => {
            let max = config.economy.max_tax_rate;
            if !(Fx::ZERO..=max).contains(rate) {
                return Err(CommandError::InvalidTaxRate(max.raw()));
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
                let gain =
                    config.value_of(goods) * config.loyalty.grant_gain_per_value / population;
                for group in city.groups.values_mut() {
                    group.loyalty = (group.loyalty + gain).min(config.loyalty.max);
                }
            }
            Ok(vec![])
        }
        Command::Deliver {
            city: city_id,
            goods,
        } => {
            let city = state
                .cities
                .get(city_id)
                .ok_or(CommandError::NoSuchCity(*city_id))?;
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
        | Command::SetQuota { city, .. }
        | Command::OfferFief { city, .. }
        | Command::AcceptFief { city }
        | Command::DeclineFief { city }
        | Command::SetTribute { city, .. }
        | Command::RefuseTribute { city }
        | Command::PayTribute { city, .. } => Err(CommandError::NotOwner(*city)),
        Command::ProposeDeal { to, .. } => Err(CommandError::NoSuchCharacter(*to)),
        Command::AcceptDeal { deal, .. }
        | Command::DeclineDeal { deal }
        | Command::CancelDeal { deal } => Err(CommandError::NotYourDeal(*deal)),
        Command::MoveTo { city } | Command::RaiseArmy { city, .. } => {
            Err(CommandError::NotOwner(*city))
        }
        Command::Plunder { city, .. }
        | Command::Raze { city, .. }
        | Command::AppointOccupationManager { city, .. } => Err(CommandError::NotOccupier(*city)),
        Command::DisbandArmy { army }
        | Command::OpenSupplyLine { army, .. }
        | Command::CloseSupplyLine { army }
        | Command::MarchArmy { army, .. }
        | Command::Attack { army, .. }
        | Command::ManWall { army, .. } => Err(CommandError::NotCommander(*army)),
        Command::DeclareWar { .. } => Err(CommandError::OwnCountry),
        Command::HaltWar { war }
        | Command::AssignMilitaryTask { war, .. }
        | Command::ProposePeace { war, .. }
        | Command::AcceptPeace { war }
        | Command::DeclinePeace { war } => Err(CommandError::NoSuchWar(*war)),
        Command::LearnSkill {
            city,
            group,
            count,
            skill,
            drop_skill,
        } => learn_skill(
            state,
            config,
            *city,
            *group,
            *count,
            skill,
            drop_skill.as_deref(),
        ),
        Command::ArrangeSkills {
            city,
            group,
            count,
            skills,
        } => arrange_skills(state, config, *city, *group, *count, skills),
    }
}

/// 检查一群人中有没有 `count` 名没在学技能的人。正在做工事的人也可以，一边干活一边学。
fn check_not_learning(city: &City, group: Id, count: u32) -> Result<(), CommandError> {
    if count == 0 {
        return Err(CommandError::ZeroCount);
    }
    let size = city
        .groups
        .get(&group)
        .ok_or(CommandError::NoSuchGroup(group))?
        .size;
    let available = size.saturating_sub(city.learning_in(group));
    if count > available {
        return Err(CommandError::NotEnoughNotLearning {
            group,
            available,
            requested: count,
        });
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
    let city = state
        .cities
        .get(&city_id)
        .ok_or(CommandError::NoSuchCity(city_id))?;
    check_not_learning(city, group_id, count)?;
    if !config.skills.contains_key(skill) {
        return Err(CommandError::NoSuchSkill(skill.to_string()));
    }
    let group = &city.groups[&group_id];
    if group.has_skill(skill) {
        return Err(CommandError::AlreadyHasSkill(
            config.skill_name(skill).to_string(),
        ));
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
        Learning {
            id,
            group: group_id,
            count,
            skills,
            skill: skill.to_string(),
            turns: 0,
        },
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
    let city = state
        .cities
        .get(&city_id)
        .ok_or(CommandError::NoSuchCity(city_id))?;
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
            return invalid(format!(
                "这群人不会「{}」，新技能要通过学技能获得",
                config.skill_name(skill)
            ));
        }
        if skills[..i].contains(skill) {
            return invalid(format!("技能「{}」重复", config.skill_name(skill)));
        }
    }
    let GameState {
        cities, next_id, ..
    } = state;
    let city = cities.get_mut(&city_id).expect("city checked above");
    Ok(vec![
        city.move_people(config, group_id, count, skills, next_id),
    ])
}

fn city_mut(state: &mut GameState, city: Id) -> Result<&mut City, CommandError> {
    state
        .cities
        .get_mut(&city)
        .ok_or(CommandError::NoSuchCity(city))
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
    let city = state
        .cities
        .get(&city_id)
        .ok_or(CommandError::NoSuchCity(city_id))?;
    let group = city
        .groups
        .get(&group_id)
        .ok_or(CommandError::NoSuchGroup(group_id))?;
    let available = city.unmanaged(group_id);
    if count > available {
        return Err(CommandError::NotEnoughUnmanaged {
            group: group_id,
            available,
            requested: count,
        });
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
    city.groups
        .get_mut(&group_id)
        .expect("group checked above")
        .size -= count;
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
    let city = state
        .cities
        .get(&city_id)
        .ok_or(CommandError::NoSuchCity(city_id))?;
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
        return Err(CommandError::OverCapacity {
            capacity,
            requested,
        });
    }

    let check_labor = |skill: Option<&str>| -> Result<(), CommandError> {
        for (group, count) in workers {
            if let Some(skill) = skill
                && !city.groups[group].has_skill(skill)
            {
                return Err(CommandError::MissingSkill {
                    group: *group,
                    skill: skill.to_string(),
                });
            }
            let available = city.unmanaged(*group);
            if *count > available {
                return Err(CommandError::NotEnoughUnmanaged {
                    group: *group,
                    available,
                    requested: *count,
                });
            }
        }
        Ok(())
    };

    let mut materials = None;
    match task {
        Task::Produce { recipe, mode } => {
            let def = config
                .recipes
                .get(recipe)
                .ok_or_else(|| CommandError::NoSuchRecipe(recipe.clone()))?;
            if let Some(resource) = &def.resource
                && !city.has_terrain(resource)
            {
                return Err(CommandError::MissingResource(resource.clone()));
            }
            // 官营只能在官营的建筑里做，民营只能在民营的建筑里做
            if let Some(facility) = &def.facility
                && !city.has_building(facility, *mode)
            {
                return Err(CommandError::MissingFacility {
                    facility: config.buildings[facility].name.clone(),
                    mode: *mode,
                });
            }
            check_labor(Some(&def.skill))?;
            if let Some(min) = def.min_specialization {
                for group in workers.keys() {
                    if config.specialization(city.groups[group].skills.len()) < min {
                        return Err(CommandError::SpecializationTooLow {
                            group: *group,
                            recipe: def.name.clone(),
                        });
                    }
                }
            }
            // 工事优先于未被管理的人口占用劳作位置，只需要和其他工事比
            if let Some(pool) = config.pool_of(recipe) {
                let available = city.free_jobs(config, &pool, *mode);
                if requested > available {
                    return Err(CommandError::NoJobs {
                        pool: config.pool_name(&pool).to_string(),
                        available,
                        requested,
                    });
                }
            }
        }
        Task::Improve { tile, improvement } => {
            let def = config
                .improvements
                .get(improvement)
                .ok_or_else(|| CommandError::NoSuchImprovement(improvement.clone()))?;
            let target = city
                .tiles
                .get(tile)
                .ok_or(CommandError::NoSuchTile(*tile))?;
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
            let target = city
                .tiles
                .get(tile)
                .ok_or(CommandError::NoSuchTile(*tile))?;
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
        Task::Reclaim { at, terrain } => {
            let cell = state.map.get(*at).ok_or(CommandError::NoSuchCell(*at))?;
            if let Some(owner) = state.city_at(*at) {
                return Err(CommandError::CellTaken {
                    at: *at,
                    city: state.cities[&owner].name.clone(),
                });
            }
            if !city.borders(*at) {
                return Err(CommandError::NotBordering(*at));
            }
            let def = &config.terrains[&cell.terrain];
            if &cell.terrain != terrain {
                return Err(CommandError::TerrainMismatch {
                    at: *at,
                    actual: def.name.clone(),
                });
            }
            let skill = def
                .reclaim_skill
                .as_deref()
                .ok_or_else(|| CommandError::CannotReclaim(def.name.clone()))?;
            if city
                .assignments
                .values()
                .any(|a| matches!(&a.task, Task::Reclaim { at: other, .. } if other == at))
            {
                return Err(CommandError::AlreadyReclaiming(*at));
            }
            check_labor(Some(skill))?;
        }
        Task::RepairWall => {
            if city.wall_hp(config).is_none() {
                return Err(CommandError::NoWall);
            }
            if city.wall_damage <= Fx::ZERO {
                return Err(CommandError::WallIntact);
            }
            if city
                .assignments
                .values()
                .any(|a| a.task == Task::RepairWall)
            {
                return Err(CommandError::AlreadyRepairing);
            }
            check_labor(Some(&config.siege.repair_skill))?;
        }
        Task::CollectTax => {
            for (group, count) in workers {
                let available = city.groups[group]
                    .size
                    .saturating_sub(city.tax_covered(*group));
                if *count > available {
                    return Err(CommandError::NotEnoughUntaxed {
                        group: *group,
                        available,
                        requested: *count,
                    });
                }
            }
        }
        Task::Sacrifice => {
            if !workers.is_empty() {
                return Err(CommandError::SacrificeTakesNoWorkers);
            }
            let vessels: i64 = config
                .sacrifice
                .vessels
                .keys()
                .map(|g| city.treasury.get(g).floor())
                .sum();
            if vessels < 1 {
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
        Assignment {
            id,
            shi,
            task: task.clone(),
            workers: workers.clone(),
            progress: Fx::ZERO,
        },
    );
    Ok(vec![id])
}
