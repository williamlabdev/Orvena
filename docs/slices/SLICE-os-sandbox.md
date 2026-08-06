# Slice: OS-level sandbox —— 子行程邊界圈禁(vertical slice)

> 給 code mode 的交接規格。實作 D5「enforcement 下移到 OS 邊界」的第一步,不擴 scope。
> 對應 `docs/benchmark-governance-differential-plan.md` §5 形態 A 的地基
> (「容器 + 唯讀掛載 + 只開 writes 路徑」的**單機、無容器**版本)。
> 這是**真 vertical slice**:交付一個可證明的 containment —— 一個宣稱 `read_only`
> 卻嘗試寫 root 外 / 開網路的命令,被 OS 擋下,evidence 從觀測生成。

## 為什麼需要它(現況缺口)

今天的 enforcement 全在 in-process tool 層:

- `FsTool::resolve_in_root` —— 圈住**模型經 WRITE 的寫入**,但這是純 Rust `std::fs`,
  管不到被 spawn 的子行程。
- `ShellTool` / `commands.yaml` 的 `intent` —— 是**人的信任聲明**,runtime 明說「不試圖
  證明一個命令真的 read_only」(見 `config/commands.rs` doc comment)。

於是一旦 `<<<RUN name>>>` 或 gate 的 `verify` 真的 spawn 出去,那個 child 帶著 orvena
行程的**全部環境權限**:它能寫 `~/.ssh`、寫 root 外任意檔案、開網路外連。in-process 的
scope lock **只約束自家迴圈**(治理文件原話),伸不進 child。這一格就是 sandbox 要補的。

本 slice 把 containment **下移到子行程 spawn 的那一刻**:每個由 orvena 觸發的 child 都在
最小權限的 OS sandbox 裡跑 —— 預設 root 外唯讀、無網路 —— 所以一個說謊的 read_only 命令、
一個被污染的 toolchain、或一個惡意 build script,都圈不出 root、也打不出電話。

## Frontmatter

```yaml
slice_id: slice-015-os-sandbox
title: OS-level sandbox — confine every spawned child to least privilege
status: IMPLEMENTED (macOS enforced & verified; Linux backend deferred to follow-up)
governance_tier: engineering   # dogfood:本 slice 全程對自己跑 engineering tier
dependencies:
  - slice-002-shell-run-tool            # 共用 CommandRunner 是唯一施力點
  - ADR-001-shell-tool-security-model   # status: ACCEPTED(intent 是信任聲明,本 slice 補其強制面)
  - ADR-003-os-sandbox-boundary         # status: ACCEPTED(本 slice 隨附,見下「隨附 ADR」)
delivers:
  - sandbox:  crates/orvena-core/src/exec/sandbox.rs      # SandboxPolicy + Backend + 能力偵測
  - macos:    crates/orvena-core/src/exec/sandbox_macos.rs # sandbox-exec + SBPL profile 生成
  - linux:    crates/orvena-core/src/exec/sandbox_linux.rs # Landlock(+seccomp)re-exec shim
  - exec:     crates/orvena-core/src/exec.rs              # CommandRunner::with_sandbox,spawn 前套 argv 前綴
  - shim:     crates/orvena-cli/src/commands/sandbox.rs   # 隱藏子命令 `orvena __sandbox`(Linux shim)
  - config:   crates/orvena-core/src/config/sandbox.rs    # sandbox.yaml / orvena.yaml 的 sandbox 區塊
  - wiring:   crates/orvena-core/src/agent/driver.rs      # 由 config+root+scope 建 Sandbox,注入 RUN 與 gate
  - gate:     crates/orvena-core/src/governance/gate.rs   # GateRunner 收 Sandbox,verify 也被圈
  - report:   crates/orvena-core/src/metrics/*            # run report 記 sandbox 狀態(enforced / unavailable)
  - scaffold: crates/orvena-cli/src/scaffold/orvena.yaml  # 預設 sandbox 區塊 + 說明註解
  - deps:     workspace 加 landlock = "0.4"(僅 linux target)
  - verify:   單元測試 + macOS/Linux containment 整合測試
```

## 實作狀態(2026-07-12,四道門檻全綠:build · test 102 passed/0 failed · clippy · boundary-check)

