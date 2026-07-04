# ADR-001: Shell 工具安全模型

> Architecture Decision Record — 記錄一次具體的架構決策及其原因。

## Frontmatter

```yaml
doc_type: adr
adr_id: ADR-001
title: Shell 工具安全模型 — 宣告式命令 allow-list
status: PROPOSED
superseded_by: null
created: 2026-07-04
updated: 2026-07-04
author: William Chiu
related_blueprint: null # Orvena v0.1 以 MVP-SCOPE.md + slice 文件代替 blueprint 鏈
```

## Context

### Problem

Agent 需要在迴圈中執行命令(跑測試、build、lint)來自我驗證,這是 MVP
must-have #1 的後半(前半 grep 已由 slice-001 交付)。但「給模型一個 shell」
與 Orvena 的兩條核心紀律直接衝突:

- **Scope lock / read-only default** — `FsTool` 的每次寫入都經過
  `ScopeDecision`;任意 shell 可以 `echo > 任何檔案`、`rm -rf`,等於在
  scope lock 旁邊開一扇沒有鎖的門。整套治理的可信度歸零。
- **Evidence & done** — 任意命令的副作用不可枚舉,無法納入 run report 的
  證據鏈。

同時,codebase 裡已存在一條「執行命令」的路徑:`GateRunner::run_verify`
用 `sh -c` 在專案根目錄執行 gate 的 `verify` 命令。本 ADR 必須回答三個
slice-001 擋下的問題:

1. 是否用 allow-list(只准 test/build/lint 這類命令)?
2. 寫入類命令是否一律走 `human` gate?
3. 與現有 `verify` gate 執行命令的機制如何統一?

### Stakeholders

| Role       | Person/Team          | Interest                                   |
| :--------- | :------------------- | :----------------------------------------- |
| Maintainer | William Chiu         | 治理紀律不被繞過;安全模型可對外說明        |
| Agent loop | orvena-core          | 迴圈內可自我驗證,縮短 observe→re-attempt   |
| Embedder   | 下游使用者/CI 環境   | 嵌入 Orvena 不等於給 LLM 開任意執行權限     |

## Decision

### Status

PROPOSED

### Context

關鍵洞察:**信任邊界不在「執行不執行命令」,而在「命令字串是誰寫的」。**

- Gate 的 `verify` 命令是人在 `gates.yaml` 裡寫死的 —— 人授權過的字串,
  `sh -c` 可接受。
- Shell 工具的命令若由模型組字串,就是不可信輸入 —— 無論加多少過濾
  (escape、黑名單)都是在跟 injection 打地鼠。

因此安全模型的核心是:**模型永遠不提供命令字串,只能引用人預先宣告的
命令名稱。**

### Options

- **Option A — 不做 shell 工具(維持現狀)**:模型只有 WRITE/SEARCH;驗證
  完全依賴 gate 迴路(gate 失敗輸出已會餵回下一輪)。最安全,但 agent 無法
  在寫檔前主動跑測試,每次驗證都要消耗一整個 loop step。
- **Option B — 自由 shell + 事後審計**:模型可執行任意命令,靠記錄與審計
  兜底。直接否決:繞過 scope lock 的傷害是即時的,審計是事後的;與
  read-only default 根本矛盾。
- **Option C — 宣告式命令 allow-list(選定)**:人在 config 宣告具名命令
  (固定 argv、標註意圖),模型以 `<<<RUN name>>>` 引用名稱;runtime 只執行
  宣告過的名稱,執行機制與 GateRunner 統一為共用 runner。
- **Option D — 模型只能觸發既有 verify gates**:把 gates 當 allow-list 重用。
  比 C 少一個 config 區塊,但混淆兩個概念 —— gate 是「完成判準」(全過即
  done),工具是「過程手段」;讓模型主動觸發 gate 會污染 done 語意。

### Decision

We decided to choose **Option C — 宣告式命令 allow-list** because:

1. **信任邊界清晰且可驗證** — 模型輸入只剩一個「名稱查表」,沒有字串進入
   shell;injection 面直接消失,而不是被過濾器壓低。
