### Options considered

| Option | Method | What it is | How it would be tested |
| :-- | :-- | :-- | :-- |
| A | conventional | The daemon keeps a queue file of export requests and drains it when the task ends. | Ask for an export during a task, restart the daemon, and check one complete export arrives within five minutes of the task ending. |
| B | constraint removed: the daemon does the export | The command line reads the mission's log itself and writes the export, so a busy daemon is never asked. | Run an export from the command line during a task and compare it byte for byte with one made while idle. |
| C | inversion | The export is written as the log grows: every event appended also extends a standing export file, and a request only copies it. | Kill the daemon between two events and check the standing file ends at a whole event. |

Chosen: B, because it needs no queue and nothing to lose on a restart. Flagged by the guard: none.