**已交付並在本機(macOS)實測驗證:**
- 施力點:`CommandRunner` 加 `sandbox` 欄位,`new()` 保持 `Disabled`(既有 `exec.rs` 測試逐字未改仍過),`with_sandbox()` 注入;`spawn_and_wait` 重構成 base argv + sandbox argv 前綴。
- macOS 後端:`sandbox-exec` + subtractive SBPL(`allow default` → `deny file-write* (subpath "/")` → 重新放行 `/dev` + root + temp;`network: deny` 加 `(deny network*)`)。
- 策略/能力偵測/fail-closed:`exec/sandbox.rs` 全套,`on_unavailable` 由 `Tier::enforces()` 導出。
- Config:`orvena.yaml` 的 `sandbox:` 選填區塊(`SandboxConfig`,serde default = disabled),`Config::validate` 併入,scaffold 預設 `enabled: true` + 摩擦說明註解。
- Wiring:driver 建單一 `Sandbox` 注入 RUN 與 gate;`RunReport.sandbox: SandboxStatus` 記錄 enforced/disabled/unavailable + warn blocker。
- **Containment 實測(關鍵):`tests/sandbox_confinement.rs` 在真實 `sandbox-exec` 下證明「寫 root 外被 OS 擋、開網路被 OS 擋、寫 root 內成功」;`first_run.rs` 端到端證明 enforced 沙箱下 `orvena run` 照常完成並落地 evidence bundle(AC-V3 dogfood)。**

**本 slice 刻意延後(honest gap,已 fail-closed 兜底):**
- **Linux Landlock/seccomp 後端(AC-9 / AC-V2-linux)**:目前 `sandbox_linux.rs` 回報 unavailable,因此 Linux 上 engineering tier **fail-closed**(拒跑,不裸跑)、light tier warn。真正的 Landlock+seccomp re-exec shim(含 `orvena __sandbox` 隱藏子命令)需在 Linux 主機上實作與驗證,列為緊接的 follow-up slice。理由:在本 macOS 環境無法編譯/驗證 Landlock,寧可**明確不可用 + fail-closed** 也不 ship 無法驗證、可能給人假安全感的程式碼。
- macOS deny-default 嚴格 profile(留 hardening ADR)。

---

## Goal

讓**每個由 orvena spawn 的子行程**(`<<<RUN>>>` 命令、gate 的 `verify` 命令)在最小權限
OS sandbox 裡執行:預設 **root 子樹外唯讀、拒網路**。價值判準:一個宣告 `read_only`、argv
卻嘗試「寫 root 外 sentinel + 開 TCP 連線」的命令,run 完後 **sentinel 不存在、連線失敗**,
而同一命令對 root 內的寫入照常成功 —— containment 從 OS 觀測得到,不靠信任 `intent`。

非目標:不改模型協議、不改 `intent` 語意(它仍是人的信任聲明,只是不再是**唯一**防線)、
不動 `FsTool` 的 in-process 邊界(那條正交且保留為 defense-in-depth)。

## Module Boundary

- **Input:** 一個要 spawn 的 base argv(`run_argv` 的固定 argv,或 `run_shell` 的
  `["sh","-c",str]`)、一個 `Sandbox`(由 config + root + scope 建好)。
- **Output:** 與現在完全相同的 `CommandOutput { stdout, stderr, exit_code, timed_out }` ——
  sandbox 對呼叫端透明;差別只在 child 的權限被 OS 收窄。
