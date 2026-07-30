#!/usr/bin/env python3
"""Measure estimate_tokens (crates/orvena-core/src/util.rs) against real tokenizers.

Produces the table in docs/token-estimate-accuracy.md. Re-run it after touching
util.rs, or when adding a provider whose tokenizer differs materially.

    pip install tiktoken
    python3 scripts/measure-token-estimate.py [extra-corpus-dir ...]

Exits non-zero if the aggregate ratio drifts outside [0.85, 1.15] against
o200k_base — a guard on the claim in the doc, not a correctness gate on Orvena.
"""

from __future__ import annotations

import math
import sys
from pathlib import Path

try:
    import tiktoken
except ImportError:  # pragma: no cover - operator-facing message
    sys.exit("needs tiktoken: pip install tiktoken")

REPO = Path(__file__).resolve().parents[1]
PATTERNS = ("*.md", "*.rs", "*.py", "*.yaml", "*.yml")
SKIP_PARTS = {"node_modules", ".venv", ".git", "target", "__pycache__", "data"}
MIN_BYTES, MAX_BYTES = 400, 200_000

# Mirrors the Rust `is_cjk` in the proposed CJK-aware variant.
CJK_RANGES = (
    (0x3000, 0x303F),  # CJK punctuation
    (0x3040, 0x30FF),  # kana
    (0x3400, 0x4DBF),  # ext A
    (0x4E00, 0x9FFF),  # unified ideographs
    (0xAC00, 0xD7AF),  # hangul syllables
    (0xF900, 0xFAFF),  # compatibility ideographs
    (0xFF00, 0xFFEF),  # fullwidth forms
)


def is_cjk(ch: str) -> bool:
    o = ord(ch)
    return any(lo <= o <= hi for lo, hi in CJK_RANGES)


def current(text: str) -> int:
    """util.rs today: ceil(chars / 4)."""
    return math.ceil(len(text) / 4.0)


def cjk_aware(text: str, weight: float = 1.0) -> int:
    """Proposed: CJK chars cost `weight` tokens, everything else a quarter."""
    cjk = sum(1 for c in text if is_cjk(c))
    return math.ceil((len(text) - cjk) / 4.0 + cjk * weight)


def collect(roots: list[Path]) -> list[tuple[Path, str]]:
    out: list[tuple[Path, str]] = []
    for root in roots:
        for pattern in PATTERNS:
            for path in root.rglob(pattern):
                if SKIP_PARTS & set(path.parts):
                    continue
                try:
                    text = path.read_text(encoding="utf-8")
                except (OSError, UnicodeDecodeError):
                    continue
                if MIN_BYTES < len(text) < MAX_BYTES:
                    out.append((path, text))
    return out


def main(argv: list[str]) -> int:
    roots = [REPO] + [Path(a).resolve() for a in argv[1:]]
    corpus = collect(roots)
    if not corpus:
        return print("no corpus found") or 1
    print(f"corpus: {len(corpus)} files from {', '.join(str(r) for r in roots)}\n")

    exit_code = 0
    for enc_name in ("cl100k_base", "o200k_base"):
        enc = tiktoken.get_encoding(enc_name)
        actual = sum(len(enc.encode(t)) for _, t in corpus)
        print(f"── {enc_name} ── actual {actual:,} tokens")

        variants = {
            "current chars/4": sum(current(t) for _, t in corpus),
            "CJK-aware w=1.0": sum(cjk_aware(t, 1.0) for _, t in corpus),
            "CJK-aware w=1.3": sum(cjk_aware(t, 1.3) for _, t in corpus),
        }
        for label, est in variants.items():
            print(f"   {label:<18} {est:>9,}   ratio {est / actual:.3f}")

        ratios = sorted(
            (current(t) / max(len(enc.encode(t)), 1), p) for p, t in corpus
        )
        worst, worst_path = ratios[0]
        print(
            f"   worst single-file under-count: {worst:.2f} "
            f"({worst_path.relative_to(REPO) if REPO in worst_path.parents else worst_path.name})"
        )

        if enc_name == "o200k_base":
            ratio = variants["current chars/4"] / actual
            if not 0.85 <= ratio <= 1.15:
                print(
                    f"   DRIFT: aggregate ratio {ratio:.3f} is outside [0.85, 1.15]; "
                    "docs/token-estimate-accuracy.md needs re-measuring"
                )
                exit_code = 1
        print()

    return exit_code


if __name__ == "__main__":
    sys.exit(main(sys.argv))
