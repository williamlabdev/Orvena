# Slice: `shell` RUN 工具 —— 宣告式具名命令 + `CommandRunner`(vertical slice)

> 給 code mode 的交接規格。實作 ADR-001「Shell 工具安全模型」的決策,不擴 scope。
> 對應 MVP-SCOPE.md must-have #1(shell/grep)之**需要安全模型才能做的那一半**。
> 這是**真 vertical slice**:交付一個 agent 實際叫得動、能自我驗證的執行能力。

## Frontmatter

```yaml
slice_id: slice-002-shell-run-tool
title: declarative shell RUN tool — named commands + shared CommandRunner
status: DONE
governance_tier: engineering   # dogfood:本 slice 全程對自己跑 engineering tier
dependencies:
  - slice-001-grep-tool        # 鏡射其 tool + 協議 + driver wiring 模式
  - ADR-001-shell-tool-security-model   # status: ACCEPTED
delivers:
  - exec:     crates/orvena-core/src/exec.rs              # 共用 CommandRunner + timeout
  - config:   crates/orvena-core/src/config/commands.rs   # commands.yaml loader
  - tool:     crates/orvena-core/src/tools/shell.rs       # ShellTool(read_only 才可觸發)
  - protocol: crates/orvena-core/src/agent/step.rs        # 新 Action::Run
  - wiring:   crates/orvena-core/src/agent/driver.rs      # 處理 Run、evidence 回饋
  - gate:     crates/orvena-core/src/governance/gate.rs   # 改用 CommandRunner(獲得 timeout)
  - scaffold: crates/orvena-cli/src/scaffold/commands.yaml # 預設 test/build/clippy
  - role:     scaffold/roles.yaml developer 加 shell.run
  - deps:     workspace 加 wait-timeout = "0.2"
  - verify:   單元測試 + round-trip 整合測試
```

## Goal

讓 agent 能在迴圈內**跑人預先宣告的命令**(測試/build/lint)來自我驗證,並**把輸出用在下一步決策**(RUN test 失敗 → 依 evidence 改檔 → RUN test 過)。核心安全模型(ADR-001):模型永遠不提供命令字串,只能以 `<<<RUN name>>>` 引用人在 config 宣告的具名命令;runtime 只執行宣告過的名稱,用固定 argv 直接 spawn(不經 shell 解譯),injection 面直接消失。價值判準:agent 在一次 run 裡能「跑測試 → 看失敗 → 改 → 再跑」,而不是每次驗證都消耗一整個 gate loop。

## Module Boundary

- **Input:** 模型輸出 `<<<RUN name\n>>>` 區塊(只有名稱,無命令字串)。
- **Output:** 命令的 stdout/stderr + exit code,在 driver 呼叫端按 bytes/lines 截斷後併入下一輪 evidence,格式與 SEARCH 對稱。
- **Related files:**
  - `crates/orvena-core/src/exec.rs` — 新 `CommandRunner`(cwd=root、捕捉 stdout/stderr、timeout),`run_argv`(固定 argv,RUN 用)+ `run_shell`(`sh -c`,gate 用)。
  - `crates/orvena-core/src/config/commands.rs` — `Commands`/`Command`/`Intent`;`timeout_secs` 選填預設 300;載入期驗證重複 name / 空 argv。
  - `crates/orvena-core/src/config/mod.rs` — `commands.yaml` **選填**載入(`read_yaml_optional`,向後相容舊專案),`validate()` 併入 commands 檢查。
  - `crates/orvena-core/src/tools/shell.rs` — `ShellTool { root, role, commands }`,`run(name)`。
  - `crates/orvena-core/src/agent/step.rs` — `Action::Run { name }` + parse。
  - `crates/orvena-core/src/agent/driver.rs` — apply 階段 Run 臂、`cap_run_output` 截斷、evidence 回饋。
  - `crates/orvena-core/src/agent/context.rs` — system prompt 教模型 RUN 語法。
  - `crates/orvena-core/src/governance/gate.rs` — `run_verify` 改用 `CommandRunner`(獲得 timeout)。

## Acceptance Criteria

### 安全模型(`shell.rs` / `commands.rs`)
- [x] AC-1:`ShellTool::run(name)` 回 `Result<CommandOutput { stdout, stderr, exit_code, timed_out }>`。
- [x] AC-2:role-gated —— `require_tool("shell.run")`,role 不允許回 `Error::Scope`(與 `fs`/`grep` 同款)。
- [x] AC-3:授權檢查固定順序,全部回 `Error::Scope`:①role 未授權 `shell.run` → ②name 未宣告 → ③命中命令 `intent == mutating`(v0.1 模型一律不可觸發 mutating)。
- [x] AC-4:固定 argv 直接 spawn,**不經 shell 解譯**(`argv` 逐字傳遞,`$HOME`/`;` 不展開);cwd = root。
- [x] AC-5:`commands.yaml` 載入期驗證重複 `name`、空 `argv` → `Error::Config`,不留到 runtime;`timeout_secs` 選填預設 300。

