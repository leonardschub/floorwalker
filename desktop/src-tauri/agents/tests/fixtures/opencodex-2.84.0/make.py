"""Writes the opencodex fixtures next to this script: `usage.jsonl` and the two quota
caches, in the shape opencodex 2.84.0 writes them (field names and nesting surveyed from
a real `~/.opencodex`, values invented). No row is copied from a real ledger: there are
no prompts, secrets, account ids or real request ids here. The extra keys
(`protocolTrace`, `routeDecision`, `spend`, ...) stand in for the fields the registry
must skip. Run with `python3 -I make.py`; the tests read the files it writes.

The timeline is built around the tests' clock: now = 2026-10-17 10:00 UTC, local time
UTC+02:00, so local midnight of the 17th is 2026-10-16 22:00 UTC.
"""

import json
import pathlib
from datetime import datetime, timezone

HERE = pathlib.Path(__file__).resolve().parent


def ms(iso):
    return int(datetime.fromisoformat(iso.replace("Z", "+00:00")).timestamp() * 1000)


def usage(inp, cache_read, cache_write, out):
    return {
        "inputTokens": inp,
        "outputTokens": out,
        "totalTokens": inp + out,
        "cachedInputTokens": cache_read,
        "cacheReadInputTokens": cache_read,
        "cacheCreationInputTokens": cache_write,
        "reasoningOutputTokens": 0,
    }


def row(rid, at, provider, model, *, surface=None, served=None, status=200, tokens=None,
        timing=None, attempts=1, failure=None, conversation="c0nv0000000000000000000000000001"):
    r = {
        "requestId": rid,
        "timestamp": ms(at),
        "provider": provider,
        "model": model,
        "admissionKind": "primary",
        "status": status,
        "durationMs": 2000,
        "usageStatus": "reported" if tokens else "unreported",
        "inboundProtocol": "messages" if surface else "responses",
        "conversationId": conversation,
        "requestedModel": "claude-haiku-5-5",
        "protocolTrace": {"placeholder": True},
    }
    if surface:
        r["surface"] = surface
    if served:
        r["servedModel"] = served
    if tokens:
        r["usage"] = usage(*tokens)
        r["totalTokens"] = r["usage"]["totalTokens"]
        r["cacheProvenance"] = "observed"
    if timing:
        first, start, last = timing
        r.update(firstOutputMs=first, genStartMs=start, lastOutputMs=last)
    if attempts:
        r["attempts"] = [
            {"ordinal": n + 1, "provider": provider, "model": served or model, "status": status}
            for n in range(attempts)
        ]
        r["routeDecision"] = {"reason": "placeholder"}
        r["spend"] = {"sends": attempts, "settled": attempts, "unresolved": 0}
    if failure:
        r.update(failureCause=failure, failureStage="headers-only", errorCode=failure.replace("-", "_"))
    return r


