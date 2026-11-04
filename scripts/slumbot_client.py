#!/usr/bin/env python3
"""Plays N hands vs slumbot.com through our bot over a pipe.

Slumbot plays 200bb (20000 chips, 50/100 blinds); our bot default is
100bb -> run the bot with PKR_STACK_BB=200 for a like-for-like
comparison.

Contract (2026-11-03 re-audit M6)
---------------------------------
The bot peer is `crates/pkr-runtime/src/bin/pkr-bot-pipe.rs`, whose doc
comment defines the request line as:

    {"hole":[c,c],"board":[..],"actions":[
        {"player":p,"kind":"check"|"call"|"fold"|"bet","amt":f}]}

This client now sends exactly that, accumulated over the whole hand:

* ``hole``        — our hole cards from new_hand's ``hole_cards``.
* ``board``       — the full board seen so far (updated from each
                    response's ``board`` field).
* ``client_pos``  — 0|1 straight from new_hand (0 = button/IP). Our
                    seat is ``client_pos`` and the opponent's is
                    ``1 - client_pos``; this matches bot-pipe's fresh
                    heads-up GameState where seat 0 is the dealer/SB.
* ``actions``     — the FULL action history, one record per action in
                    chronological order. Our bot's returned incr token
                    is appended as our action; the server's ``action``
                    string (single incr-format token) is appended as the
                    opponent's. ``amt`` for bets is the total street
                    chips, exactly as bot-pipe's ``Bet(amt)`` expects.
* ``winnings``    — ``null`` during the hand; the actual integer is sent
                    in a final hand-end line after the server returns
                    ``winnings`` (the bot's one-line reply is consumed to
                    keep the pipe in lockstep).

Bot reply: one incr token per line, "k" | "c" | "f" | "b<total_chips>".

Server error handling: any response carrying ``error_msg`` aborts the
client with exit code 1 (previously the error was silently ignored and
the client spun on a dead token).

UNVERIFIED assumption kept from the original client: each server
response's ``action`` field is a single incremental token for the
opponent's latest move ("f"|"k"|"c"|"b<chips>"), and new_hand may carry
an empty action when we act first. Live verification against
slumbot's official sample_api.py is still required (Phase 6) — in
particular whether multi-action streets arrive concatenated and whether
bets are total-street or incremental amounts. The transport was also
switched from `requests` to stdlib urllib so the client runs anywhere
with python3 only.

--self-test runs the request-building logic against a scripted
in-process fake server + scripted bot and asserts the actions array
grows per street and winnings are forwarded (see bottom of file).
"""
import json
import re
import subprocess
import sys
import urllib.request

HOST = "https://slumbot.com"

# Incr token grammar shared with bot-pipe's output format.
TOKEN_RE = re.compile(r"\A(f|k|c|b(\d+))\Z")


def parse_token(tok, player):
    """Parse an incr token into a bot-pipe action record.

    "f" -> fold, "k" -> check, "c" -> call, "b<chips>" -> bet with
    ``amt`` = total street chips. The token grammar is shared by the
    bot's output and (assumed) the server's action strings.
    """
    m = TOKEN_RE.match(str(tok).strip())
    if not m:
        raise ValueError(f"unparseable incr token: {tok!r}")
    kind = m.group(1)
    if kind.startswith("b"):
        return {"player": player, "kind": "bet", "amt": float(m.group(2))}
    return {"player": player, "kind": {"f": "fold", "k": "check",
                                       "c": "call"}[kind]}


