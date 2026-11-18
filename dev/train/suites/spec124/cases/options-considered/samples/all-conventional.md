### Options considered

| Option | Method | What it is | How it would be tested |
| :-- | :-- | :-- | :-- |
| A | conventional | The daemon keeps a queue file of export requests and drains it when the task ends. | Restart the daemon mid-task and check one export arrives. |
| B | conventional, with a database | The daemon keeps export requests in a SQLite table. | Restart the daemon mid-task and read the table. |
| C | conventional, in memory | The daemon holds export requests in a list and writes it at shutdown. | Stop the daemon cleanly and check the list was written. |

Chosen: A, because it is the least change.
