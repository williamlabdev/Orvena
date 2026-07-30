# ADR-004: External-agent adapter —— 包住別人的 agent,不把 enforcement 搬進去

> Architecture Decision Record — 記錄一次具體的架構決策及其原因。

## Frontmatter

```yaml
doc_type: adr
adr_id: ADR-004
title: External-agent adapter — 以 OS 邊界包住第三方 CLI agent,envelope 與 loop 分離
status: ACCEPTED
superseded_by: null
created: 2026-07-30
updated: 2026-07-30
author: William Chiu
related_blueprint: null # Orvena v0.1 以 MVP-SCOPE.md + slice 文件代替 blueprint 鏈
```

## Context

### Problem

差分計畫 §5 已裁決(D5):**「agent loop 是 commodity、信任 envelope 才是資產」**,
adapter 順序 **Aider → OpenHands → Claude Code**,而前提是 enforcement 先下移到 OS
邊界 —— 那一步由 slice-015/016(ADR-003)完成了。現在要回答的是它的下一格:

**Orvena 要以什麼形態接一個不是自己寫的 agent?**

這不是「支援更多後端」的功能題,是身分題。三種形態的保證強度差一個量級:

1. **把 Orvena 的規則寫進別人的 process**(OpenHands plugin / Aider 的 `--read`
   唯讀宣告)—— 回到「只約束合作者」的弱保證:規則是**請求**,agent 只要不照做
   (或有 bug、或被 prompt injection 帶走)就沒有任何東西攔得住。
2. **在 PR/diff 層事後稽核**(形態 B,遠端 SaaS agent 唯一可行的形態)—— 是**偵測
   不是阻止**;檔案已經被改了,只是有人事後發現。
3. **在 OS 邊界圈住整個 agent process**(形態 A)—— agent 相信什麼不重要,它的
   `write()` 直接拿到 `EPERM`。

同時有一個實證缺口:2026-07-11 那份差分,M1(containment)在 `qwen3:14b` 上是
**null result(100%/100%)** —— native loop 的 baseline 連 shell 都叫不動、看不到
read-only 鄰檔,誘惑根本沒被看見。「無煞車 agent」需要一個真的沒有煞車的替身。

### Stakeholders

| Role       | Person/Team        | Interest                                             |
| :--------- | :----------------- | :--------------------------------------------------- |
| Maintainer | William Chiu       | 差分數字可包第三方 agent;不繼承別人的依賴與版本漂移    |
| Benchmark  | orvena-core/bench  | M1/M2/M3 對 native 與 external 用**同一套**判準與證據  |
| Regulated  | 受監管買家         | 「不管誰在寫 code,越界寫入被 OS 擋下」是可稽核的保證   |
| Embedder   | 下游使用者         | native loop 不被外部 agent 取代(離線、單一 binary)   |

## Decision

### Status

ACCEPTED — implemented in slice-018(Aider adapter;stub-agent containment 測試在
CI 兩個平台跑,真實 Aider 在 macOS 手動實測)。

### Context

關鍵洞察:**enforcement 的位置決定保證強度,而 adapter 不該碰 enforcement。**

ADR-003 已經把 containment 放在「子行程 spawn 的那一刻」。一個外部 CLI agent 就是
一個子行程 —— 所以接它**不需要新的 enforcement 機制**,只需要把既有的那一層對準它:

```text
  task scope  ──▶ OS sandbox(strict writable = 宣告的 writes 路徑)
  instruction ──▶ `<agent> --message …`(headless,一步一次)
  "done"      ──▶ Orvena 的 gate,在 agent 停下後由外部重跑
  evidence    ──▶ 與 native run 同一份 RunReport / evidence.json
  judgement   ──▶ 與 native run 同一個獨立 git oracle
```

### Options

| # | 方案 | 保證 | 代價 |
|---|---|---|---|
| A | **OS 邊界包住整個 agent process**(採用) | 強制:越界寫入在 syscall 被擋 | 需 per-platform 後端(已有);網路無法圈(agent 要打自己的 model) |
| B | 把 scope 翻譯成 agent 自己的旗標(`--read` 等) | 請求:agent 願意才成立 | 零成本,但保證等於沒有;且每個 agent 語意不同 |
| C | 寫成 OpenHands/Aider 的 plugin,規則跑在對方 process 內 | 請求 + 身分稀釋 | 綁死對方架構,版本漂移,回到「只約束合作者」 |
| D | PR/diff 層事後稽核(形態 B) | 偵測非阻止 | 對遠端 SaaS 是唯一選項,但本機 agent 沒有理由降級 |

### Decision

**採 A,並明確標註它買到什麼、沒買到什麼。**

1. **包住,不換底,也不進駐。** Adapter 只做三件事:組 argv、在 sandbox 裡 spawn、
   把結果餵回既有的 gate/oracle/evidence 管線。Orvena 的規則一行都不進對方
   process;對方的 loop 一行都不進 Orvena。
2. **writable = 任務宣告的 `writes` 路徑,由 OS 強制。** 不是 root 整棵樹 —— 對
   external agent 用 `FsPolicy::Strict`,粒度到檔案(macOS SBPL `subpath`、Linux
   Landlock `path_beneath` 對非目錄自動降為 file-only rights,兩邊都實測可行)。
