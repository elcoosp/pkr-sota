# INVALID: earlier V4-vs-V5 H2H used a buggy bot (2026-10-08)

pkr-bot-pipe (before commit bd66376) always assigned the passed hole to
seat 0. For a SEAT-1 decision it therefore hashed the placeholder
[40,41] instead of the acting player real cards. Every seat-1 decision
in the cross-process V4-vs-V5 H2H was corrupted.

So the earlier verdict (V4 == V5, 10k x4 flat) is INVALID and must be
redone with the fixed bot.

The gadget-search H2H runs (GAD-seed42 = +1.108, GAD-seed43) used the
same buggy bot => also invalid.

Re-run in flight with the fixed binary.
