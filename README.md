# 周代列国

设计文档在 `docs/design/`。

## 目录

- `crates/zhou-core`：规则核心，只包含游戏状态和规则，不依赖引擎和网络
- `crates/zhou-cli`：命令行工具，自动结算剧本并打印每回合报告
- `config/balance.toml`：全部数值配置，修改后重新运行即可生效
- `scenarios/`：剧本，描述开局的人物、城市、人群、士人和工事

## 运行

```sh
cargo run -p zhou-cli -- --turns 8                 # 运行示例剧本 8 回合
cargo run -p zhou-cli -- --scenario scenarios/demo.toml --config config/balance.toml --turns 20
cargo run -p zhou-cli -- --turns 20 --json > out.json   # 以 JSON 输出，便于分析
cargo test                                          # 运行规则测试
```
