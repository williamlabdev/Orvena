# Handoff — 探針修復、glob、命中數:三個推論被連續推翻(0805 晚)

工作線:orvena 聰明線(capability 量尺)。接續 `HANDOFF_INVENTORY_AND_PROBE_0805.md`。

## 1. 現況一句話

探針修好並量出四格讀數,連帶開了 slice-027(SEARCH 吃 glob)與 slice-028(命中數進 evidence);
八個 commit 全推上 main,**但最後兩個(slice-028 的實作 + 文件)的 CI 還在跑,未驗**。

## 2. 已完成

| commit | 內容 | 證據 |
|---|---|---|
| `32801f1` | 修探針形狀:svc/ 唯讀、單一可寫目標 `rollup-overrides.conf` | 兩規模乾跑(seeded fail / 正解 pass / 錯服務 fail);`bench --provider offline` 端到端 |
| `3d373b0` | SLICE-026 裁決 1(toolchain 否決)、2(草擬 10–12 凍 8)+ 4 題備用(難易兩側) | — |
| `6d2881b` | 裁決 3:G 的 40 檔**否決**,縮回 12,改名 `capv2-needle-semantic` | 40 檔與 12 檔 action_counts 完全相同 |
| `bfd8331` | slice-027:SEARCH 路徑吃 glob(literal_separator、literal_prefix、選中零檔為錯) | 6 測試;workspace 全綠 + clippy |
| `6e7d013` | slice-027 重量測:死法換了、地板沒動 | `does not exist` 從 6 趟裡消失 |
| `5d2b318` | 27b 探針:也貼頂 → **沒有中間格** | 100%、2.2 步、7m43s |
| `24072f9` | slice-028:`search_hits` 進 evidence + `search_yield_rate` | 6 測試;schema 加性仍 v1 |
| `87113a6` | slice-028 讀數:**slice-027 的推論是錯的** | 失敗趟全零命中,成功趟第 1–2 次命中 |

**四格 + 兩次重跑的讀數**(`bench-runs/20260805-probe-search-scale-*.json`,皆未追蹤):

| 模型 | 通過 | 步數 | 牆鐘 | 備註 |
|---|---|---|---|---|
| qwen3:14b | 33% → 17% → **67%** | 6.5→7.8→4.3 | ~19m | 三跑同條件,**50 個百分點是雜訊** |
| qwen3.6:27b | 100% | 2.2 | 7m43s | dense,也貼頂 |
| qwen3.6:35b | 100% | 2.7 | **2m47s** | MoE,最強也最快 |

三個站得住的結論:
1. **形狀能逼出工具使用,prompt 規則不能**:語料改唯讀後 SEARCH 使用率 0% → 100%。
2. **規模不是難度**:40 檔與 12 檔一步不差(針可 grep 時,grep 成本不隨語料成長)。
3. **牆鐘由失敗步數決定,不由參數量決定**:最大的 35b 比 27b 快 2.8x、比 14b 快 7x。

## 3. 未完成與地雷

- **`24072f9` + `87113a6` 的 CI 未驗**(run 31003557276 推完仍在跑)。本地 20 target 全綠、clippy 乾淨。
- **n=3 的樣本量定不出地板**:同 binary 同模型同探針,17% → 67%。
  v2 校準若拿 14b 當地板,**repeat 必須拉高,或改用動作級讀數當主判準**(`search_yield_rate`、死法),通過率只當輔助。這件事還沒改進 SLICE-026 的校準協定。
- **14b 的失敗是瞎搜,不是不動手**(全零命中)。修法方向屬「題目線索 / pattern 能力」,
  不屬 slice-023 的 loop 紀律線——SLICE-027 內已加更正框,別再引用舊解釋。
- **bench-runs 多了 5 份未追蹤報表**,入版控仍未裁;m1-depth 兩個髒檔仍未動(第四個 session 沒碰)。
- 舊懸案未動:repo `.orvena/orvena.yaml` 仍 `max_steps: 3`;bench header endpoint 漂移;
  目錄重整(root 這次再 +3:SLICE-027、SLICE-028、本檔);深度跑擱置。
- evidence bundles 在 session scratchpad(`probe-v2-*` / `probe-v3-*` / `probe-v4-*`),session 結束會消失。

## 4. 下一步

```sh
# 1) 先確認 CI
gh run list --limit 3

# 2) v2 校準協定要改:n=3 不夠。決定 repeat 拉到多少,或改主判準
#    檔案:SLICE-026-capability-set-v2.md 的「## 校準協定」

# 3) v2 本體造題(裁決已下:10–12 題、凍 8、toolchain 不進、G 縮回 12 檔語意定位)
#    新檔 benchmarks/capability-v2.yaml,v1 原地凍結
```

## 5. 勿碰 / 等待

- **勿碰**:`benchmarks/capability.yaml` 任何一題;`docs/benchmark-results.md` 既有數字區;
  m1-depth 那兩個髒檔;`bench-runs/m1-depth-20260803/native-*.json`(競寫);
  `bench-runs/20260805-probe-search-scale-qwen3-14b.json`(第一跑,否證紀錄,數字不得引用)。
- **等 william 裁**:bench-runs 入版控;repo `max_steps` 對齊;bench header 漂移;目錄重整時點;深度跑重啟。
