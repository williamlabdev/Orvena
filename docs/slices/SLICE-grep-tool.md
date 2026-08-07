# Slice: `grep` 唯讀搜尋 —— 工具 + 協議 wiring(vertical slice)

> 給 code mode 的交接規格。Backend 在此範圍內實作,不擴 scope。
> 對應 MVP-SCOPE.md must-have #1(shell/grep)之**安全、可先做的那一半**。
> 這是**真 vertical slice**:交付一個 agent 實際叫得動的搜尋能力,不是只疊一層工具。

## Frontmatter

```yaml
slice_id: slice-001-grep-tool
title: grep read-only search — tool + action protocol
status: DONE
governance_tier: light        # 日常開發用 light;此 slice 不含高風險決策
dependencies: []              # 不依賴 shell slice / ADR
delivers:
  - tool:     crates/orvena-core/src/tools/grep.rs        # GrepTool(唯讀)
  - protocol: crates/orvena-core/src/agent/step.rs        # 新 Action::Search
  - wiring:   crates/orvena-core/src/agent/driver.rs      # 處理 Search、結果回饋
  - role:     .orvena/roles.yaml 預設 role 加 grep.search
  - deps:     crates/orvena-core/Cargo.toml               # regex + ignore(或 walkdir)
  - verify:   單元測試 + round-trip 整合測試
```

## Goal

讓 agent 能在 repo 內做**唯讀**的內容搜尋,並**把結果用在下一步決策**(搜尋 → 依命中去改檔)。純讀、不寫檔、不 shell-out,因此可獨立於 shell 安全問題先行交付。價值判準:agent 在一次 run 裡能「先 Search 再 Write」,而不是只有一個沒人呼叫的工具。

## Module Boundary

- **Input:** 模型輸出一個新的 action 區塊(pattern + 可選路徑範圍)。
- **Output:** 結構化命中結果回饋進下一輪 context/evidence,供模型接續使用。
- **Related files:**
  - `crates/orvena-core/src/tools/grep.rs` — 新工具 `GrepTool { root, role }`(**不需 `scope`**,唯讀)
  - `crates/orvena-core/src/tools/mod.rs` — `pub mod grep;` + `pub use`
  - `crates/orvena-core/src/agent/step.rs` — `Action` 新增 `Search { pattern, path }` 變體 + parse
  - `crates/orvena-core/src/agent/driver.rs` — apply 階段處理 `Search`,結果併入 `prior_evidence`/context
  - `crates/orvena-core/Cargo.toml` — 新增 `regex`、`ignore`(或 `walkdir`)
  - `crates/orvena-core/tests/` — round-trip 整合測試

## Acceptance Criteria

### Tool 行為(`grep.rs`)
- [ ] AC-1:`GrepTool::search(pattern, opts)` 回傳 `Vec<Hit { path, line_no, text }>` 或等價型別。
- [ ] AC-2:role-gated —— 呼叫前經 `require_tool("grep.search")`,行為與 `fs.rs` 一致(role 不允許回 `Error::Scope`)。
- [ ] AC-3:**純 Rust 實作,不 shell-out**——用 `regex` + `ignore`/`walkdir`,不呼叫系統 `grep`。
- [ ] AC-4:範圍限制在 `root` 內,不跟隨 symlink 逃出 root,預設略過 `.git/`、`target/`。
- [ ] AC-5:非法 regex 回 `Error`,不 panic。

### 協議 + wiring(**本 slice 的重點,勿省**)
- [ ] AC-6:`step.rs` 的 `Action` 新增 `Search { pattern, path }`;`parse_actions` 能解析一個新區塊語法(建議 `<<<SEARCH pattern\n[optional path]\n>>>`,與既有 `<<<WRITE>>>` 對稱)。
- [ ] AC-7:`driver.rs` 在 apply 階段呼叫 `GrepTool`,`report.tool_calls += 1`,並把命中結果寫進 `prior_evidence`(或 context),使**下一輪模型呼叫看得到搜尋結果**。
- [ ] AC-8:Search 失敗(如非法 regex)記成 blocker/evidence,不中斷 light-tier 迴圈,行為對齊現有 write 的錯誤處理。
- [ ] AC-9:既有 `Action::Write` 路徑與 `parses_none_when_absent` 等現有測試不回歸。

### Verification(gate 證據)
- [ ] AC-V1:單元測試涵蓋 命中 / 無命中 / 非法 regex / role 被拒。
- [ ] AC-V2:`step.rs` 測試 —— 解析 `<<<SEARCH>>>` 區塊、以及 Search 與 Write 混排。
- [ ] AC-V3:**round-trip 整合測試(關鍵)** —— 用 offline provider 腳本化兩步:第一輪回 `<<<SEARCH>>>`,第二輪根據餵回的命中回 `<<<WRITE>>>`;斷言最終檔案內容正確。證明 agent 真的能「搜尋 → 用結果去改」。
- [ ] AC-V4:`cargo build && cargo test && cargo clippy` 全綠(對齊現有 CI 的 build·test·clippy·boundary)。

## Scope

### In Scope
1. `tools/grep.rs` + `GrepTool`,`tools/mod.rs` 註冊。
2. `step.rs` 新增 `Action::Search` + parse。
3. `driver.rs` 處理 Search、結果回饋。
4. `Cargo.toml` 新增 `regex` + `ignore`/`walkdir`。
5. 預設 `roles.yaml` 加 `grep.search`。
6. 上述所有測試(含 round-trip)。

### Out of Scope(明確不做)
- ❌ 任何 shell / 命令執行工具(見下方 blocker)。
- ❌ 多 pattern / 複雜查詢語法;本 slice 一個 pattern + 可選路徑即可。
- ❌ 改動 gate / metrics / provider 邏輯(除了測試用 offline 腳本)。

## 交接給 code mode 的一句話

> 在 `orvena-core` 交付一個 role-gated、純 Rust(不 shell-out)的唯讀 grep 能力:`GrepTool` + `step.rs` 的 `Action::Search` 協議 + `driver.rs` 把結果回饋進下一輪。用 offline provider 寫一個 round-trip 測試證明 agent 能「搜尋→依結果改檔」。新增 `regex`+`ignore` 依賴。範圍限 `tools/` 與 `agent/`,`cargo build/test/clippy` 全綠。

---

## ⛔ 相關但被擋住:`shell` 工具

**不要在這條 slice 一起做。** shell 讓 agent 執行任意命令,會**繞過 scope-lock 與 read-only default**(可 `echo > 任何檔案`、`rm`),與 Orvena 核心紀律衝突。這是消費級的架構決策,需要先寫一份 ADR 決定:

- 是否用 allow-list(只准 test/build/lint 這類命令)?
- 寫入類命令是否一律走 `human` gate?
- 與現有 `verify` gate 執行命令的機制如何統一?

**建議順序:grep slice(本檔,現在做)→ ADR: shell 安全模型 → shell slice。**
要的話我下一步就產出那份 ADR 草稿(用你的 `adr-template.md`)。
