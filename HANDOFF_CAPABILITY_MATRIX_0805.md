# Handoff — capability 量尺首跑到 2×2 harness matrix(0804 深夜–0805)

## 1. 現況一句話

聰明線的尺開張並跑出第一條階梯:native 0.2.0(grounded loop)以 88% shipped、
一刀被尺否決撤掉、2×2 harness matrix 顯示 native 與 aider 互有勝負且
「模型邊界」裁決被自己人否證——全部已 push、CI 親眼驗綠,下一刀候選
slice-025(file inventory)等 william 裁。

## 2. 已完成(全部已 push,CI 驗綠)

- **capability set 首跑(post-021 基準)**:qwen3:14b × native 0.1.0 = 75%,
  完全雙峰(6 題 3/3、2 題全耗盡)。首跑第一次啟動全滅(24/24 404):
  handoff 裸指令缺 model 來源——bench 無 `--model` 參數,repo config 是
  gemini,**跑 ollama 必須 scratch `orvena init --provider ollama --model <m>`**。
- **slice-023 grounded loop,shipped(`3f813fd`)**:驗屍(bundle 最終檔 diff)
  發現兩敗題共同死因是「猜值不讀檔」;兩條 grounding 規則進 system prompt
  (兩態 parity 測試釘死),版本 bump 0.2.0。量測 **75% → 88%**,更便宜
  (3.8→3.0 步),`cap-audit-services` 0/3→3/3。
- **slice-024 search-to-locate,被尺否決並 revert**:0.3.0 量測 83%,靶題
  broken-ref 0/3 不動(零 SEARCH,rep1 還在檔內捏造內容騙 check),
  converge-deploy 回歸、tokens +24%。零收益有代價,commit 前撤。
  否證紀錄 `SLICE-024-search-to-locate.md`(`ae2b9e5`)。
- **跨模型腿**:qwen3.6:35b × 0.2.0 = **24/24 (100%)**,2.3 步,broken-ref
  3/3 正解。當時裁「14B 模型邊界」——數小時後被下一腿否證(見下)。
- **harness matrix(`7c3a797`)**:aider 0.86.2(wrapped)× {14b, 35b} =
  {96%, 96%}。aider×14b 在 broken-ref 2/3(一次 1 步解)→「模型邊界」錯,
  真相是 **harness×model**:native 從不給模型檔案清單,aider repo map 有。
  兩 harness 互有勝負:aider 贏 14b cell(96 vs 88),native 贏 35b cell
  (**100** vs 96)且是唯一全破 cell。方法論教訓已記錄:**裁決不能超出實驗
  移動過的軸**。
- **文件**:`docs/benchmark-results.md` 新增 third-number headline、首跑節、
  階梯節、cross-model 節(含否證註記,原句保留)、harness matrix 節
  (步數/token 刻意不進表——aider 是 `agent_reported` 且步的定義不同)。
  六份 raw report 進 `bench-runs/`。
- **驗證**:三個 commit(`3f813fd`/`ae2b9e5`/`7c3a797`)push 上 main,
  CI 親眼驗綠(`30931449498` 與 7c3a797 的 run,`gh run watch` 到 success)。
  cargo test 20 個 target 全綠(slice-023 前後各跑一次)。

## 3. 未完成與地雷

- **m1-depth 髒檔 2 個仍未 commit**(上上棒刻意留的):
  `bench-runs/m1-depth-20260804/` 的 json 刪除 + log 改動,等 bench-runs
  入版控裁決。本 session 沒動它們。
- **repo `.orvena/orvena.yaml` 仍 `max_steps: 3`**(舊 scaffold 殘留,
  現行 scaffold 與程式預設都是 8)——在 repo 內直接 `orvena run/bench`
  會拿到舊預算。等裁要不要對齊。
- **bench header 顯示漂移**:`--provider ollama` override 時 header 印的
  endpoint 仍是 config 裡舊 provider 的 base_url,實際請求正確——顯示層與
  builder 不一致,與 memory 裡 provider readiness 漂移同族,值得開一刀修。
- **量測空白**:35b × native 0.1.0 沒跑過(35b cell 的階梯歸因缺基線,
  但尺對 35b 已飽和,跑了也量不出後續投資,優先度低)。
- **evidence bundles 在 session scratchpad**(`capability-scratch*`,
  含 aider transcripts),session 結束會消失;raw report 已進 git,
  但要重驗屍逐步行為就得重跑。
- 舊懸案未動:深度跑擱置(97742a2 worktree 可比性約束不變)、35b temptation
  兩塊、目錄重整(root 又 +3 檔:SLICE-023/024 + 本檔)。

## 4. 下一步

```sh
# 候選 A(建議,已具機制證據):slice-025 file inventory 進 context——
# 給 native 的 prompt 加專案檔案清單(唯讀路徑也列名,不放內容),
# 直接驗證 cell:14b × native 是否 88% → ~96%。動 context.rs 的 build(),
# capability-not-obligation 紀律同 slice-023(兩態 parity)。版本 bump 0.3.0
# (0.3.0 號可重用:被否決的那個從未 commit)。量測協定同 SLICE-023 檔。

# 候選 B:35b 級的更難 set 版本(多檔跨檔、真 toolchain)——動 set = 新版本,
# 尺紀律見 benchmarks/capability.yaml 檔頭與 SLICE-022。

# 跑量測一律 scratch init(repo config 是 gemini,裸跑 404):
#   orvena init --provider ollama --model qwen3:14b   # 或 qwen3.6:35b
#   orvena bench --tasks benchmarks/capability.yaml --governance engineering \
#     --repeat 3 --out bench-runs/$(date +%Y%m%d)-capability-<model>[-<agent>].json
# aider 腿加 --agent aider。OUT 用 set -C 原子建檔搶名(腳本樣板在
# session scratchpad 會消失,照上面指令重寫即可)。
```

## 5. 勿碰 / 等待

- **勿碰**:`benchmarks/capability.yaml` 任何一題(動題=新 set 版本);
  `docs/benchmark-results.md` 既有數字區;m1-depth 那兩個髒檔;
  `bench-runs/m1-depth-20260803/native-*.json`(競寫,不可引用)。
- **等 william 裁**:slice-025 開不開(候選 A/B 擇一或都開);
  bench-runs 入版控懸案;repo config max_steps 對齊;bench header 漂移修不修;
  目錄重整時點;深度跑重啟與否。
