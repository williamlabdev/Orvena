# ADR-002: 證據包的落地位置與檔名

> Architecture Decision Record — 記錄一次具體的架構決策及其原因。

## Frontmatter

```yaml
doc_type: adr
adr_id: ADR-002
title: 證據包的落地位置與檔名 — .orvena/runs/<timestamp>/evidence.json
status: ACCEPTED
superseded_by: null
created: 2026-07-04
updated: 2026-07-04
author: William Chiu
related_blueprint: null # Orvena v0.1 以 MVP-SCOPE.md + slice 文件代替 blueprint 鏈
```

## Context

### Problem

MVP must-have #2 是 evidence-bundle exporter —— 核心賣點「evidence by
default」的最小可證版本。`RunReport` 已 derive `serde::Serialize`,且已在記憶
體帶了 `completed`、`gate_outcomes`、`blockers` 與 steps/tool_calls/tokens;
目前 `run.rs` 只把它 `println!` 到 stdout,跑完即消失。所以這個 slice 只是
「把已可序列化的 report 寫成一份檔案」,**不是新子系統**(持久化事件日誌是
MVP-SCOPE 明確 defer 的另一件事,不在此列)。

序列化格式沒有懸念(JSON,`RunReport` 的既有欄位)。真正需要一次決策的是:

1. 這份檔案**寫到哪個目錄**?目前 codebase 沒有現成的「run 輸出目錄」,唯一
   的磁碟目錄是 `.orvena/`(`CONFIG_DIR`,`orvena init` 部署 config 的地方)。
2. **一次 run 一個目錄,還是一個扁平檔名**?檔名/timestamp 怎麼定,才不會在
   連續兩次 run 之間互相覆蓋?
3. **timestamp 用什麼格式**?可讀的 ISO-8601 需要日期函式庫。

### Stakeholders

| Role       | Person/Team        | Interest                                     |
| :--------- | :----------------- | :------------------------------------------- |
| Maintainer | William Chiu       | 證據包位置可預測、可對外說明;不引入多餘依賴 |
| Agent loop | orvena-core        | 序列化路徑穩定、offline-deterministic 可測    |
| Embedder   | 下游使用者/CI 環境 | 跑完能在固定位置撿到審計包,失敗的 run 也有    |

## Decision

### Status

ACCEPTED

### Context

關鍵洞察:**序列化是 runtime 的職責,「寫到哪」是 caller 的職責。** 核心只
負責把 `RunReport` 寫成 JSON(純函式、不讀時鐘);「目錄佈局 + timestamp」是
CLI 對檔案系統的決策,兩者分開,核心才能保持 offline-deterministic 可測。

### Options

- **落地位置**
  - **Option A — 寫進 `.orvena/`(選定)**:唯一既有的磁碟目錄,`orvena init`
    已建立;使用者只需認一個 Orvena 目錄。
  - **Option B — 另建 top-level 輸出目錄(如 `evidence/` 或 `.orvena-runs/`)**:
    多一個要 gitignore、要向使用者解釋的目錄;冷啟動多一個概念。否決。

- **每次 run 的檔名**
  - **Option C — 每 run 一個子目錄 `runs/<timestamp>/evidence.json`(選定)**:
    留給未來「一次 run 多個 artifact」的成長空間,而不必變更證據包格式或搬檔。
  - **Option D — 扁平 `evidence-<timestamp>.json`**:少一層目錄,但一旦未來要
    在同一次 run 旁邊放第二份產物(如 diff、log)就得改路徑慣例。

- **timestamp 格式**
  - **Option E — Unix epoch 毫秒(選定)**:零依賴;毫秒精度避免連續兩次 run
    撞檔;等寬時可字典序排序。可讀性較差是已知取捨。
  - **Option F — ISO-8601(如 `20260704T120000Z`)**:可讀,但需引入日期函式庫
    (chrono/time),與本 slice「小而必要、別擴張」的界線相違。

### Decision

We decided to choose **A + C + E** —— 證據包寫到
`.orvena/runs/<timestamp>/evidence.json`,timestamp 為 Unix epoch 毫秒 ——
because:

