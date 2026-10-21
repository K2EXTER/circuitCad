"""Drives `ccad --strict mcp` over JSON-RPC (stdio) like an MCP client and prints the answers.

usage: python -I tools/mcp_demo.py <ccad.exe> <script file> [--lib DIR]
The script file has blocks separated by a line with only '---'; each block is one ccad_run call
(the session is kept between calls, like in Claude Code). Used to record demo transcripts.
"""
import json
import subprocess
import sys


def main():
    sys.stdout.reconfigure(encoding="utf-8")
    exe, script = sys.argv[1], sys.argv[2]
    extra = sys.argv[3:]
    blocks = [b.strip("\n") for b in open(script, encoding="utf-8").read().split("\n---\n")]
    p = subprocess.Popen([exe, *extra, "--strict", "mcp"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True, encoding="utf-8")

    def rpc(i, method, params=None):
        msg = {"jsonrpc": "2.0", "id": i, "method": method}
        if params is not None:
            msg["params"] = params
        p.stdin.write(json.dumps(msg) + "\n")
        p.stdin.flush()
        return json.loads(p.stdout.readline())

    rpc(0, "initialize", {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "demo", "version": "0"}})
    p.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
    p.stdin.flush()
    for i, b in enumerate(blocks, 1):
        keep = b.startswith("#keep")
        r = rpc(i, "tools/call", {"name": "ccad_run", "arguments": {"script": b, "keep_going": keep}})["result"]
        print("=== call %d (isError=%s)" % (i, r.get("isError")))
        print(b)
        print("--- answers")
        for c in r["content"]:
            for line in c["text"].splitlines():
                print(line)
    p.stdin.close()
    p.wait(timeout=5)


main()
