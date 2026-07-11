# Slice: OS-level sandbox —— Linux 後端(Landlock + seccomp re-exec shim)

> 給 code mode 的交接規格。接續 slice-015(macOS 已交付),補上 ADR-003 D-F 的 Linux
> 後端,不擴 scope。**必須在 Linux 主機/CI 上實作與驗證** —— 本後端全程 `#[cfg(target_os
> = "linux")]`,macOS 的 `cargo build` 編不到它,所以 macOS CI 不會擋住 Linux 編譯錯誤。
> 這是**真 vertical slice**:交付一個在真實 Landlock/seccomp 下可證明的 containment,並把
> slice-015 在 Linux 上的 fail-closed 佔位換成真正的圈禁。

## 現況(slice-015 之後)

施力點與策略機制都已就位、macOS 已 enforced;Linux 目前是**誠實的 fail-closed 佔位**:

- `exec/sandbox.rs::backend_availability` 的 `#[cfg(target_os = "linux")]` 臂回
  `sandbox_linux::unavailable_reason()` → `Sandbox` 進 `Unavailable` mode。
- `exec/sandbox_linux.rs` 只有 `unavailable_reason()` 一個函式。
- 於是 Linux 上:engineering tier → 每個 RUN/gate spawn 回 `RunError::Sandbox(Refused)`
  → 記 blocker、不裸跑;light tier → 裸跑但 `warning()` 寫進 report。**安全但沒圈。**

本 slice 把這格填實:Linux 上 `Sandbox` 進 `Confined`,`argv_prefix` 回一個 re-exec shim
前綴,shim 在**單執行緒**下套 Landlock(檔案系統)+ seccomp(網路)後 `execvp` 真命令。

## 已預先落地(macOS 可驗部分,2026-07-12,四道門檻全綠)

為把本 slice 真正 Linux-only 的面縮到最小,以下**已在 macOS 上寫好並驗證**,slice-016 只需
填 shim 主體:

- **`main.rs` runtime 重構 + `__sandbox` 前置分派**:`#[tokio::main]` 改成手動建
  multi-thread runtime + `block_on`,並在**進 runtime 前**攔 `orvena __sandbox`(單執行緒,
  async-signal-safe 的前提就位)。既有 CLI 測試(`first_run.rs`)全綠;`orvena __sandbox` 目前
  由 [`crates/orvena-cli/src/sandbox_shim.rs`](crates/orvena-cli/src/sandbox_shim.rs) 的
  `dispatch()` **fail-closed**(印訊息、`exit(70)`,絕不裸跑 wrapped 命令)。slice-016 只要把
  `dispatch()` 的主體換成「解析 `--spec`/`--` → 套 Landlock+seccomp → `execvp`」(Linux 臂)。
- **CI 加 macOS leg**:`.github/workflows/ci.yml` 的 `build-test` 改成
  `matrix.os = [ubuntu-latest, macos-latest]`,四道門檻兩平台都跑。**slice-015 的 macOS
  containment 測試自此在 CI 持續驗證**(先前 CI 只有 ubuntu,macOS enforcement 從未進 CI)。
  slice-016 的 Linux containment 測試會落在既有的 ubuntu leg,CI 結構已就緒。

因此本 slice 的 AC-6(分派在 runtime 前)與 AC-V4 的 CI 結構**已完成**;剩下的是 shim 主體
(`sandbox_linux.rs` 的 Landlock/seccomp/execvp)、後端接線、target-gated 依賴、cfg 邊界修正、
與 Linux 端到端測試 —— 全部需在 Linux 主機驗。

## 為什麼是 re-exec shim,不是 `pre_exec`(ADR-003 D-F 定案)

