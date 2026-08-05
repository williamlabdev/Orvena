# Slice(草案,等裁): capability set v2 —— 尺飽和了,得換一把

> 狀態:**DRAFT / 等 william 裁**。本檔只是設計草案,尚未動任何 YAML。
> 起因:native 0.4.0(slice-025)之後,矩陣四格全滿——
> native {14b 100%, 35b 100%} vs aider {96%, 96%}。0% 耗盡、mean 2.2 步。
> 現行 set 已經量不出任何後續 loop 投資的差別,再做刀也只是在 100% 上原地踏步。

## 這把新尺要量什麼(不是「更難」,是「還沒飽和的維度」)

現行 set 的四個鑑別維度(preservation / anchored-edit / localization /
convergence)在 14b 上全部到頂。但 slice-025 的驗屍暴露了一件事:
**其中一個維度是被洗掉的,不是被解決的**——`cap-locate-broken-ref` 現在
一步完賽,靠的是檔名 `install.md` 自己說出了內容,模型從未查證。
所以 v2 的第一條設計條件不是加難度,是**把「查證」這個維度救回來**。

四個候選維度,依「現行 set 量不到」排序:

1. **查證(verification)**——答案不能從檔名推得,必須讀過內容才知道是哪一個。
2. **枚舉(enumeration)**——一處改動牽動多處引用,要的是「找齊」而非「找到」。
3. **多跳證據(multi-hop)**——證據 A 指向 B,B 才有值。現行 audit-services
   只有一跳,slice-023 之後就變送分題。
4. **狀態重讀(re-reading)**——修 A 會改變 B 的狀況,照計畫重播的迴圈會失敗。

## 設計條件(v1 的誠實規則全部繼承,加兩條)

繼承 `benchmarks/capability.yaml` 檔頭的四條(`tests/` 唯讀、不給額外唯讀命令、
不用 toolchain、seed 50 行內),外加:

5. **沒有一題可以只靠 inventory 解掉**。每題的答案都必須至少經過一次
   READ/SEARCH,且其目標**無法從檔名推斷**。這是 slice-025 的直接教訓:
   給了地圖之後,「檔名即答案」的題目就不再量能力,只量識字。
6. **不得混入 temptation**(v1 既有規則的重申,但 v2 更容易犯)。
   inventory 讓模型看見可寫清單以外的檔,「該不該碰鄰居」的題目現在寫起來
   很順手——那是 compliance,寫進來就會讓一個數字同時量兩件事。
   v2 的做法見下方規則 7——**唯讀語料 + 一個小的可寫目標**。
   (本條原本寫的是「把候選檔全部設為 writable」,已被 0805 的探針否證:
   writable 的內容會整份進 prompt,那等於把答案送出去。)

### 一個建議:toolchain 仍然不進 v2

SLICE-024 遺留的候選 B 寫的是「多檔跨檔、真 toolchain」。前半照收,
**後半建議否決**:編譯時間會節流 convergence 題(v1 規則 3 的原意),
而且讀數會變成機器規格的函數,跨機不可比。難度應該來自資訊結構,
不是來自等 cargo。若你要保留這條路,它該是**另一把尺**(engineering
realism set),不是 capability 的 v2——兩者的可比鍵與跑法都不同。

## 八題草案

每題標注它針對的維度,以及「為什麼不能用猜的過」。

| # | id | 維度 | 形狀 | 防猜 |
|---|---|---|---|---|
| A | `capv2-locate-by-content` | 查證 | 壞連結要指向「含安裝步驟」的文件;候選 4 份檔名全是 `notes-a/notes-b/appendix/misc`,不透露主題 | 4 選 1,盲猜期望值 25%;正解放在名字最無關的 `misc.md` |
| B | `capv2-rename-propagate` | 枚舉 | 一個設定鍵改名,3 個 writable 檔各有引用;check 要求新名全數到位**且**舊名一處不留 | 放一個誘餌近似鍵(`db_host` vs `db_hostname`)必須**不動**——一次全域取代會踩到 |
| C | `capv2-migration-order` | 狀態重讀 | 兩處改動有序:先做 B 會讓 check 冒出新錯 | check 一次只報一個問題,照計畫重播必失敗,要重讀當前狀態 |
| D | `capv2-active-config` | 查證 + 枚舉 | 三份近似 config **唯讀**,registry 指明哪一份是 active;可寫的是一份 `deploy.plan`,要照 active 那份填值 | 語料唯讀 = 不構成 temptation;填錯來源不過 |
| E | `capv2-two-hop-registry` | 多跳 | check 說「see config/index.txt」,index 再指向數份 data 檔之一,值在最後那份 | 一跳解不掉;index 的指向必須讀了才知道 |
| F | `capv2-anchor-triplet` | anchored edit | 目標行三處逐字重複,只有上下文能鑑別 | 連續兩次 ambiguous,要靠 gate 回饋加寬錨點 |
| G | `capv2-needle-in-40` | 枚舉 + SEARCH | 40 個小檔(各 <20 行)**唯讀**,只有症狀描述、沒有位置;修正寫進一份小的可寫檔 | inventory 列出 40 個名字但**一個都不決定性**,且內容不在 prompt 裡——這是 slice-024 那題在公平條件下的重跑 |
| H | `capv2-converge-quota` | convergence | ≥4 個缺陷,check 每次只揭一個,且其中一個要修完前一個才會浮現 | max_steps 8 下要 ≥5 步有效推進,運氣過不了 |

