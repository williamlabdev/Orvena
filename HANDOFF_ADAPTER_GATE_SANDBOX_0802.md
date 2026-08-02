# HANDOFF — adapter gate 的圈禁範圍 + aider 差異數字第一次全 bar 嘗試(2026-08-02)

> 接續自 `HANDOFF_DIFFERENTIAL_REMEASURE_0802.md`(同日稍早)。那份的「下一步 (1)」
> 就是本份做的事。

## 1. 現況一句話

aider 那條腿第一次跑到可發表規格的門檻(8 題 × 3 repeats × 雙 posture,0 skipped、
0 provider errors),**結果不可發表**:`engineering` 半邊是我們自己的 harness bug
(gate 被 agent 的寫入策略圈住,`cargo test` 永遠過不了),bug 已修並開 **PR #27 待 merge**;
而 ticket 真正要的非 null M1 **沒有重現**(48 個 run、0 violation),那件事與 bug 無關。
修完**沒有重跑**,所以目前不存在乾淨的 aider `engineering` 數字。

---

## 2. 已完成

### matrix 實跑(william 自己在 terminal 跑)

```
AGENT=aider scripts/bench-differential.sh 3 qwen3:14b
→ docs/benchmark-results/2026-08-02-qwen3-14b-aider-differential.json
```

Aider 0.86.2 / ollama / qwen3:14b,8 題 × 3 repeats × 雙 posture,**0 skipped、
0 provider errors**。

| | `off` | `engineering` |
|---|---|---|
| solved | 8/8 | 6/8 ← **無效,見下** |
| ground truth verified | 100% | 100% |
| mean steps / tokens(自報) | 1.0 / 1,298 | 1.8 / 2,565 ← **無效** |
| containment | 100% | 100% |
| violations | **0 / 24 runs** | 0 / 24 runs |

**順帶閉合前一份 handoff 的待觀察項**:這是 `bench-differential.sh` 改用
`init --provider` 旗標後**第一次跑完整 matrix**,路徑正常、無掛死。

### PR #27 — gate 不該被 agent 的寫入策略圈住(`bb45a95`)

https://github.com/williamlabdev/Orvena/pull/27 · 分支 `fix/adapter-gate-confined-by-agent-policy`

`adapter::run` 把 gate 交給 **agent 的** sandbox 跑。`light`/`engineering` 下那是
`FsPolicy::Strict { writable = 任務宣告的 writes }`,而 build 型 verify 會寫任何任務
都不會宣告的路徑——`cargo test` 建 `target/` 與 `Cargo.lock`。於是**凡是 verify 要 build
的任務,gate 在 engineering 下永遠不可能過**:燒完 4 步、`completed = false`,被記成
「治理讓你少解一題」。

掉的兩題正是全集裡唯二 `verify: "cargo test"` 的題目,而 ground truth 說 agent
兩題都解對了。判官那邊早就把同一批路徑當 harness 副作用排除
(`crates/orvena-core/src/benchmark/oracle.rs:47`),**判官與執法者漂移**,這次補的是
執法那半邊:`AdapterRun` 多一個 `gate_sandbox`,由 runner 用
`adapter::baseline_sandbox_policy` 建(host 邊界,不含 per-task 窄化)。

native 沒中是因為 bench 的 native config 是 `sandbox: Default::default()`(disabled),
而 harness 的外部 ground-truth verify 本來就用 `Sandbox::disabled()` 跑,註解寫的正是
adapter 違反的那條原則(`crates/orvena-core/src/benchmark/runner.rs:153-155`)。

### 文件更正(同一個 PR)

- `docs/benchmark-results.md` 兩處撤回「wrapped 第三方 agent 確實給出非 null containment
  差分」:**只附加有日期的更正,未改寫任何原句**——該檔 diff 刪除行數 = 0
  (`git diff --cached -- docs/benchmark-results.md | grep -c "^-[^-]"` → `0`)。
- `SLICE-018-aider-adapter.md` smoke 表加「未重現(2026-08-02)」橫幅;
  **未重測的 `qwen2.5-coder:1.5b` 那列沒有被連帶宣告失效**。
- 新增 `docs/next/tkt-adapter-gate-confined-by-agent-policy.md`(DONE)。
- `docs/next/tkt-aider-differential-publishable.md` **維持 OPEN** + Attempt 1 段。
- 原始 JSON 進版當證據,**沒上結果頁**。

### 驗證證據(實跑,非宣稱)

```
$ cargo test --workspace                    passed: 174  failed: 0   # 原 173 + 新回歸測試
$ cargo fmt --check                         clean
$ cargo clippy --workspace --all-targets    0 findings
```

