# HANDOFF — 探針找難度帶:找到的是「這個形狀不能量」(0806)

接續 `HANDOFF_RUN_PROVENANCE_0806.md`(01:17)。工作線:orvena 聰明線(capability 量尺)。

## 1. 現況一句話

slice-029 全部落地且 CI 綠;為了定 35b 的 `repeat` 而造的新探針,量出來的結論是
**這個題目形狀根本定不出 n**——成敗由「模型抽到哪種策略」決定,而在 8 步 envelope 下
不存在「必須搜尋且搜尋能成功」的難度帶。v2 造題的可行性因此要重新判。

## 2. 已完成

### (a) slice-029 收進版控並推上去,CI 綠

上一棒留的最大殘留(33 檔全綠未 commit)已由後續 session commit,本 session 接手 push:

- `04d8a47` / `b03a3f5`(上一棒的)— push 後 CI **紅**,倒在 rustfmt。
- `ae3e0ba` `style: rustfmt the slice-029 changes` — `cargo fmt --all` 修三個被 slice-029
  撐寬的 struct literal(`ollama.rs`、`provider_parity.rs`、`run_provenance.rs`)。
  CI run `31054581242` **success**(3m24s)。
- 驗證:`cargo test --workspace` **21 target / 264 tests 全綠**、`cargo clippy --workspace
  --all-targets` **零 warning**(fmt 前後各跑一次)。

### (b) B1 已落地(不是本 session 做的,列出來避免重複)

`102632d` + `0ff65a8` 是 06:52 由**另一條並行 session** commit 的,值 `0.6 / 0.95 / 20` 無 seed,
落在 `scripts/lib/calibration-sampling.sh`(六支 bench script 都 source),不在 `.orvena/orvena.yaml`
(每支 bench 都 scratch init,寫 repo 設定等於沒寫)。本 session 只是把它們一併推上去。

### (c) provenance 可重現性:四跑驗過,但通過的理由不是原本假設的

`b3b663f`。同一支探針 × `qwen3.6:35b` 跑四次(A 熱、B 熱 +50s、C **冷啟動**、D 先載 27b),
**十個檔頭欄位零漂移**。但 `context_length_effective` 之所以四跑都 32768,
**是 server 端 `OLLAMA_CONTEXT_LENGTH=32768` 釘死的,不是記憶體協商重現**——
證據:三個 declared 不同的模型(40960 / 262144 / 262144)全讀出 32768。
**所以 32768 不能當模型性質引用**,跨機器並列前要先確認對方那個 env。
D 那跑沒測到它想測的條件(ollama 卸掉 27b 才載 35b),已照實記在 slice-029。

### (d) 新探針 `benchmarks/probes/multi-hop-enumeration.yaml`(`1ae964c`)

壓 search-scale 沒碰的兩個維度(多跳 / 枚舉),承重設計是**一次 grep 必定同時多算又少算**
(active 的 `archive-cold-standby` 名字含退役的 `archive-cold`、一個註解掉的 retention、
退役 policy 不只一個)。造完先用 26 個已知答案案例驗 `check.sh` 本身,**全部行為正確**
才跑模型(腳本 `verify-check.py` 在 session scratch,未入版控)。

三份 30-run 報表已入版控:`bench-runs/20260806-probe-multihop-12-qwen3-6-35b-{1,2,3}.json`。

## 3. 未完成與地雷

- **本 session 的核心發現:這支探針量的是策略抽籤,不是能力。**
  30 run(三次獨立 n=10,provenance 逐欄相同):
  `read-heavy (>=12 reads) 14/16 = 88%` vs `search-first (<12 reads) 0/14 = 0%`。
  read 次數是雙峰的(失敗組 4–6、通過組 13–19,中間無樣本),所以閾值放哪都一樣。
  合併 14/30 = 47%,CI **[30%, 64%]**,±10pp 需要**每格約 96 run**(35b 兩三小時/題)。
  批間 30/70/40 的差異**不需要額外機制**——n=10 本身就帶 ±16pp 抽樣誤差;
  站得住的證據是 run 內的策略分離。