fork 後、exec 前只能呼叫 async-signal-safe 函式;而 orvena 主行程是 tokio 多執行緒,
Landlock/seccomp crate 在套限制時會配置記憶體 —— 在 `Command::pre_exec` 裡做有 UB 風險。
re-exec shim 讓限制在一個**全新、單執行緒**的行程(`orvena __sandbox …`)裡施加,施加完
`execvp` 目標 argv,完全避開該風險。與 macOS 外部 `sandbox-exec` 對稱,兩平台都收斂成
**純 argv 前綴**,`CommandRunner` 端一行都不用為平台分叉。

## Frontmatter

```yaml
slice_id: slice-016-os-sandbox-linux
title: OS-level sandbox — Linux backend (Landlock + seccomp re-exec shim)
status: DRAFT
governance_tier: engineering
dependencies:
  - slice-015-os-sandbox                # 施力點、SandboxPolicy、status/fail-closed 已就位
  - ADR-003-os-sandbox-boundary         # status: ACCEPTED(D-F 指定 re-exec shim)
delivers:
  - linux:    crates/orvena-core/src/exec/sandbox_linux.rs   # available/argv_prefix/ShimSpec/run_shim
  - dispatch: crates/orvena-core/src/exec/sandbox.rs         # linux 臂接真後端(取代 unavailable 佔位)
  - shim-cli: crates/orvena-cli/src/main.rs                  # __sandbox 攔在 tokio runtime 之前
  - deps:     crates/orvena-core/Cargo.toml                  # [target.'cfg(linux)'] landlock + seccompiler
  - test-fix: crates/orvena-core/tests/sandbox_confinement.rs # 非-macos fail-closed 測試改 cfg 邊界
  - test:     crates/orvena-cli/tests/sandbox_linux.rs        # 端到端 containment(#[cfg(linux)])
  - ci:       .github/workflows/*                            # 加 ubuntu job 跑四道門檻 + linux sandbox 測試
  - docs:     SLICE-os-sandbox.md / docs/adr/ADR-003 / CHANGELOG.md
```

## Goal

在 Linux 上讓 `Sandbox::for_policy` 回 `Confined`,`argv_prefix` 回
`[<shim_exe>, "__sandbox", "--spec", <json>, "--"]` + base argv;`orvena __sandbox` 子命令
在單執行緒下:①依 spec 建 Landlock ruleset(root 子樹 + temp + `/dev` 可寫,其餘唯讀、全域
可讀可執行)→ ②`network: deny` 時套 seccomp 擋 `AF_INET`/`AF_INET6` 的 `socket(2)` → ③
`execvp` `--` 後的真 argv。

**價值判準(與 macOS 對稱):** 在真實 Landlock/seccomp 下,一個宣告 `read_only`、argv 卻
「寫 root 外 sentinel + 連一個已開的 port」的命令,run 完 **sentinel 不存在、連線失敗**,
而 root 內寫入照常成功 —— containment 從 OS 觀測得到,`RunReport.sandbox == enforced`。

## Module Boundary

