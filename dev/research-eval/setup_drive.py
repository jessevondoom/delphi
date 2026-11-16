"""Spec 114 M7: drive `delphi setup` in a scratch home at a pseudo-terminal: decline every subscription and key, accept only the
local Ollama model, and ratify the scratch home's access change. Usage: setup_drive.py <binary> <home> <model> <working dir>"""
import os, pty, re, sys, select, time

binary, home, model, cwd = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
pid, fd = pty.fork()
if pid == 0:
    os.chdir(cwd)
    os.execv(binary, [binary, "setup", "--home", home])

buf = b""
log = b""
deadline = time.time() + 120
while time.time() < deadline:
    r, _, _ = select.select([fd], [], [], 1.0)
    if not r:
        continue
    try:
        chunk = os.read(fd, 4096)
    except OSError:
        break
    if not chunk:
        break
    buf += chunk
    log += chunk
    text = buf.decode("utf-8", "replace")
    answer = None
    if "Ollama model (" in text and text.rstrip().endswith(":"):
        answer = model
    elif re.search(r"Ollama is running here.*\[[Yy]/[Nn]\] ?$", text, re.S):
        answer = "y"
    elif re.search(r"Ratify this.*\[[Yy]/[Nn]\] ?$", text, re.S):
        answer = "y"
    elif re.search(r"\[[Yy]/[Nn]\] ?$", text):
        answer = "n"
    elif re.search(r": ?$", text) and ("$/Mtok" in text or "project" in text.lower()):
        answer = ""
    if answer is not None:
        os.write(fd, (answer + "\n").encode())
        buf = b""
sys.stdout.write(log.decode("utf-8", "replace"))
_, status = os.waitpid(pid, 0)
print("\n[exit]", os.waitstatus_to_exitcode(status))
