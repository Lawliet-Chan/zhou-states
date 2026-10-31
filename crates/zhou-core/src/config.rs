use crate::fx::Fx;
use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::inventory::Inventory;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("读取配置文件失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("配置文件格式错误：{0}")]
    Parse(#[from] toml::de::Error),
    #[error("配置内容错误：{0}")]
    Invalid(String),
}

/// 全部数值配置，对应 config/balance.toml。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub seasons: Vec<String>,
    /// 人口页的行业及显示顺序。
    #[serde(default)]
    pub industries: Vec<IndustryDef>,
    /// 每季几个回合（月）
    pub months_per_season: u32,
    pub population: PopulationConfig,
    pub happiness: HappinessConfig,
    pub shi: ShiConfig,
    pub economy: EconomyConfig,
    pub sacrifice: SacrificeConfig,
    pub ritual: RitualConfig,
    pub quality: QualityConfig,
    pub learning: LearningConfig,
    pub loyalty: LoyaltyConfig,
    pub quota: QuotaConfig,
    pub fief: FiefConfig,
    pub neighbor: NeighborConfig,
    pub passage: PassageConfig,
    pub reclaim: ReclaimConfig,
    pub military: MilitaryConfig,
    pub combat: CombatConfig,
    pub siege: SiegeConfig,
    pub occupation: OccupationConfig,
    pub plunder: PlunderConfig,
    pub war: WarConfig,
    pub supply: SupplyConfig,
    pub vision: VisionConfig,
    pub units: BTreeMap<String, UnitDef>,
    /// 身份：id → 名称（天子、公、侯……）
    #[serde(default)]
    pub ranks: BTreeMap<String, String>,
    /// 物品类别：id → 名称
    pub categories: BTreeMap<String, String>,
    pub goods: BTreeMap<String, GoodDef>,
    pub skills: BTreeMap<String, SkillDef>,
    pub terrains: BTreeMap<String, TerrainDef>,
    pub recipes: BTreeMap<String, RecipeDef>,
    pub buildings: BTreeMap<String, BuildingDef>,
    pub improvements: BTreeMap<String, ImprovementDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PopulationConfig {
    pub per_people: Fx,
    pub max_skills: usize,
    /// 人群没有指定技能时的初始技能，没有人生下来什么都不会
    pub initial_skill: String,
    pub specialization: Vec<Fx>,
    pub growth_rate: Fx,
    pub growth_min_happiness: Fx,
    pub emigration_threshold: Fx,
    pub emigration_rate: Fx,
    pub starvation_rate: Fx,
    pub treasury_covers_shortfall: bool,
    pub consumption: Vec<ConsumptionRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsumptionRule {
    pub name: String,
    pub goods: Vec<String>,
    pub amount: Fx,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HappinessConfig {
    pub max: Fx,
    pub target: Fx,
    pub drift_rate: Fx,
    pub guan_penalty: Fx,
    pub tax_penalty: Fx,
    pub hunger_penalty: Fx,
    pub sacrifice_bonus: Fx,
    pub wealth_bonus: Fx,
    pub wealth_full_per_person: Fx,
    pub crowding_penalty: Fx,
    /// 会这项技能的人在农忙时被调去做别的工事，额外降低愉悦度
    pub busy_skill: String,
    /// 各季的农忙代价：整群人都被调走时每回合额外降低的愉悦度（按调走比例折算）
    pub busy_penalty: Vec<Fx>,
    pub min_output_mult: Fx,
    pub max_output_mult: Fx,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShiConfig {
    pub skill: String,
    pub base_capacity: Fx,
    pub new_capacity_ratio: Fx,
    pub ramp_turns: u32,
    pub loyalty_min_mult: Fx,
    pub loyalty_max: Fx,
    pub initial_loyalty: Fx,
    pub unpaid_loyalty_penalty: Fx,
    pub paid_loyalty_gain: Fx,
    pub upkeep: Inventory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EconomyConfig {
    pub guan_efficiency: Fx,
    pub default_tax_rate: Fx,
    pub max_tax_rate: Fx,
    pub absentee_mult: Fx,
    pub occupied_mult: Fx,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SacrificeConfig {
    /// 礼器 → 每件折合的件数，精品礼器折合更多
    pub vessels: BTreeMap<String, Fx>,
    pub culture_per_vessel: Fx,
    pub culture_max: Fx,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RitualConfig {
    pub ding_goods: Vec<String>,
    pub excess_reputation_penalty: Fx,
    pub ding_limit: BTreeMap<String, u32>,
    pub excess_tile_penalty: Fx,
    pub tile_limit: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QualityConfig {
    /// 技能槽中有 1/2/3/4 项技能时，产出中精品的比例
    pub fine_share: Vec<Fx>,
}

/// 人口对管理者个人的忠诚度。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoyaltyConfig {
    pub max: Fx,
    /// 换管理者时，各群人对新管理者的忠诚度
    pub reset: Fx,
    /// 剧本中没有指定时，开局的忠诚度
    pub initial: Fx,
    /// 不收税时每回合增加的忠诚度
    pub low_tax_gain: Fx,
    /// 税率为上限时每回合降低的忠诚度（按税率和被征税比例折算），和 low_tax_gain 相抵
    pub high_tax_loss: Fx,
    /// 赏赐民众时，人均每 1 价值的财货增加的忠诚度
    pub grant_gain_per_value: Fx,
    /// 忠诚度低于该值时开始出现下面的后果，越低越严重；忠诚度为 0 时为下面各项的值，中间线性插值
    pub threshold: Fx,
    /// 官营的人（官营生产、营建、开垦、水利）还肯出工的比例，其余的人罢工，回去自主民营
    pub guan_turnout: Fx,
    /// 收税能收上来的比例
    pub tax_share: Fx,
    /// 民营的产量倍率
    pub min_output: Fx,
}

/// 通行权。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PassageConfig {
    /// 赠与、交易通行权时不写期限，默认给这么多回合
    pub default_turns: u32,
}

/// 邻城增益：同一个所有者的城离得近，产出都会增加。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeighborConfig {
    /// 两座城的治所相距不超过这么多格，算邻城
    pub range: u32,
    /// 每有一座邻城，产出增加的比例
    pub bonus: Fx,
    /// 增加的比例的上限
    pub max: Fx,
}

/// 封授与纳贡。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FiefConfig {
    /// 封君没有指定纳贡时，每年年末自动纳贡：府库当年总产出的这个比例
    pub auto_rate: Fx,
    /// 指定的纳贡总价值超过府库一年总产出价值的这个比例，就算过火，封臣可以拒绝
    pub excessive_ratio: Fx,
    pub hostility_max: Fx,
    /// 过火的纳贡每回合给封地百姓增加的敌视：要的是上限的两倍时增加这么多，按超出的比例折算，最多两倍
    pub hostility_gain: Fx,
    /// 没有过火的纳贡时，敌视每回合消退的量
    pub hostility_decay: Fx,
    /// 封臣风评低于这个值，可以作为征讨的理由
    pub low_reputation: Fx,
    /// 合理的纳贡连续这么多年没交够，可以作为征讨的理由
    pub shortfall_years: u32,
}

/// 规定任务：每年年末结算，影响所有者对管理者的信任。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaConfig {
    /// 人物之间信任的初始值和上限
    pub initial_trust: Fx,
    pub max_trust: Fx,
    /// 完成任务时增加的信任
    pub complete_trust: Fx,
    /// 每种物资都交够、且总价值达到规定的 exceed_ratio 倍时，算超额完成，增加 exceed_trust
    pub exceed_ratio: Fx,
    pub exceed_trust: Fx,
    /// 一点没交时降低的信任，按缺口比例折算
    pub fail_trust_penalty: Fx,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReclaimConfig {
    pub season_mult: Vec<Fx>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningConfig {
    /// 学一项新技能需要的回合数
    pub turns: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoodDef {
    pub name: String,
    /// 价值，用于折算民间财富的多少
    #[serde(default = "default_value")]
    pub value: Fx,
    /// 这种物品的精品
    pub fine: Option<String>,
    /// 类别，劫掠时按类别挑
    pub category: String,
}

fn default_value() -> Fx {
    Fx::ONE
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillDef {
    #[serde(default)]
    pub industry: Option<String>,
    #[serde(default)]
    pub trade_name: Option<String>,
    pub name: String,
    pub default_recipe: Option<String>,
}

/// 展示用行业，工种由技能定义归入对应行业。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndustryDef {
    pub key: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MilitaryConfig {
    /// 能当兵的技能
    pub skill: String,
    /// 完全没有装备的兵，战斗力剩下的比例；装备不全时按缺的比例折算
    pub unequipped_mult: Fx,
}

/// 交战。战斗力 = 人数 × 兵种的攻击或防御 × 装备倍率 × 其他加成。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CombatConfig {
    /// 每点战斗力每次交战杀死对方的人数
    pub lethality: Fx,
    /// 远程攻击的距离（格）
    pub ranged_range: u32,
    /// 全军带甲时防御增加的比例
    pub armor_bonus: Fx,
    /// 守城墙的兵还手时战斗力增加的比例
    pub wall_bonus: Fx,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiegeConfig {
    /// 有城墙的建筑
    pub wall_building: String,
    pub wall_hp: Fx,
    pub center_hp: Fx,
    /// 每架攻城机每次攻击打掉的城墙、城中心的防御值
    pub engine_damage: Fx,
    /// 攻城机这种物品
    pub engine_good: String,
    /// 每点战斗力打掉的城墙防御值
    pub wall_rate: Fx,
    /// 每点战斗力打掉的城中心防御值
    pub center_rate: Fx,
    /// 修城墙的技能，每 100 名工人每回合修多少
    pub repair_skill: String,
    pub repair_per_100: Fx,
}

/// 占领：城中人群对占领者的初始忠诚度 = base + 管城者风评 × reputation_mult（最多加 reputation_max）
/// + 旧忠诚度低于 grudge_below 时每低 1 点加 grudge_mult（最多加 grudge_max）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OccupationConfig {
    pub base_loyalty: Fx,
    pub reputation_mult: Fx,
    pub reputation_max: Fx,
    pub grudge_below: Fx,
    pub grudge_mult: Fx,
    pub grudge_max: Fx,
}

/// 宣战与讲和。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WarConfig {
    /// 国家之间没有理由开战扣的风评
    pub unjustified_penalty: Fx,
    /// 讲和之后多少回合不能再开战
    pub truce_turns: u32,
    /// 封臣没有执行国战任务扣的风评
    pub task_penalty: Fx,
    /// 国君制止后封臣还动手，每次扣的风评
    pub defy_penalty: Fx,
}

/// 粮道。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SupplyConfig {
    /// 走粮道的兵种
    pub unit: String,
    /// 一条粮道的辎重兵人数，和粮道长短、前线人数无关
    pub convoy: u32,
    /// 粮道每多一格，军粮多耗的比例
    pub per_cell: Fx,
    /// 断粮的第 n 个回合，战斗力再降 n × starve_rate
    pub starve_rate: Fx,
}

/// 视野。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionConfig {
    /// 都城公开的国君身份，由配置指定。
    #[serde(default)]
    pub public_capital_rank: String,
    /// 城的地块再往外几格
    pub city: u32,
    /// 军队所在的格子再往外几格，兵种可以另设
    pub army: u32,
    /// 别人的城看得到的大致人口，从小到大
    pub tiers: Vec<SizeTier>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SizeTier {
    pub min: u32,
    pub name: String,
}

/// 劫掠市民的代价
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlunderConfig {
    pub happiness_loss: Fx,
    pub loyalty_loss: Fx,
    pub reputation_loss: Fx,
}

/// 兵种。装备按每人多少件算，编军队时从府库领，精品也算。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitDef {
    pub name: String,
    /// 必需的装备，缺了战斗力打折
    #[serde(default)]
    pub gear: Inventory,
    /// 可以选带的装备，比如步兵的甲、弓矢、攻城机
    #[serde(default)]
    pub optional: Inventory,
    /// 每回合的行军力
    pub moves: u32,
    /// 每人的近战攻击、防御
    pub attack: Fx,
    pub defense: Fx,
    /// 带了弓矢的人每人的远程攻击；不填表示不能远程
    #[serde(default)]
    pub ranged: Fx,
    /// 打完还能接着走，比如骑兵
    #[serde(default)]
    pub move_after_attack: bool,
    /// 视野，不写就按 vision.army
    #[serde(default)]
    pub vision: Option<u32>,
    /// 这个兵种走某些地形时的耗时，取代地形本身的 move_cost，比如车兵进山林慢
    #[serde(default)]
    pub terrain_cost: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerrainDef {
    pub name: String,
    /// 手画地图时代表这种地形的字
    pub symbol: String,
    /// 每块地能居住的人口
    pub housing: u32,
    /// 每块地能容纳劳作的人数
    pub jobs: u32,
    /// 每块地能建多少座建筑，官营和民营的建筑都算
    pub building_slots: u32,
    /// 开垦这种地需要的技能；不填表示开垦不出来
    pub reclaim_skill: Option<String>,
    /// 开垦需要的人力（人 × 回合）
    #[serde(default)]
    pub reclaim_labor: Fx,
    /// 军队走进这种地形的一格要花的行军力；不填表示过不去（高山、大河）
    pub move_cost: Option<u32>,
    /// 守在这种地形上的军队防御增加的比例
    #[serde(default)]
    pub defense_bonus: Fx,
}

impl TerrainDef {
    pub fn passable(&self) -> bool {
        self.move_cost.is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeDef {
    pub name: String,
    pub skill: String,
    /// 劳作所在的地块，劳作人数受地块容量限制
    pub resource: Option<String>,
    pub facility: Option<String>,
    /// 不靠地块的行业，不需要建筑也能容纳的人数；不填且没有建筑提供容量时不限人数
    pub base_jobs: Option<u32>,
    /// 专精度低于该值的人群做不了
    pub min_specialization: Option<Fx>,
    #[serde(default)]
    pub inputs: Inventory,
    pub outputs: Inventory,
    pub season_mult: Vec<Fx>,
    /// 播种的季节：这一季干这一行的人是在播种，记下种了多少；其他季节收成不超过播种量
    pub sowing_season: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildingDef {
    pub name: String,
    pub skill: Option<String>,
    pub labor: Fx,
    #[serde(default)]
    pub materials: Inventory,
    pub season_mult: Vec<Fx>,
    /// 配方 → 每座建筑增加的劳作容量
    #[serde(default)]
    pub jobs: BTreeMap<String, u32>,
    /// 每座建筑增加的居住人口
    #[serde(default)]
    pub housing: u32,
    /// 每座建筑每回合给全城人群增加的愉悦度
    #[serde(default)]
    pub happiness: Fx,
    /// 每座建筑给士人管理上限增加的比例
    #[serde(default)]
    pub shi_capacity_bonus: Fx,
    /// 只能建在这些地块上；不填表示哪里都能建
    #[serde(default)]
    pub terrains: Vec<String>,
}

/// 地块改良，比如水利。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImprovementDef {
    pub name: String,
    pub skill: Option<String>,
    pub labor: Fx,
    #[serde(default)]
    pub materials: Inventory,
    pub season_mult: Vec<Fx>,
    /// 可以改良的地块
    pub terrains: Vec<String>,
    pub max_level: u32,
    /// 每级增加的产量比例
    pub yield_bonus: Fx,
}

/// 劳作容量的来源：一种地块，或者一个不靠地块的配方。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(tag = "kind", content = "id", rename_all = "snake_case")]
pub enum Pool {
    Terrain(String),
    Recipe(String),
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        Self::from_toml(&std::fs::read_to_string(path)?)
    }

    pub fn from_toml(text: &str) -> Result<Self, ConfigError> {
        let config: Config = toml::from_str(text)?;
        config.validate()?;
        Ok(config)
    }

    /// 技能槽中有 `skill_count` 项技能时的专精度。
    pub fn specialization(&self, skill_count: usize) -> Fx {
        let table = &self.population.specialization;
        table[skill_count.clamp(1, table.len()) - 1]
    }

    /// 愉悦度对产出的倍率。
    pub fn happiness_output_mult(&self, happiness: Fx) -> Fx {
        let h = &self.happiness;
        let t = (happiness / h.max).clamp(Fx::ZERO, Fx::ONE);
        h.min_output_mult + (h.max_output_mult - h.min_output_mult) * t
    }

    /// 一堆物品按价值折算的总量。
    pub fn value_of(&self, inventory: &Inventory) -> Fx {
        inventory
            .iter()
            .map(|(good, amount)| self.goods.get(good).map_or(Fx::ZERO, |g| g.value) * *amount)
            .sum()
    }

    /// 忠诚度低带来的倍率：忠诚度不低于 threshold 时为 1，为 0 时为 `at_zero`，中间线性插值。
    pub fn disloyalty_mult(&self, loyalty: Fx, at_zero: Fx) -> Fx {
        let threshold = self.loyalty.threshold;
        if threshold <= Fx::ZERO || loyalty >= threshold {
            return Fx::ONE;
        }
        at_zero + (Fx::ONE - at_zero) * (loyalty / threshold).max(Fx::ZERO)
    }

    /// 技能槽中有 `skill_count` 项技能时，产出中精品的比例。
    pub fn fine_share(&self, skill_count: usize) -> Fx {
        let table = &self.quality.fine_share;
        table[skill_count.clamp(1, table.len()) - 1]
    }

    /// 配方的劳作容量来自哪里；None 表示不限人数。
    pub fn pool_of(&self, recipe_id: &str) -> Option<Pool> {
        let recipe = self.recipes.get(recipe_id)?;
        if let Some(terrain) = &recipe.resource {
            return Some(Pool::Terrain(terrain.clone()));
        }
        let from_buildings = self
            .buildings
            .values()
            .any(|b| b.jobs.contains_key(recipe_id));
        (recipe.base_jobs.is_some() || from_buildings).then(|| Pool::Recipe(recipe_id.to_string()))
    }

    /// 建筑里能不能劳作：给配方提供劳作位置，或者是某个配方要求的设施。
    /// 只有这类建筑区分官营民营，城墙、宫殿这类的效果对全城生效。
    pub fn is_workplace(&self, building: &str) -> bool {
        self.buildings
            .get(building)
            .is_some_and(|b| !b.jobs.is_empty())
            || self
                .recipes
                .values()
                .any(|r| r.facility.as_deref() == Some(building))
    }

    pub fn pool_name<'a>(&'a self, pool: &'a Pool) -> &'a str {
        match pool {
            Pool::Terrain(id) => self.terrains.get(id).map_or(id, |t| t.name.as_str()),
            Pool::Recipe(id) => self.recipes.get(id).map_or(id, |r| r.name.as_str()),
        }
    }

    pub fn good_name<'a>(&'a self, id: &'a str) -> &'a str {
        self.goods.get(id).map_or(id, |g| g.name.as_str())
    }

    /// 一年的回合数
    pub fn turns_per_year(&self) -> u32 {
        self.seasons.len() as u32 * self.months_per_season
    }

    pub fn skill_name<'a>(&'a self, id: &'a str) -> &'a str {
        self.skills.get(id).map_or(id, |s| s.name.as_str())
    }

    fn validate(&self) -> Result<(), ConfigError> {
        let invalid = |msg: String| Err(ConfigError::Invalid(msg));
        let seasons = self.seasons.len();
        if seasons == 0 {
            return invalid("seasons 不能为空".into());
        }
        let mut symbols = std::collections::BTreeSet::new();
        for (id, terrain) in &self.terrains {
            if terrain.symbol.split_whitespace().count() != 1
                || terrain.symbol == "·"
                || !symbols.insert(&terrain.symbol)
            {
                return invalid(format!(
                    "地形「{id}」的 symbol 必须是一个不含空格、不重复、不是「·」的符号"
                ));
            }
        }
        if self.happiness.busy_penalty.len() != seasons {
            return invalid("happiness.busy_penalty 长度必须等于季节数".into());
        }
        if !self.skills.contains_key(&self.happiness.busy_skill) {
            return invalid(format!(
                "happiness.busy_skill「{}」不在 skills 中",
                self.happiness.busy_skill
            ));
        }
        let f = &self.fief;
        if !(Fx::ZERO..=Fx::ONE).contains(&f.auto_rate)
            || f.excessive_ratio <= Fx::ZERO
            || f.hostility_max <= Fx::ZERO
        {
            return invalid(
                "fief.auto_rate 必须在 0 到 1000 之间，excessive_ratio 和 hostility_max 必须大于 0"
                    .into(),
            );
        }
        if self.neighbor.bonus < Fx::ZERO || self.neighbor.max < Fx::ZERO {
            return invalid("neighbor.bonus 和 neighbor.max 不能为负".into());
        }
        if self.passage.default_turns == 0 {
            return invalid("passage.default_turns 不能为 0".into());
        }
        if self.months_per_season == 0 {
            return invalid("months_per_season 不能为 0".into());
        }
        if self.population.specialization.len() != self.population.max_skills {
            return invalid("population.specialization 的长度必须等于 max_skills".into());
        }
        if self.quality.fine_share.len() != self.population.max_skills {
            return invalid("quality.fine_share 的长度必须等于 max_skills".into());
        }
        if !self.skills.contains_key(&self.population.initial_skill) {
            return invalid(format!(
                "population.initial_skill「{}」不在 skills 中",
                self.population.initial_skill
            ));
        }
        let l = &self.loyalty;
        for (name, value) in [
            ("guan_turnout", l.guan_turnout),
            ("tax_share", l.tax_share),
            ("min_output", l.min_output),
        ] {
            if !(Fx::ZERO..=Fx::ONE).contains(&value) {
                return invalid(format!("loyalty.{name} 必须在 0 到 1000 之间"));
            }
        }
        if self.quota.exceed_ratio < Fx::ONE {
            return invalid("quota.exceed_ratio 不能小于 1000".into());
        }
        if self.learning.turns == 0 {
            return invalid("learning.turns 必须大于 0".into());
        }
        if !self.skills.contains_key(&self.shi.skill) {
            return invalid(format!("shi.skill「{}」不在 skills 中", self.shi.skill));
        }
        let check_goods = |inv: &Inventory, owner: &str| -> Result<(), ConfigError> {
            for good in inv.0.keys() {
                if !self.goods.contains_key(good) {
                    return Err(ConfigError::Invalid(format!(
                        "{owner} 引用了不存在的物品「{good}」"
                    )));
                }
            }
            Ok(())
        };
        check_goods(&self.shi.upkeep, "shi.upkeep")?;
        for rule in &self.population.consumption {
            for good in &rule.goods {
                if !self.goods.contains_key(good) {
                    return invalid(format!("消耗「{}」引用了不存在的物品「{good}」", rule.name));
                }
            }
        }
        for good in self.sacrifice.vessels.keys().chain(&self.ritual.ding_goods) {
            if !self.goods.contains_key(good) {
                return invalid(format!("礼器「{good}」不在 goods 中"));
            }
        }
        for rank in self.ritual.ding_limit.keys() {
            if !self.ritual.tile_limit.contains_key(rank) {
                return invalid(format!("ritual.tile_limit 缺少爵位「{rank}」"));
            }
        }
        for (id, good) in &self.goods {
            if let Some(fine) = &good.fine
                && !self.goods.contains_key(fine)
            {
                return invalid(format!("物品「{id}」的精品「{fine}」不存在"));
            }
        }
        for (id, skill) in &self.skills {
            if let Some(recipe) = &skill.default_recipe
                && !self.recipes.contains_key(recipe)
            {
                return invalid(format!("技能「{id}」的 default_recipe「{recipe}」不存在"));
            }
        }
        for (id, recipe) in &self.recipes {
            if !self.skills.contains_key(&recipe.skill) {
                return invalid(format!("配方「{id}」的技能「{}」不存在", recipe.skill));
            }
            if let Some(facility) = &recipe.facility
                && !self.buildings.contains_key(facility)
            {
                return invalid(format!("配方「{id}」的设施「{facility}」不存在"));
            }
            if let Some(terrain) = &recipe.resource {
                if !self.terrains.contains_key(terrain) {
                    return invalid(format!("配方「{id}」的地块「{terrain}」不存在"));
                }
                if recipe.base_jobs.is_some() {
                    return invalid(format!("配方「{id}」靠地块劳作，不能再设 base_jobs"));
                }
            }
            if recipe.season_mult.len() != seasons {
                return invalid(format!("配方「{id}」的 season_mult 长度必须等于季节数"));
            }
            if recipe.sowing_season.is_some_and(|s| s >= seasons) {
                return invalid(format!("配方「{id}」的 sowing_season 超出季节数"));
            }
            check_goods(&recipe.inputs, &format!("配方「{id}」"))?;
            check_goods(&recipe.outputs, &format!("配方「{id}」"))?;
        }
        for (id, building) in &self.buildings {
            if let Some(skill) = &building.skill
                && !self.skills.contains_key(skill)
            {
                return invalid(format!("建筑「{id}」的技能「{skill}」不存在"));
            }
            if building.season_mult.len() != seasons {
                return invalid(format!("建筑「{id}」的 season_mult 长度必须等于季节数"));
            }
            check_goods(&building.materials, &format!("建筑「{id}」"))?;
            for recipe in building.jobs.keys() {
                match self.recipes.get(recipe) {
                    None => {
                        return invalid(format!("建筑「{id}」提供容量的配方「{recipe}」不存在"));
                    }
                    Some(r) if r.resource.is_some() => {
                        return invalid(format!(
                            "建筑「{id}」不能给靠地块劳作的配方「{recipe}」提供容量"
                        ));
                    }
                    Some(_) => {}
                }
            }
        }
        if self.reclaim.season_mult.len() != seasons {
            return invalid("reclaim.season_mult 长度必须等于季节数".into());
        }
        let m = &self.military;
        if !self.skills.contains_key(&m.skill) {
            return invalid(format!("military.skill「{}」不在 skills 中", m.skill));
        }
        if !(Fx::ZERO..=Fx::ONE).contains(&m.unequipped_mult) {
            return invalid("military.unequipped_mult 必须在 0 到 1000 之间".into());
        }
        if !self.buildings.contains_key(&self.siege.wall_building)
            || !self.goods.contains_key(&self.siege.engine_good)
            || !self.skills.contains_key(&self.siege.repair_skill)
        {
            return invalid("siege 的 wall_building、engine_good、repair_skill 必须存在".into());
        }
        if self.siege.wall_hp <= Fx::ZERO
            || self.siege.center_hp <= Fx::ZERO
            || self.combat.lethality <= Fx::ZERO
        {
            return invalid("siege.wall_hp、center_hp 和 combat.lethality 必须大于 0".into());
        }
        for (id, good) in &self.goods {
            if !self.categories.contains_key(&good.category) {
                return invalid(format!(
                    "物品「{id}」的类别「{}」不在 categories 中",
                    good.category
                ));
            }
        }
        let tiers = &self.vision.tiers;
        if tiers.first().is_none_or(|t| t.min != 0)
            || tiers.windows(2).any(|w| w[0].min >= w[1].min)
        {
            return invalid("vision.tiers 要从 min = 0 开始，按 min 从小到大排".into());
        }
        let sp = &self.supply;
        if !self.units.contains_key(&sp.unit) || sp.convoy == 0 {
            return invalid("supply.unit 必须是兵种，supply.convoy 必须大于 0".into());
        }
        if sp.per_cell < Fx::ZERO || sp.starve_rate <= Fx::ZERO {
            return invalid("supply.per_cell 不能小于 0，supply.starve_rate 必须大于 0".into());
        }
        for (id, unit) in &self.units {
            if unit.moves == 0 {
                return invalid(format!("兵种「{id}」的 moves 必须大于 0"));
            }
            for (terrain, cost) in &unit.terrain_cost {
                if !self.terrains.get(terrain).is_some_and(|t| t.passable()) || *cost == 0 {
                    return invalid(format!(
                        "兵种「{id}」的 terrain_cost 里「{terrain}」不是能走的地形，或者耗时为 0"
                    ));
                }
            }
            for (good, amount) in unit.gear.iter().chain(unit.optional.iter()) {
                if !self.goods.contains_key(good) || *amount <= Fx::ZERO {
                    return invalid(format!("兵种「{id}」的装备「{good}」不存在或数量不大于 0"));
                }
                if unit.gear.get(good) > Fx::ZERO && unit.optional.get(good) > Fx::ZERO {
                    return invalid(format!("兵种「{id}」的装备「{good}」不能既必需又可选"));
                }
            }
        }
        for (id, terrain) in &self.terrains {
            if terrain.move_cost == Some(0) {
                return invalid(format!(
                    "地形「{id}」的 move_cost 必须大于 0；过不去的地形不写 move_cost"
                ));
            }
            if !terrain.passable()
                && (terrain.reclaim_skill.is_some() || terrain.housing > 0 || terrain.jobs > 0)
            {
                return invalid(format!("地形「{id}」过不去，不能住人、劳作或开垦"));
            }
            if let Some(skill) = &terrain.reclaim_skill {
                if !self.skills.contains_key(skill) {
                    return invalid(format!("地块「{id}」的开垦技能「{skill}」不存在"));
                }
                if terrain.reclaim_labor <= Fx::ZERO {
                    return invalid(format!("地块「{id}」可以开垦，reclaim_labor 必须大于 0"));
                }
            }
        }
        for (id, building) in &self.buildings {
            for terrain in &building.terrains {
                if !self.terrains.contains_key(terrain) {
                    return invalid(format!("建筑「{id}」的地块「{terrain}」不存在"));
                }
            }
        }
        for (id, improvement) in &self.improvements {
            if let Some(skill) = &improvement.skill
                && !self.skills.contains_key(skill)
            {
                return invalid(format!("改良「{id}」的技能「{skill}」不存在"));
            }
            if improvement.season_mult.len() != seasons {
                return invalid(format!("改良「{id}」的 season_mult 长度必须等于季节数"));
            }
            for terrain in &improvement.terrains {
                if !self.terrains.contains_key(terrain) {
                    return invalid(format!("改良「{id}」的地块「{terrain}」不存在"));
                }
            }
            check_goods(&improvement.materials, &format!("改良「{id}」"))?;
        }
        Ok(())
    }
}