- **Input:** `SandboxPolicy`(parent 端)+ `ShimSpec` JSON(shim 端)+ `--` 後的真 argv。
- **Output:** 與 macOS 完全相同的 `CommandOutput`;shim 對呼叫端透明。
- **Related files & 契約:**
  - `crates/orvena-core/src/exec/sandbox_linux.rs`
    - `pub fn available() -> bool` —— 探測 Landlock 是否可用(嘗試 `Ruleset` 建立並看
      `RulesetStatus`;kernel < 5.13 或被停用 → false)。seccomp 幾乎恆在,不另擋 build。
    - `pub fn argv_prefix(policy: &SandboxPolicy) -> Result<Vec<String>, SandboxError>` ——
      解析 shim 執行檔路徑(見下「shim 執行檔解析」),把 `ShimSpec::from(policy)` 序列化成
      JSON,回 `[shim, "__sandbox", "--spec", json, "--"]`。路徑取不到 → `SandboxError::Backend`。
    - `ShimSpec { writable: Vec<PathBuf>, deny_network: bool, fail_closed: bool }`(serde)——
      **最小 wire 契約**:只帶 shim 需要的東西(已解析的 writable 清單、網路旗標、fail-closed
      旗標),不序列化整個 `SandboxPolicy`。`writable` 由 `policy.writable_paths()` 得到。
    - `pub fn run_shim(spec_json: &str, argv: &[String]) -> !` —— shim 主體:套限制 → `execvp`。
      施加失敗且 `fail_closed` → `eprintln!` + `std::process::exit(<非0>)`(**絕不** execvp 裸跑);
      `fail_closed=false`(warn)→ 印警告後仍 execvp(與 parent 的 warn 語意一致)。
  - `crates/orvena-core/src/exec/sandbox.rs`
    - `backend_availability` linux 臂:`if sandbox_linux::available() { Available } else {
      Unavailable(reason) }`(取代目前恆 Unavailable)。
    - `backend_argv_prefix` linux 臂:`sandbox_linux::argv_prefix(policy)`。
    - **不需**給 `SandboxPolicy` 加 serde —— wire 型別是 `ShimSpec`,`SandboxPolicy` 保持純。
  - `crates/orvena-cli/src/main.rs` —— **在進 tokio runtime 前**攔 `__sandbox`:
    ```rust
    fn main() {
        let args: Vec<String> = std::env::args().collect();
        if args.get(1).map(String::as_str) == Some("__sandbox") {
            // 單執行緒:此時 tokio 尚未啟動、無其他執行緒。解析 --spec / -- ,呼叫 run_shim(非回傳)。
            orvena_cli::sandbox_shim::dispatch(&args); // 內部 std::process::exit / execvp
        }
        // 原本的 #[tokio::main] 邏輯改成手動建 runtime 後 block_on(cli::run())
    }
    ```
    這是本 slice 唯一動到 async 邊界的地方:`#[tokio::main]` 展開會在多執行緒 runtime 裡跑,
    shim **必須**在那之前分派,才保證單執行緒。
  - `crates/orvena-core/Cargo.toml` —— `[target.'cfg(target_os = "linux")'.dependencies]`
    加 `landlock = "0.4"`、`seccompiler = "0.4"`;macOS build 完全不拉這兩個。

## shim 執行檔解析(embeddable 契約)

`argv_prefix` 需要「一個會把 `__sandbox` 分派到 `run_shim` 的執行檔」。順序:

1. `ORVENA_SANDBOX_SHIM` 環境變數若設,用它(測試 / embedder 覆寫用)。
2. 否則 `std::env::current_exe()`。跑真 `orvena` binary 時它就是 orvena 自己(已含 `__sandbox`
   分派),端到端測試因此**不需**覆寫。
3. 取不到 → `SandboxError::Backend` → 依 policy fail-closed/warn。

**Embedder 契約(寫進 lib.rs 文件與 CHANGELOG):** 任何把 orvena-core 連進**自己的** binary
且要開 Linux sandbox 的 embedder,必須在 `main` 最前面把 `__sandbox` 分派到
`orvena_core::exec::sandbox::run_linux_shim(...)`(core 匯出),或設 `ORVENA_SANDBOX_SHIM`
指向 orvena binary。否則 `current_exe` 沒有 `__sandbox` 分派,shim 會失敗 → fail-closed。

## Landlock / seccomp 規格

- **Landlock(檔案系統)**
  - `CompatLevel::BestEffort` + 取 kernel 支援的最高 `ABI`;但**建立後檢查 `RulesetStatus`**:
    `NotEnforced`(kernel 不支援)時,`available()` 應已回 false;若 race 到此,`fail_closed`
    → shim 退非 0,不 execvp。
  - 全域授予 **read + execute**(讓 `/bin/sh`、`cargo`、動態庫、`/proc` 讀取等能跑):
    對 `/` 加 `AccessFs::ReadFile | ReadDir | Execute`(依 ABI 能力,best-effort)。
  - 對 **writable 清單**(`spec.writable` + shim 恆加 `/dev`)授予完整寫入類 access
    (`WriteFile | MakeReg | MakeDir | RemoveFile | RemoveDir | ...`,best-effort 依 ABI)。
    `/dev` 恆加:否則寫 `/dev/null` 被擋會弄壞一票程式(與 macOS profile 對稱)。
  - writable 路徑須存在才能建 rule(`path_beneath` 會 open 路徑);不存在的 strict 路徑 →
    略過並 `eprintln!` 一行,或建立前由 parent 保證存在(root/temp/dev 恆存在)。
