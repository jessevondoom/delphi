## 7. Options considered

| | Option | How it would be tested |
| :-- | :-- | :-- |
| A, conventional | The daemon keeps a queue file of export requests and drains it when the task ends. | Ask for an export during a task, restart the daemon, and check one complete export arrives. Rejected: a queue to keep. |
| B, removes a constraint | Remove the constraint that the daemon does the export: the command line reads the log itself. | Compare an export made during a task with one made while idle, byte for byte. Chosen. |
| C, by a named method (analogy from printing) | A spool: the request is a file dropped in a folder that any later run of the program picks up. | Drop a request, start no daemon, run the program once and check the export exists. Rejected: two writers. |
