#!/usr/bin/env python3
"""Translate Slumbot wire messages into pkr-bot-pipe JSON.

Slumbot action string: streets separated by "/", tokens
  k = check, c = call, f = fold, b<N> = bet/raise to N TOTAL chips this street.
The bot wants, per decision:
  {"hole":[c,c], "board":[..], "actor":0|1, "actions":[{player,kind,amt}], ...}

Chip scale: Slumbot 100 chips = 1 bb; ours 2 chips = 1 bb -> ours = slumbot/50.

UNVERIFIED (needs slumbot sample_api.py + network):
  - whether client_pos == 0 means the client is the small blind (we assume yes)
  - exact blind/stack scaling for a 200bb match
The action-string PARSER below is unit-tested offline.

This module is imported by slumbot_client.py. It is NOT yet wired into a
live run (no network). See docs/roadmap/PLAN_STATUS_2026-10-09.md task 0.4.
"""
import re

SCALE = 50.0  # slumbot chips -> our chips


def parse_action_string(s):
    """'cc/cc/cc/cc' -> [ [('c',None),('c',None)], ... ] per street."""
    if s is None:
        return []
    streets = []
    cur = []
    for tok in re.findall(r"/|k|c|f|b\d+", s):
        if tok == "/":
            streets.append(cur)
            cur = []
        else:
            if tok[0] == "b":
                cur.append((tok[0], int(tok[1:])))
            else:
                cur.append((tok, None))
    streets.append(cur)
    return streets


def to_bot_record(action_str, hole, board, client_pos, my_seat):
    """Build the bot's JSON dict (minus 'seed') from a Slumbot record.

    my_seat: the seat the BOT occupies (0 = SB preflop, 1 = BB preflop).
    Actor per action: preflop index i -> (my_sb_seat + i) % 2 if the SB is
    seat 0; postflop first actor is the BB. We use the standard HU order:
      preflop actor(i) = i % 2          (seat 0 = SB acts first)
      postflop actor(i) = (1 + i) % 2   (seat 1 = BB acts first)
    """
    streets = parse_action_string(action_str)
    actions = []
    for si, street in enumerate(streets):
        for i, (kind, amt) in enumerate(street):
            actor = (i % 2) if si == 0 else ((1 + i) % 2)
            if kind == "k":
                actions.append({"player": actor, "kind": "check", "amt": 0.0})
            elif kind == "c":
                actions.append({"player": actor, "kind": "call", "amt": 0.0})
            elif kind == "f":
                actions.append({"player": actor, "kind": "fold", "amt": 0.0})
            elif kind == "b":
                actions.append({"player": actor, "kind": "bet", "amt": amt / SCALE})
    # actor to move = the next to act = (total actions) % 2 preflop,
    # postflop follows the BB-first rule. Approximate by the last street:
    total = sum(len(s) for s in streets)
    last_street = len(streets) - 1
    if last_street <= 0:
        nxt = total % 2
    else:
        nxt = (1 + (len(streets[-1]) if streets else 0)) % 2
    return {
        "hole": [int(hole[0]), int(hole[1])],
        "board": [int(c) for c in (board or [])],
        "actor": nxt,
        "actions": actions,
    }


def _selftest():
    assert parse_action_string("cc/cc/cc/cc") == [
        [("c", None), ("c", None)], [("c", None), ("c", None)],
        [("c", None), ("c", None)], [("c", None), ("c", None)]]
    # Slumbot puts the separator AFTER each street's actions.
    assert parse_action_string("b200/c/") == [
        [("b", 200)], [("c", None)], []]
    assert parse_action_string("") == [[]]
    r = to_bot_record("cc/", [0, 1], [20, 25, 30], 0, 0)
    assert r["actor"] == 0 and len(r["actions"]) == 2
    r = to_bot_record("b200/c/", [0, 1], [20, 25, 30], 0, 0)
    assert r["actions"][0]["kind"] == "bet"
    assert abs(r["actions"][0]["amt"] - 200 / 50.0) < 1e-6
    print("slumbot_protocol selftest OK")


if __name__ == "__main__":
    _selftest()