- **兩個失敗模式之間沒有空隙**(對 v2 可行性的直接衝擊):
  語料小到 8 步內讀得完 → 讀完必勝、搜尋必敗 → 抽籤;
  大到讀不完 → 預算先死(20 檔題六跑全部 `max_steps` 耗盡、只讀到 4–6 檔,
  **它的 0% 永遠不得當難度讀數引用**)。**在 MAX_STEPS=8 下,35b 沒有
  「必須搜尋且搜尋能成功」的區間。**
- **`MAX_STEPS` 是 `benchmark/runner.rs:36` 的寫死常數 8**,專案 `.orvena/orvena.yaml`
  的 `max_steps:` **到不了 bench runner**(實測撞到:改成 16 後 blocker 仍印 `max_steps (8)`)。
  它的 doc comment 明說改值即換 envelope、跨值不可 pool。
  → **v2 每一題都必須在 8 步內可解**,這是硬約束。
- **背景跑被中止:第三、第四次**(前兩次記在上一棒)。本 session 被砍兩趟:
  n=30(預估 15 分)零產出、batch3 第一次(**跑 7 分鐘**)零產出。
  而 batch1 跑 **23 分鐘**、batch2 跑 17 分鐘都正常完成——**所以與時長無關,原因仍不明**。
  `bench` 只在跑完才寫報表,被砍就是全部作廢;長跑建議分批,但別把分批當成已知的規避手段。
- **並行 session**:06:52 有另一條線在同一 repo commit(`102632d`/`0ff65a8`)。
  動 `crates/` 前先確認沒有第二條線在改。
- 舊懸案未動:7 份 0805 未追蹤報表入版控仍未裁;m1-depth 兩個髒檔(第六個 session 沒碰);
  repo `.orvena/orvena.yaml` 仍 `max_steps: 3`(**現已知它對 bench 無效**);
  bench header endpoint 漂移;根目錄重整(這次再 +2:探針檔不算,本檔 +1)。
- **CI**:`1ae964c` 的 run `31061770482` **success**(2m24s,實看)。
  前兩筆(`31054581242`、`31055750503`)亦 success。
  本檔自己那顆(`ac1d059`)的 CI 在寫檔當下才 queued,**未確認**——docs-only,但沒看到就是沒看到。

## 4. 下一步

```sh
# 0) 先確認最後一筆 CI
gh run list --limit 2

# 1) 裁 v2 的形狀問題(擋路的那個,見下節)。裁完才動 SLICE-026。

# 2) 若要繼續量,單趟控制在 10 run 以內、各自 OUT,跑完立刻確認報表存在:
cd <scratch> && orvena init --provider ollama --model qwen3.6:35b --non-interactive
. "$REPO/scripts/lib/calibration-sampling.sh" && apply_calibration_sampling
orvena bench --tasks <probe> --governance engineering --repeat 10 --out <OUT>

# 3) SLICE-026 尚未寫入本 session 的三個結論(策略抽籤、無空隙、8 步硬約束)——
#    等 v2 形狀裁完再一次寫進去,避免寫了又改。
```

## 5. 勿碰 / 等待

- **勿碰**:`benchmarks/capability.yaml` 任何一題;`docs/benchmark-results.md` 既有數字區;
  m1-depth 那兩個髒檔;`bench-runs/20260805-probe-search-scale-qwen3-14b.json`(否證紀錄);
  `probe-multihop-20` 的 0%(是 envelope 讀數,不是難度讀數)。
- **等 william 裁**:
  1. **v2 的形狀(最擋路)**——現有設計會讓分數含「策略抽籤」。三條路:
     (a) **改題**,讓成敗不繫於抽到哪種策略(例如唯一解法路徑,或讓搜尋也能成功);
     (b) **改判準**,不用通過率當校準值,改用死法分類(14b 那格已經這樣做了,
         SLICE-026「地板格的主判準」);
     (c) **接受 8 步 envelope 是尺的一部分**,只造「8 步內可解」的題,
         放棄「逼出搜尋」這個維度。
  2. 7 份 0805 未追蹤報表要不要入版控。
