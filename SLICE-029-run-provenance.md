# Slice: 跑況進報表 —— 「同一個模型」目前不是一個可查證的說法

> 聰明線第十刀(0806)。slice-028 掉出「n=3 定不出地板」,補跑兩次獨立呼叫
> 之後掉出更難的一件事:**所有記錄在案的變數都一致,結果仍差 100 個百分點**。
> 差異只可能在報表沒記的東西裡——而報表沒記的東西,現在沒有人查得出來。

## 缺口:報表記的欄位不足以重現一次跑

三次獨立呼叫、同一支探針、同一顆 binary(`24072f9`)、同一個 `qwen3:14b`
(`bench-runs/20260805-probe-stability-qwen3-14b-rerun{A,B}.json` 與
slice-028 那跑,詳表見 SLICE-026「地板格的主判準」):

| 呼叫 | `needle-12` | 通過率 | yield |
|---|---|---|---|
| 028(19:33) | **3/3** | 67% | 21.1% |
| A(22:05) | **0/6** | 8.3% | 1.6% |
| B(23:50) | **0/6** | 8.3% | 1.4% |

A 與 B 逐項對上,028 是離群的那一個。而 `BenchReport` 檔頭記的每一個欄位
——`provider` / `model` / `endpoint` / `governance` / `agent` / `run_id`——
三跑**完全相同**。也就是說:**報表無法分辨「A 與 B 那種一致」與「028 那種離群」
是不是同一個實驗**。這不是少知道一件事,是**不知道自己有沒有量到同一件事**。

翻代碼之後,沒被記下來的東西比預期多,而且其中三件是**外部可變的**:

1. **取樣參數不是 Orvena 設的,是 Modelfile 給的。**
   `ChatRequest`(`provider/mod.rs:38`)只有 `messages` 與 `max_tokens`;
   `Ollama::chat`(`provider/ollama.rs:38`)送出的 body 只有 `model` / `messages` /
   `stream`,**沒有 `options`**。所以實際生效的是 `qwen3:14b` 的 Modelfile 預設:
   `temperature 0.6`、`top_k 20`、`top_p 0.95`,**且沒有 seed**。
   每一趟都是一次全新抽樣——run 間變異是預期的,不是異常;
   但**這組數字來自一個 repo 控制不到的檔案**,重 pull 一次 tag 就可能換掉,
   而所有既有讀數會靜靜地位移,報表上看不出來。
2. **`model` 記的是 tag,不是 digest。** `qwen3:14b` 是可變指標。
   同名不同權重的兩次跑,報表上完全一樣。
3. **生效的 context 長度沒有記,也沒有設。** 模型宣告 `context_length 40960`
   (`/api/show`),但實際 `num_ctx` 由 ollama runtime 決定,受當下記憶體壓力與
   同時載著哪些模型影響。028 那跑的前後,同一個 session 剛跑完 27b 與 35b 的探針。
4. **server 版本沒有記。** 本機現為 ollama `0.32.5`。

028 那個 scratch 在上一個 session 的 scratchpad 裡,已經消失——**這四件事
現在一件都回查不了**。這正是本刀要擋的下一次。

## Frontmatter

```yaml
slice_id: slice-029-run-provenance
title: Run provenance in the report — "the same model" is not yet a checkable claim
status: DONE   # 機制已實作 + 7 個測試;取樣的「設成多少」仍待裁,見下
governance_tier: light
dependencies: [slice-026-capability-set-v2, slice-028-search-hits]
delivers:
  - config:   crates/orvena-core/src/config/agent.rs      # Sampling 區塊;None = inherited(非「等於預設」)
  - provider: crates/orvena-core/src/provider/mod.rs      # ProviderProvenance;Provider::provenance() 預設 None
  - ollama:   crates/orvena-core/src/provider/ollama.rs   # 送出 options;讀回 version/digest/quant/ctx
  - others:   crates/orvena-core/src/provider/{anthropic,openai_compat}.rs  # 各自可支援的參數;不支援的拒絕
  - summary:  crates/orvena-core/src/benchmark/report.rs  # RunProvenance 進三種報表檔頭
  - runner:   crates/orvena-core/src/benchmark/runner.rs  # 跑完才讀(冷 server 的 /api/ps 是空的)
  - cli:      crates/orvena-cli/src/commands/bench.rs     # provenance / sampling 兩行
  - tests:    crates/orvena-core/tests/run_provenance.rs  # 7 個測試
```

