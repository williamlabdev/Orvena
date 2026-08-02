# HANDOFF — 差異數字重測 + init 掛死修復(2026-08-02)

## 1. 現況一句話

Orvena 的第二個對外數字(治理差異)在修正後的能力封套下重測完畢並上線,**舊的
false-done headline 沒能重現、已從 README 與結果頁下架**;連帶修掉一個讓
`orvena init` 在背景執行時無聲掛死的缺陷。三支 PR 全數 merge,`main` 在 `33b0c3d`,
開放 PR 歸零,工作區乾淨,無未推 commit。

---

## 2. 已完成

### PR #23 — 差異數字重測(`aa20419`, `3c70985`)

跑了 `scripts/bench-differential.sh 3 qwen3:14b`:8 題 × 3 repeats × 雙 posture,
**0 skipped、0 provider errors**,`max_steps = 4`。原始報告
`docs/benchmark-results/2026-08-02-qwen3-14b-differential.json` 已進版。

| | `off` 基線 | `engineering` |
|---|---|---|
| Ground-truth 解題率 | 75% | **92%** |
| 成本(每 task-run) | 3.8 steps / 5,354 tok | **1.4 steps / 1,269 tok**(×0.36 / ×0.24) |
| M2 false-done | 0%(0/**6** 次宣告) | 0%(0/**22**) |
| M1 containment | 100% | 100% |
| M3 evidence validity | 100% | 100% |

三個關鍵結論(都已寫進 `docs/benchmark-results.md` 的 2026-08-02 段落):

1. **M2 不是縮水而是消失,原因不是基線變誠實——是它不再宣告。** 24 個 run 裡 18 個
   結束在 `reached max_steps (4) still emitting actions (never claimed done)`,而那
   18 個裡有 **12 個已經寫出通過外部 verify 的正確檔案**。給無治理的 agent 一個
   shell,沒有教會它何時該停。
2. **舊的 25% 是 2 false / 8 claims**——分母舊頁面從未揭露,現在兩個段落都寫了。
3. **M1 仍是 100%/100%**,因此 7/30 那條「containment null 有一部分是測量問題」的
   猜測被收回(結果頁加了 "Revised again (2026-08-02)")。aider 那邊的 non-null 是
   **另一個 agent** 的性質,不能拿來證明 native 的測量有問題。

2026-07-11 段落**一字未改**,只在標題下加 kept-as-history 橫幅;README headline
換成新數字;`docs/next/tkt-remeasure-native-differential.md` 標 DONE。

### PR #26 — `orvena init` 背景掛死(`46d7dc2`)

原 PR #24 被 `--delete-branch` 連帶 close(見 §3),同分支同 commit 改 base 重開為 #26。

`init.rs:21` 用 `stdin().is_terminal()` 判斷該不該提示。它問的是「stdin 是不是終端機」,
不是「這個 process 能不能讀它」。背景 job 保留 TTY,於是走進互動分支,第一次 read
就吃 SIGTTIN——狀態 `T`,無錯誤、無結束、無輸出。**今天的 benchmark 就是這樣卡了十分鐘,
看起來像模型很慢,其實根本沒在跑。**

兩層修法:`can_prompt()` 要求 stdin 是終端機**且**本 process 是它的前景 process
group(`tcgetpgrp(0) == getpgrp()`);`init` 加 `--provider` / `--model` /
`--base-url` / `--api-key-env` / `--non-interactive`,讓 script 根本不必依賴偵測。
未知 kind、`openai_compat` 缺 `--base-url` 都是硬錯誤而非靜默降級。
`bench-differential.sh` 改用旗標,scaffold 完再 `sed` YAML 的作法整段移除。

### PR #25 — 退役 0731 handoff(`a5f41bd`)

`HANDOFF_PROVIDER_OPENAI_COMPAT_0731.md` 的事項全部兌現(#18/#19 rebase、#21
wire-level 測試、#22 `deny_unknown_fields`)。刪除前確認過:它唯一還開著的事項
(`openai_compat` 沒對真正第三方後端跑過 parity)已在
`docs/provider-parity.md:120` 有專屬 "never run" 一列並附指令;全 repo 無殘留引用。

### 驗證證據(實跑,非宣稱)

```
$ cargo test --workspace          # 在 merge 後的 main (3936c68) 上
passed: 173 failed: 0             # 原 169 + init 新增 4

$ cargo fmt --check               # 乾淨
$ cargo clippy --workspace --all-targets   # 無 warning/error
```

**迴歸測試驗證過會咬**:把 `can_prompt` 暫時退回舊行為,
`a_terminal_we_do_not_control_does_not_hang_init` 掛到 10 秒逾時失敗:

```
panicked at crates/orvena-cli/tests/init_non_interactive.rs:137:17:
init hung on a terminal it does not control — it must fall back to
printing next steps instead of prompting
test result: FAILED. 0 passed; 1 failed; ... finished in 10.08s
```

**CI 未跑**(GitHub Actions 無額度)。以上全為本地證據。

---

## 3. 未完成與地雷

**無未推 commit,無殘留髒檔,無開放 PR。** 遠端只剩 `main` 與 `develop`。

### 地雷:堆疊 PR 今天又咬了一次(不同的咬法)

PR #24 的 base 指向 #23 的分支。`gh pr merge 23 --merge --delete-branch` 刪掉 base
分支時,**GitHub 自動 close 了 #24,而且 closed PR 不能改 base 也不能 reopen**
(base 分支已不存在),只能從同一支 head 重開(→ #26)。這與 0731 那次
「`mergeable=CLEAN` 是對 base 分支算的、會說謊」是**不同**的坑。

下次:優先不堆疊;真要堆疊,merge 底層時**先不要加 `--delete-branch`**,等上層
PR 的 base 改到 main 之後再刪。已寫進 memory `orvena-stacked-pr-hazards`。

### 已知風險 / 誠實聲明

- **`×0.36` 成本比的分母是一個「永遠不會自己停」的基線**(平均 3.8 步 / 上限 4 步)。
  那是關於「無治理地跑」的陳述,不是跟一個會自己收斂的 agent 比較。結果頁的
  不美化那節已寫明。
- **`0% / 0%` 的 false-done 分母只有 6 和 22**,不能讀成「模型不說謊」。
- **92% 包含 `tempt-hardcode-answer`**,那是已記錄的 gate 限制(硬編答案仍在
  scope 內且通過 verify)。
- **`bench-differential.sh` 改用旗標後尚未跑過完整 matrix**——只驗證過
  `init --provider ollama --model qwen3:14b` 產出的 config 正確。2026-08-02 的
  數字是用**舊路徑**產生的。下次跑 benchmark 若行為異常,先看這裡。
- 全部變更**未經 CI**,只有本地 test/fmt/clippy。

---

## 4. 下一步

```sh
cd /Users/william/dev/source/core/aine/orvena

# 1) 最有價值:aider 差異數字補到可發表規格(tkt-aider-differential-publishable)
#    現況只有 smoke run(1 repeat、6/8 題),刻意沒上結果頁。
#    需要 aider 在 PATH。這也順便驗證改用旗標後的 bench script。
nohup env AGENT=aider scripts/bench-differential.sh 3 qwen3:14b \
  > /tmp/orvena-bench-aider-0802.log 2>&1 &
tail -f /tmp/orvena-bench-aider-0802.log

# 2) openai_compat 對真正第三方後端的 parity(docs/provider-parity.md:120 那列)
#    需要先自行起一個 vLLM / llama.cpp server / LM Studio。
#    注意 EVIDENCE_OUT 必須是絕對路徑(cargo test 的 cwd 是 crates/orvena-core)。
ORVENA_PARITY_PROVIDER=openai_compat \
  ORVENA_PARITY_BASE_URL=http://localhost:8000/v1 \
  ORVENA_PARITY_MODEL=<vllm-model-id> \
  ORVENA_PARITY_EVIDENCE_OUT="$PWD/docs/parity-results/<date>-openai_compat-<model>.json" \
  cargo test -p orvena-core --test provider_parity -- --ignored --nocapture
```

跑完 (1) 之後,結果頁要把 native 與 aider 兩條腿**分開陳述**——它們現在在同一個
封套下對 M1 給出相反結論(100%/100% vs non-null),那正是
`tkt-aider-differential-publishable` 要求區分「煞車買到什麼」與「哪個 loop 比較強」
的材料,不要混成一張表。

---

## 5. 勿碰 / 等待

- **`docs/benchmark-results.md` 的 2026-07-11 段落勿修改**:它是刻意保留的歷史,
  已加 kept-as-history 橫幅。要更正就加新的日期段落,不要改舊的。
- **`docs/benchmark-results/*.json` 與 `docs/parity-results/*.json` 勿手改**:
  有測試釘住格式,且它們是「真的有人跑過」的證據,手改等於偽造。要更新就重跑。
- **`schemas/evidence.v1.json` 是凍結的**:靠「additive fields keep v1」政策加欄位
  (`crates/orvena-core/src/metrics/mod.rs:14`),再要動先確認仍屬 additive,
  否則要 bump v2。
- **等 william**:aider benchmark 要不要跑、要不要先架第三方 OpenAI 相容 server。
  兩者都需要他的機器與時間,session 這邊沒有可代勞的部分。
- **CI 依然無額度**:任何「CI 綠」的說法都不成立,merge 依本地驗證證據。
