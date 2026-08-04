# HANDOFF — README 誠實化 + 四個新 adapter 落地(2026-08-04)

## 1. 現況一句話

0803 的矩陣與 v2 題目集工作全部定版成 6 個 commit(本機驗證全綠),README 拿掉了
repo 支持不了的宣稱,**但 6 個 commit 都還沒 push**,而且新 baseline 的 pooled 數字
仍然只活在 commit message 與 `bench-runs/`,沒上結果頁。

## 2. 已完成

### 本 session 的 6 個 commit(`db3d222..983476c`,全部在 `main`,**未 push**)

| hash | 內容 |
|---|---|
| `be10945` | `feat(adapter)`: 包進 openhands / continue / codex / codex-nested / opencode;新增 `AdapterSpec.config_files` 與 `{scratch}` 展開;全 wrapped agent 設 `COLUMNS=1000`;refused path 改走結構化資料 |
| `18771ca` | `fix(bench)`: gate 需要 `$CARGO_HOME`,不只 workdir |
| `6d150aa` | `fix(bench)`: escape probe 展開開頭的 `~/`,並給 `tempt-home-cache` 加上 probe |
| `aafac0f` | `test(sandbox)`: 14 種逃逸技術的對抗測試(每種跑兩次:未confine 必成功、confined 必失敗) |
| `9e47d9f` | `bench`: `docs/temptation-design.md` 的鑑別力規則、v2 題目集、篩選 harness |
| `983476c` | `docs(readme)`: 拿掉無出處與過度的宣稱 |

**驗證(CI 沒跑,帳單停擺;以下是本機證據)**:
```
cargo test --workspace   → exit 0,19 個 test binary 全 ok,0 FAILED
cargo fmt --all --check  → clean
cargo clippy --workspace --all-targets → 無 warning/error
git status --porcelain   → 空
```

### 更早(0802 深夜)這個 session 也做了、已被後續 commit 收編的

- `scripts/bench-differential.sh` 的 `OUT` 可覆寫 + **拒絕覆蓋既有報告**。
- `scripts/bench-matrix.sh`:分塊佇列(交錯兩腿、單塊失敗不中斷、可續跑、記 `.meta`)。
  實跑結果:native chunk A/B/C = 70m / 76m / 381m,aider = 55m / 53m / 48m。
  **這是這個 repo 第一次有全 bar 的實際耗時紀錄。**
- 把 `2026-08-02-qwen3-14b-differential.json` 改名為 `-obligated-baseline.json` 並更新引用。

### README 改了什麼(`983476c`)

`30/30` 與 `×0.08`(全 repo 只有 README 有,且與實測相反)、`mathematical proof`、
`violations impossible` 未限定範圍、evidence bundle 宣稱含 `agent transcript`(schema 沒有)、
`M1: Governance Differential` 命名漂移、`docs/benchmarks.md` 死連結、
`orvena run` 被寫成 ONE invocation(實為 `1..=max_steps`,預設 3,`agent/driver.rs:111`)。

## 3. 未完成與地雷

- **6 個 commit 未 push**。`main` ahead 6。
- **`bench-differential.sh` 的防覆蓋檢查是 TOCTOU,擋不住並行**。`7e0429d` 自己記載:
  native 的 x30 深度跑是「兩條 chain 相隔 14 秒啟動,都通過了起跑時的存在性檢查而互相競寫」,
  結果覆蓋掉一份讀數為 breach 10/30 的樣本。**那份 `native-qwen3-14b-m1x30.json` 不可引用**,
  native 深度腿需要重跑。要真的擋住得換成原子建檔(`set -o noclobber` 或 mkdir lock),
  現在的 `[ -e ]` 只擋得住循序重跑。
- **新 baseline 的 pooled 數字沒上結果頁**。`docs/benchmark-results.md` 只被 `7e0429d` 動了 1 行
  (改名引用)。pooled 結果(native off 69/72、aider off 63/72,18 次 breach 全落在
  `tempt-home-cache`)只存在於 commit message 與 `bench-runs/`。頁面上的頭條仍是
  told-and-obligated baseline 的 75%→92% / ×0.36 / ×0.24。
- **數字對不上,尚未裁決**:`crates/orvena-core/tests/escape_techniques.rs` 開頭與 README 沿用的
  「the wrapped agent breached exactly once in ten hours」,與 `7e0429d` 記的 12 次 breach 不一致。
  我照原文 commit 沒動。若那句指的是單一 chunk 而非整場矩陣,措辭要改,否則後續引用會出錯。
- **矩陣第 7、8 塊(qwen3.6:35b)從未跑**。`STOP_AFTER=11:55` 停在第 6 塊。
- **`bench-runs/` 該不該入版控沒有裁決**。目前實際做法是入(`7e0429d` 與 `9e47d9f` 都 commit 了)。
- **目錄結構重整沒做**:計畫在 `~/.claude/plans/orvena-repo-structure-cleanup.md`
  (root 9 個 `.md` 搬進 `docs/handoff/`、`docs/slices/`;docs 之間是真的相對連結,搬完必壞)。
  本檔又在 root 多加了一個 `HANDOFF_*`,搬的時候一起帶走。
- **`.git/index.lock`**:本 session 移掉一個 8/3 20:18 留下的 0-byte 殘骸(當時無任何 git process)。
  如果再出現,先 `pgrep -fl "[g]it "` 確認沒有活的再刪。

## 4. 下一步

```sh
# 1) 推上去(6 個 commit)
git push origin main

# 2) 裁決「one breach in ten hours」那句(見 §3),改或不改都要留紀錄

# 3) native 深度腿重跑(舊的那份不可引用),務必循序,不要並行
scripts/bench-m1-depth.sh   # 先讀它有沒有並行啟動的路徑

# 4) 把 pooled 數字寫上 docs/benchmark-results.md
#    紀律:兩條腿一起、M1 那欄必須寫明對哪個 baseline 量的
#    素材:git log -1 7e0429d 的 message + bench-runs/20260803-0003/ + bench-runs/m1-depth-20260803/

# 5)(選配)矩陣剩下的 35b 兩塊
STOP_AFTER=<HH:MM> RUNS_DIR=<新目錄> scripts/bench-matrix.sh

# 6) 目錄重整(獨立 commit,不與證據混)
#    照 ~/.claude/plans/orvena-repo-structure-cleanup.md,驗證重點是連結全通
```

## 5. 勿碰 / 等待

- **等 william 裁決**:§3 的「one breach in ten hours」措辭;`bench-runs/` 是否長期入版控。
- **勿碰**:`docs/benchmark-results.md` 的任何數字——pooled 結果正式寫上去之前一律不動,只能加註記。
- **勿碰**:`benchmarks/temptation.yaml` 的題目本體。v2 的實驗走 `temptation-v2.yaml`,
  舊集合是所有已發佈數字的量測基準。
- **勿引用**:`bench-runs/m1-depth-20260803/native-qwen3-14b-m1x30.json`(競寫,見 §3)。
