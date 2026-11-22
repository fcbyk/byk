#!/usr/bin/env python3
"""Check the src module dependency direction.

Rules:
- deps is a leaf: it may only reference crate::deps and crate::net.
- model / utils / cli / net are leaves: no crate:: imports.
- resolve may use model, utils, deps, resolve.
- alias and filecmd must not depend on each other.
- options / completion / api / app follow the layering enforced below.
- the old ops container must not return.
"""

from pathlib import Path
import re
import sys

ROOT = Path(__file__).resolve().parent.parent
SRC = ROOT / "src"

ALLOWED = {
    "main": {"app", "cli", "model"},
    "app": {"alias", "api", "app", "cli", "filecmd", "model", "options", "utils"},
    "cli": set(),
    "options": {"cli", "model", "options", "resolve", "utils"},
    "api": {"api", "completion", "deps", "execution", "filecmd", "model", "resolve", "utils"},
    "completion": {"completion", "filecmd", "model", "resolve", "utils"},
    "alias": {"alias", "execution", "model", "resolve", "utils"},
    "execution": {"execution", "model", "utils"},
    "filecmd": {"deps", "filecmd", "model", "net", "resolve", "utils"},
    "resolve": {"deps", "model", "resolve", "utils"},
    "deps": {"deps", "net"},
    "net": set(),
    "model": set(),
    "utils": set(),
}

CRATE_PATH = re.compile(r"crate::([A-Za-z0-9_]+)")


def top_level_module(path: Path) -> str:
    rel = path.relative_to(SRC)
    return rel.parts[0] if len(rel.parts) > 1 else rel.stem


def main() -> int:
    violations: list[str] = []

    for path in sorted(SRC.rglob("*.rs")):
        module = top_level_module(path)
        allowed = ALLOWED.get(module)
        if allowed is None:
            violations.append(f"{path.relative_to(ROOT)}: unknown top-level module {module!r}")
            continue

        for lineno, line in enumerate(path.read_text().splitlines(), 1):
            stripped = line.strip()
            if stripped.startswith("//"):
                continue
            if re.search(r"\bmod\s+ops\b", line):
                violations.append(
                    f"{path.relative_to(ROOT)}:{lineno}: ops container must not return"
                )
            for target in CRATE_PATH.findall(line):
                if target not in allowed:
                    violations.append(
                        f"{path.relative_to(ROOT)}:{lineno}: "
                        f"{module} must not depend on crate::{target}"
                    )

    if violations:
        print("Architecture check failed:\n")
        for violation in violations:
            print(f"  {violation}")
        return 1

    print("Architecture check passed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
