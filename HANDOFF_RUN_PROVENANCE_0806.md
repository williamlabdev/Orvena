# Handoff — 跑況進報表(slice-029),與它翻出來的第四個前提

> 接續 `HANDOFF_PROBE_GLOB_HITS_0805.md`。工作線:orvena 聰明線(capability 量尺)。
> 寫於 0806 凌晨。

## 1. 現況一句話

上一棒的下一步是「改校準協定 + 造 v2 題」——量測結果把順序換掉了:
**地板格的判準免費解決了(死法分類,n=3 已飽和),目標帶格反而不能定價**,
因為取樣參數根本不在 repo 手上;slice-029 已把跑況記全並把取樣收回來,
**程式全綠但未 commit**,還缺 william 一個數字。

## 2. 已完成

### (a) 探針穩定性重跑 — 三次獨立呼叫共 30 run

`bench-runs/20260805-probe-stability-qwen3-14b-rerun{A,B}.json`(各 `--repeat 6`,
與 slice-028 那跑同 binary `24072f9` / 同 14b / 同 `max_steps 8` / `--governance engineering`)。

| 呼叫 | `needle-12` | `needle-40` | 通過率 | yield | 步數耗盡 |
|---|---|---|---|---|---|
| 028(n=3,19:33) | **3/3** | 1/3 | 67% | 21.1% | 33% |
| A(n=6,22:05) | **0/6** | 1/6 | 8.3% | 1.6% | 91.7% |
| B(n=6,23:50) | **0/6** | 1/6 | 8.3% | 1.4% | 91.7% |

三個結論:

1. **命中與通過是同一件事**——30 run 裡有命中的 6 趟全過、零命中的 24 趟全敗,無例外。
   所以 `search_yield_rate` 的**百分比**不是第二個讀數,它就是通過率的另一種寫法;
   能用的是它的**分類**(use 高 + hit≈0 + 燒到上限 = 瞎搜),而分類在三次呼叫裡完全一致。
   → **地板格 n=3 已飽和,拉高 repeat 買不到東西。**
   (連帶:「命中後不動手」30 趟一次都沒出現,slice-027 那個推論第二次被否證。)
2. **呼叫間可以完全可重現**——A 與 B 逐項對上。離群的是 028 的 `needle-12`。
3. **所有受控變數一致仍差 100 個百分點**——provider/model/endpoint/governance/agent/
   max_steps 全同,binary 同一顆,探針 YAML 自 `32801f1` 後未動。差異只可能在報表沒記的東西裡。

### (b) SLICE-026 校準協定已改(未 commit)

- 規則 1(35b 目標帶):加註 **repeat 尚未定價**,在跑況可重現之前不定。
- 規則 2(14b 地板):主判準改為**死法分類,不是通過率**。
- 新增「地板格的主判準」一節(上表 + 三個結論)。
- 成本估計加 0806 修正:14b 那一腿不變;35b 的 28s/run 是「2 步完賽」成本,失敗的 35b **從沒量過**。

### (c) SLICE-028 加更正框(未 commit)

原文「改用 `search_yield_rate` 當主判準」對了一半:百分比不能用,分類可以用。

### (d) SLICE-029 實作完成(未 commit)—— `cargo test --workspace` 21 target 全綠、clippy 乾淨

新檔 `SLICE-029-run-provenance.md`(status: DONE)+ `crates/orvena-core/tests/run_provenance.rs`(7 測試)。

- `config/agent.rs`:新增 `Sampling{temperature,top_p,top_k,seed}`;`ProviderSelection.sampling: Option<Sampling>`,
  **`None` = inherited,不是「等於預設」**。用 `f64` 不用 `f32`(`0.6f32` 序列化成 `0.6000000238418579`,
  那個值會真的送到後端也會落進報表)。
- `provider/mod.rs`:`ProviderProvenance` + `Provider::provenance()` 預設 `None`。
- `provider/ollama.rs`:設了才送 `options`;讀回 version / digest / quant / declared+effective ctx(全 best-effort)。
- `provider/{anthropic,openai_compat}.rs`:各自送可支援的;**anthropic 收到 `seed` 硬失敗**(Messages API 沒這參數)。
- `benchmark/{report,aggregate,runner}.rs`:`RunProvenance` 進 BenchReport / RepeatedReport / MatrixReport;
  **跑完才讀**(冷 server 的 `/api/ps` 是空的)。
- `cli/commands/bench.rs`:`print_provenance` 兩行。
- 設計取捨:取樣參數掛 provider 而非 `ChatRequest`——因此 driver 與兩個既有 wire 測試一行未動。
- 草案兩處已更正:**沒有 schema 變更**(`evidence.v1.json` 管 bundle,bench report 無 schema)。

