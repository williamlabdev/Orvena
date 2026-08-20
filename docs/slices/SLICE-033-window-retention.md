# SLICE-033 — 窗管理投資:模型自控 retention(PIN),agent 0.6.0

> 狀態:**已實測(0820,35b 格)——rung 未動,PIN 零使用;處置留裁**。
> 實作 shipped(`63a220f`,agent **0.6.0**,無 pin 時窗組裝與 0.5.0 逐
> byte 相同);35b 官方讀數 22%→22%,九 run 零 PIN 嘗試(`bench-runs/
> 20260820-capability-v3-qwen3.6-35b.json`)。預測結果:P1 落空(無綠、
> 無 PIN——不是 pin 錯,是根本沒伸手);P2 未觸發(N4 無綠);P3 成立
> (N3 2/3 無回歸);P4 依自訂判準=**rung 失敗**(35b 不動)。14b 格
> 0820 裁定不跑(rung 判定掛 35b;當日該格 ollama runner 卡死 6h14m
> 一次,重跑至 2h10m 時裁停)。**留裁:PIN 表面去留**——(i) 留着:無
> 回歸、是未來 rung(提示/範例教用法)的地基,但「有表面沒人用」進
> 可比鍵;(ii) revert 照 0.3.0 先例:一 rung 一變數,乾淨回 0.5.0。
> 讀數本身的教訓照登:缺的不是 retention 的能力面,是**規劃 retention
> 的傾向**——正是這把尺要量的軸,如今藉口已拆除。
> 依據:capability-v3 首個官方讀數(`bench-runs/20260819-capability-v3-*.json`,
> 35b 22% / 14b 11%)與其死法表;SLICE-032 的 N1/N4 設計敘事;
> SLICE-031 的證據累積機制(`driver.rs` retained_evidence)。

## 要修的不是預算,是「留不住」

slice-031 把窗從一步加深到整趟,但保留策略是**盲的**:newest-first、
整塊逐出、通知只說「steps 1–k dropped」。v3 官方讀數的死形全打在這裡:

- **N1(pin-eviction)雙格 0/3**:模型分次讀齊操作值,寫入前早讀的塊
  已被逐出;接地規則(改動依據須是本 run 看過的證據)把它逼回去重讀
  ——`dropped_reread` 每 run 1–4,全部 `budget_exhausted`。
- **N4(sentinel-span)雙格 0/3**:peak 3687–4078 貼死 4096,重讀被逐
  至 5 次;這是哨兵**設計要紅**的機制,紅得對。
- 死的不是窗空間,是**步**(A1,0818):MAX_STEPS 數的是模型呼叫,
  捆讀一步多動作免費;每一次「重讀已逐出的內容」就是純損一步。

模型現在對「什麼會被留下」**零控制權**:它讀到了關鍵值,沒有任何動作
能表達「這個要留住」。retention 目前只能靠兩條僥倖路:一步捆讀全部
(35b 探針的解法)或恰好晚讀(14b 探針 k7 的解法)。投資 = 把 retention
從僥倖變成**可選擇的動作**。

## Rung 設計:PIN(agent 0.6.0)

新動作 `PIN <step>`:把指定步的證據塊標記為不可逐出。

- **語意**:pinned 塊在 `retained_evidence` 中優先保留;逐出只發生在
  unpinned 塊上(仍 newest-first)。塊的粒度沿用現制(整步一塊)——
  span 級 pin 是更細的表面,等這一 rung 的讀數說需要再說。
- **預算誠實**:pinned 總量上限 = 預算之半(2048 tokens)。超限的 PIN
  被拒收且窗內明說(「pin refused: pinned budget full」)——沉默拒收
  會讓模型以為留住了;全窗可 pin 則等於變相加預算,rung 就不再是
  「學會取捨」而是「桌子變大」。
- **逐出順序與溢出**:逐出只挑 unpinned、最舊先;pinned 永不逐出。
  窗總量的既有 carve-out(newest 塊永遠保留,即使單塊超預算——官方
  bundle 實測存在,N1 35b rep0 peak 6854)與 pin 疊加後,最壞情況 =
  pinned 2048 + newest 塊;此時 unpinned 舊塊全逐。溢出**只能**經
  newest carve-out 發生,pin 不新增溢出通道。
