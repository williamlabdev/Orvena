# Slice: SEARCH 吃 glob —— 工具要接住模型的習慣,不是糾正它(vertical slice)

> 聰明線第八刀(0805)。第七刀(slice-026 的探針)修好之後,
> SEARCH 使用率從 0% 跳到 **100%**——形狀逼出了工具使用。
> 但同一批證據裡藏著另一件事:qwen3:14b 六趟裡四趟失敗,**死法完全相同**,
> 而且和定位能力無關。本刀處理那個死法。

## 證據:四次失敗,同一行錯誤

`bench-runs/20260805-probe-search-scale-qwen3-14b-v2.json`,四個失敗 bundle 的
`blockers` 一字不差:

```
search path 'svc/*.conf' does not exist
search path 'svc/*.conf' does not exist
reached max_steps (8) without passing all gates
```

`action_counts` 是 `{write: 0, edit: 0, read: 0, search: 5–6, run: 2–3}`——
**零 write**。它不是找錯了服務、也不是寫錯了格式,它從來沒走到寫答案那一步。
八步預算全花在反覆嘗試同一個被拒絕的搜尋路徑上。

死因在 `crates/orvena-core/src/tools/grep.rs`:搜尋路徑只吃字面位置,
glob 不支援;而錯誤訊息說的是「does not exist」——模型讀起來像**路徑打錯**,
於是換個寫法再撞一次。system prompt 那行寫的是
`[optional relative path to limit the search]`,也沒說不能用 glob。
模型沒有任何管道知道正確形式是什麼。

**這不是能力讀數,是工具可及性的讀數。** 在修好之前,任何需要 SEARCH 的題
量到的地板都摻了這個雜訊,而 v2 的校準正要拿地板當基準。

## Frontmatter

```yaml
slice_id: slice-027-search-glob
title: SEARCH accepts a glob — meet the model's prior, don't correct it
status: DONE   # 實作 + 6 個新測試;重量測見下
governance_tier: light
dependencies: [slice-001-grep-tool, slice-026-capability-set-v2]
delivers:
  - tool:   crates/orvena-core/src/tools/grep.rs      # glob 路徑 + literal_prefix + 選中零檔為錯
  - prompt: crates/orvena-core/src/agent/context.rs   # SEARCH 路徑的說明改成三種可接受形式
  - docs:   crates/orvena-core/src/agent/step.rs      # 協議註解同步
  - deps:   globset 0.4(已在 Cargo.lock 內,ignore 的既有相依,零新編譯成本)
```

## 判斷:接住 glob,而不是把錯誤訊息寫好

兩條修法都能讓 loop 活下來:

- **A. 訊息教會 affordance**:「SEARCH 吃目錄或檔案,不吃 glob——試 `svc/`」。
  便宜,但**恢復要付一步**,而八步預算裡的一步是 12.5%。
- **B. 直接支援 glob**:模型第一次就成功,零浪費。

選 B,理由是這個 repo 自己量出來的:**slice-024 已經證明「在 prompt 裡叫模型
照規矩來」這根槓桿到頭了**。同一個道理適用於錯誤訊息——它也是 prompt,
只是遲到一步。`svc/*.conf` 是 shell 習慣,不是錯誤;工具去接住它,
比要求模型記住一個非標準的限制更可靠。

(A 沒有被丟掉:路徑不存在仍然報 does not exist,glob 選中零檔另有一句
「matched no files」——見下。)

## 語義:三個不變式

1. **`literal_separator(true)`**:`svc/*.conf` 只吃 `svc/` 的直接子檔,不跨 `/`——
   和模型的 shell 心智模型一致。要遞迴就 `svc/**/*.conf`。
2. **walk 從最深的字面前綴開始**(`literal_prefix`):`svc/*.conf` 只走 `svc/` 子樹,
   不會因為帶了 glob 就掃全 repo。`*.conf` 才從 root 開始。
3. **選中零檔是錯誤,不是零命中**。這是 slice-001 既有不變式往下一層的延伸:
   「路徑不存在必須看得見,不能報 0 hits」。同理,`svc/*.rs` 選不到任何檔時
   若回報「沒有命中」,模型會去找一個**從未被搜尋過**的 pattern 的替代寫法——
   一個看不見的空集合比一個錯誤更貴。

安全邊界不變:絕對路徑與 `..` 一樣先擋(`Error::Scope`),glob 不是繞過 root 的新門。

## 驗證

`cargo test -p orvena-core` 全綠(202 lib + 各整合 target),新增 6 個測試:

| 測試 | 守的是 |
|---|---|
| `a_glob_path_selects_the_files_it_names` | `.txt` 兄弟檔不被選中,`*` 不跨 `/` |
| `a_recursive_glob_is_how_you_cross_a_separator` | `**` 才遞迴 |
| `a_glob_matching_no_file_is_an_error_not_zero_hits` | 不變式 3 |
| `a_glob_walks_only_the_subtree_it_names` | 不變式 2(`literal_prefix` 直接斷言) |
| `a_glob_cannot_escape_the_root_either` | `../*.conf` 仍是 Scope 錯誤 |
| `a_glob_whose_literal_prefix_is_missing_still_says_so` | `nope/*.conf` 仍報 does not exist |

## 重量測(這一刀的真正驗收)

修工具而不重跑,等於用推理取代讀數。用同一支探針、同一個模型重跑:
`bench-runs/20260805-probe-search-scale-qwen3-14b-v2-postglob.json`。

判讀的重點**不是通過率**,是那四個失敗的死法有沒有換掉:
`blockers` 裡還有沒有 `search path ... does not exist`,以及失敗時
`write` 是否終於 > 0(代表它至少走到了寫答案那一步)。
- 地板抬起來 → 之前的 33% 確實是工具雜訊,v2 校準要用新地板。
- 地板沒動、但死法換了 → 工具問題是真的,定位能力是另一個獨立的洞。
- 死法沒換 → 本刀的診斷錯了,回頭看 evidence。

35b 不必重跑:它六趟都沒碰到這個死法(2 步完賽),讀數不受影響。