- **seccomp(網路,`deny_network` 才套)**
  - 過濾 `socket(2)`:`domain == AF_INET(2) || domain == AF_INET6(10)` → 回 `EACCES`
    (`SeccompAction::Errno`)。**保留 `AF_UNIX`**(本機 IPC、部分 toolchain 需要)。
  - 預設動作 `Allow`(白名單只擋網路 domain,不做全域 syscall 白名單 —— 那會脆得像
    deny-default SBPL,超出本 slice)。
  - 32-bit / `socketcall` 邊角:x86_64 只需 `socket`;若要涵蓋 i686 另註,MVP 限 x86_64/aarch64。
  - 備註:Landlock ABI v4(kernel 6.7+)的 network rules 可擋 TCP bind/connect,但只涵蓋 TCP
    且要很新 kernel;seccomp 擋 socket domain 覆蓋更廣(含 UDP/raw),故選 seccomp 為主,
    Landlock-net 留作後續。

## Acceptance Criteria

### 後端接線(`sandbox.rs` / `sandbox_linux.rs`)
- [ ] AC-1:Linux 上 Landlock 可用時 `Sandbox::for_policy` 回 `Confined`、`status()==Enforced`;
      不可用時維持 slice-015 的 `Unavailable` + `on_unavailable`(fail_closed/warn)語意。
- [ ] AC-2:`argv_prefix` 回 `[shim, "__sandbox", "--spec", <json>, "--"]` + base argv;
      shim 執行檔按「env → current_exe → Backend error」順序解析。
- [ ] AC-3:`ShimSpec` 只帶 `writable`/`deny_network`/`fail_closed`;`writable` == `policy.
      writable_paths()`;JSON round-trip 可反序列化回相等值。
- [ ] AC-4:`run_shim` 施加失敗 + `fail_closed` → 非 0 退出且**不 execvp**;`warn` → 印警告後
      execvp;成功 → `execvp` `--` 後 argv(exit code / stdio 由真命令決定,parent 的 timeout
      仍有效)。
- [ ] AC-5:RUN 的「不經 shell 解譯」保留 —— shim `execvp` 目標 argv 逐字,不引入 shell;
      `run_shell`(gate)仍是 `["sh","-c",str]`,一樣被 shim 包。

### CLI 分派(async-signal-safety)
- [x] AC-6:`orvena __sandbox …` 在**進 tokio runtime 之前**於單執行緒分派;一般子命令
      (init/run/bench/doctor/status)行為與退出碼不變(`__sandbox` 是隱藏、非文件化子命令)。
      **已落地**(`main.rs` + `sandbox_shim.rs`);目前 `dispatch()` fail-closed,slice-016 填主體。
- [x] AC-7:`main` 重構後既有 CLI 測試(`first_run.rs` 等)全綠 —— runtime 改手動建 +
      `block_on` 不改變任何使用者可見行為。**已驗證**。

### 平台隔離 / 相容
- [ ] AC-8:`landlock`/`seccompiler` 為 `cfg(target_os="linux")` target 依賴;**macOS
      `cargo build/test/clippy` 不拉、不編這兩個 crate**,slice-015 的 macOS 路徑零回歸。
- [ ] AC-9:更新 `tests/sandbox_confinement.rs` —— 原 `#[cfg(not(target_os = "macos"))]` 的
      fail-closed 測試收窄成 `#[cfg(not(any(target_os="macos", target_os="linux")))]`;Linux 改由
      新的端到端 containment 測試涵蓋(否則 Linux 上「預期 Unavailable」會與新 Enforced 相矛盾)。

