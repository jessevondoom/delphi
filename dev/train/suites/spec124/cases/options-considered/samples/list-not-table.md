### Options considered

- **A, conventional:** the daemon keeps a queue file of export requests and drains it when the task ends. Test: restart the daemon mid-task and check one export arrives.
- **B, constraint removed (the daemon does the export):** the command line reads the mission's log itself. Test: compare with an export made while idle.
- **C, inversion:** the export is written as the log grows. Test: kill the daemon between two events and check the file ends at a whole event.

Chosen: B, because it needs no queue.