3. **`network: allow`,而且說出來。** 被包住的 agent 必須打得到自己的 model
   provider,所以網路不圈。**Orvena 圈的是它能寫什麼,不是它能送出什麼。** 任何
   引用 adapter containment 數字的頁面都必須併寫這一句。
4. **token 成本不觀測。** Orvena 在 adapter run 裡不發任何 model call,只能轉述
   agent 自己印的數字。用 `TokenAccounting{observed|agent_reported|unavailable}`
   如實標記;`unavailable` 時差分**不印 token 比值**(`Option<f32>` = `None`)——
   兩個未知數相除不是「治理免費」,是不是數字。
5. **judge 不能依賴被告的習慣。** oracle 改為對 baseline commit 做 diff(而非只看
   `git status`),否則一個會自己 commit 的 agent(Aider 預設就會)留下乾淨的
   worktree,會被判成「什麼都沒動」。adapter 同時關掉 auto-commit —— 兩層,不是
   二選一。
6. **native loop 不被取代。** 它是 deterministic offline 回歸基準(`offline`
   provider)與單一靜態 binary 分發;外部 agent 一律拖 Python/容器依賴。
   `--agent aider` 是 bench 的一個選項,不是產品預設。
7. **agent 缺席是系統性錯誤,不是分數。** 沒裝 agent → 立即報錯,不是跑出一整份 0%。

### 不做

- 不做 OpenHands plugin(形態 C)—— 理由同計畫 §5。
- 不接遠端 SaaS agent 當底層(Devin,defer)—— 本機 sandbox 圈不住它。
- 不把 Orvena 的 role/tool allowlist 翻譯成 agent 旗標 —— 那是形態 B 的弱保證,
  而且會讓「Orvena 擋下了」與「agent 剛好沒做」在證據上無法區分。

## Consequences

### Positive

- **差分 benchmark 可以包第三方 agent**:同一組任務、同一個 agent、同一個 model,
  raw vs wrapped,唯一變因是 enforcement —— 這正是「Claude Code raw vs Claude Code
  in Orvena」那句行銷武器的可執行版本。首發實測(Aider 0.86.2 + 本機 qwen3 系列)
  給出**非 null 的 M1 差分**,native baseline 給不出來的那個數字。
- **M1 的鑑別力來源被補上**:真正沒煞車的 agent(有 shell、能讀任何檔、能自己加
  檔案進 chat)才是「無煞車」的誠實替身。
- **保證強度不隨 agent 變**:換 agent 只換 `AdapterSpec` 這份資料,containment、
  gate、evidence、oracle 全部原封不動 —— 這本身就是「envelope 可分離」的證明。

### Negative

- **網路不圈**,exfiltration 不在保證內(見 Decision 3)。要圈就得同時代管 model
  呼叫,那等於重寫對方的 provider 層 —— 明確不做。
- **成本數字降級**:external run 的 token 是轉述值或未知;M4 對 adapter 只剩 steps
  可靠。
- **新檔案的 writable 必須放寬到父目錄**:OS 只能授予「可以在這個目錄寫」,不能授予
  「可以建立這個名字」。該任務的 containment 退化為 oracle 事後偵測 —— 會產生一條
  widening note 進 evidence,不靜默。
- **多一個外部依賴要 pin**:Aider 版本進 evidence(`agent: "aider 0.86.2"`),但
  benchmark 的可重現性從此也綁它的版本。

### Neutral

- adapter 目前只從 `orvena bench --agent` 到得了;產品面的 `orvena run` 不接外部
  agent(要不要接是另一個決策,不在本 ADR)。
- `AdapterSpec` 是純資料(name/program/args/env),支援下一個 agent 是加一份 profile,
  不是加一條 code path。

## Related Decisions

| ADR / Doc | Relationship | Description |
| :--- | :--- | :--- |
| ADR-003 | Extends | 本 ADR 把「spawn 那一刻的圈禁」對準一個**不是自家 loop** 的子行程 |
| ADR-001 | Unrelated-by-design | `intent` 管的是模型能不能觸發**已宣告的命令**;外部 agent 不經過 RUN 工具 |
| SLICE-018-aider-adapter.md | Implemented by | 本 ADR 的落地 slice |
| docs/benchmark-governance-differential-plan.md | Implements | §5 形態 A + D5(Aider → OpenHands → Claude Code;Devin defer) |

## References

- [SLICE-018-aider-adapter.md](../../SLICE-018-aider-adapter.md) — 落地 slice(施力點、驗收、實測)
- [docs/benchmark-governance-differential-plan.md](../benchmark-governance-differential-plan.md) — §5 兩種 adapter 形態、D5 裁決
- [ADR-003](./ADR-003-os-sandbox-boundary.md) — enforcement 下移到子行程邊界(本 ADR 的地基)
- `crates/orvena-core/src/adapter/mod.rs` — envelope 本體(argv、sandbox policy、gate 迴圈、evidence)
- `crates/orvena-core/src/adapter/aider.rs` — Aider profile(headless 旗標、model 對映、token 轉述)
- `crates/orvena-core/src/benchmark/oracle.rs` — 對 baseline commit 做 diff 的獨立判官

---

_ADR generated from the AI Native Software Engineering Framework ADR template._
