# Slice: search-to-locate —— 不知道在哪,就用搜的(REJECTED by the ruler)

> 聰明線第五刀候選(0804 實作、0805 量測、**同日被量尺否決並 revert**)。
> 本檔保留為否證紀錄:什麼被試了、數字說了什麼、為何撤刀。
> 「一刀一變因 + 量了才算」的紀律第一次砍向自己人——這正是 slice-022 造尺的
> 目的:壞投資死在 commit 之前,不是 merge 之後。

## 假說(當時的)

native 0.2.0 量尺 88%,`cap-locate-broken-ref` 仍 0/3:正解 `docs/install.md`
只有 SEARCH 找得到(不是 writable,內容不進 prompt;`docs/index.md` 目錄也
沒列它),而 qwen3:14b 有 SEARCH 不用。假說:補一條通用 localization 策略
(「需求描述內容不描述位置 → SEARCH 鑑別文字;prompt 已示之外可能還有檔案;
沒確認過的不准指」)可讓模型改用搜的。

## 量測結果(否證)

`bench-runs/20260804-capability-qwen3-14b-native030.json`(native 0.3.0
= 0.2.0 + 該規則,其餘不變;同 set、同 max_steps 8、同模型):

| | native 0.2.0 | native 0.3.0 |
|---|---|---|
| verified_rate | **88%** (21/24) | 83% (20/24) |
| mean_steps / tokens | 3.0 / 5,728 | 3.4 / 7,079 (+24%) |
| budget 耗盡率 | 12.5% | 16.7% |
| cap-locate-broken-ref(靶) | 0/3 | **0/3(死法不變)** |
| cap-converge-deploy | 3/3 | **2/3(回歸)** |

- **靶沒動**:三個 rep 依然一次 SEARCH 都沒發。rep0/rep2 繼續猜
  (`docs/index.md`);rep1 更糟——在 `guide.md` 裡捏造整段
  「## Setup Notes / Installation steps …」讓檔案自己滿足條件。
- **副作用實錘**:converge-deploy rep2 出現退化行為(刪 `CDN_PREFIX` 並附註
  「avoid change-related validation errors」、重複行)。規則清單變長稀釋了
  14B 對其餘規則的服從度——tokens +24%、步數 +13%、耗盡率回升。

## 裁決:revert,零收益 + 有代價

n=3 下 88% vs 83% 的差可能含噪音(converge-deploy 2/3 單 rep),但**靶題 0/3
不變是乾淨訊號**:規則沒有買到任何東西,卻確定付出了 token 與服從度稀釋的
代價。撤:prompt 規則、parity assert、版本回 0.2.0。shipped 狀態 = slice-023。

## 教訓(比刀本身值錢)

1. **prompt 這根槓桿對「誘發未用過的工具」已到頭**(對 qwen3:14b)。
   grounding 兩條(slice-023)能管住「已知檔案不亂猜值」,因為 READ 的對象
   是 evidence 點名的;但「主動想到用 SEARCH」需要模型自己產生一個 prompt
   沒點名的行動——14B 不從指令習得這件事。
2. **下一根槓桿在 driver,不在 prompt**:機械式 nudge(slice-023 遺留候選)
   對這題有具體形狀——gate evidence 出現「must point at / contains X」而本
   run 無任何 SEARCH 呼叫時,回饋一行「you have not searched for it」。
   或者更誠實地承認:這是**模型能力邊界**,換 35b 量一次就知道
   (若 35b 首跑就會用 SEARCH,則 14B 上的 driver 補丁是在遷就弱模型,
   投資價值要重估)。
3. **規則數量本身是成本**。每加一條 prompt 規則都要付「其餘規則被稀釋」的
   稅——0.3.0 的 converge-deploy 回歸就是稅單。未來 prompt 投資的門檻
   應更高:能用 driver 回饋機制表達的,不進 system prompt。

## 後續(0805 已裁一項)

- ~~**35b × capability set 先跑**~~ **已跑**:`qwen3.6:35b` × native 0.2.0 =
  **24/24 (100%)**,broken-ref 3/3。當時裁「14B 模型邊界」——**數小時後被
  harness matrix 否證**:aider × 同顆 14B 在同題 2/3(一次 1 步解)。正確
  結論是 **harness×model 邊界**:native 從不讓模型看到專案有哪些檔案,
  aider 的 repo map 有。單移 model 軸的實驗分不開這兩者——這是本檔第二個
  教訓:**裁決不能超出實驗移動過的軸**。driver search-nudge 仍然不做,
  但理由更新:正確的下一刀是 **file inventory 進 context**(slice-025 候選,
  可直接在 14b cell 驗證 88% 是否靠近 96%)。尺對 35b 級飽和的結論不變。
  見 docs/benchmark-results.md「Cross-model check」+「The harness matrix」兩節。
- bundle 分動作類型計數(READ/SEARCH/EDIT/WRITE/RUN)——本次驗屍靠最終檔
  diff 反推「零 SEARCH」,量測面應該直接記錄。仍值得做。
