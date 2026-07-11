# ADR-003: OS-level sandbox —— enforcement 下移到子行程邊界

> Architecture Decision Record — 記錄一次具體的架構決策及其原因。

## Frontmatter

```yaml
doc_type: adr
adr_id: ADR-003
title: OS-level sandbox — 子行程邊界圈禁,enforcement 下移到 OS
status: ACCEPTED
superseded_by: null
created: 2026-07-12
updated: 2026-07-12
author: William Chiu
related_blueprint: null # Orvena v0.1 以 MVP-SCOPE.md + slice 文件代替 blueprint 鏈
```

## Context

### Problem

治理差分 benchmark 的方向已定案(D5):**「agent loop 是 commodity、信任
envelope 才是資產」**,而信任 envelope 要能包住第三方 agent,前提是
enforcement 從 in-process tool 層**下移到 OS/程序邊界**。這條路的第一步,是先
把 orvena **自家 spawn 的子行程**圈住 —— 因為現況有一個具體缺口:

- `FsTool::resolve_in_root` 圈住的是**模型經 WRITE 的寫入**,但那是純 Rust
  `std::fs`,管不到被 spawn 的子行程。
- `commands.yaml` 的 `intent`(ADR-001)是**人的信任聲明**,runtime 明說「不試圖
  證明一個命令真的 read_only」。

於是一旦 `<<<RUN name>>>` 或 gate 的 `verify` 真的 spawn,那個 child 帶著 orvena
行程的**全部環境權限**:寫 `~/.ssh`、寫 root 外任意路徑、開網路外連,全都可行。
in-process 的 scope lock **只約束自家迴圈**,伸不進 child。一個宣稱 `read_only`
卻說謊的命令、一個被污染的 toolchain、一個惡意 build script —— 目前沒有任何機制
攔得住。ADR-001 已誠實標註「`intent` 是信任聲明不是沙箱」;本 ADR 補上那個沙箱。

三個需要一次決策的問題:

1. 用什麼**機制**圈禁 —— 容器?OS 原生 sandbox?哪一個不違背「單一靜態 binary」?
2. 圈禁的**策略**多嚴 —— deny-default 白名單,還是 allow-default 扣除?writable
   集合怎麼定才不擋掉合法的 build/test?網路預設開還關?
3. 機制**不可用**時(舊 kernel、缺 `sandbox-exec`)怎麼辦 —— 裸跑還是拒跑?

### Stakeholders

| Role       | Person/Team          | Interest                                       |
| :--------- | :------------------- | :--------------------------------------------- |
| Maintainer | William Chiu         | containment 可對外說明;不引入 runtime 依賴      |
| Agent loop | orvena-core          | sandbox 對 RUN/gate 透明,不擋合法 build/test    |
| Embedder   | 下游使用者/CI 環境   | 嵌入 Orvena 不等於給 spawn 的 child 開全機權限   |
| Regulated  | 受監管買家           | 「離線、無外連、寫入圈在 root」是可稽核的合規敘事 |

## Decision

### Status

ACCEPTED — implemented in slice-015 (macOS backend enforced & verified; Linux
Landlock/seccomp backend deferred to a follow-up, fails closed until it lands).

### Context

關鍵洞察:**containment 的施力點不在 tool 層,而在「子行程 spawn 的那一刻」。**

- `FsTool` 的寫入是 in-process 的,已被 `resolve_in_root` 圈住 —— 正交,保留為
  defense-in-depth,本 ADR 不動它。
- RUN 與 gate 的命令都匯流到 `CommandRunner::spawn_and_wait`(slice-002 已統一)。
  那是**唯一的 spawn 匯流點**:只要在那裡前綴一層 OS sandbox,兩條路徑一次包住。

因此本 ADR 的核心是:**在 `CommandRunner` 前綴一層 OS 原生 sandbox,讓每個 child
在最小權限下跑;信任不再落在 `intent` 的宣告,而落在 OS 的觀測強制。**

### Options

- **機制**
  - **Option A — OS 原生 sandbox(選定)**:macOS `sandbox-exec`(SBPL)、Linux
    Landlock + seccomp。零 runtime 依賴,保住「單一靜態 binary 分發」這個
    不可替代功能(治理文件 §5)。
  - **Option B — 容器 + 唯讀 bind-mount**:強度最高,是「形態 A 完整版」的終點。
    但要求 Docker/runtime,牴觸單機分發;且與 native loop 的 offline-deterministic
    基準相斥。**defer** —— 作為後續 adapter slice,不是 v0.1 地基。
  - **Option C — 純事後 diff 稽核(不圈禁)**:只記錄不阻止。這是「形態 B」對遠端
    SaaS agent 的誠實兜底,但對自家 loop 而言是把「事前阻止」降級成「事後偵測」,
    與 read-only default 的即時性矛盾。否決作為主線。

