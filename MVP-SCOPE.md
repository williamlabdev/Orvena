# Orvena — MVP Scope

**Status:** Working draft · **Updated:** 2026-07-04
**Purpose:** 把「AINE 方法論的完整性」與「Orvena 產品的可交付性」分開。這份文件只回答一個問題:
**要證明 Orvena 的核心主張,最少需要交付什麼?其餘一律 defer。**

---

## 0. 一條判準線(先讀這個)

MVP-ready 不是「方法論全部實作完」,而是:

> **選最小的垂直切片,能對外證明「bounded autonomy + evidence trail」是可執行的,並產出一個誠實的外部 benchmark 數字。其餘全部延後。**

兩個治理原則,對應第一個對話裡的張力解法:

- **建 MVP 時對自己跑 `light` tier。** 唯一硬約束是下面第 1 節的成功判準。文檔鏈(Vision→Roadmap→Blueprint→ADR)、三域閉環、metrics 報表都降級為 advisory,可事後補。
- **`engineering` tier 是產品要 demo 的能力,不是日常開發的枷鎖。** 你要證明它「可執行」,不是對每次 commit 都執行它。
- **但要 dogfood 一次:** 日常 light,唯獨「benchmark harness」這一條 slice 自己用 `engineering` tier 跑到底,一次到位地把 gate + evidence 當作對外 demo 的證據。否則你永遠沒真正跑過自己的核心賣點。

---

## 1. 成功判準(分兩層,別綁在一起)

VISION 的長期承諾是「發布一個誠實的外部 benchmark 數字」。但那把兩件難事綁死了(能跑真實任務 ＋ benchmark harness),會讓 MVP 門檻回到跟方法論一樣高。拆成兩層:

**MVP exit(唯一必達)— 手選任務證據包**

- [x] 一小組手選、可自動驗證的真實 coding 任務,Orvena 跑完後能**匯出一份證據包檔案**(gate 結果 + 完成/未完成 + blockers + steps/tool_calls/tokens)。〔slice-003,`.orvena/runs/<ts>/evidence.json`〕 ⚠️ 註:持久化 event log 依第 4 節仍 defer,證據包載的是 `RunReport` 現有欄位,尚**不含 events**。
- [x] 至少在**兩個真實 provider** 上跑通,行為一致。〔Ollama 本地 `qwen3:14b` + Gemini 雲端 `gemini-2.5-flash` 均證綠;可重跑 parity 契約見 `docs/provider-parity.md`。Gemini 暫代 Anthropic,後者有 key 隨時可加入同一契約。〕

**MVP+1(下一步,不擋 MVP)— 對外 benchmark 數字**

- [ ] 把上面的任務集擴成一個可重現 benchmark harness,算出完成率。
- [ ] 公開該數字,附方法說明,不美化。

**只有第一層是 MVP 的 exit criteria。** 下面的 must-have 都是為了讓第一層成立。

---

## 2. 現況盤點(Rust `orvena-core`,實測)

> ⚠️ 重要落差:AINE VISION v2.1 的「Real and runnable now」清單描述的是**舊 Python runtime**。目前的 **Rust 重寫版是它的子集**,以下以 Rust crate 的實際檔案為準。

| 子系統 | Rust 現況 | 檔案 |
| :--- | :--- | :--- |
| Bounded 迴圈(prepare→call→apply→gate,capped by max_steps) | ✅ 可運作 | `agent/driver.rs` |
| Provider 抽象(Anthropic/OpenAI/OpenRouter/Ollama/offline,無預設) | ✅ 可運作 | `provider/*` |
| Config-first YAML(roles/gates/context-budgets/orvena.yaml) | ✅ 可運作 | `config/*` |
| 三紀律(scope lock、read-only default、verifiable gates) | ✅ 可運作 | `governance/scope.rs`, `governance/gate.rs` |
| Human/automated gate + verify 證據 | ✅ 可運作 | `governance/gate.rs` |
| L1 regression metrics(completed/tokens/steps/tool calls + golden baseline) | ✅ 可運作 | `metrics/baseline.rs` |
| 最小 skill engine(discover→resolve→apply) | ✅ 可運作 | `skills/*` |
| CLI(init/run/doctor/status) | ✅ 可運作 | `orvena-cli/*` |
| **工具集** | ✅ `fs`(讀寫檔)+ `grep`(唯讀搜尋,slice-001)+ `shell`(宣告式 RUN,slice-002/ADR-001) | `tools/fs.rs`, `grep.rs`, `shell.rs` |
| Session snapshot / crash recovery | ❌ Rust 版尚無 | — |
| Evidence-bundle exporter(可匯出審計包) | ✅ 可運作(slice-003,ADR-002) | `metrics/evidence.rs` |
| In-process 多迴圈編排(delegation / role routing) | ❌ Rust 版尚無 | — |
| 測試覆蓋 | ✅ 37 unit + 17 整合(含 CLI 首跑、跨 provider parity 骨架;parity 預設 ignored) | `crates/**` |

