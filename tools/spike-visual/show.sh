#!/bin/sh
# SPIKE (spike-visual): print a prompt line, run the command, print its exit.
line='$'
for arg in "$@"; do
  case $arg in *[\ \{\"]*) line="$line '$arg'" ;; *) line="$line $arg" ;; esac
done
printf '%s\n' "$line"
"$@"
status=$?
printf "[exit status %s]\n" "$status"
sleep 1
