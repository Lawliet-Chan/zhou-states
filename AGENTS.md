# 其惟春秋（zhou-states）

游戏名「其惟春秋」。周代背景的多人策略游戏：持久世界，权威服务器，同时回合（像文明系列）。用中文交流，文档和代码注释也用中文。

## 现状

- 后端（Rust）已完成：城市经济、人口、士人、封建、外交、军事（行军、战斗、攻城、占领、按类别劫掠、粮道）、战争迷雾、WebSocket 服务器。
- Godot 4 前端（`client/`）做了：登录、地图（地形、城、疆域、军队、迷雾）、城和军队的详情、征发、行军、攻击、城墙、粮道、遣散、劫掠、宣战、讲和、日志；任命管理者、收回管理权、规定任务、上缴；封授、受封；指定纳贡、纳贡、拒绝过火的纳贡；「天下」页的待答复（任命、封授）；内政（任用和罢免士人、派工事、税率、学技能、调技能、赏赐）；右下角圆形结束回合键
- 前端还没做（都是主要内容）：交易（报告里已经有 `deals`）、通行权、占领后的接管和毁建筑

## 目录

- `crates/zhou-core`：规则核心，只有状态和规则，不依赖网络和引擎
- `crates/zhou-cli`：命令行，自动跑剧本、打印报告（`--player <id>` 输出某个玩家看到的 JSON）
- `crates/zhou-server`：WebSocket 服务器（tokio）
- `config/balance.toml`：全部数值；`scenarios/demo.toml`：示例剧本
- `docs/design/`：设计文档；**做前端先读 `protocol.md`**
- `client/`：Godot 前端。`scripts/net.gd`（WebSocket，自动加载 `Net`）、`game.gd`（状态和索引、查名称，自动加载 `Game`）、`num.gd`（定点数的显示和输入，不依赖别的脚本）、`map_view.gd`（画地图、点选）、`main.gd`（主界面交互）、`domestic.gd`（内政表单）；固定布局在 `scenes/` 和 `main.tscn`

## 规矩

- 数值一律放在 `config/balance.toml`，代码里不写死
- 数值一律用整数，不用浮点数。带小数的量用定点数 `Fx`（`crates/zhou-core/src/fx.rs`），以千分之一为单位：1.5 存为 1500。toml、存档、指令、报告里都直接写这个整数，读入时不换算；只有给人看的文字（事件、错误消息）显示成小数
- 结算时先乘整数（人数、回合数）再乘比例，少丢精度。千分之一的精度是定下来的，配置里产出率等被舍入（例如兵器 0.003333 → 3）造成的平衡变化之后再调
- 项目不在 git 下；不要自己初始化或提交，除非我要求
- 改完跑 `cargo test` 和 `cargo clippy --workspace --all-targets`，保持通过、没有警告
- 用户在意 token 用量：只读需要的文件，不整个仓库扫

## 前端（Godot）

- Godot 4.7.2 在 `/Applications/Godot.app`（命令行：`/Applications/Godot.app/Contents/MacOS/Godot`）。不要用 brew 装
- 先做 2D 俯视。没有美术素材，用程序绘制占位：六边形按地形上色，城画图形加城名，军队画旗子或圆标加人数，迷雾外的格子变暗
- 服务器说了算：前端不重写规则，只把玩家的意图发成指令，按服务器回的 `result` / `view` 刷新。可以做简单的按钮置灰，但以服务器返回的错误为准
- 名称和数值从登录时 `welcome.config` 里查（就是 `balance.toml` 转成的 JSON），前端不另外抄一份
- 前端需要看的后端代码只有这些：
  - `crates/zhou-core/src/view.rs`（`PlayerReport`）
  - `crates/zhou-core/src/turn.rs` 里的报告结构（`CityReport`、`ArmyReport`、`CellReport`……）
  - `crates/zhou-core/src/command.rs`（`Command` 枚举，指令的 JSON 格式）
- 六边形轴向坐标 `{q, r}`，奇数行向右缩进半格：`col = q + (r - (r & 1)) / 2`，`row = r`。界面上显示的坐标用 `(col,row)`，和服务器消息里一致（`Game.hex_label`）
- Godot 解析 JSON 时数字全是浮点数：读的时候要 `int()`；发指令时 `Game.send_command` 会把整数值的浮点数转成整数，否则服务器不认
- 定点数显示用 `Game.fx()`（小数）、`Game.pct()`（百分数），只用整数运算
- 字体用系统字体，冬青黑体（Hiragino Sans GB）放第一：苹方是字体集，Godot 会挑到繁体那款，缺简体字

## 联调

```sh
cargo run -p zhou-server -- --addr 127.0.0.1:7878 --turn-seconds 0
```

- 有 `saves/world.json` 就读存档，否则从 `scenarios/demo.toml` 开局；想重新开局就删存档
- 示例剧本的人物：2 周王，3 姬虎，4 史伯，5 卫伯
- 示例剧本的城：53 成周（周王），6 邶（姬虎），30 鄘（姬虎所有、史伯管理），41 卫（卫伯）
- 登录：`{"type":"login","player":3}`
- 跑前端：`/Applications/Godot.app/Contents/MacOS/Godot --path client`
- 前端自动联调（截图后退出）：`... --path client -- --server=ws://127.0.0.1:7878 --player=3 --cmd='{"type":"march_army",...}' --select=q,r --press=按钮文字 --tab=1 --screenshot=/路径.png`；`--cmd` 可以写多个，`--press` 按下详情面板里的按钮（打开对话框截图用）
- 一个人只能直接管理一座城：示例剧本里每个人物都已经在管一座城，测任命要先收回或用新剧本
- 示例剧本里没人会「军武」，征发不了；要测军队就复制一份剧本、给一群人加上 `martial`，用 `--scenario` 和 `--save` 指到临时目录

## 已经否掉的想法（不要再提）

- 随军粮：不做。打下来的城直接当粮道起点

## 还没做

- 城墙圈和墙内地块的标记，城墙扩建
- 斥候、间谍
- 登录密码、账号
- 事件还是中文文本，没有结构化的事件类型
- 视野不受地形影响；事件按回合末的视野判断谁看得到，而不是事件发生时的视野
