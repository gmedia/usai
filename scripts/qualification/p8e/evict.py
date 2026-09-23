#!/usr/bin/env python3
"""Drop the page cache for the given files, so the next reader pays for them.

A floor cell is a claim about how small a machine an application fits on, and
the kernel charges a file's pages to the cgroup that *first* faults them in.
The harness runs the same binaries on the host (to build, to migrate, to drive
load), and `docker build` writes the image layer, so by the time a cell starts
its executable is usually already resident and already billed to somebody
else: the box then runs on memory nobody asked it to pay for. Measured on
VM 47 with a 121 MB `node`: 41 MB resident, 8 MB charged.

`posix_fadvise(POSIX_FADV_DONTNEED)` evicts clean page cache for a file and
needs no privileges (unlike `/proc/sys/vm/drop_caches`), so a cell can start
with a cold cache and be charged for everything it touches.

    evict.py <path>...      # files, or directories walked recursively

Prints one line per path with what was still resident afterwards.
"""
import ctypes
import mmap
import os
import sys

libc = ctypes.CDLL("libc.so.6", use_errno=True)


def resident_bytes(path: str) -> int:
    """How much of the file is in the page cache right now (mincore)."""
    size = os.path.getsize(path)
    if size == 0:
        return 0
    fd = os.open(path, os.O_RDONLY)
    try:
        mapped = mmap.mmap(fd, 0, access=mmap.ACCESS_COPY)
    except (ValueError, OSError):
        os.close(fd)
        return 0
    try:
        pages = (size + mmap.PAGESIZE - 1) // mmap.PAGESIZE
        vec = (ctypes.c_ubyte * pages)()
        addr = ctypes.addressof(ctypes.c_char.from_buffer(mapped))
        if libc.mincore(ctypes.c_void_p(addr), ctypes.c_size_t(size), vec) != 0:
            return -1
        resident = sum(v & 1 for v in vec)
        del vec
        return resident * mmap.PAGESIZE
    finally:
        mapped.close()
        os.close(fd)


def evict(path: str) -> None:
    fd = os.open(path, os.O_RDONLY)
    try:
        # Writeback first: dirty pages cannot be dropped, and a file the
        # build just wrote is dirty.
        os.fsync(fd)
    except OSError:
        pass
    try:
        os.posix_fadvise(fd, 0, 0, os.POSIX_FADV_DONTNEED)
    finally:
        os.close(fd)


def walk(target: str):
    if os.path.isdir(target):
        for root, _, names in os.walk(target):
            for name in names:
                path = os.path.join(root, name)
                if os.path.isfile(path) and not os.path.islink(path):
                    yield path
    elif os.path.isfile(target):
        yield target


def main(targets: list[str]) -> int:
    for target in targets:
        total = left = 0
        for path in walk(target):
            try:
                evict(path)
                total += os.path.getsize(path)
                left += max(resident_bytes(path), 0)
            except OSError as e:
                print(f"evict: {path}: {e}", file=sys.stderr)
        print(f"evicted {target}: {total / 1e6:.1f} MB, {left / 1e6:.1f} MB still resident")
    return 0


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print(__doc__, file=sys.stderr)
        raise SystemExit(2)
    raise SystemExit(main(sys.argv[1:]))
