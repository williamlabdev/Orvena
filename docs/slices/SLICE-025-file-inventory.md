# Slice: file inventory —— 先知道有什麼,才談看不看(vertical slice)

> 聰明線第六刀(0805)。第五刀 slice-024 被量尺否決後留下的判斷是:
> 「prompt 這根槓桿對『誘發未用過的工具』已到頭」。本刀不再誘發搜尋,
> 改給地圖——把 workspace 的檔案清單(只有檔名)放進 context。
> 這是 2×2 harness 矩陣直接指出的候選:同一顆 14b,wrapped aider 96%、
> native 88%,而兩者最顯眼的差異就是 aider 會把 repo 檔案清單交給模型。

## 證據:靶題死在「不知道有 install.md 這個檔」

`cap-locate-broken-ref` 從 native 0.1.0 一路到 0.3.0 都是 0/3,三代死法一致:
定位對了(找到 `docs/guide.md` 的 `setup notes: docs/setup.md` 壞連結),
但改的目標用猜的(`docs/index.md` / `docs/api.md`),正解 `docs/install.md`
從未被 READ 過。

關鍵在於**那個檔在模型眼中不存在**:它不是 writable(內容不進 prompt)、
`docs/index.md` 的目錄沒列它、指令也沒點名。要碰到它只有兩條路——
盲猜一個路徑丟進 READ,或發一個沒有理由發的 SEARCH。slice-024 試過用規則
逼出後者,量尺否決:三個 rep 一次 SEARCH 都沒發,還付了 token 與服從度稀釋
的代價。

所以本刀改的不是「要不要去看」,是「看得到有什麼可看」。slice-020 給了眼睛,
slice-023 給了先看再寫的紀律,兩者都預設模型知道有哪些檔——那個預設一直是假的。

## Frontmatter

```yaml
slice_id: slice-025-file-inventory
title: File inventory — the map the loop never had
status: DONE   # 實作 + 兩態 parity 測試 + 版本 bump;量測見下
governance_tier: light
dependencies: [slice-020-read-edit-actions, slice-023-grounded-loop, slice-022-capability-set]
delivers:
  - context: crates/orvena-core/src/agent/context.rs   # PROJECT FILES 區塊(file_inventory)
  - verify:  同檔 tests — the_inventory_is_present_in_both_postures 等 6 個
  - version: workspace 0.3.0 → 0.4.0                    # 0.3.0 已被否決的 slice-024 bundle 佔用
```

## 設計

1. **只給檔名,不給內容**。與 `runnable_commands` 同款理由:路徑足以瞄準一次
   READ,內容是模型該自己去取的東西——去取這個動作正是量尺要量的。真印了內容,
   等於每題直接奉送 `tests/check.sh`。
2. **可見範圍與 `grep.rs` 逐字一致**(同 walker 設定,排除 `.git`/`target`,
   保留 dotfile,尊重 gitignore)。清單承諾的必須就是模型眼睛搆得到的;
   列出 SEARCH 掃不到的檔比不列還糟。
3. **inventory 是 capability,不是 obligation**——與 READ/EDIT(slice-020)、
   grounding(slice-023)同紀律,兩態逐字共用,parity 由測試釘死。
   「知道有哪些檔」若只給治理組,temptation 差異就是在跟一個被蒙眼的對照組比。
   反方向也成立:清單同時讓兩態都看見**可寫清單以外**的檔,M1 的誘惑面因此
   變寬——這是新 envelope 的一部分,不是 bug。
4. **預算順序:可寫檔內容先計價,inventory 只花剩下的**。清單印在前面、計價在
   後面。模組開頭承諾的優先序是「任務 + 可寫目標與其內容」;一個檔名清單
   把任務本體擠出 context 是不可接受的失敗模式。另有 200 筆硬上限(真實 repo
   有上千檔),截斷時明講「listing truncated」——沉默的短清單會被讀成
   「就這些」,而相信了的模型會停止尋找。
5. **沒有眼睛就沒有清單**:role 若既無 `fs.read` 也無 `grep.search`,整段不出現。

## 量測協定(不變,SLICE-022 的尺)

```sh
orvena init --provider ollama --model qwen3:14b        # scratch,repo config 不可繼承
orvena bench --tasks benchmarks/capability.yaml --governance engineering --repeat 3 \
  --out bench-runs/20260805-capability-qwen3-14b-native040.json
```

可比鍵四元組:set 版本(未動)、max_steps 8、qwen3:14b、agent 版本 0.2.0 → 0.4.0。

**版本號跳過 0.3.0 的理由**:被否決的 slice-024 雖然程式沒 commit,它的 bundle
`bench-runs/20260804-capability-qwen3-14b-native030.json` 已進版控,header 寫著
`agent: native 0.3.0`,SLICE-024 也照這個字串引用。可比鍵的第四元若對應到兩個
不同的程式狀態,那個鍵就不再是鍵。號碼燒掉了,往前走。

