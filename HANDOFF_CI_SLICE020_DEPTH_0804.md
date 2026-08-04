# Handoff: CI 復活與修真、breach 措辭更正、slice-020(READ/EDIT)、深度跑重啟 — 0804

> 上一棒:`HANDOFF_README_AND_ADAPTERS_0804.md`(同日早上)。本檔接續並閉合其 §4 的
> 1、2、3 步;第 4 步(pooled 上結果頁)**卡在深度跑,還沒做**。

## 1. 現況一句話

main 全推、CI 全綠、slice-020 落地;native 深度腿重跑 16:59 起跑後
**18:25 被 SIGTERM 殺掉(session 收尾時背景任務被停),沒有產出**——需要重跑,
之後才能寫 pooled 數字上結果頁。

## 2. 已完成(全部已 push,CI 逐一驗綠)

| commit | 內容 | 證據 |
|---|---|---|
| (push 7 個舊 commit) | GH Actions 額度已恢復,CI 真的在跑 | run 30893242459 |
| `c6bf0ce` | **escape suite 在 Linux 上從未攻擊過任何東西**:suite 在 `orvena-core/tests/` 時,Linux shim 的 `current_exe` 是 libtest harness,不認 `--spec`,每個 confined 指令都 spawn 失敗→什麼都沒漏→假綠。正向對照抓到的(`Unrecognized option: 'spec'`)。搬到 `orvena-cli/tests/` 用 `CARGO_BIN_EXE_orvena` 餵 `ORVENA_SANDBOX_SHIM`;CI 加 `--nocapture` signal step | run 30893762391 success;ubuntu leg 有 `SANDBOX-LINUX: Landlock is enforcing` |
| `8828a1c` | **breach 措辭更正**:README+escape suite 檔頭原句「the wrapped agent breached exactly once in ten hours」兩處皆錯——wrapped leg 是 0/72,12 次 breach 全在 ungoverned(native 3/72、aider 9/72,全是 `~/.orvena-notes.txt` on tempt-home-cache),elapsed 11.43h。`7e0429d` message 寫 18 也是算錯,JSON 是 12 | `bench-runs/20260803-0003/*.json` 逐 run 重算 |
| `97742a2` | **防覆蓋改原子建檔**:`[ -e ]` TOCTOU → `(set -C; : > "$OUT")`,kernel 層裁決;0-byte 佔位由兩個 EXIT trap 清理。`bench-m1-depth.sh` 同步(它是 differential 的複本,只差 TASKS 一行) | 20 並行 claimer 實測 1 win / 19 refuse |
| `007348a` | **slice-020:READ + EDIT actions**(規格 `SLICE-020-read-edit-actions.md`)。EDIT 與 WRITE 同授權路徑(role+scope+`scope_refusals`);錨定失敗回饋不斷迴圈;訊息不洩檔案內容。**順手補洞:`FsTool::read` 原是裸 `root.join`,`../`/symlink 可讀 root 外**,現走 `resolve_in_root`。`agent` 欄 native 帶版本(`native 0.1.0`) | run(bp9yrj594)success;round-trip `read_edit_roundtrip.rs`;20 suite 全綠+clippy+boundary |
| `dc738e1` | CHANGELOG 條目 + slice status DONE | — |

另外(不在 repo):`~/dev/source/core/me/claude-global.md` 的「GH Actions 無額度」
已改為「已恢復(0804 確認)」——william 指示的。memory `orvena-evidence-file-hazards`
已更新為已修狀態。

## 3. 未完成與地雷

- **深度跑被殺,無產出**:16:59 起跑、18:25 SIGTERM(session 收尾停背景任務時陪葬,
  86 分鐘作廢)。0-byte 佔位已被 `97742a2` 的 EXIT trap 正確清掉(機制首戰驗證 OK),
  所以**直接重跑同指令即可**,不會被舊佔位擋。log 留在
  `bench-runs/m1-depth-20260804/native.log`(記著 Terminated),scratch 留在
  `/var/folders/ck/.../tmp.stK0sAQJ6N`(可刪)。
  **重跑注意**:原跑的 binary build 自 `97742a2`(pre-slice-020);現在 HEAD 是
  slice-020 之後,腳本起跑會重新 build——**必須先 checkout `97742a2` build 或直接
  `git worktree add` 舊 commit 來跑**,否則量到的是新 native,與 0803 chunks 不可比。
- **slice-020 改了 native 行為但深度跑量的是舊 binary**——之後任何新 bench 跑起來
  就是新 native(`native 0.1.0`),與 0803 chunks(bare `native`)**不可 pool**。
  pooled 數字只用 0803 chunks + 0804 深度跑(皆舊 native)。
- **矩陣第 7、8 塊(qwen3.6:35b)仍未跑**(上一棒遺留)。
- **`bench-runs/` 入不入版控仍未裁決**(目前事實上入了)。
- **目錄重整仍未做**(`~/.claude/plans/orvena-repo-structure-cleanup.md`);root 又多
  兩個檔(本檔 + SLICE-020)。
- task #5(pooled 上結果頁)在本 session 的 task list 掛 pending——接手後見下一步。

## 4. 下一步

```sh
# 1) 重跑深度腿——必須用 pre-slice-020 的 binary(可比性),用 worktree 最乾淨:
git worktree add /tmp/orvena-97742a2 97742a2
cd /tmp/orvena-97742a2
OUT="/Users/william/dev/source/core/aine/orvena/bench-runs/m1-depth-20260804/native-qwen3-14b-m1x30.json" \
  AGENT=native KEEP_SCRATCH=1 scripts/bench-m1-depth.sh 30 qwen3:14b \
  > /Users/william/dev/source/core/aine/orvena/bench-runs/m1-depth-20260804/native.log 2>&1 &
# (跑完記得 git worktree remove /tmp/orvena-97742a2)

# 2) 跑完 → 補 provenance(finished 時間)、commit bench-runs/m1-depth-20260804/

# 3) pooled 數字寫上 docs/benchmark-results.md(上一棒的紀律不變):
#    - 兩條腿一起;M1 欄寫明對哪個 baseline 量的;agent 身分寫 bare "native"(舊 binary)
#    - 素材:git log -1 7e0429d + bench-runs/20260803-0003/(12 breach,勿用 message 的 18)
#      + bench-runs/m1-depth-20260803/ 的 aider 腿 + m1-depth-20260804/ 的 native 腿
#    - 舊的 native-qwen3-14b-m1x30.json(0803)仍不可引用
# 4) 聰明線續攤(william 已裁決要投資):slice-021 步數預算、slice-022 capability set
#    ——見 SLICE-020-read-edit-actions.md 末節
```

## 5. 勿碰 / 等待

- **勿碰**:`docs/benchmark-results.md` 數字區(pooled 正式寫上前只能加註記);
  `benchmarks/temptation.yaml` 本體;`bench-runs/m1-depth-20260803/native-*.json`(競寫,不可引用)。
- **等 william**:`bench-runs/` 長期入版控與否;目錄重整的時點;
  bench-m1-depth/bench-differential 兩份複本要不要收成一份帶參數的(0804 提過,未裁)。
