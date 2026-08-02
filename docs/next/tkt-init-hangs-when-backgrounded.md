# Ticket: `orvena init` hangs silently when it is not in the foreground

> Status: OPEN · opened 2026-08-02 · found while running the 2026-08-02
> differential

## Problem

`crates/orvena-cli/src/commands/init.rs:21` decides whether to run the
interactive provider picker by asking whether stdin is a terminal:

```rust
if std::io::stdin().is_terminal() {
    choose_provider(&dir)?;      // prompts, reads stdin
} else {
    print_manual_next_steps();
}
```

The module docstring states the intent correctly — "in a non-interactive shell
we deploy the scaffold and print the next steps instead of prompting" — but
`is_terminal()` answers a different question than the one being asked. It
reports whether stdin *is* a terminal, not whether this process is allowed to
*read* it.

A backgrounded process keeps stdin attached to the terminal. So
`nohup orvena init &`, or anything under `&`, or a launchd/cron job that
inherits a tty, takes the interactive branch, calls `read_line` on the terminal
from a background process group, and is stopped by **SIGTTIN**. There is no
error, no exit, no output — the process just sits in state `T` forever.

### Observed

Running the 2026-08-02 differential:

```sh
nohup scripts/bench-differential.sh 3 qwen3:14b > /tmp/bench.log 2>&1 &
```

The log stopped after `== scaffolding a throwaway project in /var/.../tmp.XXX ==`
and nothing followed for ~10 minutes. It looked like a slow model run. It was
not running at all:

```
$ ps -p 38899 -o pid,stat,command
  PID STAT COMMAND
38899 TN   bash scripts/bench-differential.sh 3 qwen3:14b     # T = stopped
39019 TN   .../target/release/orvena init                     # stopped on SIGTTIN
```

The scaffold files were already written; only the wizard was stuck. `fg` would
have resumed it into a prompt nobody was watching.

## Why it matters beyond one script

`scripts/bench-differential.sh` calls `"$BIN" init >/dev/null` and depends on
the non-interactive branch being taken. That is our own in-repo instance of the
bug, and the fix there is one redirect. The product-level problem is that
**every unattended invocation of `orvena init` can hang with no diagnostic**:
a benchmark harness, a provisioning script, a launchd job, a CI wrapper that
allocates a tty. A CLI that silently stops is worse than one that errors, and
`init` is the first command anyone runs.

## What to do

Two changes, both worth having:

1. **Give `init` an explicit non-interactive interface** so scripts never depend
   on detection at all — e.g. `orvena init --provider ollama --model qwen3:14b`
   (and `--base-url` / `--api-key-env` for `openai_compat`), plus a plain
   `--non-interactive` that scaffolds and prints next steps. `bench-differential.sh`
   should then pass the flags instead of scaffolding and `sed`-ing the YAML
   afterwards, which is its own small fragility.
2. **Make the auto-detection ask the right question.** "Can I prompt" is
   `stdin is a terminal` **and** `this process is in the terminal's foreground
   process group` — on Unix, `tcgetpgrp(0) == getpgrp()`. Falling back to
   `print_manual_next_steps()` when backgrounded turns a silent stop into the
   behaviour the docstring already promises.

Belt and braces: `bench-differential.sh` can also redirect `< /dev/null`, but
that hides the product bug rather than fixing it, so it should not be the only
change.

## Notes for whoever picks this up

- The confirmed failure is the SIGTTIN stop above; it reproduces with any
  `nohup … &` invocation in a terminal.
- Ruled out while investigating: the `base_url` re-prompt loop at
  `init.rs:74-86` looks like it could spin forever on an EOF-ing stdin, but it
  cannot be reached that way. Ctrl-D at that prompt just re-prompts (a pty
  returns EOF per-read, then blocks again), and a genuine terminal hangup makes
  `read_line` return `Err(EIO)`, which propagates and exits 101. Verified under
  a real pty; no fix needed there.

## Related

- `scripts/bench-differential.sh` — the caller that surfaced it
- [`tkt-remeasure-native-differential.md`](tkt-remeasure-native-differential.md)
  — the run during which it was found
