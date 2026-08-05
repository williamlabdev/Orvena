# Slice: SEARCH 命中數進 evidence —— 「搜了」與「搜到了」是兩個讀數

> 聰明線第九刀(0805)。slice-027 修好 glob 之後,14b 的死法從「被工具擋下」
> 換成「連發八次 SEARCH、一個字都不寫、撞死在預算上」。
> 我當時寫下的解釋是「有能力搜尋、沒能力把結果轉成動作」——
> 但那是**推論,不是讀數**。本刀把它變成讀數。

## 缺口:`action_counts.search` 分不出兩種相反的失敗

bundle 記得到「發了幾次 SEARCH」,記不到**那些 SEARCH 有沒有回傳東西**。
於是兩種需要相反修法的失敗長得一模一樣:

| 失敗 | 讀數(修前) | 該怎麼修 |
|---|---|---|
| 搜錯字串,零命中 | `search: 8` | 題目/prompt 給的線索不足,或模型 pattern 能力不行 |
| 搜對了,拿到命中卻不動手 | `search: 8` | loop 紀律問題(slice-023 的另一半:看了要動手) |

**順序和命中數一樣重要**:一趟在**命中之後**還繼續搜的,不是在找,是在原地打轉。

這個缺口不是理論的——slice-027 的結論就卡在這裡,只能寫「最合理的解釋」。
判讀證據時被迫用推論補洞,下一次就會有人把推論當成量測引用。

## Frontmatter

```yaml
slice_id: slice-028-search-hits
title: Per-search hit counts — "it searched" and "searching worked" are two readings
status: DONE   # 實作 + 6 個測試;欄位的首次實地讀數見下
governance_tier: light
dependencies: [slice-026-capability-set-v2, slice-027-search-glob]
delivers:
  - evidence: crates/orvena-core/src/metrics/mod.rs        # RunReport.search_hits: Vec<Option<u32>>
  - driver:   crates/orvena-core/src/agent/driver.rs       # 每次 SEARCH 記命中數,錯誤記 null
  - summary:  crates/orvena-core/src/benchmark/report.rs   # search_yield_rate(每次搜尋,非每趟)
  - runner:   crates/orvena-core/src/benchmark/runner.rs   # 聚合 + 3 個分母測試
  - cli:      crates/orvena-cli/src/commands/bench.rs      # eyes 那行多一個 yield 數字
  - schema:   schemas/evidence.v1.json                     # 加性欄位,仍是 v1
```

## 三個不變式(都與既有的記錄紀律同源)

1. **`null` ≠ 0**。錯誤的 SEARCH(壞 regex、越界路徑)記 `null`,不記 0 命中——
   它**沒有看過任何檔**,算成 miss 等於為了工具邊界怪罪模型兩次(一次在 blockers、
   一次在 yield)。這正是 slice-027 用一整條 14b 腿學到的教訓。
2. **空陣列 ≠ 全部落空**。沒搜就是沒搜;不可歸因的 run(wrapped agent)也是空的。
   與 `action_counts` 的 `None ≠ 全零` 同一條規矩。
3. **yield 的分母是「每次搜尋」,不是「每趟 run」**。一趟搜六次、只有一次命中,
   per-run 讀數會顯示 100%(它「有」找到),把五次落空藏起來——而那五次正是
   「拿著錯 pattern 猛敲」的形狀。

## 判讀方式(兩個數字要一起看)

CLI 的 `eyes:` 那行現在是兩個數:

```
eyes: SEARCH used in 100% of runs  |  25% of searches returned a hit
```

- **use 高、yield 低** → 在瞎搜。線索不足或 pattern 能力不足。
- **use 高、yield 高、通過率低** → **找到了卻不動手**。loop 紀律問題,
  不是視力問題——這是 slice-027 那個推論真正該有的證據形式。
- **use 低** → 根本沒去找(slice-024 的老問題)。

## 驗證

`cargo test --workspace` 20 個 target 全綠、clippy 乾淨。新增測試:

| 測試 | 守的是 |
|---|---|
| `search_results_drive_the_next_write`(擴充) | 命中數按順序記錄 |
| `invalid_regex_is_a_blocker_but_does_not_stop_the_loop`(擴充) | 不變式 1:錯誤記 `null` |
| `search_yield_rate_is_per_search_not_per_run` | 不變式 3 |
| `an_errored_search_is_not_counted_as_a_miss` | 不變式 1(分母側) |
| `nothing_searched_is_none_not_zero` | 不變式 2 |
| `the_validator_and_the_schema_engine_agree_on_broken_bundles`(擴充) | `null` 合法、字串非法,且兩個引擎一致 |

schema 是**加性欄位,仍是 v1**(舊 bundle 沒有此欄位,讀回來是空陣列——
依不變式 2,那不等於「每次搜尋都落空」)。

## 首次實地讀數:回頭問 slice-027 那個沒答完的問題

用同一支探針、同一個 14b 重跑,這次帶著新欄位:
`bench-runs/20260805-probe-search-scale-qwen3-14b-v2-postglob-hits.json`。
要答的就是那句被我寫成推論的話——**它是搜錯了,還是搜到了不動手**。
（結果見下節;跑完補。）
