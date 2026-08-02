# HANDOFF — gate 繼承 host TMPDIR + qwen3.6:35b 首次 smoke(2026-08-02)

> 接續自 `HANDOFF_ADAPTER_GATE_SANDBOX_0802.md`(同日稍早)。那份的「下一步 (1)
> merge PR #27」已由 william 執行;「(2) 重跑 aider matrix」改用 `qwen3.6:35b`
> 執行到 smoke 階段就停,原因見下。

---

## 1. 現況一句話

PR #27 已 merge,但**同一類 bug 還有第二個逃逸點**——gate 繼承 host 的 `TMPDIR`,
`cargo test` 的 doctest 步驟因此必敗;在 `qwen3.6:35b` 的 smoke 裡抓到、已修並開
**PR #28 待 william merge**;**全 bar 仍未跑**,因此仍不存在乾淨的 aider engineering 數字。

---

## 2. 已完成

### PR #27 merge(william 自己在 terminal 跑)

`gh pr merge 27 --squash --delete-branch` → merge commit `3da7ae7`,
2026-08-02 10:12 UTC,分支已刪。本地 `git pull` fast-forward 乾淨。

### qwen3.6:35b smoke(1 repeat × 8 題 × 兩 posture,16 runs)

```sh
KEEP_SCRATCH=1 ORVENA_AGENT_TIMEOUT_SECS=1800 \
AGENT=aider scripts/bench-differential.sh 1 qwen3.6:35b
```

| 檢查項 | 結果 |
|---|---|
| timeout | **0**(attempt 1 在 600s 下掛過 3 次) |
| skipped / provider error | 0 / 0 |
| M1(`off` 半邊 8 runs) | **仍是 null**,0 violation |
| `engineering` 半邊 | **作廢**——見下 |

**兩個對後續規劃有用的副產品(實測,非推論)**:

- **35b 比 14b 快一倍**:72.56 vs 38.05 tok/s(`ollama run --verbose`)。它是 MoE,
  所以「模型更大 = 跑更久」不成立;timeout 反而可以縮回較小的值。
- **aider 的 context 實測 ~9k**(9084 / 9090,跑的當下用 `ollama ps` 觀測),
  **不是**原本懷疑的 litellm 2048 預設。所以 M1 是 null **不能**用
  「模型看不到完整情境」解釋。這個數字只在 run 進行中存在,log 裡沒有。

### PR #28 — gate 不該繼承 host 的 TMPDIR(`78354e1`)

https://github.com/williamlabdev/Orvena/pull/28

根因:agent 的 invocation 自 slice-018 起就把 `TMPDIR`/`XDG_CACHE_HOME` 導向
`.orvena-agent/`(受限的 child 寫不到 host temp),**gate 沒有比照**,於是 gate 的
child 繼承了 operator 的 `TMPDIR`——在可寫集合之外。`cargo test` 會跑 `rustdoc`,
它在 `std::env::temp_dir()` 底下建 doctest 目錄,因此必敗:

```
error: failed to create temporary directory: PathError {
         path: "/var/folders/…/T/rustdoctestby7Ij4",
         err: Os { code: 1, kind: PermissionDenied, message: "Operation not permitted" } }
```

改動:`GateRunner::run_with_env` 疊加環境到 verify 指令;`GateRunner::run` 是它帶空
env 的呼叫,所以 **native loop 與 oracle 完全沒動**。`adapter::run` 給 gate **自己的**
`gate-tmp`/`gate-cache`,不共用 agent 的——量測不該從受測對象寫得到的目錄讀東西。
兩者都在 violation oracle 已排除的 scratch 底下。

**刻意沒做:放寬 system temp 為可寫。** benchmark 的 workdir 本來就跑在 `mktemp -d`
裡,那個授權會涵蓋 workdir、其唯讀檔與所有 escape probe——containment 變成空殼卻仍
回報 `enforced`,那比「沒有數字」更糟,因為它跟真數字無法區分。
`benchmark::runner::temp_extra_writable` 已為此丟掉該授權,本 PR 修的是另一端。

### 驗證證據(實跑,非宣稱)

- `cargo test --release` 全通過(129 + 11 + 4 + … ,0 failure)
- `cargo fmt --check` 乾淨;`cargo clippy --all-targets` 無警告
- 新回歸測試 `a_gate_that_needs_temp_still_passes_under_confinement`
  **實測過「沒有修正時會失敗」**:暫時把呼叫改回 `GateRunner::run` 重跑,失敗形狀
  與實跑一致(4 步、`reached max_steps`);還原後通過。它同時斷言 containment,
  所以「乾脆不圈禁」這種偷懶修法會被擋下。
- `KEEP_SCRATCH` 四種組合實跑驗證(成功/失敗 × 開/關),含「失敗時仍保留 scratch
  且 exit code 1 沒被 trap 吃掉」

**CI 未跑**(Actions 無額度)。以上全為本地證據。

### 文件

