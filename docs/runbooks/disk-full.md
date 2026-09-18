# Disk full

The runtime writes nothing while serving (the artifact is read-only,
`USAI_COMPILE_CACHE=0` in the image, logs go to stderr). A full host disk
therefore shows up through PostgreSQL: statements fail with SQLSTATE class
53 (`sql_53100 … could not extend file`), which the runtime answers as
**503** and logs as `application error … code="sql_53100"`. Reads keep
working until PostgreSQL itself stops.

Inside a container, `/tmp` is a tmpfs counted against the memory limit:
filling it is memory pressure, not disk pressure (see memory-pressure).

Recovery is PostgreSQL's: free space, and the next statement succeeds — no
runtime restart.