### 協議 + wiring(**本 slice 的重點**)
- [x] AC-6:`step.rs` 新增 `Action::Run { name }`;`parse_actions` 解析 `<<<RUN name\n>>>`,與 WRITE/SEARCH 對稱、混排不亂序。
- [x] AC-7:`driver.rs` 在既有 parse_actions 迴圈的 match 新增 Run 臂;**不**額外 `tool_calls += 1`(match 前已通用計數);evidence 在呼叫端按 bytes/lines 截斷後寫入,格式 `RUN 'name' → exit N:\n<截斷輸出>`。
- [x] AC-8:**授權失敗**(`Error::Scope`)比照 write —— engineering tier push blocker 並 `return finished(false)`,light 續跑。
- [x] AC-9:**執行失敗**(read_only 命令 exit ≠ 0 / timeout)**evidence-only** —— 輸出餵回 `prior_evidence`,**不 push blocker、engineering 也不硬停**(比照失敗的 gate,**不**比照非法 regex)。
- [x] AC-10:既有 `Action::Write`/`Search` 路徑與 grep/search 現有測試不回歸。

### 統一執行路徑(`CommandRunner`)
- [x] AC-11:gate 與 RUN 共用 `CommandRunner`;gate 保留 `sh -c`(人寫字串)、RUN 用固定 argv。`CommandRunner` 回原始輸出(截斷在呼叫端),**不影響既有 gate evidence** 行為。
- [x] AC-12:**gate 逾時 = verify 失敗**(`GateOutcome.passed = false`,evidence 記逾時原因),不再無限 hang。`Gate.timeout_secs` 選填預設 300。

### Verification(gate 證據)
- [x] AC-V1:單元測試 —— `exec.rs`(固定 argv 不經 shell、非零 exit 被捕捉、timeout kill、spawn 失敗)、`commands.rs`(重複 name / 空 argv / intent 反序列化 / timeout 預設)、`shell.rs`(read_only 成功、mutating 拒、未宣告拒、role 拒、順序)、`gate.rs`(exit0 過、非零失敗、逾時失敗、human escalate)、`step.rs`(RUN 解析 / 混排)。
- [x] AC-V2:**round-trip 整合測試(關鍵)** —— `tests/run_roundtrip.rs` 用 scripted provider:①未宣告名稱 engineering 硬停記 blocker;②mutating 即使宣告也被拒;③「RUN test 失敗(exit≠0)→ 依 evidence 改檔 → RUN test 過」全程 engineering 不因 test 失敗中斷、最終 `finished(true)`、且**無 blocker**;④gate 逾時 → `passed=false`。
- [x] AC-V3:`cargo build --workspace` · `cargo test --workspace` · `cargo clippy --workspace --all-targets -- -D warnings` · `scripts/boundary-check.sh` 全綠。

## Scope

### In Scope
1. `exec.rs` + `CommandRunner`(共用,含 timeout)。
2. `config/commands.rs` + `commands.yaml`(選填載入 + 驗證)。
3. `tools/shell.rs` + `ShellTool`,`tools/mod.rs` 註冊。
4. `step.rs` 新增 `Action::Run` + parse;`driver.rs` Run 臂 + 截斷 + evidence。
5. `context.rs` system prompt 教 RUN 語法。
6. `gate.rs` 重構為 `CommandRunner`(獲得 timeout);`Gate.timeout_secs`。
7. scaffold `commands.yaml`(test/build/clippy read_only)+ roles.yaml developer 加 `shell.run`。
8. 上述所有測試(含 round-trip)。

### Out of Scope(明確不做)
- ❌ 參數化命令(如 `cargo test foo`);v0.1 只有固定 argv,參數化 allow-list 留給後續 ADR。
- ❌ 模型觸發 `mutating` 命令 / 任意命令字串;安全模型明示禁止。
- ❌ reviewer 預設給 `shell.run`(唯讀審查定位;要開需專案自宣告)。
- ❌ 沙箱化(`intent` 是人的信任聲明,不是沙箱)。

## 交接給 code mode 的一句話

> 依 ADR-001 在 `orvena-core` 交付宣告式 shell RUN:人在 `commands.yaml` 宣告具名命令(固定 argv + `intent`),模型以 `<<<RUN name>>>` 引用;runtime 只跑宣告過的 `read_only` 名稱、固定 argv 直接 spawn。抽共用 `CommandRunner`(gate 與 RUN 共用、加 timeout)。授權失敗比照 write 硬停、執行失敗 evidence-only 比照失敗的 gate。用 scripted provider 寫 round-trip 證明「RUN 失敗 → 改檔 → RUN 過」。範圍限 `exec/config/tools/agent/governance`,四道門檻全綠。