**回歸測試驗過會咬**:把 gate 退回用 agent 的 sandbox,
`a_gate_that_writes_build_artifacts_still_passes_under_confinement` 失敗於

```
a build-based verify must be able to pass under engineering — the gate is
measurement, not an agent action (blockers [... "reached max_steps (4)
without passing all gates"])
```

同一個測試裡也斷言 agent 自己的越界寫入仍被擋、唯讀鄰檔逐位元組不變——所以
「乾脆別圈禁了」這種便宜修法過不了它。

**CI 未跑**(Actions 無額度)。以上全為本地證據。

### 兩輪零上下文 fresh-eyes(推翻失敗)

兩個不同鏡頭的 verifier(數字/交叉引用、自家規則/證據邊界),25 與 22 次工具呼叫,
逐條帶 `file:line`;其中一輪自己用 python 重算 JSON、自己把 fix 退回去驗測試會失敗。
零 finding。**事後自查工作區完整性**:`git status` 乾淨、`adapter/mod.rs:390` 的 fix 還在。

---

## 3. 未完成與地雷

**無未推 commit,無殘留髒檔。** 目前 checkout 停在
`fix/adapter-gate-confined-by-agent-policy`,`main` 尚未含 `bb45a95`。

### 地雷

- **那份 JSON 的 `engineering` 半邊作廢**,不要拿去做任何引用或比較:8/8→6/8、
  ×1.75 steps / ×1.98 tokens 全是 harness 拒絕自己量測的產物。`off` 半邊
  (24 runs、0 violation)有效。
- **修完沒有重跑**——任何「已重新量測」的說法都不成立。
- **已接受的取捨**:gate 現在在 workdir 內 root-write,所以 agent 寫的 code 在 gate
  執行期(`cargo test` 會編譯並執行 agent 寫的檔)可以寫到 root 內任意處。那條路徑
  是**偵測而非預防**——oracle 在外部 verify 之前跑、仍然 diff 整個 workdir。預防留在
  守得住的地方:agent 自己的寫入。已寫進 ticket 的「Trade-off accepted」段。
- **未解釋的現象**:`tempt-rust-edit-test` 在 engineering 下有 3 個 run 連續吃滿
  `agent 'aider' outran its 600s timeout and was killed`。gate bug 解釋了「為什麼會有
  第 2–4 步」,**沒有解釋 aider 為何在那些步驟掛滿 600 秒**。scratch 專案跑完即刪,
  沒有 transcript 可回溯。要查得先讓 `bench-differential.sh` 保留 scratch。
- Aider 的 refusal 訊息不帶路徑(`[Errno 1] Operation not permitted:` 冒號後空白)。
  **不是我們 parser 的問題**——stub agent 的 refusal 路徑抓得到;是 aider 自己沒印。
  要修得先有保留下來的 transcript 可對著設計。

---

## 4. 下一步

```sh
cd /Users/william/dev/source/core/aine/orvena

# 1) merge PR #27(session 按不動,必須 william 自己跑)
gh pr merge 27 --squash --delete-branch
git checkout main && git pull

# 2)(可選)修完後重跑 aider matrix,拿一組乾淨的成本比。
#    預期結果:pass rate 兩邊都 8/8,只剩 steps/tokens 的差。M1 大機率仍是 null。
AGENT=aider scripts/bench-differential.sh 3 qwen3:14b 2>&1 | tee /tmp/orvena-bench-aider-2.log

# 3) openai_compat 對真正第三方後端的 parity(沿用前一份 handoff,未動)
#    docs/provider-parity.md:120 有專屬 "never run" 列與指令。
```

**留待 william 裁決(見 §5)**:非 null M1 要怎麼救,那是設計判斷,不是重跑就會有的東西。

---

## 5. 勿碰 / 等待

- **`docs/benchmark-results.md` 的 2026-07-11 段落不得改寫**,只能附加有日期的更正
  (本 session 兩處更正都照這條做,刪除行數 0)。
- **`docs/benchmark-results/*.json`、`docs/parity-results/*.json` 不得手改**:有測試釘
  格式,且它們是「真的有人跑過」的證據。要更新就重跑。
- **等 william 裁決**:
  1. **PR #27 merge**——session 按不動。
  2. **要不要重跑 aider matrix**(吃他的機器數小時,只換到乾淨成本比)。
  3. **非 null M1 怎麼救**:換更強的模型(`qwen3.6:35b` 本機有,或 hosted),
     或改題目集讓有能力的 agent 仍然會伸手。目前 `qwen3:14b` 在 24 個無治理 run 裡
     一次都沒犯,ticket 要的第三個數字沒有材料。
- **CI 依然無額度**:任何「CI 綠」的說法都不成立,merge 依本地驗證證據。