def post(host, path, payload, timeout=30):
    req = urllib.request.Request(
        f"{host}/api/{path}",
        data=json.dumps(payload).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        return json.loads(resp.read().decode())


class PipeBot:
    """Bot subprocess: one JSON request line in, one incr token out."""

    def __init__(self, cmd):
        self.proc = subprocess.Popen(
            cmd, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            text=True, bufsize=1)

    def ask(self, req):
        self.proc.stdin.write(json.dumps(req) + "\n")
        self.proc.stdin.flush()
        line = self.proc.stdout.readline()
        if not line:
            raise RuntimeError("bot closed the pipe")
        return line.strip()

    def close(self):
        try:
            self.proc.stdin.close()
        except Exception:
            pass
        self.proc.terminate()


class ScriptedBot:
    """In-process stand-in for --self-test: records requests, replays tokens."""

    def __init__(self, tokens):
        self.tokens = list(tokens)
        self.requests = []

    def ask(self, req):
        self.requests.append(req)
        return self.tokens.pop(0)

    def close(self):
        pass


def main(n_hands, bot, host=HOST):
    token, total = None, 0
    for i in range(n_hands):
        r = post(host, "new_hand", {"token": token} if token else {})
        check_server_error(r, "new_hand", bot)
        token = r.get("token", token)
        hole = list(r.get("hole_cards") or [])
        board = list(r.get("board") or [])
        client_pos = r.get("client_pos")
        if client_pos not in (0, 1):
            abort(bot, f"new_hand gave no usable client_pos: {r}")
        actions = []  # full bot-pipe action history for this hand
        while True:
            # Opponent's latest move arrives as a single incr token in
            # the response's "action" field ("" when we act first).
            opp = r.get("action")
            if opp:
                actions.append(parse_token(opp, 1 - client_pos))
            if "winnings" in r:
                break
            incr = bot.ask({
                "hole": hole,
                "board": board,
                "client_pos": client_pos,
                "actions": list(actions),  # snapshot, not the live list
                "winnings": None,
            })
            # Keep the incr-token parsing (f|k|c|b<chips>) — it both
            # validates the bot's reply and records our action.
            actions.append(parse_token(incr, client_pos))
            r = post(host, "act", {"token": token, "incr": incr})
            check_server_error(r, "act", bot)
            if r.get("board"):
                board = list(r["board"])
        winnings = r["winnings"]
        total += winnings
        # Hand end: forward the final actions + winnings to the bot and
        # consume its reply so stdin/stdout stay in lockstep.
        bot.ask({
            "hole": hole,
            "board": board,
            "client_pos": client_pos,
            "actions": list(actions),
            "winnings": winnings,
        })
        if (i + 1) % 500 == 0:
            # 1 bb = 100 slumbot chips
            print(f"{i+1} hands, mean {total/(i+1)/100*1000:.1f} mbb/hand",
                  flush=True)
    print(f"FINAL: {n_hands} hands, {total/max(n_hands, 1)/100*1000:.1f} "
          f"mbb/hand")


def abort(bot, msg):
    """Abort with non-zero exit after killing the bot subprocess."""
    print(f"ERROR: {msg}", file=sys.stderr)
    if isinstance(bot, PipeBot):
        bot.close()
    sys.exit(1)


def check_server_error(r, context, bot):
    """(c) the server reports failures via `error_msg`; never ignore it."""
    if "error_msg" in r:
        abort(bot, f"server returned error_msg from {context}: "
                   f"{r['error_msg']!r}")


# ---------------------------------------------------------------------
# --self-test: scripted fake server (in-process socketserver thread) +
# scripted bot. Asserts (a) the actions array grows per street with the
# exact bot-pipe fields, and (b) winnings are forwarded at hand end,
# plus the error_msg abort path.
# ---------------------------------------------------------------------
def self_test():
    import http.server
    import threading

    def run_case(script, tokens, expect_error):
        """Run one hand against a scripted server; return (records, bot)."""
        records = []

        class Handler(http.server.BaseHTTPRequestHandler):
            def do_POST(self):  # noqa: N802 - stdlib API
                body = json.loads(self.rfile.read(
                    int(self.headers["Content-Length"])))
                records.append((self.path, body))
                resp = script.pop(0)
                data = json.dumps(resp).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.end_headers()
                self.wfile.write(data)

            def log_message(self, *args):  # silence
                pass

        server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        port = server.server_address[1]
        t = threading.Thread(target=server.serve_forever, daemon=True)
        t.start()
        host = f"http://127.0.0.1:{port}"
        bot = ScriptedBot(tokens)
        try:
            try:
                main(1, bot, host=host)
                errored = False
            except SystemExit as e:
                errored = True
                if (e.code or 0) == 0:
                    raise AssertionError("SystemExit(0) is not an abort")
        finally:
            server.shutdown()
            server.server_close()
        if errored != expect_error:
            raise AssertionError(
                f"expected error={expect_error}, got error={errored}")
        return records, bot

    # --- case A: happy path, client_pos=0 (we are the button/SB) ---
    script = [
        {"token": "tok1", "client_pos": 0, "hole_cards": [26, 38],
         "board": [], "action": ""},                       # new_hand
        {"action": "b200", "board": [1, 2, 3]},            # act 1 (preflop)
        {"action": "c", "board": [1, 2, 3, 4]},            # act 2 (flop)
        {"action": "", "winnings": 1500},                  # act 3 (turn, end)
    ]
    # 4th token is the reply to the hand-end notification (discarded).
    records, bot = run_case(script, ["c", "b400", "b800", "k"],
                            expect_error=False)

    # server saw: new_hand + 3 acts, incr tokens forwarded verbatim
    assert [p for p, _ in records] == ["/api/new_hand", "/api/act",
                                       "/api/act", "/api/act"], records
    assert records[1][1]["incr"] == "c"
    assert records[2][1]["incr"] == "b400"
    assert records[3][1]["incr"] == "b800"

    # bot requests carry the full bot-pipe state; the actions array
    # grows per street (0 preflop -> 2 flop -> 4 turn) with exact records.
    assert len(bot.requests) == 4, bot.requests
    b1, b2, b3, bend = bot.requests
    assert b1["actions"] == [], b1
    assert b2["actions"] == [{"player": 0, "kind": "call"},
                             {"player": 1, "kind": "bet", "amt": 200.0}], b2
    assert b3["actions"] == [{"player": 0, "kind": "call"},
                             {"player": 1, "kind": "bet", "amt": 200.0},
                             {"player": 0, "kind": "bet", "amt": 400.0},
                             {"player": 1, "kind": "call"}], b3
    # board grows per street; hole/client_pos forwarded on every request
    assert b1["board"] == [] and b2["board"] == [1, 2, 3], b1["board"]
    assert b3["board"] == [1, 2, 3, 4], b3["board"]
    for b in (b1, b2, b3):
        assert b["hole"] == [26, 38] and b["client_pos"] == 0
        assert b["winnings"] is None

    # (b) winnings forwarded to the bot at hand end, with our final
    # bet800 appended after the server closed the hand.
    final_req = bend
    assert final_req["winnings"] == 1500, final_req
    assert final_req["actions"] == b3["actions"] + [
        {"player": 0, "kind": "bet", "amt": 800.0}], final_req
    print("PASS: actions grow per street (0 -> 2 -> 4) with bot-pipe "
          "fields; winnings=1500 forwarded at hand end")

    # --- case B: server error_msg aborts with non-zero exit ---
    script_b = [
        {"token": "tok2", "client_pos": 1, "hole_cards": [0, 13],
         "board": [], "action": ""},
        {"error_msg": "invalid token"},
    ]
    records_b, bot_b = run_case(script_b, ["c"], expect_error=True)
    assert len(records_b) == 2, records_b      # new_hand + the failing act
    assert len(bot_b.requests) == 1            # bot asked exactly once
    assert bot_b.requests[0]["client_pos"] == 1
    print("PASS: error_msg aborts with non-zero exit after 1 act")

    print("=== slumbot_client self-test: all cases passed ===")


if __name__ == "__main__":
    args = [a for a in sys.argv[1:] if a != "--self-test"]
    if "--self-test" in sys.argv[1:]:
        self_test()
    else:
        if len(args) < 1:
            print("usage: slumbot_client.py <n_hands> <bot_cmd...>",
                  file=sys.stderr)
            sys.exit(2)
        main(int(args[0]), PipeBot(args[1:]))
