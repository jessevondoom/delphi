### Options considered

| Option | Method | What it is | How it would be tested |
| :-- | :-- | :-- | :-- |
| A | conventional | The daemon keeps a queue file of export requests and drains it when the task ends. | Restart the daemon mid-task and check one export arrives. |
| B | constraint removed: the queue is a file | The daemon keeps a queue file of export requests, and drains it when the task ends! | Restart the daemon mid-task and count the exports. |
| C | inversion | When the task ends the daemon drains a queue file it keeps of export requests. | Ask twice and check one export arrives. |

Chosen: A, because it is the least change.