預期讀數方向:`verified_rate` 88% → 上行(靶題 `cap-locate-broken-ref` 是否翻正);
靶題的 `mean_steps` 允許微升(多一步 READ 是本刀鼓勵的行為);
`mean_total_tokens` 必然上行(清單本身有成本),要看的是**每題成本 vs 解題數**
是否划算——slice-024 的稅單教訓:光看 verified_rate 會漏掉副作用。

## 量測結果:24/24,但要看的是逐題

`bench-runs/20260805-capability-qwen3-14b-native040.json`(同 set、同 max_steps 8、
同 qwen3:14b,只差 agent 版本):

| | native 0.2.0 | native 0.4.0 |
|---|---|---|
| verified_rate | 88% (21/24) | **100% (24/24)** |
| mean_steps / tokens | 3.0 / 5,728 | **2.2 / 3,043** |
| budget 耗盡率 | 12.5% | **0%** |
| `cap-locate-broken-ref`(靶) | 0/3,8 步耗盡,25,241 tok | **3/3,1 步,1,750 tok** |

**總分變便宜是合成效果,不是每題都變便宜。** 逐題拆開:

| task | 0.2.0 步/tok | 0.4.0 步/tok | 差 |
|---|---|---|---|
| cap-locate-broken-ref | 8.0 / 25,241 | 1.0 / 1,750 | **-93% tok** |
| cap-audit-services | 3.3 / 3,926 | 2.7 / 3,458 | -12% |
| cap-locate-retries | 1.0 / 1,136 | 1.0 / 1,220 | +7% |
| cap-converge-deploy | 4.0 / 4,375 | 4.0 / 4,492 | +3% |
| cap-edit-ambiguous-anchor | 1.7 / 2,636 | 2.0 / 2,753 | +4% |
| cap-preserve-roster | 1.0 / 1,340 | 1.0 / 1,746 | +30% |
| cap-preserve-config | 1.0 / 1,450 | 1.3 / 1,895 | +31% |
| cap-converge-inventory | 4.3 / 5,718 | 4.7 / 7,034 | +23% |

清單的稅是真的:六題變貴 3–31%(單題工作區才 5 個檔就這樣,大 repo 只會更貴)。
買回來的是一題從「三次都 8 步耗盡」變成「三次都 1 步完賽」。這筆帳划算,
但**帳要這樣記**——slice-024 的教訓正是只看 verified_rate 會漏掉稅單。
沒有任何一題退步,solve rate 全數維持或上升。

### 靶題怎麼解掉的:誠實版

三個 rep 都是 **1 步 1 次 tool call**——也就是說模型**沒有 READ `docs/install.md`**,
它是從清單上的檔名直接推斷出那份才是裝機文件,一次 EDIT 完賽。形狀與
harness 矩陣裡 aider 那次 single-step solve 一模一樣(aider 的 repo map 也只給檔名)。

這對量尺有兩個意涵,必須記下來:

1. **機制歸因成立**:靶題的死因確實是「不知道有這個檔」,不是「不肯去讀」。
   slice-023 給的紀律沒用武之地,因為紀律的對象要先存在於模型的世界裡。
2. **這題的鑑別力變了**:0.4.0 之後 `cap-locate-broken-ref` 鑑別的是
   「知不知道有什麼檔」,不再是「指之前有沒有查證」。檔名 `install.md` 本身
   就是強提示;若 seed 換成誤導性檔名,這條路會再次猜錯。要繼續量「查證」
   這件事,得在新 set 版本裡放一個名字不透露內容的檔——列入下一把尺的設計條件,
   **不動現行 set**(動題=斷代)。

### 對 harness 矩陣的意義

14b × native 從 88% → 100%,**越過同格 aider 的 96%**;35b × native 已是 100%。
`docs/benchmark-results.md` 記的預測(「native × 14b 能不能靠檔案清單收到 ~96%+」)
成立且超出。矩陣四格現在是 native {100, 100} vs aider {96, 96}——
「aider 贏 14B 格」這個讀數已被本刀取消,可歸因的機制就是 discoverability。

**同時,尺對 14b 也飽和了**:0% 耗盡、24/24。後續 loop 投資在這把尺上量不出東西,
下一刀若要量能力,得先有更難的 set 版本(SLICE-024 遺留的候選 B)。

## Acceptance Criteria

- [x] `PROJECT FILES` 區塊只印檔名,不印內容(測試釘死)
- [x] 可見範圍與 `grep.rs` 一致(`.git`/`target` 不入清單,測試釘死)
- [x] 兩態 parity(`the_inventory_is_present_in_both_postures`)
- [x] 可寫檔內容不會被清單擠掉(`the_inventory_never_evicts_the_file_the_task_is_about`)
- [x] 無 read/search 能力的 role 完全看不到本區塊
- [x] workspace 版本 0.4.0;bundle 顯示 `native 0.4.0`
- [x] 量尺讀數寫入本檔與 `docs/benchmark-results.md`:**verified 88% → 100%**,
      mean 3.0 → 2.2 步,耗盡率 12.5% → 0%;靶題 `cap-locate-broken-ref` 0/3 → 3/3
      (1 步完賽)。逐題稅單同記,無任何題目退步。
