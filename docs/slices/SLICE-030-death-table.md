# Slice: 死法表 —— 校準跑的產出不是分數,是怎麼死的

> 聰明線第十一刀(0806)。SLICE-026 的 0806 裁決推出的第三件事:
> 30 run 實測顯示通過率要 ~96 run 才穩,而死法分類在 n=3 已飽和、
> 跨三次獨立呼叫完全一致(即使其中一次通過率差了 59 個百分點)。
> 所以校準跑要輸出的不是一個百分比,是**每題的死法表**。本刀把它做進報表。

## 缺口:repeated 報表的每題欄位只剩一個百分比

`TaskPassRate` 原本只有 `solved / runs / pass_rate`——正是實測證明
不穩的那個讀數。死法要用的三樣東西(`action_counts` 分佈、耗盡率、
命中/通過的對應)都在 `runs[]` 裡,但散在每個 repeat 的 `results` 深處,
讀死法等於重新做一次 transcript 考古。SLICE-026 的選題規則
(「死法分佈不是單一策略的函數」)需要**逐 run 的列**才判得出雙峰;
地板格的主判準(瞎搜 = use 高 + hit≈0 + 燒到上限)需要**每題**的分類,
set 層的平均正好把它蓋掉。

## 形狀:三個新欄位,全部 additive

`TaskPassRate` 增加(舊報表讀回一律是預設值,`deaths` 空 = 未記錄,不是零):

- **`deaths: Vec<DeathRow>`** —— 每個 measured run 一列,依 repeat 順序;
  `rep` 是 `runs[]` 的索引,列可以追回完整 `TaskResult` 與證據包。
  列上有 `solved` / `verified` / `exit` / `steps` / `total_tokens` /
  `action_counts` / `search_hits` / `search`(分類,見下)。
- **`exhaustion_rate`** —— 這一題 measured runs 裡 `budget_exhausted` 的比例。
  與 set 層的 `budget_exhaustion_rate` 並存:地板模型在不同題上死法不同,
  平均正好藏掉這件事。
- **`search_vs_solved: SearchSolveTable`** —— 命中/通過的對應表,
  每格 `solved`/`failed` 兩個計數。30 run 實測是 hit ⇔ 通過無例外;
  這張表讓「對應成立」或「對應破了」(那會是新發現:找到了卻不動手,
  至今零觀測)變成每題可查,不必讀 transcript。

`SearchOutcome` 是 run 層的**分類,不是比率**——比率已被證明是通過率的
另一種寫法(SLICE-026 第 1 點),能用的是類別:

| 類別 | 定義 | 讀法 |
|---|---|---|
| `hit` | 至少一次 SEARCH 有命中 | 找到了 |
| `miss` | 有搜尋真的看了檔案,全部 0 命中 | 搜錯東西——模型問題 |
| `blocked` | 有發搜尋,但沒有任何一次真的看了檔案(全部死在工具邊界) | slice-027 那種死法——工具問題 |
| `no_search` | 沒發過搜尋 | |
| `unattributable` | `action_counts` 是 `None`(wrapped agent) | 紀錄缺口,不是行為發現 |

兩個刻意的區分,各對應一次踩過的雷:

1. **`blocked` ≠ `miss`**。slice-027 修掉的死法若再出現(新的工具邊界),
   它必須顯示為工具問題,不能混進「搜錯東西」讓模型背鍋。
   錯誤的搜尋(`search_hits` 裡的 `None`)永遠不算 miss。
2. **`unattributable` ≠ `no_search`**。`action_counts` 的 `None` 是
   「不是 Orvena 能歸因的」,不是全零——這條契約已經寫在
   `TaskResult::action_counts` 上,分類層照樣遵守。

## CLI:每題多一行

```
  needle-12          0/6 solved  (0%)
                     deaths: 6/6 exhausted  |  search→solved: miss 0/5, blocked 0/1
```

只在 `deaths` 非空時印——報表比欄位老的時候,「沒量到」不印成「0% 耗盡」。

## 驗證

`cargo test --workspace`、fmt、clippy 全綠。新測試:分類的五個邊界
(混雜 error/miss 中有一個 hit 仍是 hit;全零是 miss 不是 blocked;
全 error 是 blocked 不是 miss;沒搜尋與不可歸因是兩類)、tally 的配對、
以及**舊版 JSON 反序列化**(缺欄位讀回預設,「沒記錄」不得偽裝成「量到零」)。

## 沒做的(這一刀刻意停在哪)

- **set 檔頭的出廠校準欄位**(裁決第 3 點的後半:寫死法不寫分數)——
  那是 `capability-v2.yaml` 的檔頭格式,依 SLICE-026 的門,
  YAML 動工前要先過 DRAFT 定案。本刀只把讀數側備好。
- 分類**不回饋**進 loop 行為——量測/治理分離(踩過三次的那條)照舊。
