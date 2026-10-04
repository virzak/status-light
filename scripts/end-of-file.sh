#!/bin/bash
for file in "$@"; do
  if ! [[ -s "$file" && -z "$(tail -c 1 "$file")" ]]; then
    echo "$file: No newline at end of file!"
    exit 1
  fi
done