### Verification(gate 證據)
- [ ] AC-V1:單元測試(linux-gated)—— `ShimSpec` serde round-trip、`argv_prefix` 形狀 +
      env 覆寫解析、`available()` 冒煙。
- [ ] AC-V2:**containment 端到端(關鍵,`crates/orvena-cli/tests/sandbox_linux.rs`,
      `#[cfg(target_os = "linux")]`)** —— 驅動真 `orvena` binary:
      ①`orvena __sandbox --spec <deny,root-only> -- sh -c 'echo pwned > <root外>'` → 命令失敗、
      sentinel 不存在;②`… -- sh -c 'echo hi > <root內>'` → 成功、檔案存在;
      ③開一個 `TcpListener` 取 port,`… -- <連該 port>` → 失敗(control:同命令無 shim 前綴 →
      成功;control 不成立則 skip,不 false-fail)。
- [ ] AC-V3:**dogfood** —— 在 Linux 上 `enabled:true, network:deny(vendored deps 或 offline
      gate), root_write` 跑一次真 `orvena run`,`RunReport.sandbox == enforced` 且 loop 照常完成、
      落地 evidence bundle(對稱 macOS 的 `first_run.rs`)。
- [~] AC-V4:**CI 兩平台 matrix 已就緒**(`ubuntu-latest` + `macos-latest`,四道門檻),
      **已預先落地**。本 slice 需讓新的 linux sandbox 端到端測試(AC-V2)在 ubuntu leg 上跑綠;
      四道門檻在**兩個平台**都綠才算 slice 完成。

## Scope

### In Scope
1. `sandbox_linux.rs`:`available` / `argv_prefix` / `ShimSpec` / `run_shim`(Landlock + seccomp)。
2. `sandbox.rs` linux 兩臂接真後端;core 匯出 `run_linux_shim`。
3. `main.rs` 在 runtime 前分派 `__sandbox`;runtime 改手動建 + `block_on`。
4. Cargo target-gated 依賴;`sandbox_confinement.rs` 的 cfg 邊界修正。
5. linux 單元測試 + `sandbox_linux.rs` 端到端 containment 測試。
6. CI ubuntu job;docs(slice-015 AC-9 打勾、ADR-003 D-F 標記已實作、CHANGELOG)。

### Out of Scope(明確不做)
- ❌ 包住第三方 agent(Aider/OpenHands/Claude Code)—— 仍是後續 adapter slice。
- ❌ Landlock ABI v4 network rules / 全域 seccomp syscall 白名單 —— 過度收斂,留 hardening。
- ❌ Windows 後端 —— 續走 fail-closed/warn。
- ❌ i686/`socketcall` 完整覆蓋 —— MVP 限 x86_64/aarch64,其餘另註。
- ❌ 動 macOS profile 或 `SandboxPolicy` 型別(除了讓 core 匯出 shim 入口)。

## 交接給 code mode 的一句話

> 在 Linux 主機上填實 slice-015 的 Linux 佔位:`sandbox_linux.rs` 提供 `available()`(探
> Landlock)、`argv_prefix()`(回 `[shim,"__sandbox","--spec",json,"--"]`,shim 執行檔按
> env→current_exe 解析)、`run_shim()`(單執行緒套 Landlock「root+temp+/dev 可寫、其餘唯讀、
> 全域可讀執行」+ seccomp 擋 AF_INET/INET6 socket,失敗且 fail_closed 則退非 0 不 execvp,否
> 則 execvp `--` 後 argv);`main.rs` 在 tokio runtime 前分派 `__sandbox`;依賴 target-gate 到
> linux;修 `sandbox_confinement.rs` 的 cfg 邊界;加 `orvena-cli/tests/sandbox_linux.rs` 用真
> binary 證明「寫 root 外被擋、開網路被擋、root 內成功」;CI 加 ubuntu job,兩平台四道門檻全綠。