1. **不新增使用者要認的目錄** — `.orvena/` 已是 Orvena 的磁碟根,`orvena init`
   已建立;證據包放在其下的 `runs/` 一目了然。
2. **每 run 一個子目錄留下成長空間** — 未來要在同一次 run 旁放更多產物時,
   `runs/<timestamp>/` 已是天然容器,證據包格式(`evidence.json`)不必變。
3. **零新依賴符合本 slice 的範圍界線** — epoch 毫秒用 `std::time` 即可產生;
   為了好看的日期字串引入 chrono 不划算,留給後續 slice 有需要再換。
4. **關注點分離、可測** — 核心只提供 `write_bundle(report, path)`(純序列化 +
   建目錄)與 `bundle_path(base, timestamp)`(純路徑組裝);讀時鐘產生 timestamp
   留在 CLI。核心因此保持 offline-deterministic,round-trip 測試可用固定
   timestamp 斷言佈局。

具體規格(本 slice 已實作):

1. **路徑** — `bundle_path(base_dir, timestamp) =
   <base_dir>/runs/<timestamp>/evidence.json`;`base_dir` 由 CLI 傳入
   `CONFIG_DIR`(`.orvena`)。
2. **格式** — `serde_json::to_string_pretty(&RunReport)`;欄位即 `RunReport`
   既有欄位(`completed`、`steps`、`input_tokens`、`output_tokens`、
   `tool_calls`、`gate_outcomes`、`blockers`、`task`)。round-trip 保證:寫出的
   檔案可反序列化回相等的 `RunReport`。
3. **失敗路徑也要落地** — `run.rs` 在 `print_report()` 之後、`!completed` 的
   `bail!` **之前**寫檔。無論 `completed` true/false 都產出一份;失敗的證據跟
   成功的一樣重要,這正是證據包最有價值的時刻。
4. **timestamp** — `SystemTime::now()` 距 `UNIX_EPOCH` 的毫秒數,由 CLI
   (`run.rs::run_timestamp`)產生。

## Consequences

### Positive

- 跑完一律在 `.orvena/runs/<timestamp>/evidence.json` 撿到審計包,**失敗的 run
  也有** —— 核心賣點「evidence by default」有了最小可證版本。
- 核心的 exporter 是純函式(序列化 + 路徑組裝),不讀時鐘、不依賴網路,
  offline round-trip 可完整測到。
- 沒有新增依賴、沒有新增子系統;只用到既有的 `serde_json` 與 `std::time`。

### Negative

- Epoch 毫秒不好給人肉眼讀。緩解:每份 bundle 內含 `task` 欄位可辨識;要換成
  ISO-8601 只需改 `run_timestamp` 一處,不動格式或路徑慣例。
- `.orvena/runs/` 會隨每次 run 累積,v0.1 不做輪替/清理。留給後續(需要時再加
  保留策略或 `orvena clean`)。

### Neutral

- 「寫到哪/timestamp」的決策留在 CLI,「序列化」留在核心 —— 與既有分工
  (核心可嵌入、CLI 只是 thin frontend)一致。
- 一次 run 一個子目錄目前只放一份 `evidence.json`;多產物是預留而非現況。

## Related Decisions

| ADR / Doc      | Relationship | Description                                        |
| :------------- | :----------- | :------------------------------------------------- |
| MVP-SCOPE.md   | Implements   | must-have #2(evidence-bundle exporter)之落地決策  |
| ADR-001        | Relates to   | 同屬 v0.1 治理紀律;gate 證據字串是本包欄位的來源之一 |

## References

- [MVP-SCOPE.md](../../MVP-SCOPE.md) — 第 1 節(MVP exit = 匯出證據包)、第 3 節 must-have #2
- `crates/orvena-core/src/metrics/evidence.rs` — exporter(`write_bundle` / `bundle_path`)
- `crates/orvena-core/src/metrics/mod.rs` — `RunReport`(被序列化的既有欄位)
- `crates/orvena-cli/src/commands/run.rs` — 呼叫端(timestamp + 落地 + 失敗路徑)

---

_ADR generated from the AI Native Software Engineering Framework ADR template._
