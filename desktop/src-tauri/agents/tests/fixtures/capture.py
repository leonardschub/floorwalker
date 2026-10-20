#!/usr/bin/env python3
"""Captures a scrubbed `~/.claude`-shaped fixture tree for the agent registry's tests.

Reads the real `~/.claude` read-only and writes a copy of chosen sessions in which
every string is replaced by a placeholder unless its key is a known structural field
(record type, ids, timestamps, version, model, tool name, ...). So no prompt, message,
tool input or tool output survives, and no account or organisation id. Every UUID is
replaced by a fresh random one, consistently across files and file names. Working
directories are replaced by the fake path given for their project.

Name the output after the Claude Code version the sessions were written by
(`claude-code-<version>`): the transcript format is internal and changes between
versions, so when real data stops parsing, capture a new tree from the new version
rather than editing an old one.

    python3 -I capture.py OUT_DIR SRC_PROJECT_DIR=FAKE_CWD:SESSION[,SESSION...] ...

where SESSION is a session id, optionally followed by `+AGENT_ID` for each subagent
to capture with it (its `subagents/agent-<id>.jsonl` and `.meta.json`). Agent ids are
random hex and kept as they are; the meta's description and worktree are scrubbed.

Examples (as used for claude-code-2.1.301 and, with subagents, claude-code-2.1.302):

    python3 -I capture.py claude-code-2.1.301 \\
        -home-you-Projects-AgentGamified=/home/dev/Projects/AgentGamified:2043786a-...
    python3 -I capture.py claude-code-2.1.302 \\
        -home-you-Projects-Lantern=/home/dev/Projects/Orchard:6706f1cc-...+a1317fd98b265dbc2+...
"""

import json
import os
import re
import sys
import uuid

CLAUDE = os.path.expanduser("~/.claude")
UUID = re.compile(r"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}")
PLACEHOLDER = "[scrubbed]"

# Keys whose string values are structure, not content, and are kept as they are.
KEEP = {
    "type", "subtype", "role", "level", "operation", "permissionMode", "mode",
    "uuid", "parentUuid", "logicalParentUuid", "leafUuid", "promptId", "messageId",
    "sessionId", "session_id", "agentId", "toolUseId", "id", "requestId", "tool_use_id",
    "timestamp", "version", "userType", "entrypoint", "querySource",
    "model", "requestedModel", "advisorModel", "effort", "perTurnEffort",
    "stop_reason", "service_tier", "speed", "agentType", "status",
    "parentAgentId", "requestShape",
}

uuids = {}
# One placeholder per distinct title, so a title repeated across records stays equal.
titles = {"customTitle": {}, "aiTitle": {}}


def new_uuid(match):
    old = match.group(0)
    if old not in uuids:
        uuids[old] = str(uuid.uuid4())
    return uuids[old]


def scrub(value, key, cwd, parent):
    if isinstance(value, dict):
        out = {}
        for k, v in value.items():
            name = PLACEHOLDER + "-key" if "/" in k else k
            out[name] = scrub(v, k, cwd, value)
        return out
    if isinstance(value, list):
        return [scrub(v, key, cwd, parent) for v in value]
    if not isinstance(value, str):
        return value
    if key in KEEP:
        return UUID.sub(new_uuid, value)
    if key == "name" and parent.get("type") == "tool_use":
        return value
    if key == "cwd":
        return cwd
    if key == "gitBranch":
        return "main"
    if key in titles:
        seen = titles[key]
        if value not in seen:
            seen[value] = f"{'Custom' if key == 'customTitle' else 'AI'} title {len(seen) + 1}"
        return seen[value]
    if key == "thinking":
        return ""
    return PLACEHOLDER


def capture_transcript(src, dst, cwd):
    with open(src) as f:
        lines = f.readlines()
    with open(dst, "w") as out:
        for line in lines:
            try:
                record = json.loads(line)
            except ValueError:
                continue
            if record.get("type") == "api-request-blob":
                continue
            out.write(json.dumps(scrub(record, None, cwd, {}), separators=(",", ":")) + "\n")


def capture_session(src_project, out_project, session_id, cwd, agent_ids):
    new_id = UUID.sub(new_uuid, session_id)
    src = os.path.join(CLAUDE, "projects", src_project)
    capture_transcript(os.path.join(src, session_id + ".jsonl"),
                       os.path.join(out_project, new_id + ".jsonl"), cwd)
    if agent_ids:
        os.makedirs(os.path.join(out_project, new_id, "subagents"), exist_ok=True)
    for agent_id in agent_ids:
        name = os.path.join(session_id, "subagents", "agent-" + agent_id)
        out_name = os.path.join(out_project, new_id, "subagents", "agent-" + agent_id)
        capture_transcript(os.path.join(src, name + ".jsonl"), out_name + ".jsonl", cwd)
        with open(os.path.join(src, name + ".meta.json")) as f:
            meta = scrub(json.load(f), None, cwd, {})
        with open(out_name + ".meta.json", "w") as out:
            json.dump(meta, out)
    title = os.path.join(CLAUDE, "projects", src_project, session_id, "custom-title.json")
    if os.path.exists(title):
        os.makedirs(os.path.join(out_project, new_id), exist_ok=True)
        with open(title) as f:
            record = scrub(json.load(f), None, cwd, {})
        with open(os.path.join(out_project, new_id, "custom-title.json"), "w") as out:
            json.dump(record, out)


def main(out_dir, specs):
    for spec in specs:
        src_project, rest = spec.split("=", 1)
        cwd, sessions = rest.rsplit(":", 1)
        out_project = os.path.join(out_dir, "projects", re.sub(r"[^A-Za-z0-9]", "-", cwd))
        os.makedirs(out_project, exist_ok=True)
        for session in sessions.split(","):
            session_id, *agent_ids = session.split("+")
            capture_session(src_project, out_project, session_id, cwd, agent_ids)
    print(json.dumps(uuids, indent=1))


if __name__ == "__main__":
    main(sys.argv[1], sys.argv[2:])