**取樣參數不進 `ChatRequest`,進 provider 本身**:它是「這個後端被設定成怎樣」,
不是「這一次請求要什麼」。因此 `ChatRequest` 與 driver 一行未動,
兩個既有的 wire 測試也不必改。

**沒有 schema 變更**:`schemas/evidence.v1.json` 管的是 evidence bundle,
而 provenance 屬於 bench report——後者本來就沒有 JSON schema。草案寫錯了,此處更正。

## 三個不變式

1. **「沒記到」與「等於預設」是兩回事。** provenance 欄位讀不到時記 `null`,
   不記推測值——一份舊報表沒有 digest,不代表它跑的是現在這顆權重。
   與 `action_counts` 的 `None ≠ 全零`、`search_hits` 的 `null ≠ 0` 同一條規矩。
2. **記的必須是「生效值」,不是「請求值」。** `num_ctx` 要記 runtime 實際給的,
   不是我們送出去的;兩者不同時,不同的那件事本身就是情報。
3. **provenance 不進任何比率的分母。** 它是身分,不是讀數;
   它的用途是讓兩份報表能被判定為「可不可比」,而不是產生新的分數。

## 判讀方式

檔頭多一個可重現性區塊,兩份報表可比與否變成一眼可判:

```
provenance: ollama 0.32.5 | qwen3:14b @ sha256:… | Q4_K_M | num_ctx 40960
sampling:   temperature 0.6  top_k 20  top_p 0.95  seed —
```

- **digest 不同** → 兩份報表不可比,不必再看下去。
- **`num_ctx` 不同** → 可比性存疑;這正是 028 與 A/B 之間最可能的差異點。
- **sampling 不同** → 不可比。而 `seed —` 這一行本身就在說:**這份讀數不可重現,
  只能重複抽樣**。

## 造這一刀時翻出來的第四件事:兩個校準格從來不同溫

查 `/api/show` 才看到的,三個格的 Modelfile 預設**並不一致**:

| 模型 | temperature | top_k | top_p | 其他 |
|---|---|---|---|---|
| `qwen3:14b` | **0.6** | 20 | 0.95 | `repeat_penalty 1` |
| `qwen3.6:27b` | **1** | 20 | 0.95 | `min_p 0`、`presence_penalty 1.5` |
| `qwen3.6:35b` | **1** | 20 | 0.95 | 同上 |

也就是說 SLICE-026 那張「三個模型跑完」的表、以及 v2 校準協定要用的**地板格與
目標帶格,從來不是在同一組取樣條件下量的**——而報表上完全看不出來。

範圍要講清楚,不要擴大:

- **同模型的階梯不受影響。** 0.1.0→0.4.0 那條是同一顆 14b 跨 agent 版本,
  全程同條件,仍然成立。
- **受影響的是跨模型的並列**(SLICE-026 的 14b/27b/35b 表、
  `docs/benchmark-results.md` 的跨模型欄)。那些數字不是錯的,
  它們量的是「各模型在各自出廠設定下」的表現——但那不是我們以為在量的東西,
  也不是「同一把尺」該有的意思。

這件事本身沒有動任何既有數字,只是讓它可見。要不要重跑、要不要在結果頁加註,
與下面那個裁決是同一件事。

## 待裁:取樣策略(這一刀真正的設計問題)

機制已經做完,`None` = inherited 也已如實記錄與印出——**現狀行為一行未變**,
既有設定跑出來的報表只是多了一行「sampling: inherited,不可重現」。
剩下的是**設成多少**,那是 william 要裁的,因為它改變尺量的是什麼:

