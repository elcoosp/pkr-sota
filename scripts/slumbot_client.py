#!/usr/bin/env python3
"""Plays N hands vs slumbot.com through our bot over a pipe.

VERIFY the wire format against slumbot's official sample_api.py before
trusting any number. Slumbot plays 200bb (20000 chips, 50/100 blinds);
our bot default is 100bb -> run the bot with PKR_STACK_BB=200 for a
like-for-like comparison.

Bot protocol (stdin/stdout, one JSON line each way):
  -> {"action": "<slumbot action str>", "hole": [...], "board": [...],
      "client_pos": 0|1, "winnings": <int|null>}
  <- one incr token: "k" | "c" | "f" | "b<total_chips>"
"""
import requests, sys, json, subprocess, re

HOST = "https://slumbot.com"

def post(path, payload):
    r = requests.post(f"{HOST}/api/{path}", json=payload, timeout=10)
    r.raise_for_status()
    return r.json()

def main(n_hands, bot_cmd):
    bot = subprocess.Popen(bot_cmd, stdin=subprocess.PIPE,
                           stdout=subprocess.PIPE, text=True, bufsize=1)
    token, total = None, 0
    for i in range(n_hands):
        r = post("new_hand", {"token": token} if token else {})
        token = r.get("token", token)
        while "winnings" not in r:
            bot.stdin.write(json.dumps({
                "action": r.get("action"),
                "hole": r.get("hole_cards"),
                "board": r.get("board"),
                "client_pos": r.get("client_pos"),
            }) + "\n")
            incr = bot.stdout.readline().strip()
            r = post("act", {"token": token, "incr": incr})
        total += r["winnings"]
        if (i + 1) % 500 == 0:
            # 1 bb = 100 slumbot chips
            print(f"{i+1} hands, mean {total/(i+1)/100*1000:.1f} mbb/hand", flush=True)
    print(f"FINAL: {n_hands} hands, {total/n_hands/100*1000:.1f} mbb/hand")

if __name__ == "__main__":
    main(int(sys.argv[1]), sys.argv[2:])