- **策略(profile 形態)**
  - **Option D — deny-default 白名單**:最嚴,但 macOS 全量 `deny default` SBPL
    極脆(要逐一放行 mach lookup/sysctl,多數程式根本起不來)。留作 hardening。
  - **Option E — allow-default 扣除(選定)**:`allow default` 再**扣掉**「root 外
    寫入」與「網路」—— 即「唯讀掛載 except root + 拒網路」。強度足以擋 exfil 與
    出界寫入,relaxed-but-real,程式照常啟動。

- **writable 集合**
  - **Option F — 只放行 scope 的 `allowed_modifications`**:最小,但 `cargo test`
    要寫 `target/`、`npm test` 要寫 `node_modules/`,直接擋死合法 build。
  - **Option G — root 子樹 + 系統 temp(選定為預設 `root_write`)**:build 產物
    都在 root 內,零摩擦;`strict`(F 的行為 + 明列 build cache)作為 regulated
    敘事的 opt-in,非預設。

- **不可用時**
  - **Option H — 裸跑**:可用性優先,但一個宣稱 enforced 的 run 其實沒圈,證據
    鏈說謊。否決作為 engineering 預設。
  - **Option I — fail-closed,綁 tier(選定)**:engineering 拒 spawn 記 blocker、
    light 裸跑但把警告寫進 evidence 與 report。呼應 gate.rs 既有「絕不吐空
    evidence / fail closed」哲學,強度隨 tier。

### Decision

We decided to choose **A + E + G + I** —— 用 OS 原生 sandbox,以 allow-default
扣除策略把 child 圈在「root 內可寫、root 外唯讀、預設拒網路」,不可用時依 tier
fail-closed —— because:

1. **施力點對、施力點唯一** — RUN 與 gate 都經 `CommandRunner`,一處前綴即全包;
   不必動 tool 層,也不必為每個工具各寫一套。`FsTool` 的 in-process 邊界正交保留,
   兩層 defense-in-depth。
2. **不犧牲單機分發** — OS 原生機制不引入 Docker/runtime,「單一靜態 binary」與
   「offline-deterministic 基準」兩個不可替代功能都保住(治理文件 §5)。
3. **strength/friction 取捨務實** — allow-default 扣除擋得住真正的傷害(exfil、
   出界寫入),又不會因 deny-default 的脆弱把 `cargo build` 弄到起不來;root_write
   讓合法 build 零摩擦,strict 留給要嚴的人 opt-in。
4. **fail-closed 讓證據鏈不說謊** — 一個標 enforced 的 run,要嘛真的圈住、要嘛
   明白記下「這次沒圈」;絕不出現「以為圈了其實裸跑」的靜默狀態。

具體規格(留給 os-sandbox slice 實作,見 SLICE-os-sandbox.md):

1. **施力點** — `CommandRunner` 加 `sandbox` 欄位;`new()` 保持 `Disabled`
   (**既有 exec.rs 測試逐字不改仍過**,向後相容),新增 `with_sandbox(...)`。
   `spawn_and_wait` 在建 `Command` 前把 `sandbox.argv_prefix()` 接到 base argv
   最前面。RUN(固定 argv)與 gate(`sh -c`)都被包 —— 且 RUN 的「不經 shell
   解譯」性質保留:sandbox-exec / shim 都 `exec` 目標 argv,不引入 shell。

2. **平台後端,統一收斂成純 argv 前綴**:
   - **macOS** — `["sandbox-exec", "-p", <SBPL>]`。profile 為 `allow default`
     + `(deny file-write* (subpath "/"))` + `(allow file-write* (subpath
     "<root_canon>"))` + temp;`network: deny` 時加 `(deny network*)`。root
     路徑 canonical 化並跳脫引號。
   - **Linux** — `[<shim>, "__sandbox", "--spec", <json>, "--"]`,一個隱藏
     CLI 子命令:套 Landlock(root 子樹 + temp + `/dev` 可寫、其餘唯讀可讀可執行)
     + seccomp(deny `socket(AF_INET/AF_INET6)`)後 `execvp` 剩餘 argv。**刻意不用
     `pre_exec`** —— fork 後 exec 前只能呼叫 async-signal-safe 函式,而 tokio 多執行緒
     + Landlock crate 會配置記憶體,`pre_exec` 施加 Landlock 有 UB 風險;re-exec shim
     完全避開,且與 macOS 的外部 `sandbox-exec` 對稱。**已於 slice-016 實作**
     (`landlock` + `seccompiler`,cross-target compile-verified;runtime 待 Linux CI)。

3. **策略** — `SandboxPolicy { root_canon, network: Deny|Allow, filesystem:
   RootWrite|Strict{writable}, extra_writable, on_unavailable }`。預設
   `network: deny`、`filesystem: root_write`;`Strict` 的 writable 由 scope 的
   `allowed_modifications` + 明列 build cache 導出。

4. **網路預設 deny,但誠實標註摩擦** — `cargo build` 首跑要抓 crates。scaffold
   `orvena.yaml` 的 sandbox 區塊註解明講「首跑/需連網的命令請 `network: allow`
   或先 vendor 依賴」;dogfood 用 vendored 依賴跑 deny。離線可重現正是 benchmark
   資產,方向一致。