rows = [
    # Today's last instant before local midnight: 7 days, not Today.
    row("ocx-a1", "2026-10-16T21:59:59.999Z", "anthropic-native", "claude-opus-5-5",
        surface="claude", tokens=(1000, 600, 100, 50), timing=(800, 700, 1700)),
    # Local midnight exactly: Today.
    row("ocx-b2", "2026-10-16T22:00:00.000Z", "anthropic-native", "claude-opus-5-5",
        surface="claude", served="claude-opus-5-5", tokens=(2000, 1500, 0, 100), timing=(400, 300, 1300)),
    # Codex (no surface), routed: served model differs from the alias; failed over once.
    row("ocx-c3", "2026-10-17T08:00:00.000Z", "openai", "codex-auto", served="gpt-6.1-sol",
        tokens=(5000, 4000, 0, 200), timing=(1200, 1000, 3000), attempts=2),
    # Grok Build, rate-limited: an error with no tokens or timings.
    row("ocx-d4", "2026-10-17T09:00:00.000Z", "xai", "grok-5", surface="grok", status=429,
        failure="rate-limit"),
    # Claude Desktop, status 200 but a failure field set: an error.
    row("ocx-e5", "2026-10-17T09:30:00.000Z", "anthropic", "claude-sonnet-5-5",
        surface="claude-desktop", served="claude-sonnet-5-5", tokens=(300, 0, 0, 10),
        timing=(900, 850, 1350), failure="client-cancelled"),
    row("ocx-f6", "2026-10-17T09:45:00.000Z", "anthropic-native", "claude-opus-5-5",
        surface="claude", tokens=(1000, 900, 0, 30), timing=(600, 500, 1100), attempts=0),
    # The first instant of 7 days (local 10-11 00:00) and the last one before it.
    row("ocx-g7", "2026-10-10T22:00:00.000Z", "anthropic-native", "claude-opus-5-5",
        surface="claude", tokens=(100, 0, 0, 10)),
    row("ocx-h8", "2026-10-10T21:59:59.999Z", "openai", "gpt-6.1-sol", served="gpt-6.1-sol",
        tokens=(100, 0, 0, 10)),
    # The first instant of 30 days (local 09-18 00:00) and the last one before it.
    row("ocx-i9", "2026-09-17T22:00:00.000Z", "openai", "gpt-6.1-sol", served="gpt-6.1-sol",
        tokens=(100, 0, 0, 10)),
    row("ocx-j10", "2026-09-17T21:59:59.999Z", "anthropic-native", "claude-opus-5-5",
        surface="claude", tokens=(100, 0, 0, 10)),
]

lines = [json.dumps(r, separators=(",", ":")) for r in rows]
# A corrupt line and a row missing its required fields: both skipped.
lines.insert(3, '{"requestId":"ocx-broken","timestamp":')
lines.insert(5, '{"note":"not a usage row"}')
(HERE / "usage.jsonl").write_text("\n".join(lines) + "\n")

NOW_S = ms("2026-10-17T10:00:00Z") // 1000
codex = {
    "version": 1,
    "quotas": {
        "__main__": {
            "updatedAt": NOW_S * 1000 - 60_000,
            "credits": {"hasCredits": False, "unlimited": False, "balance": 0},
            "weeklyPercent": 43,
            "weeklyResetAt": NOW_S + 3 * 86400,  # seconds, as opencodex stores Codex resets
            "shortPercent": 12,
            "shortResetAt": NOW_S + 3600,
            "shortWindowSeconds": 18000,
        },
        "0f3a9c5e7b1d2468ace0f3a9c5e7b1d2": {
            "updatedAt": NOW_S * 1000 - 60_000,
            "weeklyPercent": 80,
            "weeklyResetAt": NOW_S + 86400,
        },
    },
    "mainPolicyQuota": {"identityKey": "placeholder", "quota": {"weeklyPercent": 43}},
    "history": {"version": 1, "accounts": {}},
}
(HERE / "codex-quota-cache.json").write_text(json.dumps(codex, indent=2) + "\n")

provider = {
    "version": 1,
    "rows": {
        "anthropic\u00001111111111111111111111111111111111111111": {
            "fiveHourPercent": 25,
            "fiveHourResetAt": (NOW_S + 7200) * 1000,  # milliseconds here
            "weeklyPercent": 40,
            "weeklyResetAt": (NOW_S + 4 * 86400) * 1000,
            # A window that has already reset: its figure is out of date.
            "monthlyPercent": 99,
            "monthlyResetAt": (NOW_S - 3600) * 1000,
            "customWindows": [
                {"label": "Fable", "scope": "model", "percent": 10, "resetAt": (NOW_S + 4 * 86400) * 1000}
            ],
            "updatedAt": NOW_S * 1000 - 60_000,
        },
        "2222222222222222222222222222222222222222": {
            "weeklyPercent": 5,
            "weeklyResetAt": (NOW_S + 5 * 86400) * 1000,
            "updatedAt": NOW_S * 1000 - 60_000,
        },
    },
    "kiroVerdicts": {},
}
(HERE / "provider-account-quota-cache.json").write_text(json.dumps(provider, indent=2) + "\n")