**實地 smoke(新 binary,repeat 1,`scratchpad/smoke-provenance.json`)**:

```
provenance: ollama 0.32.5 | digest bdbd181c33f2 | Q4_K_M | ctx 32768 of 40960
sampling:   inherited from the backend — not repo-controlled, not reproducible
```

**第一次跑就抓到東西:effective ctx 32768,declared 40960。** runtime 給的比模型宣告的少 20%,
而這在本刀之前完全不可見——它正是 028 離群的頭號候選。

### (e) 翻出來的第四個前提:兩個校準格從來不同溫

`/api/show` 讀到的 Modelfile 預設:**`qwen3:14b` temperature 0.6**;
**`qwen3.6:27b` / `35b` temperature 1**(還多 `presence_penalty 1.5`)。

範圍別擴大:**同模型的階梯不受影響**(0.1.0→0.4.0 是同一顆 14b 跨 agent 版本,全程同條件)。
受影響的是**跨模型並列**——SLICE-026 那張三模型表、`docs/benchmark-results.md` 的跨模型欄。
那些數字不是錯的,它們量的是「各模型在各自出廠設定下」的表現,但那不是我們以為在量的東西。
**本 session 沒有動任何既有數字**,只是讓它可見。

## 3. 未完成與地雷

- **本 session 零 commit、零 push**。33 個 modified + 4 個 untracked(含 `SLICE-029-run-provenance.md`
  與 `crates/orvena-core/tests/run_provenance.rs`)。**程式已全綠但未進版控,這是最大的殘留。**
- 33 個 modified 裡有 **41 處是機械插入 `sampling: None,`**(補齊 struct literal),
  分佈在 adapters / tests / registry。逐檔看會嚇到,但那是同一個一行變更。
- **CI 未跑**(沒 push)。本地證據:`cargo test --workspace` 21 target 全綠、`cargo clippy --workspace --all-targets` 乾淨。
- **35b 的 repeat 仍未定價**,前提是先確定跑況可重現(現在有工具了,但還沒重跑驗證)。
- bench-runs 現有 **7 份未追蹤報表**(5 份 0805 探針 + 2 份 stability),入版控仍未裁;
  m1-depth 兩個髒檔仍未動(第五個 session 沒碰)。
- 舊懸案未動:repo `.orvena/orvena.yaml` 仍 `max_steps: 3`;bench header endpoint 漂移;
  目錄重整(root 這次再 +2:SLICE-029、本檔);深度跑擱置。
- 背景跑被中止過兩次(rerunA 第一次從零被停、rerunB 第一次 9 分鐘被停),原因不明;
  script 是冪等的(OUT 已存在就跳過),重跑不會重複計算。

## 4. 下一步

```sh
# 1) 先把 slice-029 收進版控(本 session 唯一的大殘留)
cargo test --workspace && cargo clippy --workspace --all-targets   # 再驗一次
git add SLICE-026-capability-set-v2.md SLICE-028-search-hits.md SLICE-029-run-provenance.md \
        crates/ && git commit && git push && gh run list --limit 2

# 2) 等 william 裁 B1/B2(見下節),裁完寫進 orvena.yaml 的 provider.sampling
#    B1 = 所有模型同一組取樣;B2 = 各自維持原值

# 3) 裁完才輪到:用同一支探針重跑兩次驗「檔頭完全一致」→ 再定 35b 的 repeat → 造 v2 題
#    造題檔案:benchmarks/capability-v2.yaml(v1 原地凍結)
```

## 5. 勿碰 / 等待

- **勿碰**:`benchmarks/capability.yaml` 任何一題;`docs/benchmark-results.md` 既有數字區
  (跨模型欄已知不同溫,但**加註與否等裁**,不要自行改);m1-depth 那兩個髒檔;
  `bench-runs/20260805-probe-search-scale-qwen3-14b.json`(第一跑,否證紀錄,數字不得引用)。
- **等 william 裁**:
  1. **B 底下的分岔(最擋路的一個)**——`Sampling` 目前預設 `None`(inherited),
     **在給出數字之前沒有任何既有讀數被動到**。
     **B1 統一**:跨模型第一次真的可比;代價是既有跨模型並列全變「舊條件」,要重跑或加註。
     **B2 各自維持**:既有數字完全連續;但跨模型仍不可比,而那正是兩格校準要做的事。
     上一棒的建議是 **B1**(兩格校準的前提就是「除了模型什麼都一樣」)。
  2. 本 session 的 33 個 modified 要不要就這樣一筆 commit。
  3. bench-runs 入版控;repo `max_steps` 對齊;bench header 漂移;目錄重整時點;深度跑重啟。
