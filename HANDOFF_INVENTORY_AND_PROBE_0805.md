# Handoff — file inventory 收滿量尺,探針否證自己(0805)

## 1. 現況一句話

slice-025(file inventory)讓 native × 14b 從 88% → **100%**,capability 量尺
**兩個模型都飽和**,下一把尺(v2)的草案已寫但等裁;順手做的分動作計數已上線,
而為 v2 準備的 SEARCH 規模探針**第一跑就否證了探針自己**(不是模型)。

## 2. 已完成(全部已 push,CI 綠)

- **slice-025 file inventory** — `45a52c6`。`crates/orvena-core/src/agent/context.rs`
  新增 `file_inventory()`:`PROJECT FILES` 區塊只印檔名、可見範圍與 `grep.rs`
  逐字一致(排 `.git`/`target`、留 dotfile、尊重 gitignore)、200 筆上限且截斷明說。
  預算順序:**清單印在前、計價在後**,只花可寫檔內容剩下的額度(測試釘死)。
  兩態 parity 釘死(capability 非 obligation)。版本 **0.2.0 → 0.4.0**。
  6 個新測試;`cargo test --workspace` 20 個 target 全綠、fmt/clippy 乾淨。
- **量測** — `aa8d29d` + `bench-runs/20260805-capability-qwen3-14b-native040.json`:
  verified **88% → 100% (24/24)**、mean 3.0 → 2.2 步、耗盡率 12.5% → **0%**;
  靶題 `cap-locate-broken-ref` 0/3(8 步耗盡 25,241 tok)→ **3/3(1 步 1,750 tok)**。
  逐題稅單同記:**八題有六題變貴 3–31%**,總分變便宜完全來自砍掉那一次 8 步空轉。
- **slice-026 分動作計數** — `9da5d66`。`ActionCounts`(write/edit/read/search/run)
  + `RunReport.action_counts: Option<_>`;**`None` = 無法歸因,不是全零**
  (wrapped agent 記的是 invocation;舊 bundle 沒記過)。bench 新增
  `search_use_rate`,**分母只算可歸因的 run**;CLI 只在可歸因時才印。
  **刻意不 bump 版本**:純儀器改動,迴圈行為沒動,bump 會在可比鍵上假造 envelope。
- **CI**:run `30984431852` success(3m27s)。`9da5d66` 的 run 未確認(推完即收尾)。

## 3. 未完成與地雷

- **`9da5d66` 的 CI 未驗**(推上去就收尾了)。本地 20 個 target 全綠、clippy 乾淨。
- **探針 `benchmarks/probes/search-scale.yaml` 目前無效**,檔頭已用
  ⚠️ 標記且說明修法。首跑 14b 兩個規模都 3/3、一步、**零 SEARCH 零 READ**——
  死因是 **writable 檔的內容會整份進 prompt**,40 個檔全設 writable 等於把針送出去。
  `bench-runs/20260805-probe-search-scale-qwen3-14b.json` 保留為否證紀錄,
  **數字不得引用**。35b 那腿**沒跑**(探針無效,跑了沒意義)。
- **由此得到的儀器級發現(會改寫 v2 設計)**:「在 writable 檔之間定位」
  不是可量的維度;語料必須唯讀 + 另設小的可寫目標。連帶:
  **v1 的 `cap-locate-retries`(三個 writable conf)一直是送分題**,
  它的 3/3 不能當定位能力的證據。
- **量尺兩格皆飽和**(14b、35b 都 100%,0% 耗盡):後續 loop 投資在 v1 上量不出東西。
- **m1-depth 兩個髒檔仍未 commit**(第三個 session 沒動它們),等 bench-runs 入版控裁決。
- 舊懸案未動:repo `.orvena/orvena.yaml` 仍 `max_steps: 3`;bench header endpoint
  顯示漂移;目錄重整(root 這次再 +2:SLICE-025、SLICE-026 與本檔共 +3);深度跑擱置。
- **evidence bundles 在 session scratchpad**(`cap025/`),session 結束會消失。

## 4. 下一步

```sh
# 1) 先確認 9da5d66 的 CI
gh run list --limit 3

# 2) 修探針(seeds 可原樣沿用,只改形狀):svc/ 改唯讀,另設一個可寫目標
#    生成器在 session scratchpad 會消失,重寫約 20 行;或直接手改 YAML 的
#    writes: 欄位 + 加一個 writable 目標檔與對應 check。
#    改完跑兩個規模 × 兩模型:
orvena init --provider ollama --model qwen3:14b      # scratch,repo config 是 gemini,裸跑 404
orvena bench --tasks benchmarks/probes/search-scale.yaml --governance engineering --repeat 3 \
  --out bench-runs/$(date +%Y%m%d)-probe-search-scale-<model>-v2.json
# 判讀看 action_counts / search_use_rate,不看通過率

# 3) v2 set 本體:等 william 裁完 SLICE-026 的三件事再開工
```

## 5. 勿碰 / 等待

- **勿碰**:`benchmarks/capability.yaml` 任何一題(動題=新 set 版本);
  `docs/benchmark-results.md` 既有數字區;m1-depth 那兩個髒檔;
  `bench-runs/m1-depth-20260803/native-*.json`(競寫,不可引用);
  探針那份 bundle 的數字。
- **等 william 裁**(SLICE-026 檔尾列的三件 + 舊帳):
  1. toolchain 進不進 v2(我建議否決,理由:編譯時間節流 convergence 題、
     讀數變成機器規格的函數;要做就另立 engineering realism set);
  2. v2 題數 8 還是 10–12;
  3. G(needle)的規模——探針修好後才有數據支撐;
  4. bench-runs 入版控;5. repo config `max_steps` 對齊;6. bench header 漂移;
  7. 目錄重整時點;8. 深度跑重啟與否。
