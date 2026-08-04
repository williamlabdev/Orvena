# Slice: 步數預算 —— max_steps 放大 + 迴圈利用率量測修復(vertical slice)

> 給 code mode 的交接規格。Backend 在此範圍內實作,不擴 scope。
> 聰明線第二刀(0804 裁決的投資線,第一刀是 slice-020)。slice-020 給了迴圈
> 眼睛(READ)和手術刀(EDIT),但步數預算還是為「整檔盲寫、一步定生死」的
> v0.1 迴圈定的尺寸:read → edit → run → 看 gate 證據 → 再修,一圈就是 3-4 步,
> 預算根本不夠新迴圈把能力用出來。

## 為什麼放大與修量測必須同一刀

`docs/benchmark-results.md` 已經自己招了(「不美化」一節):

> **The cost ratio is measured against a baseline that burns its whole budget.**
> The baseline averaged 3.8 of a possible 4 steps because nothing ever tells it
> to stop.

M4 的 `overhead_steps_ratio = governed mean_steps / baseline mean_steps`,而
baseline 幾乎永遠燒滿預算 → 分母 ≈ `max_steps`。**只放大預算不修量測,×0.36 會
機械地變成 ×0.18**——數字變漂亮,行為一點沒變。這是量測誠實問題,不是調參問題,
所以兩件事綁成一個 slice:預算變大之前,利用率必須先可解讀。

現況的結構性缺陷:bundle 只記 `steps`,不記預算,「用了 3 步」無法解讀
(3/4 是燒滿,3/8 是收斂);「為什麼停」只活在 blocker 散文裡
(`reached max_steps (4) ...`),消費者要 parse 散文才能分「自己收斂」和
「被預算切掉」。`provider_error` 欄位當年就是為了同一類問題加的 typed flag——
本 slice 把同樣的紀律補到迴圈終點上。

## Frontmatter

```yaml
slice_id: slice-021-step-budget
title: Step budget enlargement + loop-utilization measurement fix
status: DONE   # 全 AC 以單元/整合測試釘住;bench 常數與 config 預設同步放大
governance_tier: light
dependencies: [slice-020-read-edit-actions]   # 預算是為 READ/EDIT 迴圈重新定尺
delivers:
  - metrics:  crates/orvena-core/src/metrics/mod.rs       # RunReport.max_steps + RunReport.exit(typed ExitReason)
  - driver:   crates/orvena-core/src/agent/driver.rs      # 每個出口點設 exit;記 max_steps
  - adapter:  crates/orvena-core/src/adapter/mod.rs       # 同上(wrapped agent 腿)
  - config:   crates/orvena-core/src/config/agent.rs      # default_max_steps 3 → 8
  - bench:    crates/orvena-core/src/benchmark/            # MAX_STEPS 4 → 8;TaskResult.exit;每臂 budget_exhaustion_rate;Differential 帶兩臂耗盡率
  - schema:   schemas/evidence.v1.json                    # 文件化 max_steps / exit(additive,留 v1)
  - verify:   單元測試 + 既有 exit-path/schema 測試擴充
```

## ExitReason(typed 迴圈終點)

```rust
pub enum ExitReason {
    GatesPassed,      // governed:所有 gate 過(native + adapter)
    ClaimedDone,      // ungoverned:自己宣稱 done(native 零動作;adapter exit 0)
    BudgetExhausted,  // 燒滿 max_steps 還沒到上面任一種
    NeedsHuman,       // human gate 停下
    HardBlocked,      // enforcement 硬停(engineering tier 的 scope violation)
    ProviderError,    // provider 掛了(native 腿)
    AgentError,       // wrapped agent 起不來 / ungoverned 單發非零退出(adapter 腿)
    Unrecorded,       // serde default:舊 bundle 沒這欄
}
```

- `completed` 與 `blockers` 語意**一字不動**——`exit` 是加欄,不是改欄。
- 對照表(driver):`finished(true)` 且 governed → `GatesPassed`;ungoverned
  零動作 → `ClaimedDone`;human gate → `NeedsHuman`;enforcing tier 的
  scope 硬停 → `HardBlocked`;provider chat 失敗 → `ProviderError`;
  迴圈走完 → `BudgetExhausted`。
- 對照表(adapter):sandbox 拒起(fail-closed)→ `AgentError`;ungoverned
  單發 exit 0 → `ClaimedDone`、非零 → `AgentError`;governed 全 gate 過 →
  `GatesPassed`;human gate → `NeedsHuman`;走完 → `BudgetExhausted`。

