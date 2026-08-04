# Slice: READ + EDIT actions —— native loop 的能力補洞(vertical slice)

> 給 code mode 的交接規格。Backend 在此範圍內實作,不擴 scope。
> 這是 native loop「聰明程度」投資線的第一刀(0804 裁決)。定位不變:native loop
> 是量測夾具與 envelope-native 的參考實作,不與外部 agent 比寫 code——但它現在
> 笨到傷害量測本身:看不到檔案內容,只能整檔盲寫,`false_blocks` 因此量不出東西。

## 為什麼是這兩個動作

v0.1 協議只有 `WRITE`(整檔重寫)、`SEARCH`、`RUN`。agent **沒有任何讀檔手段**:
grep 命中行是它唯一的視窗,然後就得整檔重寫。這一條解釋了大半的行為上限:

- 不敢動大檔(重寫必丟內容)→ 修復迴圈空轉,`prior_evidence` 的 gate 回饋用不上。
- 對邊界的試探全是盲的 → native leg 的 `scope_refusals`(`false_blocks` 唯一來源)
  貧乏,治理量測跟著貧乏。

READ 給它眼睛,EDIT 給它手術刀。步數預算與 capability set 是後續 slice,不在此。

## Frontmatter

```yaml
slice_id: slice-020-read-edit-actions
title: READ + EDIT actions for the native loop
status: TODO
governance_tier: light
dependencies: []          # 不動 sandbox、不動 bench 語意
delivers:
  - tool:     crates/orvena-core/src/tools/fs.rs          # FsTool::edit(錨定替換)
  - protocol: crates/orvena-core/src/agent/step.rs        # Action::Read / Action::Edit
  - wiring:   crates/orvena-core/src/agent/driver.rs      # 兩動作的 apply + 證據回饋
  - prompt:   crates/orvena-core/src/agent/context.rs     # 協議說明新增兩區塊
  - identity: 報告的 agent 欄 native 帶版本                # 見「量測身分」一節
  - verify:   單元測試 + offline round-trip 整合測試
```

## 協議(與既有區塊語法對稱)

```text
<<<READ relative/path
>>>

<<<EDIT relative/path
<必須在檔中恰好出現一次的舊文>
===
<替換成的新文>
>>>
```

- `READ`:內容以證據回饋到下一步(同 SEARCH/RUN 的路徑),超長截尾(cap 行為
  對齊 `cap_run_output`,截尾要明說,不能無聲)。
- `EDIT`:body 以**單獨一行 `===`** 分隔 old/new。已知限制:old/new 內容本身
  含單獨一行 `===` 時無法表達——v1 接受此限制,解析取**第一個**分隔行,並在
  slice 文件記載(整檔 `WRITE` 仍在,是逃生口)。
- old 在檔中必須**恰好匹配一次**:0 次或 >1 次都是失敗,以證據回饋
  (`EDIT failed: anchor not found / anchor ambiguous (N matches)`),不硬斷迴圈
  ——對齊 SEARCH 非法 regex 的處理,讓模型下一步自己修。

## 權限與治理語意(勿漂移)

- `READ` 走 `FsTool::read` → **role-gated `fs.read`**,不受 scope 限制(讀不改狀態,
  對齊 grep:全 repo 可讀、WRITABLE 才可寫)。role 拒絕 → `Error::Scope`,處理
  同既有 forbidden write(engineering 硬停、light 記錄續跑)。
- `EDIT` 是寫入:**role-gated `fs.write` + scope-gated**,與 `WRITE` 完全同一條
  authorization 路徑(實作上必須經 `FsTool`,不得旁路)。scope 拒絕 →
  `report.scope_refusals` 記 path——這正是 `false_blocks` 的進料,不能漏。
- **錨定失敗的錯誤訊息不得回帶檔案內容**(只說 not found / N matches):一個有
  `fs.write` 而無 `fs.read` 的 role,不能拿 EDIT 的失敗訊息當讀檔側信道。

## 量測身分(本 slice 的先決樁)

報告 `agent` 欄現在是裸的 `"native"`(aider 記 `"aider 0.86.2"`)。本 slice 是
數字發佈以來第一次改 native 行為,**先**把版本進欄位(`native <crate version>`,
如 `native 0.1.0`),再改行為——舊數字才知道自己量的是誰。bench 聚合對 `agent`
欄只 pass-through 的話,改動點應只在產報告處一處。

## 量測與策略分離(踩過三次的雷,再釘一次)

- `context.rs` 的協議說明是**兩種 posture 共用**的(對齊既有測試
  「the action protocol is unchanged」):ungoverned baseline 拿到一樣的 READ/EDIT
  能力與一樣的說明。能力是量測平台的一部分,義務才是治理的變因。
- scope 義務行(`scope_rules`)一字不動。
- `benchmarks/temptation.yaml` 本體與既有 bench 語意不動;已發佈數字不因本 slice
  重算(它們量的是 `native`(無版本)+ 舊協議,結果頁不回填)。

## Acceptance Criteria

### 協議(`step.rs`)
- [ ] AC-1:`Action` 新增 `Read { path }`、`Edit { path, old, new }`;`parse_actions`
      解析兩區塊,與既有三種混排不互相干擾。
- [ ] AC-2:EDIT 解析——`===` 前為 old、後為 new;缺分隔行為解析失敗(不產出
      action,或產出可回饋的失敗證據,擇一並測試釘住);多個 `===` 取第一個。
- [ ] AC-3:既有 WRITE/SEARCH/RUN 的 parse 測試零回歸。

### 工具(`fs.rs`)
- [ ] AC-4:`FsTool::edit(rel, old, new)`——require `fs.write` + scope 檢查與
      `write` 同路徑;恰好一次匹配才寫;0/多次回可辨識的 `Error`(訊息不含檔案內容)。
- [ ] AC-5:`READ` 沿用既有 `FsTool::read`,不新開路徑。

### Wiring(`driver.rs`)
- [ ] AC-6:兩動作 `report.tool_calls += 1`;READ 內容與 EDIT 失敗訊息進
      `tool_evidence`;scope/role 拒絕的分流(硬停 vs 記錄)與同類既有動作一致,
      EDIT 的 scope 拒絕記入 `report.scope_refusals`。
- [ ] AC-7:READ 輸出截尾且截尾可見(對齊 `cap_run_output` 的做法)。

### Prompt(`context.rs`)
- [ ] AC-8:系統提示新增兩區塊的格式說明,兩 posture 共用;既有
      「protocol is unchanged across postures」類測試同步擴充。

### 身分
- [ ] AC-9:native 產出的報告 `agent` 欄帶 crate 版本;bench JSON 快照/測試同步。

### Verification(gate 證據)
- [ ] AC-V1:單元測試——EDIT 的 0/1/多次匹配、role 拒 READ、scope 拒 EDIT、
      失敗訊息不洩內容。
- [ ] AC-V2:offline round-trip(對齊 `search_roundtrip.rs` 的做法)——腳本化
      「READ 看到內容 → 據其 EDIT → gate 過」兩~三步,斷言最終檔案內容;
      證明「讀了才改」真的接通,不是又一個沒人叫的工具。
- [ ] AC-V3:`cargo build && cargo test && cargo fmt --check && cargo clippy -D warnings`
      全綠(CI 同 bar)。

## 後續(不在本 slice)

- slice-021(暫):步數預算——`max_steps` 預設放大 + 修復迴圈利用率量測。
- slice-022(暫):capability task set——temptation set 量守規,不量做事;
  聰明要有自己的尺(M2/M4 前後對照),verify-gate 當 oracle。
