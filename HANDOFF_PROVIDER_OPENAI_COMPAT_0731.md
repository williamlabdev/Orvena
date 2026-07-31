# HANDOFF — openai_compat provider + CTR 修正(2026-07-31)

## 1. 現況一句話

Orvena 新增通用 `openai_compat` provider kind(讓 vLLM / llama.cpp / LM Studio / Groq 等 OpenAI 相容端點成為一等公民),隨後的 CTR 抓出 3 個 🔴 並全數修復,共 4 個 commit 已推上 `main`,CI 綠燈;**唯一未閉合的是兩個既有 PR(#18、#19)會因本次變更而編譯失敗,需要 rebase**。

---

## 2. 已完成

### 推送的 commit(`b736ef4..60a5987`,已在 `origin/main`)

| commit | 內容 |
|---|---|
| `5294036` | feat:`openai_compat` provider kind 本體 |
| `3ea45b5` | fix:CTR 的 3 個 🔴 + `_ =>` 靜默預設陷阱 + 文件收尾 |
| `b47eb9a` | fix:provenance、parity artifact、init picker 誠實化、scaffold sandbox 註解 |
| `60a5987` | docs:全 repo provider kind 列表掃描 + bench 腳本支援 `API_KEY_ENV` |

(`6347ed6`、`1d3573b` 是本 session 之前就在本地、這次一併推出去的。)

### 驗證證據(皆為實跑,非宣稱)

```
cargo fmt --check                              → clean
cargo clippy --workspace --all-targets -D warnings → clean
cargo test --workspace                         → 135 passed, 0 failed
gh run list --branch main --limit 1            → completed success (run 30592290887, 2m27s)
```

真實端到端 smoke test(非 mock,打本機 Ollama 的 OpenAI 相容端點):

```sh
ORVENA_PARITY_PROVIDER=openai_compat \
ORVENA_PARITY_BASE_URL=http://localhost:11434/v1 \
ORVENA_PARITY_MODEL=qwen3:14b \
  cargo test -p orvena-core --test provider_parity -- --ignored --nocapture
# → completed=true steps=1 tokens=373 gates=[("hello-exists", true)]
```

三個 🔴 的修正各自用**實跑情境**驗證(非只靠單元測試):

| 🔴 | 修前 | 修後(實測輸出) |
|---|---|---|
| #1 `anthropic` + `api_key_env` | `doctor` 全綠 → run 死在 `ANTHROPIC_API_KEY` | `✗ provider 'anthropic': ANTHROPIC_API_KEY not set` |
| #2 `openai_compat` 缺 `base_url` | `All checks passed` | `✗ provider 'openai_compat': base_url is not set` |
| #3 `offline` + 殘留 `api_key_env` | 被擋,且錯誤訊息叫你跑 `--provider offline`(循環建議) | `✓ ready`,run 正常完成 |

### 新增的可稽核 artifact

`docs/parity-results/2026-07-31-openai_compat-qwen3-14b.json` — 真跑產出,自帶
`provider` / `model` / `endpoint`,由 `evidence_schema.rs` 的測試釘住「必須 schema-valid 且能自述」。

---

## 3. 未完成與地雷

### 🔴 地雷:PR #18 / #19 會編譯失敗(不是單純文字衝突)

兩個 PR 都**落後 main 14 個 commit**,且是**堆疊**的(`#19` 的 base 是 `#18` 的分支,不是 main)。

我這次改動了它們碰到的檔案,而且是**破壞性簽名變更**:

- `crates/orvena-core/src/metrics/mod.rs` — `RunReport` 新增 `provider` / `model` / `endpoint` 三個欄位 + `with_provenance()`(PR #18 有碰這個檔)
- `crates/orvena-core/src/benchmark/report.rs` — `BenchReport` / `RepeatedReport` / `MatrixReport` 三個 struct 新增**必填** `endpoint` 欄位
- `crates/orvena-core/src/benchmark/aggregate.rs` — `aggregate()` 簽名多一個參數
- `crates/orvena-core/tests/benchmark.rs` — `ProviderSelection` 的 10 處 struct literal 加了 `api_key_env`(兩個 PR 都碰這個檔)

**關鍵陷阱**:`gh pr view 19` 顯示 `mergeable=CLEAN`,**那是騙人的**——它是對 `#18` 的分支算的,不是對 main。即使 git 文字合併乾淨,rebase 到 main 後任何建構上述 struct 的程式碼都會編譯失敗(missing field / arity mismatch)。`#18` 目前 `mergeable=UNKNOWN`(GitHub 尚未算出)。

### 未閉合

- PR **#18** `feat/slice-018-aider-adapter`(base=main)— OPEN,14 behind
- PR **#19** `feat/slice-019-temptation-capability`(base=#18 分支)— OPEN,14 behind

### 已知風險 / 誠實聲明

- **`openai_compat` 只對 Ollama 的相容端點實跑過**,沒對真正的 vLLM / llama.cpp / LM Studio / Groq 跑過。`docs/provider-parity.md` 已如實標註,狀態表有一列專門寫「never run」。
- **無 wire-level 測試證明「不送 Authorization header」**——repo 無 mock HTTP 依賴(`wiremock`/`httpmock` 皆無),現有測試斷言的是 struct 欄位 `api_key.is_none()`,不是實際 header。
- **`api_key_evn` 這類拼錯會被 serde 靜默忽略**(全 repo 無 `deny_unknown_fields`),對 `openai_compat` 等於靜默降級成無驗證。已寫進 rustdoc 與 scaffold 註解警告,但**程式層沒有防護**。
- 本批變更**直接推 main**,未走 PR。此 repo 近期兩種都有(`b736ef4` 是 merge PR #20,但 `1d3573b`/`6347ed6` 是直推)。

---

## 4. 下一步

```sh
cd /Users/william/dev/source/core/aine/orvena

# 1) 先救 PR #18(在它下面的 #19 才有得救)
git fetch origin
git checkout feat/slice-018-aider-adapter
git rebase origin/main
#    衝突熱點:crates/orvena-core/src/metrics/mod.rs(RunReport 新欄位)
#              crates/orvena-core/tests/benchmark.rs(ProviderSelection 加 api_key_env)
cargo build --workspace --tests    # ← 用編譯器當檢查清單,別手動找
cargo test --workspace
git push --force-with-lease

# 2) 再救 PR #19(base 是 #18,務必等 #18 rebase 完)
git checkout feat/slice-019-temptation-capability
git rebase origin/main             # 建議直接改 base 為 main,解除堆疊
cargo build --workspace --tests
cargo test --workspace
git push --force-with-lease
```

補齊 `openai_compat` 覆蓋(選一即可,任一都比現況強):

```sh
# 對真正的第三方後端跑 parity,並留下 artifact
ORVENA_PARITY_PROVIDER=openai_compat \
  ORVENA_PARITY_BASE_URL=http://localhost:8000/v1 \
  ORVENA_PARITY_MODEL=<vllm-model-id> \
  ORVENA_PARITY_EVIDENCE_OUT="$PWD/docs/parity-results/<date>-openai_compat-<model>.json" \
  cargo test -p orvena-core --test provider_parity -- --ignored --nocapture
# 注意:EVIDENCE_OUT 必須是絕對路徑(cargo test 的 cwd 是 crates/orvena-core)
```

---

## 5. 勿碰 / 等待

- **等 william 裁決**:
  1. `openai_compat` 是否要補 wire-level 測試(需引入 `wiremock` 之類的 dev-dependency,是新依賴)。
  2. 是否要對 provider config 加 `deny_unknown_fields`,把 `api_key_evn` 拼錯從靜默降級變成硬錯誤(會使既有含未知欄位的 config 開始報錯,屬破壞性)。
  3. 這類功能級變更今後走 PR 還是直推 main。
- **勿碰**:PR #18 / #19 的分支——除非正在執行上面的 rebase。它們是別人(或別的 session)的活線,且互相堆疊,亂動會連鎖。
- **`docs/parity-results/*.json` 勿手改**:有測試釘住格式,且它們是「真的有人跑過」的證據,手改等於偽造。要更新就重跑產生。
- **`schemas/evidence.v1.json` 是凍結的**:本次是靠「additive fields keep v1」政策(`crates/orvena-core/src/metrics/mod.rs:14`)加欄位,再要動請先確認仍屬 additive,否則要 bump v2。

---

*本 session 的 CTR 報告(16 條 finding,含 2 支零上下文 fresh-eyes 的產出)未落檔,只存在對話中。若需留痕,來源是 4 個 commit 的 message——每個都寫明了修的是哪條 finding 與為什麼。*
