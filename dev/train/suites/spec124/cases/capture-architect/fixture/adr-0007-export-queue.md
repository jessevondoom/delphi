# ADR 0007: the export queue's file format

- **Decision:** jobs are stored as one binary file per job, in a format of our own.
- **Context:** exports are queued while the daemon is busy and must survive a restart.
- **Options:** one binary file per job; one append-only JSON Lines log; a SQLite table.
- **Choice:** one binary file per job, because a job can be read without parsing the others.
- **Accepted drawbacks:** the format needs its own reader and writer.
- **Reversibility:** two-way.
- **Reversal trigger:** none stated.
