# Slice: grounded loop —— 寫之前先讀證據（vertical slice）

> 聰明線第四刀（0804,首個由 capability 量尺讀數直接驅動的投資;
> 第一刀 slice-020 READ/EDIT,第二刀 slice-021 步數預算,第三刀 slice-022 量尺)。
> 首跑(native 0.1.0 + qwen3:14b)verified 75%,完全雙峰:6 題 3/3(平均 2.3 步),
> 2 題三次全部 `budget_exhausted`。本 slice 修的是那 2 題共同的死因。

## 證據:死因不是搜不到,是「用猜的,不用讀的」

首跑 bundles(`bench-runs/20260804-capability-qwen3-14b.json` + KEEP_SCRATCH
最終檔 diff)顯示:

- **`cap-locate-broken-ref` 0/3**:三個 rep 都正確定位到 `docs/guide.md` 的
  `setup notes: docs/setup.md` 行——定位能力沒問題——但改的目標是**猜**的
  (rep0/rep2 → `docs/index.md`,rep1 → `docs/api.md`),從未 READ 候選文件
  確認哪份真的含 installation steps(正解 `docs/install.md`)。
- **`cap-audit-services` 0/3**:迴圈**有在收斂**(rep 間卡在不同缺陷,rep1/rep2
  連 indexer 9310 都修對了),但 scheduler 的 port 填 8201/8080——**編出來的**——
  而正確值 8460 就躺在 `tests/registry.txt`,check 的回饋甚至明講
  「see tests/registry.txt」。全程沒有一次 READ registry。
- **預算不是瓶頸**:audit-services 只有 2 個缺陷,grounded 打法 4 步內完賽;
  8 步預算下 guessing 才是死因。加預算是買數字,不是提能力。

## Frontmatter

```yaml
slice_id: slice-023-grounded-loop
title: Grounded loop — read the evidence before you write
status: DONE   # prompt 紀律 + 兩態 parity 測試 + 版本 bump;量測另跑
governance_tier: light
dependencies: [slice-020-read-edit-actions, slice-022-capability-set]
delivers:
  - prompt:  crates/orvena-core/src/agent/context.rs   # grounding 兩條紀律進 system prompt
  - verify:  同檔 tests — the_grounding_discipline_is_present_in_both_postures
  - version: workspace 0.1.0 → 0.2.0                    # agent 版本是可比鍵的一員
```

## 設計:一刀只動一個變因

1. **只動 prompt,不動 driver**。候選機制有二:(a) system prompt 的 grounding
   紀律;(b) driver 端「evidence 提及未讀檔案 → 機械式 nudge」。一次上兩個,
   量尺動了也不知道歸因給誰。先上 (a) 量一次;(b) 是下一刀的候選,
   只在 (a) 量不動時才投資。
2. **Grounding 是 capability,不是 obligation**。與 slice-020 的 READ/EDIT 同款
   紀律:兩條規則放在 system prompt 兩態共用段,ungoverned baseline 也拿到——
   否則 temptation 差異矩陣就是在跟一個「比治理組更愛猜」的稻草人對照
   (tkt-m1-null-is-structural 的教訓,方向相反地再犯一次)。parity 由測試釘死。
3. **兩條規則,對著兩種死法**:
   - 「never invent a value your change depends on — READ/SEARCH first」
     → 對 broken-ref 的猜目標。
   - 「when evidence names a file you have not read, READ it before attempting
     another change」 → 對 audit-services 的無視 `see tests/registry.txt`。
4. **agent 版本 bump 0.1.0 → 0.2.0**:prompt 是迴圈行為的一部分,bundle 記的
   `native <version>` 就是為了這一刻——0.2.0 的數字與 0.1.0 的數字不同 envelope,
   可比鍵四元組(set 版本, max_steps, model, agent 版本)差在第四元。

## 量測協定(不變,SLICE-022 的尺)

```sh
# scratch init(repo config 是別的 provider/model,不可繼承):
orvena init --provider ollama --model qwen3:14b
orvena bench --tasks benchmarks/capability.yaml --governance engineering --repeat 3 \
  --out bench-runs/<date>-capability-<model>-native020.json
```

預期讀數方向:`verified_rate` 75% → 上行(兩題 0/3 是否翻正);解掉題的
`mean_steps` 允許微升(多花一步 READ 是本 slice 鼓勵的行為,不是回歸)。
`budget_exhaustion_rate` 25% → 下行。若 broken-ref/audit-services 仍 0/3,
機械式 nudge(候選機制 b)升格為 slice-024。

## Acceptance Criteria

- [x] system prompt 含兩條 grounding 紀律,兩態(governed/ungoverned)逐字一致
- [x] parity 測試釘死(`the_grounding_discipline_is_present_in_both_postures`)
- [x] workspace 版本 0.2.0;bench header/bundle 顯示 `native 0.2.0`
- [x] 既有測試全綠(20 個 test target,0 failed)
- [x] 0.2.0 量尺讀數寫入 `docs/benchmark-results.md`:**verified 75% → 88%**,
      mean 3.8 → 3.0 步,耗盡率 25% → 12.5%;`cap-audit-services` 0/3 → 3/3
      (evidence-pointer 規則命中)。`cap-locate-broken-ref` 仍 0/3——後續的
      prompt 嘗試(slice-024)被量尺否決,見 SLICE-024-search-to-locate.md。

## 後續(不在本 slice)

- 機械式 evidence-pointer nudge(driver 掃 evidence 中「存在且未讀」的路徑,
  回饋提示)——只在本刀量不動時投資。
- SEARCH 使用率量測(bundle 目前只記 tool_calls 總數,不分動作類型)——
  若要歸因「定位能力」,需要分動作計數。