5. **不可用 → fail-closed,綁 tier** — `on_unavailable` 未明設時預設由
   `Tier::enforces()` 導出:**engineering → fail_closed**(`Sandbox::Unavailable`,
   spawn 一律 `Err`,記 blocker),**light → warn**(裸跑但一次性警告寫進 evidence
   與 report)。RUN 的 spawn 失敗比照 ADR-001 §3 執行失敗(evidence-only 餵回)
   **但額外**記一個 blocker;gate 的 verify 比照逾時(`passed: false` + 原因)。

6. **可稽核** — run report 記 sandbox 狀態(`enforced` / `disabled(warn)` /
   `unavailable`),讓證據包能回答「這次 run 到底有沒有圈」。

7. **模型認知校正** — `context.rs` system prompt 一句話告知模型:命令在 sandbox
   內跑(root 外唯讀、無網路),以校正它對「能做什麼」的預期。

## Consequences

### Positive

- **containment 從信任下移到觀測強制** — 一個宣稱 `read_only` 卻想寫 root 外 /
  開網路的命令被 OS 擋下;`intent` 從「唯一防線」降為「第一道分流」,ADR-001 的
  已知風險(宣告錯誤的人為失誤)被 OS 兜底。
- **保住單機分發與離線基準** — 零 runtime 依賴;`network: deny` 的離線跑法正好是
  benchmark 的可重現資產。
- **D5 地基就位** — 「enforcement 下移到 OS 邊界」的第一步落地,後續包第三方
  agent(Aider → OpenHands → Claude Code)有了可複用的 sandbox 前綴機制。
- **對 regulated 敘事直接可用** — 「離線、無外連、寫入圈在 root」是可稽核的合規
  句子,不再只是口號。

### Negative

- **allow-default 扣除不是最嚴** — 一個惡意命令仍可在 root 內搞破壞(改壞 repo)。
  緩解:strict 模式收窄 writable、未來 worktree 隔離 + diff 稽核、以及 deny-default
  hardening ADR。本 ADR 明說是 relaxed-but-real 的第一版。
- **網路 deny 有真實摩擦** — 首跑 `cargo build` 會失敗。緩解:scaffold 註解、
  `network: allow` 逃生門、vendored 依賴指引。
- **平台覆蓋不齊** — Windows 無原生後端,走 fail-closed/warn;Linux 網路 deny 需
  seccomp 且 kernel 相依。緩解:能力偵測 + tier 綁定的 fail-closed,絕不靜默裸跑。
- **Linux 多一個隱藏子命令** — `orvena __sandbox` 是 re-exec shim。這是為了避開
  `pre_exec` 的 async-signal-safety UB 所付的結構代價,視為必要成本。

### Neutral

- sandbox 對呼叫端**透明** — 回傳仍是 `CommandOutput`,`success()`/`timed_out`
  語意不變;只有越界路徑的結果改變,通過路徑的可觀測輸出不變。
- `intent`(ADR-001)與 sandbox(本 ADR)是**兩層**:前者是人的信任分流(決定
  模型「能不能觸發」),後者是 OS 的強制圈禁(決定命令「能做到什麼」)。兩者正交,
  都保留。
- 強度仍隨 tier — light 下 sandbox 不可用只警告不停,與既有 scope/gate 的 tier
  分流一致;治理強度由 tier 決定,不由 sandbox 自身決定。

## Related Decisions

| ADR / Doc                              | Relationship | Description                                          |
| :------------------------------------- | :----------- | :--------------------------------------------------- |
| ADR-001                                | Extends      | `intent` 是信任聲明不是沙箱;本 ADR 補上那個沙箱、圈住 RUN/gate 的 child |
| SLICE-os-sandbox.md                    | Implemented by | 本 ADR 的落地 slice(slice-015)                     |
| docs/benchmark-governance-differential-plan.md | Implements   | §5 / D5「enforcement 下移到 OS 邊界」的第一步        |

## References

- [SLICE-os-sandbox.md](../../SLICE-os-sandbox.md) — 落地 slice(施力點、平台後端、驗收)
- [docs/benchmark-governance-differential-plan.md](../benchmark-governance-differential-plan.md) — §5 兩種 adapter 形態、D5 裁決
- [ADR-001](./ADR-001-shell-tool-security-model.md) — 「`intent` 是信任聲明不是沙箱」的原始標註
- `crates/orvena-core/src/exec.rs` — `CommandRunner::spawn_and_wait`(唯一 spawn 匯流點,施力點)
- `crates/orvena-core/src/tools/fs.rs` — `FsTool::resolve_in_root`(正交保留的 in-process 邊界)
- `crates/orvena-core/src/config/agent.rs` — `Tier::enforces()`(fail-closed 綁定來源)

---

_ADR generated from the AI Native Software Engineering Framework ADR template._