---

## 3. MVP Must-Have(為了達成第 1 節,缺一不可)

按優先序,全部是「小而必要」:

1. **shell + grep 工具。**(大)只有 `fs` 無法完成真實 coding 任務,更無法跑 benchmark。這是最高優先、也是最大的功能缺口。 **✅ 已交付**(grep:slice-001;宣告式 shell RUN:slice-002/ADR-001)。
2. **Evidence-bundle exporter。**(小 — 幾乎免費)`RunReport` 已經 derive `serde::Serialize`,且已在記憶體帶了 `gate_outcomes` 與 `blockers`;目前 `run.rs` 只 `println!` 到 stdout。所以這項只是「把已可序列化的 report 寫成一份檔案」,不是新子系統。核心賣點「evidence by default」的最小可證版本,成本很低,優先做掉。 **✅ 已交付**(slice-003/ADR-002;成功與失敗的 run 都落地)。
3. **verify gate 的可靠性。**(中)「done = 你的 test 指令 exit 0」要在真實專案上穩定成立。目前只有 4 個測試(2 unit + `loop_offline` 整合),先把這條的回歸測試補起來。 **✅ 已交付**(slice-004:修 verify 靜默失敗的回饋收斂 bug + 回歸測試)。
4. **doctor / init 的乾淨首跑。**(中)新使用者能 `init → run → 拿到證據包`,不卡在設定。這是唯一面向外部信任的入口,必須無痛。 **✅ 已交付**(slice-005:`--provider` 覆寫 + 就緒 preflight;`--provider offline` 零設定首跑)。
5. **最小 benchmark harness → 屬 MVP+1。** 見第 1 節;不擋 MVP exit,別排進第一層。

> 對 provider 的答覆(第二個問題):**維持現有 BYO 抽象即可,不在 MVP 階段選 local 或 cloud。** Anthropic 當首跑推薦,Ollama 覆蓋私有/受監管場景。MVP 不需要新增 provider,只需保證 evidence/gate 行為在 **Anthropic + Ollama 兩個真實 provider** 上一致(offline 只作回歸基準,見第 5 節)。

---

## 4. 明確 Defer(MVP 不做,標記出處避免罪惡感)

這些對應 VISION 的 "Aspirational / in progress" 與 AINE Roadmap 的 V2/V3,**刻意延後**:

- Session 持久化事件日誌 / real-time metrics engine → V1 後
- In-process / 跨行程 多 agent 編排(delegation、routing、handoff contracts)→ **AINE M-8~M-12(V2)**
- 沙箱化 plugin 系統(第三方程式碼安全)→ V3
- goal / reflection / plugin 子系統接入預設 run path → 延後
- 治理自動化引擎、delivery analytics dashboard、多專案聚合 → **AINE M-13~M-16(V3)**
- 內部 "LV" 成熟度分級 → 不進入公開產品敘事(VISION 已定調)
- 完整文檔鏈與三域閉環的「每次都執行」→ 只在需要 demo `engineering` tier 時執行

---

## 5. 唯一要盯的風險

**Provider 抽象越通用,`engineering` tier 的保證(evidence / gate / 可重現)就越難跨 provider 一致。**
→ MVP 階段先只承諾 **Anthropic + Ollama 兩個真實 provider** 上 evidence 與 gate 行為一致,其餘標為 best-effort。
→ 注意:**offline 是 deterministic stub,只能當回歸基準,不能當跨 provider 一致性的證據**——跟一個沒有真實模型行為的 stub 一致,證明不了什麼。這是未來真正的工程重點,不是「local vs cloud」的選擇題。

---

## 6. 一句話總結

> 方法論的完整性是 Orvena 的**賣點**,不是它的**交付節奏**。
> **MVP = shell/grep 工具 + evidence exporter(近乎免費)+ gate 可靠性,在手選任務上匯出證據包。**
> **MVP+1 = 擴成 benchmark harness,對外發布一個誠實的完成率數字。**
> 兩者分開,別綁在一起。其餘全部 defer。
