# Requirement: exports while the daemon is busy

A user asks for an export of a mission's log while the daemon is running a task. Today the request is refused with "busy".

The requirement: an export asked for while the daemon is busy is delivered once, complete, within five minutes of the task
ending, and is not lost if the daemon restarts in between.

Non-goals: exporting more than one mission at a time; changing the export's file format; a network service.

What the obvious design assumes: the daemon does the export; the request is kept on disk; the user waits for it.