- **Related files:**
  - `crates/orvena-core/src/exec/sandbox.rs` —— `SandboxPolicy`、`Sandbox`(enum:`Confined(Backend)` / `Disabled`)、`Sandbox::for_policy(policy) -> Sandbox`(按 `target_os` + 能力偵測選 backend)、`argv_prefix()`。
  - `crates/orvena-core/src/exec/sandbox_macos.rs` —— 由 policy 生成 SBPL,回 `["sandbox-exec","-p",profile]`。
  - `crates/orvena-core/src/exec/sandbox_linux.rs` —— 回 `[current_exe,"__sandbox","--policy",json]`;shim 主體(套 Landlock/seccomp 後 `execvp` 剩餘 argv)。
  - `crates/orvena-core/src/exec.rs` —— `CommandRunner` 加 `sandbox: Sandbox` 欄位;`new(...)` 保留(= `Disabled`,向後相容既有測試),新增 `with_sandbox(cwd, timeout, sandbox)`;`spawn_and_wait` 在建 `Command` 前把 `sandbox.argv_prefix()` 接到 base argv 前。
  - `crates/orvena-cli/src/commands/sandbox.rs` —— 隱藏子命令 `orvena __sandbox --policy <json> -- <argv...>`(Linux:apply → execvp;非 Linux:直接 execvp,理論上不被呼叫)。
  - `crates/orvena-core/src/config/sandbox.rs` —— `SandboxConfig`;serde `default` 讓舊 config 無縫。
  - `crates/orvena-core/src/config/mod.rs` —— `Config` 加 `sandbox`;`orvena.yaml` 內選填 `sandbox:` 區塊(非新檔,收斂到既有檔)。
  - `crates/orvena-core/src/agent/driver.rs` —— 建**一個** `Sandbox`(root + scope writable + config),同時交給 `ShellTool` 的 runner 與 `GateRunner`。
  - `crates/orvena-core/src/governance/gate.rs` —— `GateRunner::run(gate, cwd, &sandbox)`;`run_verify` 用 `CommandRunner::with_sandbox`。
  - `crates/orvena-core/src/agent/context.rs` —— system prompt 一句話告知模型:命令在 sandbox 內跑(root 外唯讀、無網路),以校正它對「能做什麼」的預期。

## 隨附 ADR(ADR-003 核心決策,寫進 `docs/adr/`,本節為摘要)

- **D-A 施力點:子行程邊界,不是容器。** 單機、單一靜態 binary 分發是不可替代功能
  (治理文件 §5),不能要求 Docker。因此用 **OS 原生機制**(macOS `sandbox-exec` / Linux
  Landlock),不引入 runtime 依賴。容器化留給未來「形態 A 完整版」。
- **D-B 預設策略:subtractive,不是 deny-default。** macOS 全量 `deny default` SBPL 極脆
  (要逐一放行 mach lookup / sysctl,程式常起不來)。採 `allow default` 再**扣掉**
  「root 外寫入」與「網路」——即「唯讀掛載 except root + 拒網路」。強度足夠(擋 exfil、
  擋出界寫入),relaxed-but-real;deny-default 嚴格檔留作後續 hardening ADR。
- **D-C writable 集合:root 子樹 + 系統 temp。** build/test 合法要寫 `target/`、
  `node_modules/`、快取 —— 這些在 root 內,`root_write` 模式直接放行,零摩擦。`strict`
  模式(只放行 scope 的 `allowed_modifications` + 明列 build cache)是 opt-in 的
  regulated 敘事,不是預設。
- **D-D 網路預設 `deny`。** 但 `cargo build` 首跑要抓 crates —— 摩擦真實。決策:預設 deny,
  scaffold 註解明講「首跑/需連網的命令請 `network: allow` 或先 vendor 依賴」;dogfood 用
  vendored 依賴跑 deny。**離線可重現正是 benchmark 資產**,方向一致。
- **D-E 不可用時 fail-closed(綁 tier)。** sandbox 開啟但平台機制不可用(`sandbox-exec`
  不存在 / Landlock kernel 太舊 / 要求 deny 網路但 kernel 不支援)時:
  `on_unavailable` 預設由 `Tier::enforces()` 決定 —— **engineering → fail_closed**
  (命令拒 spawn,記 blocker,絕不裸跑);**light → warn**(裸跑但把警告寫進 evidence 與
  report,永不靜默)。呼應 gate.rs 既有「絕不吐空 evidence / fail closed」哲學。
- **D-F Linux 用 re-exec shim,不用 `pre_exec`。** fork 後、exec 前只能呼叫
  async-signal-safe 函式,而 tokio 多執行緒 + Landlock crate 會配置記憶體 —— `pre_exec`
  施加 Landlock 有 UB 風險。改用 `orvena __sandbox` 自我 re-exec shim:child 先變成 shim、
  shim 套限制後 `execvp` 真命令。與 macOS 的外部 `sandbox-exec` 對稱,都收斂成**純 argv 前綴**。

## Acceptance Criteria