| 選項 | 量到的是 | 代價 |
|---|---|---|
| **A. 只記不設**(維持現狀) | 「這顆模型在它出廠設定下的表現」 | 讀數永遠是抽樣;Modelfile 一換,舊數字靜靜位移(記了 digest 至少看得出來) |
| **B. 顯式設定取樣參數,不設 seed** | 同上,但參數由 repo 控制 | 與別人「直接跑 ollama」的體感脫鉤;要決定設成多少,而那個數字本身需要理由 |
| **C. 顯式設定 + 固定 seed** | 單一路徑的可重現讀數 | **repeat 失去意義**(同 seed 同輸出),等於把「這顆模型多穩」這個維度整個丟掉——而那正是 slice-028 之後最想量的東西 |

**已裁(0806,william):B。** A 讓外部檔案能改寫我們的歷史數字,
C 把 repeat 這個維度殺掉。B 保留抽樣(所以 repeat 仍有意義),同時讓「校準跑」
與「階梯跑」的取樣條件由 repo 固定下來、寫進檔頭。若要 C,也應該是**額外一組跑**
(fixed-seed 的回歸跑),不是取代 repeat。

**B 底下還有一個分岔,是上一節那張表逼出來的、裁決當時還不知道的**:

| | 做法 | 代價 |
|---|---|---|
| **B1 統一** | 所有模型同一組取樣 | 跨模型第一次真的可比;但既有的跨模型並列全部變成「舊條件」,要重跑或加註 |
| **B2 各自維持** | 每個模型沿用它原本的值,只是寫進 config | 既有數字完全連續;但**跨模型仍不可比**,而那正是兩格校準要做的事 |

`Sampling` 目前是 `Option`,預設 `None`(inherited),`orvena init` 也沒有多寫東西
——**所以在你給出數字之前,沒有任何既有讀數被動到**。選定之後是 config 一個區塊的事。
我的判斷是 **B1**:兩格校準的整個前提就是「除了模型以外什麼都一樣」,
B2 會讓那個前提永遠不成立。代價(既有跨模型表要加註)是一次性的,而且那張表
本來就已經不成立了,只是現在看得見。

**這一刀要排在 35b 的 repeat 定價之前**(SLICE-026 校準協定規則 1):
在跑況能被重現之前,一趟十幾小時的校準有可能整個落在「028 那種狀態」,
而讀報表的人分辨不出來。

## 驗證

`cargo test --workspace` 21 個 target 全綠、clippy 乾淨。新增 `tests/run_provenance.rs`:

| 測試 | 守的是 |
|---|---|
| `a_report_written_before_this_slice_reads_back_as_not_recorded` | 不變式 1:舊報表讀回是 `None`,不是空區塊 |
| `inherited_sampling_is_not_the_same_state_as_recorded_sampling` | 不變式 1:inherited ≠ 「剛好同值」 |
| `declared_and_effective_context_are_both_kept_when_they_differ` | 不變式 2:runtime 值不被 declared 蓋掉 |
| `provenance_never_moves_a_rate` | 不變式 3:清掉 provenance 後兩份報表 byte-identical |
| `inherited_sampling_sends_no_options_key_at_all` | 沒設就**不送 `options`**,而不是把 Modelfile 的值抄回去送 |
| `configured_sampling_crosses_the_wire_verbatim` | 設了就照送;未設的 seed **缺席**而非送 0(0 是合法 seed) |
| `anthropic_refuses_a_seed_it_cannot_honor` | 不支援的參數硬失敗,不靜默丟棄 |

後兩類走 wire-level(一次性 `TcpListener`,沿用 `provider_wire.rs` 的做法,
不引入 mock-HTTP 依賴)——會不會真的送出去,只有看 socket 才算數。

`f64` 而非 `f32`:`0.6f32` 序列化成 `0.6000000238418579`,那個值會真的送到後端、
也會落進每一份公開報表。crate 其他地方的 rate 維持 `f32`(那些是算出來的,
末位不帶意圖)。

實地驗收:拿本刀後的 binary 重跑同一支探針兩次,**檔頭應完全一致**;
若某次不一致,那就是 028 當時發生過而我們沒看見的那件事。
