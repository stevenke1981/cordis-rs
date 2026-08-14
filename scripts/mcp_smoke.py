#!/usr/bin/env python3
"""Exercise both CORDIS MCP protocol paths and one evidence-bound task."""

from __future__ import annotations

import argparse
import json
import subprocess
import tempfile
from pathlib import Path


class Client:
    def __init__(self, binary: Path, data_dir: Path) -> None:
        self.process = subprocess.Popen(
            [str(binary), "--data-dir", str(data_dir)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
            bufsize=1,
        )
        self.next_id = 1

    def request(self, method: str, params: dict | None = None) -> dict:
        assert self.process.stdin and self.process.stdout
        request_id = self.next_id
        self.next_id += 1
        payload = {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": method,
            "params": params or {},
        }
        self.process.stdin.write(json.dumps(payload, ensure_ascii=False) + "\n")
        self.process.stdin.flush()
        line = self.process.stdout.readline()
        if not line:
            stderr = self.process.stderr.read() if self.process.stderr else ""
            raise RuntimeError(f"MCP process ended without a response: {stderr}")
        response = json.loads(line)
        if response.get("id") != request_id:
            raise AssertionError(f"response id mismatch: {response}")
        if "error" in response:
            raise AssertionError(f"MCP error for {method}: {response['error']}")
        return response["result"]

    def tool(self, name: str, arguments: dict) -> dict:
        result = self.request("tools/call", {"name": name, "arguments": arguments})
        if result.get("isError"):
            raise AssertionError(f"tool {name} failed: {result}")
        if "structuredContent" not in result or "content" not in result:
            raise AssertionError(f"tool {name} did not return both result forms")
        return result["structuredContent"]

    def close(self) -> None:
        if self.process.stdin:
            self.process.stdin.close()
        self.process.terminate()
        try:
            self.process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=5)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("binary", type=Path)
    args = parser.parse_args()
    binary = args.binary.resolve()
    if not binary.is_file():
        raise SystemExit(f"MCP binary not found: {binary}")

    with tempfile.TemporaryDirectory(prefix="cordis-mcp-smoke-") as tmp:
        client = Client(binary, Path(tmp) / "state")
        try:
            initialized = client.request(
                "initialize",
                {
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": {"name": "smoke", "version": "1"},
                },
            )
            assert initialized["protocolVersion"] == "2025-11-25"
            discovered = client.request(
                "server/discover",
                {"_meta": {"io.modelcontextprotocol/protocolVersion": "2026-07-28"}},
            )
            assert "2026-07-28" in discovered["supportedVersions"]
            listed = client.request("tools/list")
            names = [item["name"] for item in listed["tools"]]
            required = {
                "cordis_begin",
                "cordis_check_action",
                "cordis_finish",
                "cordis_workflow_begin",
                "cordis_status",
            }
            assert required.issubset(names), required - set(names)

            task_id = "mcp-smoke-task"
            begun = client.tool(
                "cordis_begin",
                {
                    "task_id": task_id,
                    "goal": "Verify native MCP lifecycle",
                    "project_id": "mcp-smoke",
                    "domain": "software",
                    "stakes": "low",
                    "complexity": 0.1,
                    "scope_in": ["workspace"],
                    "scope_out": ["production"],
                    "acceptance_evidence": [
                        {
                            "id": "verified",
                            "description": "MCP lifecycle passes",
                            "required": True,
                        }
                    ],
                },
            )
            assert begun["task_id"] == task_id
            checked = client.tool(
                "cordis_check_action",
                {
                    "task_id": task_id,
                    "description": "Verify MCP lifecycle",
                    "purpose": "Prove the local MCP task",
                    "action_class": "verify",
                    "tool": "local-smoke",
                    "target": "workspace",
                },
            )
            assert checked["permit"]["allowed"] is True, checked
            client.tool(
                "cordis_observe",
                {
                    "task_id": task_id,
                    "event_type": "test_passed",
                    "summary": "native stdio MCP responded with structured results",
                    "tool": "mcp-smoke",
                    "scope": "project",
                    "trust": "observed",
                },
            )
            finished = client.tool(
                "cordis_finish",
                {
                    "task_id": task_id,
                    "outcome": "success",
                    "evidence": [
                        {
                            "kind": "test",
                            "summary": "MCP lifecycle passes",
                            "passed": True,
                            "acceptance_id": "verified",
                            "source_id": "mcp-smoke-run",
                            "trust": "observed",
                        }
                    ],
                    "lesson": "Native MCP lifecycle completed.",
                },
            )
            assert finished["event"]["outcome"] == "success", finished
            status = client.tool("cordis_status", {})
            assert status["runtime"]["core"]["counts"]["task_records"] >= 1, status
        finally:
            client.close()
    print(json.dumps({"status": "passed", "binary": str(binary)}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
