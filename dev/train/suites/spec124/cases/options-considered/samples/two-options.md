### Options considered

| Option | Method | What it is | How it would be tested |
| :-- | :-- | :-- | :-- |
| A | conventional | The daemon keeps a queue file of export requests and drains it when the task ends. | Restart the daemon mid-task and check one export arrives. |
| B | constraint removed: the daemon does the export | The command line reads the mission's log itself. | Compare with an export made while idle. |

Chosen: A, because it is the least change.