2. **與既有紀律同構** — 「名稱查表 + role gating + tier 分流」和
   `fs.write` 的 `ScopeDecision`、`grep.search` 的 `require_tool` 是同一個
   模式,治理故事一致(config-first:行為都在 YAML)。
3. **統一而不混淆執行路徑** — gate 與 RUN 共用一個 `CommandRunner`
   (cwd=root、捕捉 stdout/stderr 為 evidence、逾時上限),但 gate 保留
   `sh -c`(人寫的字串),RUN 用固定 argv 直接 spawn(不經 shell 解譯),
   一條 runner、兩種輸入信任等級。

具體規格(留給 shell slice 實作):

1. **Config** — `orvena.yaml`(或獨立 `commands.yaml`)新增:

   ```yaml
   commands:
     - name: test
       argv: [cargo, test]
       intent: read_only        # read_only | mutating
       timeout_secs: 300
     - name: fmt-fix
       argv: [cargo, fmt]
       intent: mutating
   ```

2. **協議** — `step.rs` 新增 `Action::Run { name }`,語法 `<<<RUN name\n>>>`,
   與 WRITE/SEARCH 對稱。未宣告的名稱 → `Error::Scope` 等級的 blocker,
   tier 分流與 write 一致(engineering 硬停、light 記錄續跑)。
3. **Role gating** — 工具名 `shell.run`;reviewer 可允許(其可用範圍仍受
   allow-list 與 intent 限制)。
4. **寫入類命令(回答問題 2:是,一律走 human)** — v0.1 模型只能引用
   `intent: read_only` 的命令;`mutating` 命令宣告了也不可由模型觸發,只能
   作為 human gatekeeper 確認後的 gate/人工操作。`intent` 是人宣告的信任
   聲明,runtime 不試圖驗證命令真的唯讀 —— 這是文件明示的人的責任。
5. **Evidence** — RUN 的輸出(截斷上限,比照 grep `MAX_HITS` 的做法)寫進
   下一輪 evidence,`report.tool_calls += 1`,與 SEARCH 同一條回饋路徑。

## Consequences

### Positive

- 模型獲得迴圈內自我驗證能力(跑測試再改檔),不必用整個 step 換 gate 回饋。
- 任意執行面為零:沒有模型字串進 shell;`rm`、重導向等根本不可表達。
- Gate 與工具共用 runner,evidence 格式與逾時行為一致,report 可比較。

### Negative

- 每個專案要先宣告命令才能用,冷啟動多一步 config(`orvena init` scaffold
  可給 `cargo test`/`cargo build`/`cargo clippy` 預設緩解)。
- 模型無法表達臨時命令(如只跑單一測試 `cargo test foo`);v0.1 明確不支援
  參數化,參數化 allow-list(白名單 args pattern)留給後續 ADR。
- `intent: read_only` 是信任聲明不是沙箱;宣告錯誤的人為失誤仍可能讓
  mutating 命令被模型觸發。緩解:scaffold 預設全部 read_only 且真的唯讀、
  文件紅字警告。

### Neutral

- gates 與 commands 在 config 中是兩個區塊、兩種語意(判準 vs 手段),
  即使底層 runner 共用。
- Light tier 下 allow-list 違規只記 blocker 不停迴圈,與現有 write/search
  行為一致 —— 治理強度由 tier 決定,不由工具自身決定。

## Related Decisions

| ADR / Doc            | Relationship | Description                                    |
| :------------------- | :----------- | :--------------------------------------------- |
| SLICE-grep-tool.md   | Depends on   | 唯讀搜尋已交付;本 ADR 解鎖被其擋下的 shell slice |
| MVP-SCOPE.md         | Implements   | must-have #1(shell/grep)的後半之決策前提       |

## References

- [SLICE-grep-tool.md](../../SLICE-grep-tool.md) — 「⛔ 相關但被擋住:shell 工具」一節
- [MVP-SCOPE.md](../../MVP-SCOPE.md)
- `crates/orvena-core/src/governance/gate.rs` — 既有 `GateRunner`(統一對象)
- `crates/orvena-core/src/tools/grep.rs` — role-gated 唯讀工具的參照實作

---

_ADR generated from the AI Native Software Engineering Framework ADR template._