## 量測語意(勿漂移)

- **兩臂同預算**:差異矩陣比的是 identical envelope,`max_steps` 是 envelope
  參數,governed 與 baseline 一起放大。adapter 的 ungoverned 單發(max_steps=1,
  單次完整 agent invocation)語意不變——那是「wrapped agent 自己的迴圈在裡面」,
  不是步數。
- **量測與策略分離**(踩過三次的雷):`exit`/`max_steps` 是觀測欄位,不得反過來
  影響迴圈行為;ungoverned 照舊不諮詢 gate、不多得也不少得任何 prompt 義務。
- **M4 的誠實化**:每臂 summary 新增 `budget_exhaustion_rate`(ran 且非排除運行
  中 `BudgetExhausted` 的占比);`Differential` 新增
  `baseline_budget_exhaustion_rate` / `governed_budget_exhaustion_rate`。
  `overhead_steps_ratio` 公式不動,但從此發佈時旁邊就躺著「分母有多少是被
  right-censor 的」——結果頁引用比值而不引用耗盡率,一眼可查。
- 排除規則不動:provider-error 運行照舊踢出所有分母;`budget_exhaustion_rate`
  的分母同 M4(ran 且未排除)。

## 預算值

- `default_max_steps()`:3 → **8**(config 層,產品預設)。
- bench `MAX_STEPS`:4 → **8**(兩處常數同一來源化不強求,但值必須一致)。
- 為什麼是 8:READ/EDIT 迴圈的最小有意義軌跡是
  「search/read 定位 → edit → run/gate → 讀證據再修 → gate」≈ 4-5 步,8 給它
  一次完整的重試機會而不至於讓燒滿預算的 baseline 把 bench 牆鐘時間翻太多
  (最壞情況 ×2)。8 是可調參數,不是聖數;調它只需要動兩個常數 + 本檔。
- **可比性**:bench 預算變了 + slice-020 已改 native 行為 → 之後任何新跑
  都是新 native、新 envelope,與 0803/0804 舊資料**不可 pool**(handoff 已釘)。
  bundle 現在自帶 `max_steps`,這條規則從此可機器檢查。

## Acceptance Criteria

### Metrics(`metrics/mod.rs`)

- AC-M1:`RunReport` 新增 `max_steps: u32`(`#[serde(default)]`,舊 bundle 讀回 0
  = 未記載)與 `exit: ExitReason`(`#[serde(default)]` = `Unrecorded`)。
- AC-M2:舊 bundle JSON(無這兩欄)反序列化 → `max_steps == 0` 且
  `exit == Unrecorded`,既有欄位全部不變(schema 留 v1)。

### Driver / Adapter

- AC-D1:driver 的**每一個** return 路徑都設好 `exit`,且 `report.max_steps`
  在迴圈前就記下——包括 provider-error 早退路徑。
- AC-D2:adapter 同上;ungoverned 單發的 `max_steps` 記 1(事實如此)。
- AC-D3:T4 型測試(永遠不過的 gate)斷言 `exit == BudgetExhausted`;
  T1 型(gate 過)斷言 `GatesPassed`;human gate 斷言 `NeedsHuman`。

### Bench(`benchmark/`)

- AC-B1:`MAX_STEPS == 8`;config `default_max_steps() == 8`。
- AC-B2:`TaskResult` 帶 `exit`(runner 從 RunReport 原樣搬運)。
- AC-B3:每臂 summary 有 `budget_exhaustion_rate`;`Differential` 帶兩臂耗盡率;
  聚合測試釘住計算(含排除 provider-error 的分母)。

### Schema

- AC-S1:`schemas/evidence.v1.json` 文件化 `max_steps` 與 `exit`(optional,
  additive,`additionalProperties` 本來就 true);M3 有效性測試全綠。

### Verification

- AC-V1:`cargo test` 全綠、`cargo fmt --check`、`cargo clippy -- -D warnings` 乾淨。
- AC-V2:CHANGELOG 記載本 slice(Added:預算放大 + typed exit;明說「只放大
  不修量測會讓 ×0.36 機械變好」這條動機)。

## 後續(不在本 slice)

- slice-022(暫):capability task set——temptation set 量守規,不量做事;
  聰明要有自己的尺(M2/M4 前後對照),verify-gate 當 oracle。
- 結果頁引用紀律:下次寫 pooled/新數字時,M4 行必須同時給耗盡率
  (本 slice 只給欄位,不動 `docs/benchmark-results.md` 數字區)。
