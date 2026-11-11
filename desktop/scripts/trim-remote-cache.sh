#!/bin/bash
# Run by remote.sh on the host, from its gpui folder, before it copies or builds
# anything: gives disk back when the host is short, and refuses the run when it
# still is. remote.sh's age limit doesn't help when several checkouts are all in
# use, and these folders filled the cloud box on 9 Nov, which runs a runner too:
# its saves started failing (THREADBEAR-1R).
#
#   trim-remote-cache.sh <this checkout's key>
#
# Exit 3: too little disk left to build on.
set -u
own="${1:?usage: trim-remote-cache.sh <key>}"

# Under this, idle checkouts lose their target folders: room for a build from
# nothing (a debug target has reached 7 GiB) with some to spare.
TRIM_GIB=12
# Under these a run isn't started at all: it would take the last of the disk
# from whatever else runs on the host. A first build writes far more than one
# with a target to build on.
REFUSE_GIB=3
REFUSE_COLD_GIB=8
# A target with anything changed this recently is in use. Cargo writes as it
# compiles, and remote.sh touches .in-use inside it every minute of a run, so
# this also covers a run that's only driving a built binary.
BUSY_MINUTES=15

free_gib() {
  local kb
  kb="$(df -Pk . | awk 'NR==2 {print $4}')"
  case "$kb" in
    # A filesystem past its reserved blocks can report less than nothing.
    -[0123456789]*) echo 0; return ;;
    # A df we can't read says nothing: don't delete or refuse on it.
    ""|*[!0123456789]*) echo 999999; return ;;
  esac
  echo $((kb / 1048576))
}

# Removes a target folder if the host is short and nothing's using it. Look
# inside it, not at it: rsync sets a checkout's own time to the Mac's, and cargo
# working inside a folder doesn't change that folder's.
evict() {
  [ "$(free_gib)" -lt "$TRIM_GIB" ] || return 0
  [ -d "$1" ] || return 0
  [ -z "$(find "$1" -mmin -"$BUSY_MINUTES" 2>/dev/null | head -1)" ] || return 0
  echo "Short of disk on this host, so removing $1, which nothing has used lately. Its checkout rebuilds next time it runs." >&2
  # Moved aside first, as the deleting takes a while and a run that starts in
  # that time should find no folder rather than half of one. It's still called
  # target-*, so one left by a crash here goes the next time room is short.
  local gone="target-gone-$$-${1#target-}"
  mv "$1" "$gone" && rm -rf "$gone"
}

# The one folder every checkout shared before they each got their own.
evict target
# Longest since a run first: remote.sh touches each target as a run starts.
# Names are target-<hex>, so the unquoted list is safe.
for t in $(ls -dtr target-* 2>/dev/null); do
  [ "$t" = "target-$own" ] || evict "$t"
done

need="$REFUSE_GIB"
# By size, not whether it's empty: a first build that died early leaves a
# folder with next to nothing to build on.
kb="$(du -sk "target-$own" 2>/dev/null | awk '{print $1}')"
[ "${kb:-0}" -ge 1048576 ] || need="$REFUSE_COLD_GIB"
free="$(free_gib)"
if [ "$free" -lt "$need" ]; then
  echo "This host has only $free GiB free and this run wants $need, with every other build folder in use, so it won't start. Free some space there and run it again." >&2
  exit 3
fi
