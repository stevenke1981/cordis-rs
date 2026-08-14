#!/usr/bin/env python3
"""Static audit usable even when the Rust toolchain is unavailable.

This is not a substitute for cargo build/test. It validates workspace structure,
TOML/JSON syntax, Rust module targets and balanced delimiters while rejecting
obvious unfinished implementation markers.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from dataclasses import dataclass, asdict
from pathlib import Path
from typing import Iterable

try:
    import tomllib
except ModuleNotFoundError as exc:  # pragma: no cover - Python 3.11+ expected
    raise SystemExit("static audit requires Python 3.11+ for tomllib") from exc

ROOT = Path(__file__).resolve().parents[1]
REQUIRED_DOCS = [
    "README.md", "SPEC.md", "PLAN.md", "IMPLEMENTATION_STATUS.md",
    "spec/SPEC.md", "spec/ACCEPTANCE.md", "spec/INVARIANTS.md",
    "docs/ARCHITECTURE.md", "docs/COMPATIBILITY.md", "docs/SECURITY.md",
    "docs/TESTING.md", "docs/OPERATIONS.md", "docs/ROADMAP.md",
    "SECURITY.md", "CONTRIBUTING.md", "CODE_OF_CONDUCT.md", "CHANGELOG.md",
]
REQUIRED_SCHEMAS = [
    "cordis.authorization.v1.schema.json", "cordis.task.v1.schema.json",
    "cordis.difficulty.v1.schema.json", "cordis.evidence.v1.schema.json",
    "cordis.plan.v1.schema.json", "cordis.step-result.v1.schema.json",
    "cordis.memory-item.v1.schema.json",
]
UNFINISHED = re.compile(r"\b(?:TODO|FIXME|XXX)\b|\btodo!\s*\(|\bunimplemented!\s*\(", re.IGNORECASE)
MODULE = re.compile(r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;")

@dataclass
class Finding:
    level: str
    check: str
    path: str
    message: str

class Audit:
    def __init__(self) -> None:
        self.findings: list[Finding] = []
        self.stats: dict[str, int] = {}

    def error(self, check: str, path: Path | str, message: str) -> None:
        self.findings.append(Finding("error", check, str(path), message))

    def warning(self, check: str, path: Path | str, message: str) -> None:
        self.findings.append(Finding("warning", check, str(path), message))

    def count(self, key: str, value: int = 1) -> None:
        self.stats[key] = self.stats.get(key, 0) + value


def load_toml(path: Path, audit: Audit) -> dict:
    try:
        with path.open("rb") as handle:
            value = tomllib.load(handle)
        audit.count("toml_files")
        return value
    except Exception as exc:  # noqa: BLE001
        audit.error("toml", path.relative_to(ROOT), str(exc))
        return {}


def audit_workspace(audit: Audit) -> None:
    root_manifest = load_toml(ROOT / "Cargo.toml", audit)
    members = root_manifest.get("workspace", {}).get("members", [])
    if len(members) != 12:
        audit.error("workspace", "Cargo.toml", f"expected 12 members, found {len(members)}")
    package_names: set[str] = set()
    for member in members:
        member_path = ROOT / member
        manifest = member_path / "Cargo.toml"
        if not manifest.is_file():
            audit.error("workspace", member, "missing Cargo.toml")
            continue
        value = load_toml(manifest, audit)
        name = value.get("package", {}).get("name")
        if not name:
            audit.error("workspace", manifest.relative_to(ROOT), "missing package.name")
        elif name in package_names:
            audit.error("workspace", manifest.relative_to(ROOT), f"duplicate package name {name}")
        else:
            package_names.add(name)
        if not (member_path / "src").is_dir():
            audit.error("workspace", member, "missing src directory")
        for section in ("dependencies", "dev-dependencies", "build-dependencies"):
            for dep, spec in value.get(section, {}).items():
                if isinstance(spec, dict) and "path" in spec:
                    target = (member_path / spec["path"]).resolve()
                    if not (target / "Cargo.toml").is_file():
                        audit.error("workspace", manifest.relative_to(ROOT), f"path dependency {dep} not found: {target}")
    audit.stats["workspace_members"] = len(members)

    for extra in ["rust-toolchain.toml", "deny.toml"]:
        load_toml(ROOT / extra, audit)


def strip_rust(source: str) -> str:
    """Replace comments and string/char literal contents with spaces."""
    out = list(source)
    i, n = 0, len(source)
    block_depth = 0
    state = "code"
    raw_hashes = 0
    while i < n:
        if state == "block":
            if source.startswith("/*", i):
                block_depth += 1
                out[i:i+2] = "  "
                i += 2
            elif source.startswith("*/", i):
                block_depth -= 1
                out[i:i+2] = "  "
                i += 2
                if block_depth == 0:
                    state = "code"
            else:
                if source[i] != "\n": out[i] = " "
                i += 1
            continue
        if state == "line":
            if source[i] == "\n":
                state = "code"
            else:
                out[i] = " "
            i += 1
            continue
        if state in {"string", "char"}:
            quote = '"' if state == "string" else "'"
            if source[i] == "\\":
                out[i] = " "
                if i + 1 < n:
                    if source[i+1] != "\n": out[i+1] = " "
                    i += 2
                else:
                    i += 1
            elif source[i] == quote:
                out[i] = " "
                i += 1
                state = "code"
            else:
                if source[i] != "\n": out[i] = " "
                i += 1
            continue
        if state == "raw":
            end = '"' + ('#' * raw_hashes)
            if source.startswith(end, i):
                out[i:i+len(end)] = " " * len(end)
                i += len(end)
                state = "code"
            else:
                if source[i] != "\n": out[i] = " "
                i += 1
            continue

        if source.startswith("//", i):
            out[i:i+2] = "  "; i += 2; state = "line"; continue
        if source.startswith("/*", i):
            out[i:i+2] = "  "; i += 2; state = "block"; block_depth = 1; continue
        # r###"..."### and br###"..."###
        m = re.match(r"(?:br|r)(#{0,255})\"", source[i:])
        if m:
            token = m.group(0)
            raw_hashes = len(m.group(1))
            out[i:i+len(token)] = " " * len(token)
            i += len(token); state = "raw"; continue
        if source.startswith('b"', i):
            out[i:i+2] = "  "; i += 2; state = "string"; continue
        if source[i] == '"':
            out[i] = " "; i += 1; state = "string"; continue
        # Lifetimes are not chars. Treat only an apostrophe followed by a closing apostrophe pattern as char.
        if source[i] == "'" and i + 2 < n:
            look = source[i+1:i+8]
            if look.startswith("\\") or "'" in look:
                out[i] = " "; i += 1; state = "char"; continue
        i += 1
    return "".join(out)


def check_delimiters(text: str) -> str | None:
    pairs = {')': '(', ']': '[', '}': '{'}
    stack: list[tuple[str, int]] = []
    for index, char in enumerate(text):
        if char in "([{":
            stack.append((char, index))
        elif char in pairs:
            if not stack or stack[-1][0] != pairs[char]:
                return f"unmatched {char!r} at byte {index}"
            stack.pop()
    if stack:
        char, index = stack[-1]
        return f"unclosed {char!r} from byte {index}"
    return None


def audit_rust(audit: Audit) -> None:
    rust_files = sorted(ROOT.glob("crates/**/*.rs"))
    audit.stats["rust_files"] = len(rust_files)
    audit.stats["rust_lines"] = 0
    for path in rust_files:
        rel = path.relative_to(ROOT)
        source = path.read_text(encoding="utf-8")
        audit.stats["rust_lines"] += source.count("\n") + 1
        if match := UNFINISHED.search(source):
            audit.error("unfinished", rel, f"unfinished marker: {match.group(0)}")
        stripped = strip_rust(source)
        if error := check_delimiters(stripped):
            audit.error("rust-delimiters", rel, error)
        for module in MODULE.findall(stripped):
            direct = path.parent / f"{module}.rs"
            nested = path.parent / module / "mod.rs"
            if not direct.is_file() and not nested.is_file():
                audit.error("rust-module", rel, f"mod {module}; has no {direct.name} or {module}/mod.rs")
        if "unsafe {" in stripped or "unsafe fn" in stripped or "unsafe impl" in stripped:
            audit.error("unsafe", rel, "unsafe code conflicts with workspace forbid")


def audit_json(audit: Audit) -> None:
    files = sorted((ROOT / "schemas").glob("*.json")) + sorted((ROOT / "conformance").rglob("*.json")) + sorted((ROOT / "examples").glob("*.json"))
    audit.stats["json_files"] = len(files)
    for path in files:
        try:
            value = json.loads(path.read_text(encoding="utf-8"))
            if not isinstance(value, (dict, list)):
                audit.error("json", path.relative_to(ROOT), "top-level JSON must be object or array")
        except Exception as exc:  # noqa: BLE001
            audit.error("json", path.relative_to(ROOT), str(exc))
    existing = {path.name for path in (ROOT / "schemas").glob("*.json")}
    for required in REQUIRED_SCHEMAS:
        if required not in existing:
            audit.error("schema", Path("schemas") / required, "required schema is missing")
    # Resolve local JSON Schema references.
    for path in (ROOT / "schemas").glob("*.json"):
        try:
            value = json.loads(path.read_text(encoding="utf-8"))
        except Exception:
            continue
        for ref in iter_refs(value):
            if ref.startswith(("http://", "https://", "#")):
                continue
            target = ROOT / "schemas" / ref.split("#", 1)[0]
            if not target.is_file():
                audit.error("schema-ref", path.relative_to(ROOT), f"missing local $ref target {ref}")


def iter_refs(value: object) -> Iterable[str]:
    if isinstance(value, dict):
        if isinstance(value.get("$ref"), str):
            yield value["$ref"]
        for child in value.values():
            yield from iter_refs(child)
    elif isinstance(value, list):
        for child in value:
            yield from iter_refs(child)


def validate_conformance_semantics(audit: Audit) -> None:
    base = ROOT / "conformance"
    try:
        manifest = json.loads((base / "manifest.json").read_text(encoding="utf-8"))
    except Exception as exc:  # noqa: BLE001
        audit.error("conformance", "conformance/manifest.json", str(exc)); return
    for case in manifest.get("cases", []):
        path = base / case.get("path", "")
        if not path.is_file():
            audit.error("conformance", path.relative_to(ROOT), "fixture listed in manifest is missing")
            continue
        data = json.loads(path.read_text(encoding="utf-8"))
        kind, expected = case.get("kind"), case.get("expected")
        actual_valid = True
        if kind == "task":
            auth = data.get("authorization", {})
            if auth.get("status") == "granted" and not str(auth.get("basis", "")).strip(): actual_valid = False
            for left, right in [("allowed_actions","denied_actions"),("allowed_tools","denied_tools"),("allowed_targets","denied_targets")]:
                if {str(x).lower() for x in auth.get(left, [])} & {str(x).lower() for x in auth.get(right, [])}: actual_valid = False
            if not data.get("acceptance_evidence"): actual_valid = False
        elif kind == "plan":
            actual_valid = bool(data.get("steps")) and not has_cycle(data.get("steps", []))
        elif kind == "runtime-plan":
            actual_valid = bool(data.get("steps")) and not has_cycle(data.get("steps", [])) and is_sequential(data.get("steps", []))
        elif kind == "step-result":
            actual_valid = bool(data.get("evidence")) and int(data.get("plan_version", 0)) > 0
        if kind in {"task","plan","runtime-plan","step-result"}:
            wanted = expected == "valid"
            if actual_valid != wanted:
                audit.error("conformance", path.relative_to(ROOT), f"semantic precheck expected {expected}, got {'valid' if actual_valid else 'invalid'}")


def has_cycle(steps: list[dict]) -> bool:
    by_id = {item.get("id"): item for item in steps}
    visiting: set[str] = set(); visited: set[str] = set()
    def visit(node: str) -> bool:
        if node in visiting: return True
        if node in visited: return False
        visiting.add(node)
        for dep in by_id.get(node, {}).get("depends_on", []):
            if dep not in by_id or visit(dep): return True
        visiting.remove(node); visited.add(node); return False
    return any(visit(str(node)) for node in by_id)


def is_sequential(steps: list[dict]) -> bool:
    roots = [s for s in steps if not s.get("depends_on")]
    if len(roots) != 1: return False
    incoming = {s.get("id"): [] for s in steps}
    for step in steps:
        successor = step.get("on_success")
        if successor in incoming: incoming[successor].append(step.get("id"))
    root_id = roots[0].get("id")
    for step in steps:
        if step.get("id") == root_id: continue
        parents = incoming.get(step.get("id"), [])
        if len(parents) != 1 or step.get("depends_on") != parents: return False
    return True


def audit_docs(audit: Audit) -> None:
    for item in REQUIRED_DOCS:
        path = ROOT / item
        if not path.is_file() or not path.read_text(encoding="utf-8").strip():
            audit.error("docs", item, "required non-empty document is missing")
    # Only check local Markdown links that look like files.
    link_re = re.compile(r"\[[^\]]+\]\(([^)]+)\)")
    for path in sorted(ROOT.glob("*.md")) + sorted((ROOT / "docs").rglob("*.md")) + sorted((ROOT / "spec").rglob("*.md")):
        text = path.read_text(encoding="utf-8")
        for target in link_re.findall(text):
            target = target.split("#", 1)[0]
            if not target or "://" in target or target.startswith("#"):
                continue
            resolved = (path.parent / target).resolve()
            try: resolved.relative_to(ROOT)
            except ValueError: continue
            if not resolved.exists():
                audit.error("docs-link", path.relative_to(ROOT), f"broken local link: {target}")


def audit_scripts(audit: Audit) -> None:
    scripts = sorted((ROOT / "scripts").glob("*.py"))
    for path in scripts:
        try:
            compile(path.read_text(encoding="utf-8"), str(path), "exec")
        except SyntaxError as exc:
            audit.error("python", path.relative_to(ROOT), str(exc))
    audit.stats["python_scripts"] = len(scripts)


def audit_git(audit: Audit, require_clean: bool) -> None:
    if not (ROOT / ".git").is_dir():
        if require_clean:
            audit.error("git", ".git", "repository is not initialized")
        else:
            audit.warning("git", ".git", "repository not initialized yet")
        return
    def git(*args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(["git", *args], cwd=ROOT, text=True, capture_output=True, check=False)
    count = git("rev-list", "--count", "HEAD")
    if count.returncode != 0 or int((count.stdout or "0").strip() or 0) < 1:
        audit.error("git", ".git", "repository has no commit")
    status = git("status", "--porcelain")
    if require_clean and status.stdout.strip():
        audit.error("git", ".git", f"working tree is not clean:\n{status.stdout}")
    tracked = git("ls-files")
    audit.stats["git_tracked_files"] = len([line for line in tracked.stdout.splitlines() if line])


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--require-git-clean", action="store_true")
    parser.add_argument("--report", type=Path)
    args = parser.parse_args()

    audit = Audit()
    audit_workspace(audit)
    audit_rust(audit)
    audit_json(audit)
    validate_conformance_semantics(audit)
    audit_docs(audit)
    audit_scripts(audit)
    audit_git(audit, args.require_git_clean)

    errors = [item for item in audit.findings if item.level == "error"]
    warnings = [item for item in audit.findings if item.level == "warning"]
    report = {
        "schema": "cordis.static-audit.v1",
        "root": str(ROOT),
        "status": "passed" if not errors else "failed",
        "stats": audit.stats,
        "error_count": len(errors),
        "warning_count": len(warnings),
        "findings": [asdict(item) for item in audit.findings],
        "limitations": [
            "Static audit does not parse Rust type semantics.",
            "cargo fmt, clippy, build and test remain required.",
            "Cargo.lock must be generated, reviewed and committed in the first verified Cargo environment.",
        ],
    }
    text = json.dumps(report, ensure_ascii=False, indent=2)
    if args.report:
        destination = args.report if args.report.is_absolute() else ROOT / args.report
        destination.write_text(text + "\n", encoding="utf-8")
    print(text)
    return 1 if errors else 0

if __name__ == "__main__":
    raise SystemExit(main())
