//! 周代列国的规则核心：只包含游戏状态和规则，不依赖任何引擎或网络。

pub mod command;
pub mod config;
pub mod inventory;
pub mod scenario;
pub mod state;
pub mod turn;

pub use command::{Command, CommandError, apply};
pub use config::Config;
pub use inventory::Inventory;
pub use scenario::load_scenario;
pub use state::{Character, GameState, Id, Mode, Task};
pub use turn::{TurnReport, end_turn};
