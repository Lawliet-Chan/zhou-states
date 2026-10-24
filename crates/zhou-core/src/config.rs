use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
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
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub seasons: Vec<String>,
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
    pub reclaim: ReclaimConfig,
    pub goods: BTreeMap<String, GoodDef>,
    pub skills: BTreeMap<String, SkillDef>,
    pub terrains: BTreeMap<String, TerrainDef>,
    pub recipes: BTreeMap<String, RecipeDef>,
    pub buildings: BTreeMap<String, BuildingDef>,
    pub improvements: BTreeMap<String, ImprovementDef>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PopulationConfig {
    pub per_people: f64,
    pub max_skills: usize,
    /// 人群没有指定技能时的初始技能，没有人生下来什么都不会
    pub initial_skill: String,
    pub specialization: Vec<f64>,
    pub growth_rate: f64,
    pub growth_min_happiness: f64,
    pub emigration_threshold: f64,
    pub emigration_rate: f64,
    pub starvation_rate: f64,
    pub treasury_covers_shortfall: bool,
    pub consumption: Vec<ConsumptionRule>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConsumptionRule {
    pub name: String,
    pub goods: Vec<String>,
    pub amount: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HappinessConfig {
    pub max: f64,
    pub target: f64,
    pub drift_rate: f64,
    pub guan_penalty: f64,
    pub tax_penalty: f64,
    pub hunger_penalty: f64,
    pub sacrifice_bonus: f64,
    pub wealth_bonus: f64,
    pub wealth_full_per_person: f64,
    pub crowding_penalty: f64,
    pub min_output_mult: f64,
    pub max_output_mult: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ShiConfig {
    pub skill: String,
    pub base_capacity: f64,
    pub new_capacity_ratio: f64,
    pub ramp_turns: u32,
    pub loyalty_min_mult: f64,
    pub loyalty_max: f64,
    pub initial_loyalty: f64,
    pub unpaid_loyalty_penalty: f64,
    pub paid_loyalty_gain: f64,
    pub upkeep: Inventory,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EconomyConfig {
    pub guan_efficiency: f64,
    pub default_tax_rate: f64,
    pub max_tax_rate: f64,
    pub absentee_mult: f64,
    pub occupied_mult: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SacrificeConfig {
    /// 礼器 → 每件折合的件数，精品礼器折合更多
    pub vessels: BTreeMap<String, f64>,
    pub culture_per_vessel: f64,
    pub culture_max: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RitualConfig {
    pub ding_goods: Vec<String>,
    pub excess_reputation_penalty: f64,
    pub ding_limit: BTreeMap<String, u32>,
    pub excess_tile_penalty: f64,
    pub tile_limit: BTreeMap<String, u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QualityConfig {
    /// 技能槽中有 1/2/3/4 项技能时，产出中精品的比例
    pub fine_share: Vec<f64>,
}

/// 人口对管理者个人的忠诚度。
#[derive(Debug, Clone, Deserialize)]
pub struct LoyaltyConfig {
    pub max: f64,
    /// 换管理者时，各群人对新管理者的忠诚度
    pub reset: f64,
    /// 剧本中没有指定时，开局的忠诚度
    pub initial: f64,
    /// 不收税时每回合增加的忠诚度
    pub low_tax_gain: f64,
    /// 税率为上限时每回合降低的忠诚度（按税率和被征税比例折算），和 low_tax_gain 相抵
    pub high_tax_loss: f64,
    /// 赏赐民众时，人均每 1 价值的财货增加的忠诚度
    pub grant_gain_per_value: f64,
    /// 忠诚度低于该值时开始出现下面的后果，越低越严重；忠诚度为 0 时为下面各项的值，中间线性插值
    pub threshold: f64,
    /// 官营的人（官营生产、营建、开垦、水利）还肯出工的比例，其余的人罢工，回去自主民营
    pub guan_turnout: f64,
    /// 收税能收上来的比例
    pub tax_share: f64,
    /// 民营的产量倍率
    pub min_output: f64,
}

/// 规定任务：每年冬季结算，影响所有者对管理者的信任。
#[derive(Debug, Clone, Deserialize)]
pub struct QuotaConfig {
    /// 人物之间信任的初始值和上限
    pub initial_trust: f64,
    pub max_trust: f64,
    /// 完成任务时增加的信任
    pub complete_trust: f64,
    /// 每种物资都交够、且总价值达到规定的 exceed_ratio 倍时，算超额完成，增加 exceed_trust
    pub exceed_ratio: f64,
    pub exceed_trust: f64,
    /// 一点没交时降低的信任，按缺口比例折算
    pub fail_trust_penalty: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ReclaimConfig {
    pub season_mult: Vec<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LearningConfig {
    /// 学一项新技能需要的回合数
    pub turns: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GoodDef {
    pub name: String,
    /// 价值，用于折算民间财富的多少
    #[serde(default = "default_value")]
    pub value: f64,
    /// 这种物品的精品
    pub fine: Option<String>,
}

fn default_value() -> f64 {
    1.0
}

#[derive(Debug, Clone, Deserialize)]
pub struct SkillDef {
    pub name: String,
    pub default_recipe: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TerrainDef {
    pub name: String,
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
    pub reclaim_labor: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RecipeDef {
    pub name: String,
    pub skill: String,
    /// 劳作所在的地块，劳作人数受地块容量限制
    pub resource: Option<String>,
    pub facility: Option<String>,
    /// 不靠地块的行业，不需要建筑也能容纳的人数；不填且没有建筑提供容量时不限人数
    pub base_jobs: Option<u32>,
    /// 专精度低于该值的人群做不了
    pub min_specialization: Option<f64>,
    #[serde(default)]
    pub inputs: Inventory,
    pub outputs: Inventory,
    pub season_mult: Vec<f64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BuildingDef {
    pub name: String,
    pub skill: Option<String>,
    pub labor: f64,
    #[serde(default)]
    pub materials: Inventory,
    pub season_mult: Vec<f64>,
    /// 配方 → 每座建筑增加的劳作容量
    #[serde(default)]
    pub jobs: BTreeMap<String, u32>,
    /// 每座建筑增加的居住人口
    #[serde(default)]
    pub housing: u32,
    /// 每座建筑每回合给全城人群增加的愉悦度
    #[serde(default)]
    pub happiness: f64,
    /// 每座建筑给士人管理上限增加的比例
    #[serde(default)]
    pub shi_capacity_bonus: f64,
    /// 只能建在这些地块上；不填表示哪里都能建
    #[serde(default)]
    pub terrains: Vec<String>,
}

/// 地块改良，比如水利。
#[derive(Debug, Clone, Deserialize)]
pub struct ImprovementDef {
    pub name: String,
    pub skill: Option<String>,
    pub labor: f64,
    #[serde(default)]
    pub materials: Inventory,
    pub season_mult: Vec<f64>,
    /// 可以改良的地块
    pub terrains: Vec<String>,
    pub max_level: u32,
    /// 每级增加的产量比例
    pub yield_bonus: f64,
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
    pub fn specialization(&self, skill_count: usize) -> f64 {
        let table = &self.population.specialization;
        table[skill_count.clamp(1, table.len()) - 1]
    }

    /// 愉悦度对产出的倍率。
    pub fn happiness_output_mult(&self, happiness: f64) -> f64 {
        let h = &self.happiness;
        let t = (happiness / h.max).clamp(0.0, 1.0);
        h.min_output_mult + (h.max_output_mult - h.min_output_mult) * t
    }

    /// 一堆物品按价值折算的总量。
    pub fn value_of(&self, inventory: &Inventory) -> f64 {
        inventory.iter().map(|(good, amount)| self.goods.get(good).map_or(0.0, |g| g.value) * amount).sum()
    }

    /// 忠诚度低带来的倍率：忠诚度不低于 threshold 时为 1，为 0 时为 `at_zero`，中间线性插值。
    pub fn disloyalty_mult(&self, loyalty: f64, at_zero: f64) -> f64 {
        let threshold = self.loyalty.threshold;
        if threshold <= 0.0 || loyalty >= threshold {
            return 1.0;
        }
        at_zero + (1.0 - at_zero) * (loyalty / threshold).max(0.0)
    }

    /// 技能槽中有 `skill_count` 项技能时，产出中精品的比例。
    pub fn fine_share(&self, skill_count: usize) -> f64 {
        let table = &self.quality.fine_share;
        table[skill_count.clamp(1, table.len()) - 1]
    }

    /// 配方的劳作容量来自哪里；None 表示不限人数。
    pub fn pool_of(&self, recipe_id: &str) -> Option<Pool> {
        let recipe = self.recipes.get(recipe_id)?;
        if let Some(terrain) = &recipe.resource {
            return Some(Pool::Terrain(terrain.clone()));
        }
        let from_buildings = self.buildings.values().any(|b| b.jobs.contains_key(recipe_id));
        (recipe.base_jobs.is_some() || from_buildings).then(|| Pool::Recipe(recipe_id.to_string()))
    }

    /// 建筑里能不能劳作：给配方提供劳作位置，或者是某个配方要求的设施。
    /// 只有这类建筑区分官营民营，城墙、宫殿这类的效果对全城生效。
    pub fn is_workplace(&self, building: &str) -> bool {
        self.buildings.get(building).is_some_and(|b| !b.jobs.is_empty())
            || self.recipes.values().any(|r| r.facility.as_deref() == Some(building))
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

    pub fn skill_name<'a>(&'a self, id: &'a str) -> &'a str {
        self.skills.get(id).map_or(id, |s| s.name.as_str())
    }

    fn validate(&self) -> Result<(), ConfigError> {
        let invalid = |msg: String| Err(ConfigError::Invalid(msg));
        let seasons = self.seasons.len();
        if seasons == 0 {
            return invalid("seasons 不能为空".into());
        }
        if self.population.specialization.len() != self.population.max_skills {
            return invalid("population.specialization 的长度必须等于 max_skills".into());
        }
        if self.quality.fine_share.len() != self.population.max_skills {
            return invalid("quality.fine_share 的长度必须等于 max_skills".into());
        }
        if !self.skills.contains_key(&self.population.initial_skill) {
            return invalid(format!("population.initial_skill「{}」不在 skills 中", self.population.initial_skill));
        }
        let l = &self.loyalty;
        for (name, value) in [("guan_turnout", l.guan_turnout), ("tax_share", l.tax_share), ("min_output", l.min_output)] {
            if !(0.0..=1.0).contains(&value) {
                return invalid(format!("loyalty.{name} 必须在 0 到 1 之间"));
            }
        }
        if self.quota.exceed_ratio < 1.0 {
            return invalid("quota.exceed_ratio 不能小于 1".into());
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
                    return Err(ConfigError::Invalid(format!("{owner} 引用了不存在的物品「{good}」")));
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
                    None => return invalid(format!("建筑「{id}」提供容量的配方「{recipe}」不存在")),
                    Some(r) if r.resource.is_some() => {
                        return invalid(format!("建筑「{id}」不能给靠地块劳作的配方「{recipe}」提供容量"));
                    }
                    Some(_) => {}
                }
            }
        }
        if self.reclaim.season_mult.len() != seasons {
            return invalid("reclaim.season_mult 长度必须等于季节数".into());
        }
        for (id, terrain) in &self.terrains {
            if let Some(skill) = &terrain.reclaim_skill {
                if !self.skills.contains_key(skill) {
                    return invalid(format!("地块「{id}」的开垦技能「{skill}」不存在"));
                }
                if terrain.reclaim_labor <= 0.0 {
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