### 施力點與透明性(`exec.rs` / `sandbox.rs`)
- [x] AC-1:`CommandRunner::new(cwd, timeout)` 行為不變(= `Sandbox::Disabled`),既有 `exec.rs` 單元測試**逐字不改**仍過(向後相容)。
- [x] AC-2:`CommandRunner::with_sandbox(cwd, timeout, sandbox)` 在 spawn 前把 `sandbox.argv_prefix()` 接到 base argv **最前面**;`Disabled` 的前綴為空 → 與現況等價。
- [x] AC-3:sandbox 對呼叫端透明 —— 回傳仍是 `CommandOutput`,`success()`/`timed_out` 語意不變;`run_argv` 的「不經 shell 解譯」性質保留(sandbox-exec / shim 都 `exec` 目標 argv,不引入 shell)。
- [x] AC-4:`run_shell`(gate 路徑,人寫字串,仍 `sh -c`)同樣被 sandbox 包住。

### 策略與能力偵測(`sandbox.rs`)
- [x] AC-5:`SandboxPolicy` 由 `{ root_canon, network: Deny|Allow, filesystem: RootWrite|Strict{writable}, extra_writable(temp), on_unavailable }` 構成;`Strict` 的 writable 由 scope 的 `allowed_modifications` + 明列 build cache 導出。
- [x] AC-6:`Sandbox::for_policy` 按 `target_os` 選 backend 並做**能力偵測**(macOS:`sandbox-exec` 存在;Linux:Landlock ABI 可用);不可用時依 `on_unavailable`:`fail_closed` → `Sandbox::Unavailable`(spawn 一律 `Err`),`warn` → `Sandbox::Disabled` + 一次性警告旗標。
- [x] AC-7:`on_unavailable` 未在 config 明設時,預設由 `Tier::enforces()` 導出(engineering=fail_closed、light=warn)。

### 平台後端
- [x] AC-8(macOS):由 policy 生成的 SBPL 為 `allow default` + `(deny file-write* (subpath "/"))` + `(allow file-write* (subpath "<root_canon>"))` + temp;`network: deny` 時加 `(deny network*)`。root 路徑正確 canonical 化並跳脫引號。
- [ ] AC-9(Linux):**DEFERRED** —— 目前 `sandbox_linux.rs` 回報 unavailable(fail-closed 兜底)。`argv_prefix` 回 `[current_exe, "__sandbox", "--policy", <json>]`、shim 建 Landlock ruleset + seccomp 擋 `socket(2)` 的完整實作,留給 Linux 主機上的 follow-up slice。
- [x] AC-10:非 macOS/非 Linux target → backend 一律「不可用」,套 AC-6 邏輯(engineering 專案在不支援平台上 fail-closed,不裸跑)。

### Config + wiring(**本 slice 的整合重點**)
- [x] AC-11:`orvena.yaml` 無 `sandbox:` 區塊時 `SandboxConfig::default()`(向後相容);`Config::validate` 對衝突設定(如 `filesystem: strict` 但無任何 writable)給 `Error::Config`。
- [x] AC-12:`driver.rs` 建**單一** `Sandbox` 並同時注入 RUN(`ShellTool` 的 runner)與 `GateRunner`;RUN 與 gate 的 child **都**被圈。
- [x] AC-13:sandbox 不可用且 `fail_closed` 時,RUN 命令的 spawn 失敗比照 slice-002 AC-9 的執行失敗**evidence-only 餵回**、但**額外**在 report 記一個 blocker(「sandbox unavailable, refused to run unconfined」);gate 的 verify 同樣 `passed=false` + 逾時式 evidence。
- [x] AC-14:run report 記錄 sandbox 狀態(`enforced` / `disabled(warn)` / `unavailable`),讓 evidence 鏈可稽核「這次 run 到底有沒有圈」。

### 不回歸
- [x] AC-15:既有 `fs.rs`/`grep.rs`/`shell.rs`/`gate.rs`/`step.rs`/`driver.rs` 測試與 slice-002 round-trip 全綠;sandbox 預設不改變**通過**路徑的可觀測輸出(只改變越界路徑的結果)。