- 新增 `docs/next/tkt-gate-inherits-host-tmpdir.md`(DONE)
- `docs/next/tkt-aider-differential-publishable.md` **維持 OPEN** + Attempt 2 段
- CHANGELOG 新增一條 Fixed

---

## 3. 未完成與地雷

**無未推 commit。** 目前 checkout 停在 `fix/gate-inherits-host-tmpdir`,
`main` 尚未含 `78354e1`。

### 地雷

- **全 bar 沒跑。** 任何「已重新量測 aider」的說法都不成立。attempt 1(14b)和
  attempt 2(35b)的 `engineering` 半邊**都**是量測損害,兩份都不可引用——
  attempt 2 的 ×1.75 steps / ×3.28 tokens 同樣作廢。
- **smoke 的報告 JSON 已刻意刪除**,沒有留在 `docs/benchmark-results/`。那個目錄的
  語意是「有人跑過全 bar」,一份治理半邊作廢的 1-repeat smoke 放在那裡會被讀成結果。
- **`KEEP_SCRATCH` 的代價:scratch 現在要自己刪。** 本次 smoke 的 scratch 留在
  `/var/folders/ck/3p8_2q4s25x5wsfvjw7_k9y40000gn/T/tmp.aAFTbLuORI`(16MB),
  裡面是這次能查到根因的 transcript。不需要了就 `rm -rf`;macOS 也會自行清 temp。
- **PR #27 的取捨仍然成立**:gate 在 workdir 內 root-write,agent 寫的 code 在 gate
  執行期可以寫到 root 內任意處。那條路徑是偵測而非預防(oracle 在外部 verify 前跑、
  仍 diff 整個 workdir)。
- **仍未解釋**:attempt 1 那三個 600s 掛死。attempt 2 用 35b + 1800s 是 0 timeout,
  但那是換了模型又放寬 timeout,**沒有回答 14b 當時為何掛滿**。要查得用 14b 重跑
  並帶 `KEEP_SCRATCH=1`。
- **aider 的 refusal 訊息不帶路徑**(`[Errno 1] Operation not permitted:` 冒號後空白)。
  不是我們 parser 的問題;是 aider 自己沒印。現在有 `KEEP_SCRATCH` 可留 transcript
  對著設計了。
- **未動**:`openai_compat` 對真正第三方後端(vLLM / llama.cpp / LM Studio)的 parity,
  `docs/provider-parity.md:120` 有專屬 "never run" 列與指令。

---

## 4. 下一步

```sh
cd /Users/william/dev/source/core/aine/orvena

# 1) merge PR #28(session 按不動,必須 william 自己跑)
gh pr merge 28 --squash --delete-branch
git checkout main && git pull

# 2) 全 bar。timeout 可以縮——35b 實測 72 tok/s,smoke 在 1800s 下 0 timeout。
#    KEEP_SCRATCH 留著:出事才有 transcript 可查。
caffeinate -is env KEEP_SCRATCH=1 ORVENA_AGENT_TIMEOUT_SECS=900 AGENT=aider \
  scripts/bench-differential.sh 3 qwen3.6:35b 2>&1 \
  | tee /tmp/orvena-bench-aider-35b-full.log

# 3) 驗收:兩個 mode 都要 ran=8 / skipped=0 / provider_errors=0 / repeat=3,
#    否則這組數字只是證據不是結果
python3 -c "
import json,sys
d=json.load(open(sys.argv[1]))
for m in d['modes']:
    print(m['governance'],'ran=',m['ran'],'skipped=',m['skipped'],
          'provider_errors=',m['provider_errors'],'repeat=',m['repeat'],
          'containment=',m['containment_rate'],'mean_steps=',m['mean_steps'])
" docs/benchmark-results/<date>-qwen3.6-35b-aider-differential.json

# 4) 跑完刪 scratch(路徑在 log 結尾印出)
```

**發表前提**(照 ticket §Why it is not just "run it again"):只含 filesystem
containment、token 為 agent 自報、釘 aider 0.86.2 三條 caveat 要寫在頁面上;
`docs/benchmark-results.md` 的 2026-07-11 段不得改寫,只能附加有日期的更正。
真要發表先過 `/ctr`。

---

## 5. 勿碰 / 等待

- **`docs/benchmark-results.md` 的 2026-07-11 段落不得改寫**,只能附加有日期的更正。
- **`docs/benchmark-results/*.json`、`docs/parity-results/*.json` 不得手改**:有測試
  釘格式,且它們是「真的有人跑過」的證據。要更新就重跑。
- **等 william 裁決**:
  1. **PR #28 merge**——session 按不動。
  2. **非 null M1 怎麼救**(仍未閉合,attempt 2 沒有推進這題)。現在已知:
     不是 context 太小造成的(實測 ~9k),也不是模型太慢(35b 比 14b 快)。
     剩下的路是**改題目集**讓有能力的 agent 仍然會伸手,或**再換模型**
     (本機還有 `gemma4:26b`、`qwen3.6:27b`,或走 hosted)。那是設計判斷。
  3. **要不要花時間查 attempt 1 的 600s 掛死**——需要用 14b 專門重跑一次。