G 值得特別說:slice-024 想用 prompt 規則逼出 SEARCH,失敗了;slice-025 給了地圖,
但地圖上只有 5 個檔,掃一遍就夠。**40 個檔的地圖等於沒有地圖**——這時模型要嘛
學會 SEARCH,要嘛把 40 個檔全 READ 一遍撞死在預算上。那一格才是「定位能力」
的真讀數,而現行 set 從來沒量到過。

## 校準協定(先跑,再凍;凍了就不准調)

新尺最容易犯的錯是「照著現有 agent 調到好看的分數」。紀律:

1. 草案成形後,先跑 **qwen3.6:35b × native 0.4.0**(現行最強格)。
   目標帶 **50–75%**:>85% 表示還是飽和,加難度;<35% 表示題目在量噪音,不是能力。
2. 同時跑 **qwen3:14b × native 0.4.0** 定地板。兩個讀數都進 set 的檔頭當出廠校準。
3. **校準只能靠「選題」,不能靠「調題」**;分數一旦被引用,set 就凍結——
   之後任何一題的改動都是新版本(v1 的規則,v2 照舊)。
4. 校準跑本身**不是階梯讀數**:它量的是尺,不是 agent。寫進檔頭,不進結果頁的階梯表。

## 檔案與可比鍵

- 建議**開新檔 `benchmarks/capability-v2.yaml`,v1 原地凍結**(不是改 v1)。
  v1 仍有用:它是弱模型/回歸用的地板尺,而且 0.1.0→0.4.0 那條階梯要能重跑驗證。
- 可比鍵第一元從 `capability.yaml @ <commit>` 變成 `capability-v2.yaml @ <commit>`。
  **v1 與 v2 的數字永不 pool**,並列而已——與跨模型同紀律。

## 成本估計

8 題 × 3 rep × 2 模型 = 兩趟 bench。14b 那趟約與本次相當(~10 分鐘量級),
35b 較慢。加上題目與 check 的撰寫,這一刀的重量在 v1 造尺(slice-022)之上,
因為要寫 40 檔的 seed 與四個新型 check。

## 探針第一跑:否證的是探針,不是模型(0805)

`bench-runs/20260805-probe-search-scale-qwen3-14b.json`——qwen3:14b 在 12 檔與
40 檔**兩個規模都 3/3、一步完賽、零 SEARCH 零 READ**(單一 EDIT)。

死因是我的設計自撞:為了守規則 6(不混入 temptation)把 40 個檔全設 writable,
**而 writable 檔的內容會整份印進 prompt**(`Current files in scope:`)。
針從頭到尾躺在 context 裡,沒有任何東西需要定位。

這反而是關於**儀器**的一級發現,直接改寫 v2 的設計條件:

7. **「在 writable 檔之間定位」不是一個可量的維度**。要被搜的語料必須是
   **read-only**,可寫目標另設一個小檔——v1 的 `cap-audit-services`
   (registry 唯讀、`services.list` 可寫)才是誠實的形狀。
   回頭看,v1 的 `cap-locate-retries`(三個 writable conf)一直是送分題,
   原因同此;它的 3/3 不能當作「定位能力」的證據。
8. 規則 6 與規則 7 的張力要靠**形狀**解,不是靠把檔設成 writable:
   唯讀語料不構成 temptation(v1 的 `tests/` 一直是唯讀),
   只有「把唯讀檔當成捷徑去改」才是,而那由題目的 check 決定,不由可寫性決定。

**下一步**:svc/ 改唯讀、另設一個可寫目標(例如一份要被修正的 `rollup.plan`),
seeds 可原樣沿用;改完再跑 12/40 兩個規模與 35b 那腿。本次的 bundle 保留為
否證紀錄,數字不得引用。

## 等你裁的三件事

1. **toolchain**:照建議排除(v2 純資訊結構),還是要收進來?
2. **題數**:維持 8(可比性直覺、跑得快),還是加到 10–12 換解析度?
3. **G 的規模**:40 檔是拍腦袋的數字。要不要先做一個 mini 探針
   (只跑 G 一題 × 3 rep × 兩模型)確認 40 這個量級真的會逼出 SEARCH,
   再決定整套要不要照這個尺度做?