### Verification(gate 證據)
- [x] AC-V1:單元測試 —— `sandbox.rs`(policy 建構、`for_policy` 選 backend、能力偵測 stub、fail_closed→spawn `Err`、tier→`on_unavailable` 導出)、macOS profile 生成(路徑跳脫、network deny/allow 分支)、`config/sandbox.rs`(default / strict 無 writable → `Error::Config` / network 反序列化)。
- [x] AC-V2:**containment 整合測試(關鍵)** —— `tests/sandbox_confinement.rs`:
  - `#[cfg(target_os = "macos")]`:宣告一個 `read_only` 命令,argv 為「寫 root 外 sentinel + 對本機開一個 TCP 連線」;斷言 run 後 **root 外 sentinel 不存在**、且 `network: deny` 下連線**失敗**;同時「寫 root 內檔案」的命令**成功**。
  - `#[cfg(target_os = "linux")]`:同上,經 Landlock/seccomp shim。
  - 其他平台:斷言 engineering + enabled → `Sandbox::Unavailable` 且命令拒 spawn(fail-closed)。
- [x] AC-V3:**dogfood 整合** —— 專案自身以 `sandbox.filesystem: root_write`、`network: allow`(或 vendored deps 下 `deny`)跑 `cargo test` RUN,證明 sandbox 不擋自家合法 build/test 寫入(`target/` 在 root 內)。
- [x] AC-V4:`cargo build --workspace` · `cargo test --workspace` · `cargo clippy --workspace --all-targets -- -D warnings` · `scripts/boundary-check.sh` 全綠(macOS 開發機為準;Linux CI 跑 Linux 分支)。

## Scope

### In Scope
1. `exec/sandbox.rs` —— `SandboxPolicy` / `Sandbox` / `for_policy` / `argv_prefix` / 能力偵測 / fail-closed。
2. macOS backend(`sandbox-exec` + SBPL 生成)。
3. Linux backend(`orvena __sandbox` re-exec shim:Landlock 檔案系統 + seccomp 網路)。
4. `CommandRunner::with_sandbox` + `spawn_and_wait` 的 argv 前綴接入(RUN 與 gate 共用)。
5. `config/sandbox.rs` + `orvena.yaml` 選填 `sandbox:` 區塊 + `Config` 接線 + `validate`。
6. `driver.rs` 建 Sandbox 並注入 RUN 與 gate;`gate.rs` 收 Sandbox。
7. run report 記 sandbox 狀態;`context.rs` 一句話告知模型。
8. scaffold `orvena.yaml` 預設 sandbox 區塊 + 摩擦說明註解;`docs/adr/ADR-003`。
9. 上述所有測試(含 containment 整合測試)。

### Out of Scope(明確不做)
- ❌ 容器 / Docker / 唯讀 bind-mount —— 那是「形態 A 完整版」,本 slice 是單機 OS 原生版。
- ❌ 包住**第三方 agent**(Aider / OpenHands / Claude Code)—— 本 slice 只圈 orvena 自家 spawn 的 child;adapter 是後續 slice(D5 順序:Aider → OpenHands → Claude Code)。
- ❌ worktree 隔離 + 獨立 diff 稽核 —— 正交能力,另開 slice。
- ❌ 動 `FsTool::resolve_in_root` 或 `intent` 語意 —— 兩者保留為 defense-in-depth。
- ❌ macOS deny-default 嚴格 profile —— 先出 subtractive 版(D-B),嚴格檔留 hardening ADR。
- ❌ Windows 沙箱後端 —— 非開發平台;走 AC-10 的「不可用 → fail-closed/warn」。

## 交接給 code mode 的一句話

> 把 containment 下移到子行程邊界:在 `CommandRunner`(RUN 與 gate 的唯一 spawn 匯流點)前
> 綴一層 OS sandbox —— macOS 用 `sandbox-exec` + subtractive SBPL(root 外唯讀、拒網路),
> Linux 用 `orvena __sandbox` re-exec shim(Landlock + seccomp),不可用時依 tier
> fail-closed。`new()` 保持 Disabled(既有測試不改),sandbox 由 driver 從 config+root+scope
> 建好注入 RUN 與 gate。用 macOS/Linux `tests/sandbox_confinement.rs` 證明「宣稱 read_only
> 卻想寫 root 外 / 開網路的命令被 OS 擋下,root 內寫入照常」。範圍限
> `exec/config/agent/governance/metrics/cli-shim`,四道門檻全綠。
