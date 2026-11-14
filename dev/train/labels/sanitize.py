#!/usr/bin/env python3
"""Copy stdin to stdout with local paths and account details removed: the scratch directory becomes <scratch>, the home
directory ~, the user name <user>; in JSON lines, any key that names an account, an e-mail or an organisation is dropped.

    python3 -I sanitize.py <scratch dir> < in > out
"""
import json
import os
import re
import sys

scratch = sys.argv[1]
home = os.path.expanduser("~")
user = os.path.basename(home)
# Also the host's own local configuration as its init event reports it (the operator's plugins and local paths).
DROP = re.compile(r"(email|account|organi[sz]ation|org_id|org_uuid|user_id|userid|^plugins$|memory_paths|messaging_socket_path)", re.I)


def clean_str(s):
    for a, b in ((scratch, "<scratch>"), ("/private" + scratch, "<scratch>"), (home, "~"), (user, "<user>")):
        if a:
            s = s.replace(a, b)
    s = s.replace("/private<scratch>", "<scratch>")
    # Names an operator's own instructions may put in a host's answer (an internal channel, a team): a comma-separated list in
    # LABELS_REDACT, never written in this file. Each is replaced, with or without a leading `#`.
    for name in filter(None, (x.strip() for x in os.environ.get("LABELS_REDACT", "").split(","))):
        s = re.sub("#?" + re.escape(name), "[an internal name]", s)
    return s


def clean(v):
    if isinstance(v, dict):
        return {k: clean(x) for k, x in v.items() if not DROP.search(k)}
    if isinstance(v, list):
        return [clean(x) for x in v]
    if isinstance(v, str):
        return clean_str(v)
    return v


for line in sys.stdin:
    try:
        v = json.loads(line)
    except ValueError:
        sys.stdout.write(clean_str(line))
        continue
    sys.stdout.write(json.dumps(clean(v), ensure_ascii=False) + "\n")