- **步成本**:PIN 與其他動作同步併發(一步=一次呼叫,動作不限量,
  `driver.rs:252-254` 的 parse→for 迴圈),所以 pin 本身近乎免費——
  貴的是**判斷什麼值得 pin**,那正是這把尺(ordering / restraint)
  要量的東西。可否證前提:N1 的操作值在讀取當步可辨(complaint 指
  列)——若模型用了 PIN 但 pin 錯塊,那是能力讀數(尺在量的東西),
  不是 rung 缺陷。
- **已逐出步不可 pin**(拒收且明說)。這不是實作方便,是**軸保護**:
  log 其實留有全部塊,允許 pin 已逐出步 = 從 log 免步復原(recall),
  「留不住的東西可以隨時叫回來」會讓整條窗壓力軸蒸發——與 draft-4
  的 SEARCH 洞同型(0807 裁定否決過一次)。實作時不得順手做成 recall。
- **兩腿同表面**:動作說明進 prompt 時兩 posture 同文(量測/治理分離,
  破過三次的那條);ungoverned 腿同樣拿到 PIN。
- **儀器**:report 加 `pins`(count / pinned_steps / refused);f4 義務
  照舊——N4 若翻綠,動作日誌必須顯示綠來自 pin/retention 行為才算讀數。
- **可比鍵**:agent 版本第四元換 **0.6.0**,與 0.5.0 數字永不 pool。

## 為什麼不是別的刀(都攤開,尺會裁)

- **加 `EVIDENCE_BUDGET_TOKENS`**:買大桌子。窗夠大就沒人需要 retention,
  N4 會「綠得沒機制」——f4 動作日誌驗證正是為擋這種綠設的。留作對照
  rung 的候選,不當第一刀。
- **自動顯著性保留**(harness 猜哪塊重要):把「模型會不會取捨」換成
  「啟發式猜得準不準」,量的東西就不是模型能力了。slice-024 已否證過
  勸誘/代勞路線一次。
- **具名逐出**(通知列出丟了哪些路徑):資訊改善、無 retention 語意,
  單獨成 rung 候選(0.7.0 候補),與 PIN 疊加會變兩變數。
- **重讀去重**(同目標重讀取代舊塊):記帳修正,但 v3 死形是「重讀
  *已逐出*的內容」,取代語意打不到痛點;低槓桿,不排 rung。

## 預測(先寫,實測打臉照登)

1. **N1 35b ≥1/3 翻綠且 PIN 在解法軌跡中承重**(pin 的塊正是寫入所引
   的值來源)。「含 PIN」是承重條件不是裝飾:無 PIN 的僥倖解基率
   ≈1/9(探針合併),n=3 下零效應仍有 ~30% 機率湊出 ≥1/3 綠——單看
   通過率這條預測驗不了 PIN;綠但軌跡無 PIN(如純捆讀)不計入確認,
   照登為「PIN 對 N1 無效」。
2. **N4 翻綠若發生,f4 驗證必須過**(日誌顯示 pin 了哨兵所需 span 的
   來源步);「綠但 f4 不過」= 哨兵抓到假 retention,rung 記缺陷。
3. **N3 不退步**(35b ≥2/3、14b ≥1/3)——PIN 是加法表面,不該干擾
   既有解法。噪音警語:N3 真值 2/3 時 n=3 觀測到 ≤1/3 的機率約 26%,
   單輪退步訊號**先加一輪確認 bundle** 再裁;確認輪仍退才比照 0.3.0
   先例 revert。
4. 14b N1 仍可能 0/3(探針顯示其解法是 retention 僥倖,PIN 需要主動
   判斷,地板格未必用得起來)——14b 不動不算 rung 失敗,35b 不動才算。

## 驗收(P1,零額度)

- `retained_evidence` 帶 pin 集合的單元測試:pin 保留、超限拒收、
  拒收通知入窗、pinned 半額上限、無 pin 時行為與 0.5.0 逐 byte 相同。
- 動作解析:`PIN <step>` 進 action parser,非法步號(未存在/已逐出)
  拒收且明說;兩 posture prompt 同文。
- 全 test suite 綠;之後 v3 兩格官方 bundle(`scripts/bench-capv3.sh`)
  對照 22%/11% 基線,尺裁決 rung 去留。
