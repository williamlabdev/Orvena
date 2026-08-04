# Handoff: slice-021 + slice-022 —— 聰明線三刀齊了,尺已備好未量(0804 晚)

## 1. 現況一句話

深度跑照 william 裁示擱置(先讓 native 變強再測);slice-021(步數預算 8 + typed
exit)與 slice-022(capability set 量尺)當晚落地、全推上 main、CI 逐一驗綠;
下一棒是 capability set 首跑量測 session。

## 2. 已完成(全部已 push,CI 親眼驗綠)

- **深度跑重啟又停止**:接手時先按上一棒指令用 `97742a2` worktree 重啟了深度跑
  (有搶到 OUT、開始 build),william 隨即裁示「先不用跑,先讓 native 變強再測」
  ——已 TaskStop 停乾淨,EXIT trap 清掉 0-byte 佔位(第二次實戰驗證 OK),
  `/tmp/orvena-97742a2` worktree 已移除。
- **slice-021 步數預算**(`7012c7d` feat + `4f4bf73` docs;CI run 30908390874 success):
  - config `default_max_steps` 3→8、bench `MAX_STEPS` 4→8(scaffold yaml 與 README 同步)。
  - 量測同刀修復:bundle 新增 `max_steps` 與 typed `exit`(8 變體含 `unrecorded`),
    driver 10 個出口 + adapter 5 個出口全設;`RepeatedReport.budget_exhaustion_rate`、
    `Differential` 帶兩臂耗盡率;schema v1 additive,`exit` 文件化成 enum。
  - 動機釘進 CHANGELOG:只放大不修量測,×0.36 會機械變好——分母是燒滿預算的 baseline。
- **slice-022 capability set**(`1d30697` feat + `4d83e93` docs;CI run 30910851869 success):
  - `benchmarks/capability.yaml` 8 題:2 保全(高熵錨定+行數守恆)、1 錨定歧義
    (EDIT 要吃 anchor ambiguous 回饋)、2 收斂(check 一次揭露一個缺陷)、
    2 定位(只給症狀)、1 綜合(唯讀 registry 對帳)。
  - 誠實規則與 temptation 鏡像:無 escape probes、無額外 commands、無 toolchain、
    `tests/` 永不可寫——`benchmark.rs` 新增 set 不變式測試釘死。
  - **每題兩態實測過**:種子紅(訊息可行動)、預期修復綠;驗證腳本在 scratchpad
    (session 專屬,不留 repo)。
- 驗證:兩個 slice 各自 `cargo test` 20 個 binaries 全綠、fmt/clippy 乾淨。

## 3. 未完成與地雷

- **capability set 首跑完全沒跑**:尺落地了,「native 變強了沒」還是沒有數字。
  首跑協定見 SLICE-022 檔「量測協定」節。
- **深度跑(pooled 上結果頁那條線)被裁示擱置**,不是取消:舊 native 的 pooled
  素材(0803 chunks + 未跑成的 0804 深度腿)還在等。若之後要跑,仍必須用
  `97742a2` worktree(可比性),指令在 HANDOFF_CI_SLICE020_DEPTH_0804 §4。
- **髒檔 2 個未 commit**(刻意):`bench-runs/m1-depth-20260804/` 下被殺跑的
  json 刪除 + log 改動——`bench-runs/` 入不入版控懸而未決,別替 william 決定。
- **slice-021 之後任何新 bench 都是新 envelope**(max_steps 8 + native 0.1.0),
  與 0803/0804 所有舊資料不可 pool;bundle 記了 `max_steps`,可機器查。
- 舊懸案未動:35b 兩塊、目錄重整(root 又 +2 個 SLICE 檔,現在 4 個)、
  `docs/benchmark-results.md` 數字區勿碰紀律不變。

## 4. 下一步

```sh
# capability set 首跑(post-slice-021 native 的基準;需本機 ollama 起 qwen3:14b):
cargo build --release
OUT=bench-runs/$(date +%Y%m%d)-capability-qwen3-14b.json
target/release/orvena bench --tasks benchmarks/capability.yaml \
  --governance engineering --repeat 3 --provider ollama --out "$OUT"
# 跑完看三個數:verified_rate / mean_steps / budget_exhaustion_rate
# (耗盡率高=8 步還不夠或迴圈空轉;verified_rate 撞 100% = 尺要加難題=新版本)
# 結果寫 docs/benchmark-results.md 時帶可比鍵:set 版本+max_steps+model+agent 版本
```

- 首跑前先確認沒有別條 bench 在跑(memory: 證據檔地雷)。
- 若要補「前」(pre-slice-020)的對照:`git worktree add /tmp/orvena-97742a2 97742a2`
  用舊 binary 跑同一個 set——但舊 binary 沒 READ/EDIT,預期慘,那正是對照的意義。

## 5. 勿碰 / 等待

- **勿碰**:`docs/benchmark-results.md` 數字區;`benchmarks/temptation.yaml` 本體;
  `benchmarks/capability.yaml` 任何一題(動題=新 set 版本,首跑前別動);
  `bench-runs/m1-depth-20260803/native-*.json`(競寫,不可引用)。
- **等 william**:`bench-runs/` 入版控裁決;目錄重整時點;深度跑(pooled 線)
  何時重啟;bench-m1-depth/bench-differential 兩複本收斂與否(0804 提過,未裁)。
