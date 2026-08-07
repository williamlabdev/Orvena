# Slice: capability task set —— 聰明的量尺(vertical slice)

> 給 code mode 的交接規格。Backend 在此範圍內實作,不擴 scope。
> 聰明線第三刀(0804 裁決;第一刀 slice-020 READ/EDIT,第二刀 slice-021 步數預算)。
> temptation set 量守規,不量做事;realworld set(07-04)5 題全部 1 步解掉,
> 是天花板不是尺。native 變強了沒,現在沒有任何一個數字量得出來——本 slice 補的
> 就是這把尺。

## 為什麼要新 set,而不是加大 realworld

realworld 的題型(單檔、指名檔案、指名 bug)對 v0.1 的盲寫迴圈就已可解,
所以它對 slice-020/021 的投資**不敏感**:READ/EDIT/8 步在上面量不出差異。
量尺要能鑑別,題目必須落在「舊迴圈結構性做不到、新迴圈做得到」的區間:

| 鑑別對象 | 題型 | 舊迴圈為何失敗 |
|---|---|---|
| READ(眼睛) | **保全題**:檔案 40+ 行高熵內容,只錯一處,verify 錨定多條原句 + 行數 | 盲寫無法重建沒讀過的內容 |
| EDIT(手術刀) | **錨定題**:錯值所在行與他處幾乎重複,錨必須取到唯一 | 整檔重寫等同保全題失敗;EDIT 錨歧義的回饋要能利用 |
| SEARCH(定位) | **定位題**:症狀不指名檔案,錯在多個可寫檔之一 | 不會搜就得盲改全部,踩保全錨 |
| 步數預算(slice-021) | **收斂題**:check 一次只揭露一個缺陷,3+ 個缺陷疊置 | 3-4 步預算不夠一次修一個 |

## Frontmatter

```yaml
slice_id: slice-022-capability-set
title: Capability task set — the smartness ruler
status: DONE   # set + 不變式測試 + 量測協定文件化;首跑數字不在本 slice
governance_tier: light
dependencies: [slice-020-read-edit-actions, slice-021-step-budget]
delivers:
  - set:      benchmarks/capability.yaml            # 8 題:3 保全、1 錨定、2 收斂、2 定位(含綜合)
  - verify:   crates/orvena-core/tests/benchmark.rs # parse + capability 專屬不變式
  - protocol: 本檔「量測協定」一節 + set 檔頭註解
```

## 誠實規則(與 temptation set 對稱,方向相反)

1. **不量守規**:沒有 escape probes、沒有 out-of-scope 捷徑、writes 給得自然
   寬鬆。capability set 裡藏 temptation 會讓一個數字量兩件事,兩件都量壞。
2. **check 可被 agent 讀,且允許它說出目標條件**——任何真 CI 的紅燈都會說
   期望值是什麼;這不是洩題。鑑別力來自**熵**:verify 錨定多條 agent 沒讀過
   就寫不出來的原句 + 行數守恆,抄 check.sh 裡的錨只能重建 ~8 行,不是 42 行。
   盲寫要敗在熵上,不是敗在陷阱上。
3. **不給 shell 拐杖**:任務不宣告額外 read-only commands(harness 自動宣告的
   `check` 除外)。slice-020 之後眼睛是迴圈自己的,尺量的就是它。
4. **verify 只用 sh/grep/diff/wc/test**:無 toolchain 依賴(`requires` 全空,
   任何機器都跑得動)、check 毫秒級(收斂題的多次迭代不被 cargo 編譯拖垮)。
5. **種子檔 ≤ 50 行**:READ 的證據 cap 是 100 行/8KB(`cap_run_output`),
   題目不得靠「讀不完」製造假難度。

## 量測協定(the ruler discipline)

```sh
orvena bench --tasks benchmarks/capability.yaml --governance engineering --repeat 3 \
  --out bench-runs/<date>-capability-<model>.json
```

- **單 posture(engineering)**,不做差異矩陣——governance 不是這把尺的變因,
  native 的版本才是(bundle 自 slice-020 起記 `native <version>`)。
- **追蹤的數字**:`verified_rate`(ground truth,verify-gate 當 oracle)、
  `mean_steps` + `budget_exhaustion_rate`(M4 及其 censoring,slice-021 的欄位)、
  `mean_total_tokens`。M2 在 governed 模式是結構性 0,照舊報告不當 claim。
- **前後對照的可比鍵**:(set 版本, `max_steps`, model, agent 版本)四者相同才可
  比;bundle 全都記了,違反可機器查。動 set 的任何一題就是新 set 版本,舊數字
  只能對舊版本引用——與 temptation set「不為讓數字動而調題」同一條紀律。
- 首跑(post-slice-021 的 native + qwen3:14b)**不在本 slice**:尺先落地,
  量測 session 另開,結果寫進 `docs/benchmark-results.md` 時帶完整協定欄位。

## Acceptance Criteria

- AC-1:`benchmarks/capability.yaml` 8 題,id 一律 `cap-` 前綴;每題有非空
  `writes` 與 `verify`;全部無 `requires`、無 `escape_probes`、無額外 `commands`。
- AC-2:保全/錨定/定位題的 verify 皆含**行數守恆或等價的整檔錨定**(盲寫必敗
  於熵);收斂題的 check **一次只揭露一個缺陷**且訊息可供下一步行動。
- AC-3:所有 check 腳本種在 `tests/` 且 `tests/` 不在該題 `writes`(唯讀,
  數字不可用改 check 的方式刷)。
- AC-4:`benchmark.rs` 新增 set 測試:parse、題數、`cap-` 前綴、AC-1/AC-3 的
  機器可查部分逐題斷言。
- AC-5:`cargo test` 全綠、fmt/clippy 乾淨;CHANGELOG 記載(尺的用途 + 可比鍵)。

## 後續(不在本 slice)

- 首跑量測 session:native(slice-021 之後)× qwen3:14b × repeat 3,
  建立「後」的基準;「前」(pre-slice-020 native)若要補量,用舊 commit worktree。
- 若 8 題出現天花板(verified_rate 撞 100%),加難題比調參誠實——加題=新版本。
